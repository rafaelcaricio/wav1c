#!/usr/bin/env python3

from __future__ import annotations

import argparse
import csv
import datetime as dt
import filecmp
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any
from urllib.parse import urlparse
from urllib.request import Request, urlopen


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REAL_CONTENT_MANIFEST = REPO_ROOT / "scripts/quality_manifests/vmaf_resource_real_content.json"
TOOL_USAGE_RE = re.compile(
    r"tool_usage uv_non_dc_blocks=(?P<uv>\d+)\s+"
    r"inter_newmv_blocks=(?P<newmv>\d+)\s+"
    r"restoration_non_none_units=(?P<lr>\d+)\s+"
    r"seg1_blocks=(?P<seg1>\d+)"
)


def log(msg: str) -> None:
    print(msg, file=sys.stderr)


def run_cmd(cmd: list[str], check: bool = True, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(
        cmd,
        cwd=str(cwd) if cwd else None,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if check and proc.returncode != 0:
        cmd_str = " ".join(cmd)
        raise RuntimeError(
            f"Command failed ({proc.returncode}): {cmd_str}\n"
            f"stdout:\n{proc.stdout}\n"
            f"stderr:\n{proc.stderr}\n"
        )
    return proc


def parse_tool_usage_from_stderr(stderr: str) -> dict[str, int] | None:
    for line in stderr.splitlines():
        m = TOOL_USAGE_RE.search(line.strip())
        if not m:
            continue
        return {
            "tool_uv_non_dc_blocks": int(m.group("uv")),
            "tool_inter_newmv_blocks": int(m.group("newmv")),
            "tool_restoration_non_none_units": int(m.group("lr")),
            "tool_seg1_blocks": int(m.group("seg1")),
        }
    return None


def safe_float(token: str | None) -> float | None:
    if token is None:
        return None
    t = token.strip().lower()
    if t in {"nan", ""}:
        return None
    if t in {"inf", "+inf"}:
        return float("inf")
    if t == "-inf":
        return float("-inf")
    try:
        return float(t)
    except ValueError:
        return None


def now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def find_default_dav1d() -> Path | None:
    env = os.environ.get("DAV1D")
    if env:
        p = Path(env)
        if p.exists():
            return p

    local = (REPO_ROOT / "../dav1d/build/tools/dav1d").resolve()
    if local.exists():
        return local

    which = shutil.which("dav1d")
    if which:
        return Path(which)
    return None


def find_default_vmaf_model() -> Path | None:
    env = os.environ.get("VMAF_MODEL")
    if env:
        p = Path(env)
        if p.exists():
            return p

    preferred = Path("/Users/rafaelcaricio/development/vmaf/model/vmaf_v0.6.1.json")
    if preferred.exists():
        return preferred

    fallback = Path("/Users/rafaelcaricio/development/vmaf/model/vmaf_float_v0.6.1.json")
    if fallback.exists():
        return fallback
    return None


def find_encoder_bin(requested: str | None, build: bool) -> Path:
    if requested:
        p = Path(requested).expanduser().resolve()
        if not p.exists():
            raise FileNotFoundError(f"Encoder binary not found: {p}")
        return p

    candidates = [REPO_ROOT / "target/debug/wav1c", REPO_ROOT / "target/debug/wav1c-cli"]
    for c in candidates:
        if c.exists():
            return c

    if not build:
        raise FileNotFoundError(
            "Encoder binary not found in target/debug. Pass --build or --encoder-bin."
        )

    log("Building wav1c-cli...")
    run_cmd(["cargo", "build", "-p", "wav1c-cli"], cwd=REPO_ROOT)
    for c in candidates:
        if c.exists():
            return c
    raise FileNotFoundError("Build finished but wav1c-cli binary was not found.")


def ffprobe_duration_seconds(ffprobe_bin: str, video_path: Path) -> float:
    def _probe(select: str) -> float | None:
        proc = run_cmd(
            [
                ffprobe_bin,
                "-v",
                "error",
                "-show_entries",
                select,
                "-of",
                "default=nokey=1:noprint_wrappers=1",
                str(video_path),
            ],
            check=False,
        )
        if proc.returncode != 0:
            return None
        val = safe_float(proc.stdout.strip().splitlines()[-1] if proc.stdout.strip() else None)
        if val is None or val <= 0 or math.isnan(val):
            return None
        return float(val)

    stream_duration = _probe("stream=duration")
    if stream_duration is not None:
        return stream_duration

    format_duration = _probe("format=duration")
    if format_duration is not None:
        return format_duration

    raise RuntimeError(f"Could not determine video duration for {video_path}")


def parse_q_list(q_values: str) -> list[int]:
    out: list[int] = []
    for tok in q_values.split(","):
        tok = tok.strip()
        if not tok:
            continue
        q = int(tok)
        if q < 0 or q > 255:
            raise ValueError(f"Q value out of range [0..255]: {q}")
        out.append(q)
    if not out:
        raise ValueError("No valid Q values were provided.")
    return sorted(set(out))


def list_y4m_clips(clips_dir: Path) -> list[Path]:
    clips = sorted(p for p in clips_dir.glob("*.y4m") if p.is_file())
    if not clips:
        raise FileNotFoundError(f"No .y4m clips found in {clips_dir}")
    return clips


def looks_like_url(value: str) -> bool:
    scheme = urlparse(value).scheme.lower()
    return scheme in {"http", "https"}


def sanitize_clip_name(name: str) -> str:
    out = re.sub(r"[^A-Za-z0-9._-]+", "_", name.strip())
    out = out.strip("._-")
    if not out:
        raise ValueError(f"Invalid clip name: {name!r}")
    return out


def resolve_local_source(candidate: str, manifest_dir: Path) -> Path:
    p = Path(candidate).expanduser()
    if p.is_absolute():
        return p
    from_manifest = (manifest_dir / p).resolve()
    if from_manifest.exists():
        return from_manifest
    return (REPO_ROOT / p).resolve()


def clip_source_candidates(clip: dict[str, Any]) -> list[str]:
    if "sources" in clip:
        raw = clip["sources"]
        if not isinstance(raw, list):
            raise ValueError("'sources' must be a list when present.")
        vals = [str(v).strip() for v in raw if str(v).strip()]
        if vals:
            return vals

    single = clip.get("source") or clip.get("url") or clip.get("path")
    if single is None:
        raise ValueError("Clip entry must define 'source'/'url'/'path' or a non-empty 'sources' list.")
    val = str(single).strip()
    if not val:
        raise ValueError("Clip source cannot be empty.")
    return [val]


def choose_clip_source(candidates: list[str], manifest_dir: Path) -> tuple[Path | None, str | None]:
    first_url: str | None = None
    missing_paths: list[str] = []
    for candidate in candidates:
        if looks_like_url(candidate):
            if first_url is None:
                first_url = candidate
            continue

        local_path = resolve_local_source(candidate, manifest_dir)
        if local_path.exists():
            return local_path, None
        missing_paths.append(str(local_path))

    if first_url is not None:
        return None, first_url

    raise FileNotFoundError(
        "Could not resolve any local source path. Tried:\n" + "\n".join(f"- {p}" for p in missing_paths)
    )


def download_to_path(url: str, dst: Path, force: bool) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    if dst.exists() and not force:
        log(f"Reusing download: {dst}")
        return

    tmp = dst.with_suffix(dst.suffix + ".part")
    req = Request(url, headers={"User-Agent": "wav1c-quality-pipeline/1.0"})
    log(f"Downloading {url}")
    try:
        with urlopen(req, timeout=120) as resp, tmp.open("wb") as f:
            shutil.copyfileobj(resp, f)
        tmp.replace(dst)
    finally:
        if tmp.exists():
            tmp.unlink(missing_ok=True)


def parse_required_int(clip: dict[str, Any], key: str) -> int:
    if key not in clip:
        raise ValueError(f"Missing required key for rawvideo source: {key}")
    value = int(clip[key])
    if value <= 0:
        raise ValueError(f"Expected positive value for {key}, got: {clip[key]!r}")
    return value


def parse_optional_float(clip: dict[str, Any], key: str) -> float | None:
    if key not in clip or clip[key] is None:
        return None
    return float(clip[key])


def convert_source_to_y4m(
    ffmpeg_bin: str,
    source_path: Path,
    output_path: Path,
    clip: dict[str, Any],
) -> None:
    input_type = str(clip.get("input_type", "")).strip().lower()
    if not input_type:
        input_type = "rawvideo" if source_path.suffix.lower() == ".yuv" else "video"
    if input_type not in {"rawvideo", "video"}:
        raise ValueError(f"Unsupported input_type={input_type!r}. Expected 'rawvideo' or 'video'.")

    cmd = [ffmpeg_bin, "-hide_banner", "-y"]
    if input_type == "rawvideo":
        width = parse_required_int(clip, "width")
        height = parse_required_int(clip, "height")
        pix_fmt = str(clip.get("pix_fmt", "yuv420p"))
        fps = clip.get("fps", 24)
        cmd += [
            "-f",
            "rawvideo",
            "-pixel_format",
            pix_fmt,
            "-video_size",
            f"{width}x{height}",
            "-framerate",
            str(fps),
            "-i",
            str(source_path),
        ]
    else:
        cmd += ["-i", str(source_path)]

    start_sec = parse_optional_float(clip, "start_sec")
    duration_sec = parse_optional_float(clip, "duration_sec")
    if start_sec is not None and start_sec < 0:
        raise ValueError(f"start_sec must be >= 0, got {start_sec}")
    if duration_sec is not None and duration_sec <= 0:
        raise ValueError(f"duration_sec must be > 0, got {duration_sec}")

    if start_sec is not None:
        cmd += ["-ss", str(start_sec)]
    if duration_sec is not None:
        cmd += ["-t", str(duration_sec)]

    output_width = clip.get("output_width")
    output_height = clip.get("output_height")
    vf_parts: list[str] = []
    if (output_width is None) != (output_height is None):
        raise ValueError("output_width and output_height must either both be set or both be omitted.")
    if output_width is not None:
        scale_flags = str(clip.get("scale_flags", "bicubic"))
        vf_parts.append(f"scale={int(output_width)}:{int(output_height)}:flags={scale_flags}")
    if vf_parts:
        cmd += ["-vf", ",".join(vf_parts)]

    output_fps = clip.get("output_fps")
    if output_fps is not None:
        cmd += ["-r", str(output_fps)]

    cmd += ["-map", "0:v:0", "-pix_fmt", "yuv420p", str(output_path)]
    run_cmd(cmd)


def cmd_prepare_real(args: argparse.Namespace) -> int:
    manifest_path = Path(args.manifest).expanduser().resolve()
    if not manifest_path.exists():
        raise FileNotFoundError(f"Manifest not found: {manifest_path}")

    out_dir = Path(args.out_dir).expanduser().resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    cache_dir = (
        Path(args.cache_dir).expanduser().resolve()
        if args.cache_dir
        else (out_dir / "_downloads").resolve()
    )
    cache_dir.mkdir(parents=True, exist_ok=True)

    manifest = json.loads(manifest_path.read_text())
    clips = manifest.get("clips")
    if not isinstance(clips, list) or not clips:
        raise ValueError(f"Manifest {manifest_path} must contain a non-empty 'clips' list.")

    if args.max_clips > 0:
        clips = clips[: args.max_clips]

    results: list[dict[str, Any]] = []
    seen_outputs: set[str] = set()
    for idx, raw_clip in enumerate(clips, start=1):
        if not isinstance(raw_clip, dict):
            raise ValueError(f"Manifest clip entry #{idx} is not an object.")
        clip = dict(raw_clip)

        clip_name = sanitize_clip_name(str(clip.get("name", f"clip_{idx}")))
        output_name = str(clip.get("output_name", f"{clip_name}.y4m"))
        if not output_name.lower().endswith(".y4m"):
            output_name += ".y4m"
        if output_name in seen_outputs:
            raise ValueError(f"Duplicate output clip name in manifest: {output_name}")
        seen_outputs.add(output_name)

        output_path = out_dir / output_name
        candidates = clip_source_candidates(clip)
        local_source, url_source = choose_clip_source(candidates, manifest_path.parent)

        if local_source is not None:
            source_path = local_source
            source_info = str(local_source)
        else:
            assert url_source is not None
            parsed = Path(urlparse(url_source).path).name
            download_name = str(clip.get("download_name", parsed if parsed else f"{clip_name}.bin"))
            source_path = cache_dir / download_name
            download_to_path(url_source, source_path, args.force_download)
            source_info = url_source

        if output_path.exists() and not args.force_convert:
            log(f"Reusing prepared clip: {output_path.name}")
        else:
            log(f"[{idx}/{len(clips)}] Preparing {output_path.name}")
            convert_source_to_y4m(args.ffmpeg, source_path, output_path, clip)

        duration_s = ffprobe_duration_seconds(args.ffprobe, output_path)
        results.append(
            {
                "clip_name": output_path.name,
                "clip_path": str(output_path),
                "source_path": str(source_path),
                "source": source_info,
                "duration_s": duration_s,
            }
        )

    payload = {
        "metadata": {
            "created_at_utc": now_iso(),
            "manifest_path": str(manifest_path),
            "out_dir": str(out_dir),
            "cache_dir": str(cache_dir),
            "ffmpeg": args.ffmpeg,
            "ffprobe": args.ffprobe,
            "clip_count": len(results),
        },
        "clips": results,
    }
    (out_dir / "prepared_manifest.json").write_text(json.dumps(payload, indent=2))
    write_csv(out_dir / "prepared_manifest.csv", results)

    log(f"Prepared {len(results)} clips at {out_dir}")
    log(f"Manifest written to {out_dir / 'prepared_manifest.json'}")
    return 0


def parse_psnr(ffmpeg_output: str) -> dict[str, float | None]:
    m = re.search(
        r"PSNR y:(?P<y>[-+A-Za-z0-9.]+)\s+u:(?P<u>[-+A-Za-z0-9.]+)\s+v:(?P<v>[-+A-Za-z0-9.]+)\s+average:(?P<avg>[-+A-Za-z0-9.]+)",
        ffmpeg_output,
    )
    if not m:
        raise RuntimeError(f"Could not parse PSNR output:\n{ffmpeg_output}")
    return {
        "psnr_y": safe_float(m.group("y")),
        "psnr_u": safe_float(m.group("u")),
        "psnr_v": safe_float(m.group("v")),
        "psnr_avg": safe_float(m.group("avg")),
    }


def parse_ssim(ffmpeg_output: str) -> dict[str, float | None]:
    m = re.search(
        r"SSIM Y:(?P<y>[-+A-Za-z0-9.]+)\s+\([^)]+\)\s+U:(?P<u>[-+A-Za-z0-9.]+)\s+\([^)]+\)\s+V:(?P<v>[-+A-Za-z0-9.]+)\s+\([^)]+\)\s+All:(?P<all>[-+A-Za-z0-9.]+)",
        ffmpeg_output,
    )
    if not m:
        raise RuntimeError(f"Could not parse SSIM output:\n{ffmpeg_output}")
    return {
        "ssim_y": safe_float(m.group("y")),
        "ssim_u": safe_float(m.group("u")),
        "ssim_v": safe_float(m.group("v")),
        "ssim_all": safe_float(m.group("all")),
    }


def run_psnr(ffmpeg_bin: str, decoded_y4m: Path, ref_y4m: Path) -> dict[str, float | None]:
    proc = run_cmd(
        [
            ffmpeg_bin,
            "-hide_banner",
            "-nostats",
            "-i",
            str(decoded_y4m),
            "-i",
            str(ref_y4m),
            "-lavfi",
            "[0:v]setpts=PTS-STARTPTS[dist];[1:v]setpts=PTS-STARTPTS[ref];[dist][ref]psnr",
            "-f",
            "null",
            "-",
        ]
    )
    return parse_psnr(proc.stdout + "\n" + proc.stderr)


def run_ssim(ffmpeg_bin: str, decoded_y4m: Path, ref_y4m: Path) -> dict[str, float | None]:
    proc = run_cmd(
        [
            ffmpeg_bin,
            "-hide_banner",
            "-nostats",
            "-i",
            str(decoded_y4m),
            "-i",
            str(ref_y4m),
            "-lavfi",
            "[0:v]setpts=PTS-STARTPTS[dist];[1:v]setpts=PTS-STARTPTS[ref];[dist][ref]ssim",
            "-f",
            "null",
            "-",
        ]
    )
    return parse_ssim(proc.stdout + "\n" + proc.stderr)


def parse_vmaf_json(vmaf_json_path: Path) -> float | None:
    if not vmaf_json_path.exists():
        return None
    data = json.loads(vmaf_json_path.read_text())
    pooled = data.get("pooled_metrics", {})
    vmaf = pooled.get("vmaf", {})
    mean = vmaf.get("mean")
    if mean is not None:
        return safe_float(str(mean))

    frames = data.get("frames", [])
    vals: list[float] = []
    for frame in frames:
        metrics = frame.get("metrics", {})
        val = safe_float(str(metrics.get("vmaf")))
        if val is not None and math.isfinite(val):
            vals.append(val)
    if not vals:
        return None
    return sum(vals) / len(vals)


def run_vmaf(
    ffmpeg_bin: str,
    decoded_y4m: Path,
    ref_y4m: Path,
    vmaf_json_path: Path,
    model_path: Path | None,
    threads: int,
    subsample: int,
) -> float | None:
    vmaf_json_path.parent.mkdir(parents=True, exist_ok=True)

    opts = [
        f"log_fmt=json",
        f"log_path={vmaf_json_path}",
        f"n_threads={threads}",
        f"n_subsample={subsample}",
    ]
    if model_path:
        opts.append(f"model=path={model_path}")

    filter_expr = (
        "[0:v]setpts=PTS-STARTPTS[dist];"
        "[1:v]setpts=PTS-STARTPTS[ref];"
        f"[dist][ref]libvmaf={':'.join(opts)}"
    )

    run_cmd(
        [
            ffmpeg_bin,
            "-hide_banner",
            "-nostats",
            "-i",
            str(decoded_y4m),
            "-i",
            str(ref_y4m),
            "-lavfi",
            filter_expr,
            "-f",
            "null",
            "-",
        ]
    )
    return parse_vmaf_json(vmaf_json_path)


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    if not rows:
        path.write_text("")
        return
    all_keys: set[str] = set()
    for r in rows:
        all_keys.update(r.keys())
    keys = sorted(all_keys)
    with path.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=keys)
        writer.writeheader()
        for r in rows:
            writer.writerow(r)


def row_key(row: dict[str, Any]) -> tuple[str, int]:
    return (str(row["clip_name"]), int(row["q"]))


def linear_interp(xs: list[float], ys: list[float], x: float) -> float | None:
    if len(xs) < 2:
        return None
    if x < xs[0] or x > xs[-1]:
        return None
    if x == xs[0]:
        return ys[0]
    if x == xs[-1]:
        return ys[-1]
    for i in range(1, len(xs)):
        if x <= xs[i]:
            x0, x1 = xs[i - 1], xs[i]
            y0, y1 = ys[i - 1], ys[i]
            if x1 == x0:
                return y0
            t = (x - x0) / (x1 - x0)
            return y0 + t * (y1 - y0)
    return None


def bd_rate_percent(
    anchor_rates: list[float],
    anchor_quality: list[float],
    test_rates: list[float],
    test_quality: list[float],
    samples: int = 200,
) -> float | None:
    if len(anchor_rates) < 2 or len(test_rates) < 2:
        return None

    a_pairs = sorted(zip(anchor_quality, anchor_rates), key=lambda x: x[0])
    t_pairs = sorted(zip(test_quality, test_rates), key=lambda x: x[0])

    a_q = [p[0] for p in a_pairs]
    a_lr = [math.log(p[1]) for p in a_pairs]
    t_q = [p[0] for p in t_pairs]
    t_lr = [math.log(p[1]) for p in t_pairs]

    q_min = max(min(a_q), min(t_q))
    q_max = min(max(a_q), max(t_q))
    if q_max <= q_min:
        return None

    diffs: list[float] = []
    for i in range(samples):
        q = q_min + (q_max - q_min) * (i / (samples - 1))
        a = linear_interp(a_q, a_lr, q)
        b = linear_interp(t_q, t_lr, q)
        if a is None or b is None:
            continue
        diffs.append(b - a)
    if not diffs:
        return None

    avg = sum(diffs) / len(diffs)
    return (math.exp(avg) - 1.0) * 100.0


def bd_quality_delta(
    anchor_rates: list[float],
    anchor_quality: list[float],
    test_rates: list[float],
    test_quality: list[float],
    samples: int = 200,
) -> float | None:
    if len(anchor_rates) < 2 or len(test_rates) < 2:
        return None

    a_pairs = sorted((math.log(r), q) for r, q in zip(anchor_rates, anchor_quality))
    t_pairs = sorted((math.log(r), q) for r, q in zip(test_rates, test_quality))

    a_lr = [p[0] for p in a_pairs]
    a_q = [p[1] for p in a_pairs]
    t_lr = [p[0] for p in t_pairs]
    t_q = [p[1] for p in t_pairs]

    lr_min = max(min(a_lr), min(t_lr))
    lr_max = min(max(a_lr), max(t_lr))
    if lr_max <= lr_min:
        return None

    deltas: list[float] = []
    for i in range(samples):
        lr = lr_min + (lr_max - lr_min) * (i / (samples - 1))
        qa = linear_interp(a_lr, a_q, lr)
        qb = linear_interp(t_lr, t_q, lr)
        if qa is None or qb is None:
            continue
        deltas.append(qb - qa)
    if not deltas:
        return None

    return sum(deltas) / len(deltas)


def cmd_generate_clips(args: argparse.Namespace) -> int:
    out_dir = Path(args.out_dir).expanduser().resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    ffmpeg_bin = args.ffmpeg
    width, height, fps, duration = args.width, args.height, args.fps, args.duration

    specs = [
        ("testsrc2", f"testsrc2=size={width}x{height}:rate={fps}"),
        ("mandelbrot", f"mandelbrot=size={width}x{height}:rate={fps}"),
        ("cellauto", f"cellauto=s={width}x{height}:rate={fps}"),
        ("life", f"life=s={width}x{height}:rate={fps}:seed=1"),
        ("smptehdbars", f"smptehdbars=size={width}x{height}:rate={fps}"),
    ]

    for name, src in specs:
        clip_path = out_dir / f"{name}_{width}x{height}_{fps}fps_{duration}s.y4m"
        log(f"Generating {clip_path.name}")
        run_cmd(
            [
                ffmpeg_bin,
                "-hide_banner",
                "-y",
                "-f",
                "lavfi",
                "-i",
                src,
                "-t",
                str(duration),
                "-pix_fmt",
                "yuv420p",
                str(clip_path),
            ]
        )

    log(f"Generated {len(specs)} clips at {out_dir}")
    return 0


def cmd_run(args: argparse.Namespace) -> int:
    clips_dir = Path(args.clips_dir).expanduser().resolve()
    out_root = Path(args.out_dir).expanduser().resolve()
    out_dir = out_root / args.tag
    out_dir.mkdir(parents=True, exist_ok=True)

    ivf_dir = out_dir / "ivf"
    dec_dir = out_dir / "decoded"
    logs_dir = out_dir / "logs"
    for d in [ivf_dir, dec_dir, logs_dir]:
        d.mkdir(parents=True, exist_ok=True)

    ffmpeg_bin = args.ffmpeg
    ffprobe_bin = args.ffprobe
    encoder_bin = find_encoder_bin(args.encoder_bin, args.build)
    dav1d = Path(args.dav1d).expanduser().resolve() if args.dav1d else find_default_dav1d()
    if not dav1d:
        raise FileNotFoundError("dav1d not found. Pass --dav1d or set DAV1D.")
    if not dav1d.exists():
        raise FileNotFoundError(f"dav1d binary not found: {dav1d}")

    model_path: Path | None = None
    if args.enable_vmaf:
        if args.vmaf_model:
            model_path = Path(args.vmaf_model).expanduser().resolve()
            if not model_path.exists():
                raise FileNotFoundError(f"VMAF model not found: {model_path}")
        else:
            model_path = find_default_vmaf_model()
            if model_path is None:
                raise FileNotFoundError(
                    "VMAF model not found. Pass --vmaf-model or disable with --no-vmaf."
                )

    q_values = parse_q_list(args.q_values)
    clips = list_y4m_clips(clips_dir)
    if args.max_clips > 0:
        clips = clips[: args.max_clips]

    ffmpeg_ver = run_cmd([ffmpeg_bin, "-version"], check=False).stdout.splitlines()
    dav1d_ver = run_cmd([str(dav1d), "--version"], check=False).stdout.strip()
    git_head = run_cmd(["git", "rev-parse", "HEAD"], cwd=REPO_ROOT, check=False).stdout.strip()
    git_status = run_cmd(["git", "status", "--porcelain"], cwd=REPO_ROOT, check=False).stdout.strip()

    rows: list[dict[str, Any]] = []
    total_jobs = len(clips) * len(q_values)
    job_index = 0

    for clip in clips:
        duration_s = ffprobe_duration_seconds(ffprobe_bin, clip)
        for q in q_values:
            job_index += 1
            clip_stem = clip.stem
            base_name = f"{clip_stem}_q{q}"
            ivf_path = ivf_dir / f"{base_name}.ivf"
            decoded_path = dec_dir / f"{base_name}.y4m"
            vmaf_log = logs_dir / f"{base_name}.vmaf.json"

            log(f"[{job_index}/{total_jobs}] {clip.name} q={q}")
            row: dict[str, Any] = {
                "clip_name": clip.name,
                "clip_path": str(clip),
                "q": q,
                "duration_s": duration_s,
                "status": "ok",
            }

            try:
                t0 = time.perf_counter()
                enc = run_cmd(
                    [
                        str(encoder_bin),
                        str(clip),
                        "-o",
                        str(ivf_path),
                        "-q",
                        str(q),
                    ]
                )
                t1 = time.perf_counter()
                row["encode_sec"] = t1 - t0
                row["encoder_stderr"] = enc.stderr.strip()
                tool_usage = parse_tool_usage_from_stderr(enc.stderr)
                if tool_usage:
                    row.update(tool_usage)
                row["ivf_path"] = str(ivf_path)
                row["ivf_size_bytes"] = ivf_path.stat().st_size
                row["bitrate_kbps"] = (row["ivf_size_bytes"] * 8.0) / (duration_s * 1000.0)

                t2 = time.perf_counter()
                run_cmd([str(dav1d), "-i", str(ivf_path), "-o", str(decoded_path)])
                t3 = time.perf_counter()
                row["decode_sec"] = t3 - t2
                row["decoded_path"] = str(decoded_path)

                t4 = time.perf_counter()
                row.update(run_psnr(ffmpeg_bin, decoded_path, clip))
                row.update(run_ssim(ffmpeg_bin, decoded_path, clip))
                if args.enable_vmaf:
                    row["vmaf"] = run_vmaf(
                        ffmpeg_bin,
                        decoded_path,
                        clip,
                        vmaf_log,
                        model_path,
                        args.vmaf_threads,
                        args.vmaf_subsample,
                    )
                else:
                    row["vmaf"] = None
                t5 = time.perf_counter()
                row["metric_sec"] = t5 - t4
            except Exception as e:  # noqa: BLE001
                row["status"] = "error"
                row["error"] = str(e)
                log(f"ERROR for {clip.name} q={q}: {e}")
                if not args.continue_on_error:
                    rows.append(row)
                    write_csv(out_dir / "results.csv", rows)
                    (out_dir / "results.json").write_text(
                        json.dumps(
                            {
                                "metadata": {
                                    "tag": args.tag,
                                    "created_at_utc": now_iso(),
                                    "git_head": git_head,
                                    "git_dirty": bool(git_status),
                                },
                                "rows": rows,
                            },
                            indent=2,
                        )
                    )
                    return 1

            rows.append(row)

    summary_by_q: dict[int, dict[str, float]] = {}
    for q in q_values:
        subset = [r for r in rows if r.get("q") == q and r.get("status") == "ok"]
        if not subset:
            continue

        def avg(key: str) -> float | None:
            vals = [safe_float(str(r.get(key))) for r in subset]
            vals = [v for v in vals if v is not None and math.isfinite(v)]
            if not vals:
                return None
            return float(sum(vals) / len(vals))

        summary_by_q[q] = {
            "avg_bitrate_kbps": avg("bitrate_kbps"),
            "avg_psnr_avg": avg("psnr_avg"),
            "avg_ssim_all": avg("ssim_all"),
            "avg_vmaf": avg("vmaf"),
        }

    payload = {
        "metadata": {
            "tag": args.tag,
            "created_at_utc": now_iso(),
            "repo_root": str(REPO_ROOT),
            "git_head": git_head,
            "git_dirty": bool(git_status),
            "encoder_bin": str(encoder_bin),
            "dav1d": str(dav1d),
            "dav1d_version": dav1d_ver,
            "ffmpeg": ffmpeg_bin,
            "ffmpeg_version_line": ffmpeg_ver[0] if ffmpeg_ver else "",
            "vmaf_model": str(model_path) if model_path else None,
            "q_values": q_values,
            "clips_dir": str(clips_dir),
        },
        "rows": rows,
        "summary_by_q": summary_by_q,
    }

    (out_dir / "results.json").write_text(json.dumps(payload, indent=2))
    write_csv(out_dir / "results.csv", rows)
    (out_dir / "summary.json").write_text(json.dumps(summary_by_q, indent=2))

    ok_count = sum(1 for r in rows if r.get("status") == "ok")
    err_count = len(rows) - ok_count
    log(f"Run complete: {ok_count} ok, {err_count} errors")
    log(f"Results written to: {out_dir}")
    return 0 if err_count == 0 else 2


def load_results(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text())
    if "rows" not in data:
        raise ValueError(f"Invalid results json (missing rows): {path}")
    return data


def resolve_required_tool_fields(spec: str) -> list[str]:
    if not spec.strip():
        return []
    alias = {
        "uv_non_dc_blocks": "tool_uv_non_dc_blocks",
        "tool_uv_non_dc_blocks": "tool_uv_non_dc_blocks",
        "inter_newmv_blocks": "tool_inter_newmv_blocks",
        "tool_inter_newmv_blocks": "tool_inter_newmv_blocks",
        "restoration_non_none_units": "tool_restoration_non_none_units",
        "tool_restoration_non_none_units": "tool_restoration_non_none_units",
        "seg1_blocks": "tool_seg1_blocks",
        "tool_seg1_blocks": "tool_seg1_blocks",
    }
    out: list[str] = []
    for raw in spec.split(","):
        key = raw.strip()
        if not key:
            continue
        if key not in alias:
            valid = ", ".join(sorted(alias.keys()))
            raise ValueError(f"Unknown tool usage key '{key}'. Valid values: {valid}")
        canon = alias[key]
        if canon not in out:
            out.append(canon)
    return out


def choose_sanity_key(
    keys: list[tuple[str, int]],
    clip_name: str | None,
    q_value: int | None,
) -> tuple[str, int]:
    if clip_name is not None and q_value is not None:
        key = (clip_name, q_value)
        if key not in keys:
            raise RuntimeError(f"Sanity point {key} is not present in overlapping anchor/test points.")
        return key

    if clip_name is not None:
        candidates = [k for k in keys if k[0] == clip_name]
        if not candidates:
            raise RuntimeError(f"No overlapping points for sanity clip '{clip_name}'.")
        return sorted(candidates, key=lambda x: x[1])[0]

    if q_value is not None:
        candidates = [k for k in keys if k[1] == q_value]
        if not candidates:
            raise RuntimeError(f"No overlapping points for sanity q={q_value}.")
        return sorted(candidates, key=lambda x: x[0])[0]

    return keys[0]


def cmd_compare(args: argparse.Namespace) -> int:
    anchor_path = Path(args.anchor).expanduser().resolve()
    test_path = Path(args.test).expanduser().resolve()
    out_path = Path(args.out).expanduser().resolve()
    out_path.parent.mkdir(parents=True, exist_ok=True)

    anchor = load_results(anchor_path)
    test = load_results(test_path)
    anchor_rows = [r for r in anchor["rows"] if r.get("status") == "ok"]
    test_rows = [r for r in test["rows"] if r.get("status") == "ok"]

    a_map = {row_key(r): r for r in anchor_rows}
    t_map = {row_key(r): r for r in test_rows}
    keys = sorted(set(a_map.keys()) & set(t_map.keys()))
    if not keys:
        raise RuntimeError("No overlapping (clip, q) points between anchor and test runs.")

    required_tool_fields = resolve_required_tool_fields(args.require_tool_usage)
    tool_usage_totals_test: dict[str, int] = {}
    for field in required_tool_fields:
        total = 0
        for row in test_rows:
            raw = row.get(field)
            if raw is None:
                continue
            value = safe_float(str(raw))
            if value is None or not math.isfinite(value):
                continue
            total += int(round(value))
        tool_usage_totals_test[field] = total
    missing_tool_usage = [f for f, total in tool_usage_totals_test.items() if total <= 0]
    if missing_tool_usage:
        missing_str = ", ".join(missing_tool_usage)
        raise RuntimeError(f"Required tool usage was zero in test run: {missing_str}")

    sanity_check: dict[str, Any] | None = None
    if args.require_diff:
        sanity_key = choose_sanity_key(keys, args.sanity_clip, args.sanity_q)
        a_row = a_map[sanity_key]
        t_row = t_map[sanity_key]

        anchor_ivf = Path(str(a_row.get("ivf_path", ""))).expanduser()
        test_ivf = Path(str(t_row.get("ivf_path", ""))).expanduser()
        if not anchor_ivf.exists():
            raise FileNotFoundError(f"Anchor ivf_path not found for sanity point {sanity_key}: {anchor_ivf}")
        if not test_ivf.exists():
            raise FileNotFoundError(f"Test ivf_path not found for sanity point {sanity_key}: {test_ivf}")

        if filecmp.cmp(anchor_ivf, test_ivf, shallow=False):
            raise RuntimeError(
                f"Sanity A/B failed for {sanity_key}: candidate IVF is byte-identical to anchor."
            )

        dav1d = Path(args.dav1d).expanduser().resolve() if args.dav1d else find_default_dav1d()
        if dav1d is None or not dav1d.exists():
            raise FileNotFoundError(
                "Sanity decode requested but dav1d was not found. Pass --dav1d or set DAV1D."
            )
        with tempfile.TemporaryDirectory(prefix="wav1c_sanity_") as tmp:
            sanity_out = Path(tmp) / "sanity_decode.y4m"
            run_cmd([str(dav1d), "-i", str(test_ivf), "-o", str(sanity_out)])

        sanity_check = {
            "point": {"clip_name": sanity_key[0], "q": sanity_key[1]},
            "anchor_ivf_path": str(anchor_ivf),
            "test_ivf_path": str(test_ivf),
            "ivf_different": True,
            "dav1d": str(dav1d),
            "dav1d_decode_ok": True,
        }

    deltas: list[dict[str, Any]] = []
    for k in keys:
        a = a_map[k]
        t = t_map[k]
        row = {
            "clip_name": k[0],
            "q": k[1],
            "anchor_bitrate_kbps": a.get("bitrate_kbps"),
            "test_bitrate_kbps": t.get("bitrate_kbps"),
            "delta_bitrate_kbps": None,
            "delta_psnr_avg": None,
            "delta_ssim_all": None,
            "delta_vmaf": None,
        }
        for metric in ["bitrate_kbps", "psnr_avg", "ssim_all", "vmaf"]:
            av = safe_float(str(a.get(metric)))
            tv = safe_float(str(t.get(metric)))
            if av is not None and tv is not None and math.isfinite(av) and math.isfinite(tv):
                row[f"delta_{metric}"] = tv - av
        deltas.append(row)

    by_q: dict[int, dict[str, float | None]] = {}
    for q in sorted({int(d["q"]) for d in deltas}):
        subset = [d for d in deltas if int(d["q"]) == q]
        by_q[q] = {}
        for metric in ["delta_bitrate_kbps", "delta_psnr_avg", "delta_ssim_all", "delta_vmaf"]:
            vals = [safe_float(str(r.get(metric))) for r in subset]
            vals = [v for v in vals if v is not None and math.isfinite(v)]
            by_q[q][metric] = (sum(vals) / len(vals)) if vals else None

    overall: dict[str, float | None] = {}
    for metric in ["delta_bitrate_kbps", "delta_psnr_avg", "delta_ssim_all", "delta_vmaf"]:
        vals = [safe_float(str(r.get(metric))) for r in deltas]
        vals = [v for v in vals if v is not None and math.isfinite(v)]
        overall[metric] = (sum(vals) / len(vals)) if vals else None

    bd_psnr: dict[str, float | None] = {}
    bd_vmaf: dict[str, float | None] = {}
    clips = sorted({k[0] for k in keys})
    for clip in clips:
        a_pts = [a_map[k] for k in keys if k[0] == clip]
        t_pts = [t_map[k] for k in keys if k[0] == clip]

        def points(rows: list[dict[str, Any]], metric: str) -> tuple[list[float], list[float]]:
            rates: list[float] = []
            quals: list[float] = []
            for r in rows:
                rate = safe_float(str(r.get("bitrate_kbps")))
                qual = safe_float(str(r.get(metric)))
                if rate is None or qual is None:
                    continue
                if not (math.isfinite(rate) and math.isfinite(qual)):
                    continue
                if rate <= 0:
                    continue
                rates.append(rate)
                quals.append(qual)
            return rates, quals

        a_r_psnr, a_q_psnr = points(a_pts, "psnr_avg")
        t_r_psnr, t_q_psnr = points(t_pts, "psnr_avg")
        bd_psnr[clip] = bd_rate_percent(a_r_psnr, a_q_psnr, t_r_psnr, t_q_psnr)

        a_r_vmaf, a_q_vmaf = points(a_pts, "vmaf")
        t_r_vmaf, t_q_vmaf = points(t_pts, "vmaf")
        bd_vmaf[clip] = bd_quality_delta(a_r_vmaf, a_q_vmaf, t_r_vmaf, t_q_vmaf)

    avg_bd_rate_psnr = None
    vals_bd = [v for v in bd_psnr.values() if v is not None and math.isfinite(v)]
    if vals_bd:
        avg_bd_rate_psnr = sum(vals_bd) / len(vals_bd)

    avg_bd_vmaf = None
    vals_bdv = [v for v in bd_vmaf.values() if v is not None and math.isfinite(v)]
    if vals_bdv:
        avg_bd_vmaf = sum(vals_bdv) / len(vals_bdv)

    payload = {
        "metadata": {
            "created_at_utc": now_iso(),
            "anchor": str(anchor_path),
            "test": str(test_path),
            "anchor_tag": anchor.get("metadata", {}).get("tag"),
            "test_tag": test.get("metadata", {}).get("tag"),
            "required_tool_usage_fields": required_tool_fields,
        },
        "overall_avg_deltas": overall,
        "per_q_avg_deltas": by_q,
        "per_point_deltas": deltas,
        "bd_rate_psnr_percent_per_clip": bd_psnr,
        "bd_vmaf_per_clip": bd_vmaf,
        "avg_bd_rate_psnr_percent": avg_bd_rate_psnr,
        "avg_bd_vmaf": avg_bd_vmaf,
        "tool_usage_totals_test": tool_usage_totals_test,
        "sanity_check": sanity_check,
    }

    out_path.write_text(json.dumps(payload, indent=2))
    write_csv(out_path.with_suffix(".csv"), deltas)

    print(json.dumps(payload["overall_avg_deltas"], indent=2))
    print(f"avg_bd_rate_psnr_percent={avg_bd_rate_psnr}")
    print(f"avg_bd_vmaf={avg_bd_vmaf}")
    print(f"wrote {out_path}")

    if args.fail_on_regression:
        psnr_reg = overall.get("delta_psnr_avg")
        vmaf_reg = overall.get("delta_vmaf")
        bd_reg = avg_bd_rate_psnr
        failed = False
        if psnr_reg is not None and psnr_reg < 0:
            log(f"Regression: delta_psnr_avg={psnr_reg:.4f} < 0")
            failed = True
        if vmaf_reg is not None and vmaf_reg < 0:
            log(f"Regression: delta_vmaf={vmaf_reg:.4f} < 0")
            failed = True
        if bd_reg is not None and bd_reg > 0:
            log(f"Regression: avg_bd_rate_psnr_percent={bd_reg:.4f} > 0")
            failed = True
        if failed:
            return 3

    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Quality pipeline for wav1c (encode/decode/metrics/compare)."
    )
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_gen = sub.add_parser("generate-clips", help="Generate deterministic synthetic Y4M clips.")
    p_gen.add_argument("--out-dir", required=True, help="Directory to write generated .y4m clips.")
    p_gen.add_argument("--width", type=int, default=320)
    p_gen.add_argument("--height", type=int, default=240)
    p_gen.add_argument("--fps", type=int, default=25)
    p_gen.add_argument("--duration", type=int, default=4, help="Clip duration in seconds.")
    p_gen.add_argument("--ffmpeg", default="ffmpeg")
    p_gen.set_defaults(func=cmd_generate_clips)

    p_prep = sub.add_parser(
        "prepare-real",
        help="Prepare real-content clips from a manifest (local paths and/or downloadable URLs).",
    )
    p_prep.add_argument(
        "--manifest",
        default=str(DEFAULT_REAL_CONTENT_MANIFEST),
        help="JSON manifest with clip sources and conversion metadata.",
    )
    p_prep.add_argument(
        "--out-dir",
        required=True,
        help="Directory to place normalized .y4m clips (used directly by the run command).",
    )
    p_prep.add_argument(
        "--cache-dir",
        default=None,
        help="Directory for downloaded source files (defaults to <out-dir>/_downloads).",
    )
    p_prep.add_argument("--ffmpeg", default="ffmpeg")
    p_prep.add_argument("--ffprobe", default="ffprobe")
    p_prep.add_argument("--max-clips", type=int, default=0, help="Limit number of manifest entries.")
    p_prep.add_argument("--force-download", action="store_true", help="Redownload URL sources.")
    p_prep.add_argument("--force-convert", action="store_true", help="Rebuild output .y4m clips.")
    p_prep.set_defaults(func=cmd_prepare_real)

    p_run = sub.add_parser("run", help="Run encode/decode/metrics for a set of clips and Q values.")
    p_run.add_argument("--clips-dir", required=True, help="Directory containing source .y4m clips.")
    p_run.add_argument("--out-dir", required=True, help="Root output directory.")
    p_run.add_argument("--tag", required=True, help="Run tag, used as output subdirectory name.")
    p_run.add_argument("--q-values", default="64,96,128,160,192,224", help="Comma-separated q values.")
    p_run.add_argument("--max-clips", type=int, default=0, help="Limit number of clips (0 = all).")
    p_run.add_argument("--encoder-bin", default=None, help="Path to wav1c CLI binary.")
    p_run.add_argument("--build", action="store_true", help="Build wav1c-cli if binary is missing.")
    p_run.add_argument("--dav1d", default=None, help="Path to dav1d binary.")
    p_run.add_argument("--ffmpeg", default="ffmpeg")
    p_run.add_argument("--ffprobe", default="ffprobe")
    p_run.add_argument("--enable-vmaf", action="store_true", default=True)
    p_run.add_argument("--no-vmaf", dest="enable_vmaf", action="store_false")
    p_run.add_argument("--vmaf-model", default=None, help="Path to VMAF model json.")
    p_run.add_argument("--vmaf-threads", type=int, default=0)
    p_run.add_argument("--vmaf-subsample", type=int, default=1)
    p_run.add_argument("--continue-on-error", action="store_true")
    p_run.set_defaults(func=cmd_run)

    p_cmp = sub.add_parser("compare", help="Compare two run result json files.")
    p_cmp.add_argument("--anchor", required=True, help="Baseline results.json")
    p_cmp.add_argument("--test", required=True, help="Candidate results.json")
    p_cmp.add_argument("--out", required=True, help="Output compare json path.")
    p_cmp.add_argument(
        "--require-tool-usage",
        default="",
        help=(
            "Comma-separated tool usage counters that must be >0 in test rows. "
            "Aliases: uv_non_dc_blocks, inter_newmv_blocks, restoration_non_none_units, seg1_blocks."
        ),
    )
    p_cmp.add_argument(
        "--require-diff",
        action="store_true",
        help="Fail if a selected sanity point IVF is byte-identical between anchor and test.",
    )
    p_cmp.add_argument("--sanity-clip", default=None, help="Clip name for --require-diff sanity point.")
    p_cmp.add_argument("--sanity-q", type=int, default=None, help="Q for --require-diff sanity point.")
    p_cmp.add_argument("--dav1d", default=None, help="Path to dav1d binary for --require-diff decode check.")
    p_cmp.add_argument("--fail-on-regression", action="store_true")
    p_cmp.set_defaults(func=cmd_compare)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
