# wav1c - Wondrous AV1 Encoder

[![crates.io](https://img.shields.io/crates/v/wav1c.svg)](https://crates.io/crates/wav1c)
[![license](https://img.shields.io/crates/l/wav1c.svg)](LICENSE)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

A spec-compliant AV1 encoder written from scratch in safe Rust with zero runtime dependencies.

`wav1c` can be used as:
- A Rust library (`wav1c` crate)
- A CLI encoder (`wav1c-cli`)
- A C ABI library (`wav1c-ffi`)
- A WebAssembly module (`wav1c-wasm`)
- An FFmpeg external encoder (`libwav1c`)

## Feature Summary

- AV1 4:2:0 encoding for SDR and HDR workflows
- Bit depths: 8-bit and 10-bit
- HDR signaling:
  - Sequence-header color signaling (range + color description)
  - HDR metadata OBUs:
    - CLL (`max_cll`, `max_fall`)
    - MDCV (mastering display metadata)
  - AVIF compatibility signaling:
    - AVIF item properties: `clli` (+ optional `mdcv`)
    - Single-frame AVIF sequence headers set `still_picture=1`
- Y4M parsing:
  - `C420*` 8-bit and `C420p10`
  - `XCOLORRANGE=FULL|LIMITED` in stream and `FRAME` headers
  - Typed parse errors for malformed/truncated input
- Intra + inter coding pipeline with RD decisions, transforms, and entropy coding
- B-frame pipeline support
- Large-dimension support in core encoder via AV1 multi-tile payload assembly (memory permitting)

Current scope limits:
- Chroma format: 4:2:0 only
- Bit depth: 8/10-bit only (no 12-bit)
- Container limits in CLI:
  - IVF/MP4: `width <= 65535`, `height <= 65535`
  - AVIF: large dimensions supported

## Workspace Layout

```text
wav1c/          Core library (Rust API, bitstream generation)
wav1c-cli/      Command-line encoder
wav1c-ffi/      C FFI shared/static library
wav1c-wasm/     WebAssembly bindings (wasm-bindgen)
docs/           Project documentation (including HDR + HEIC guide)
```

## Build

```bash
cargo build --workspace --release
```

## CLI Usage

All examples below use the workspace binary:

```bash
cargo run -q -p wav1c-cli -- <args...>
```

Basic SDR encode from Y4M:

```bash
cargo run -q -p wav1c-cli -- input.y4m -o output.ivf
```

Solid-color frame encode (5 positional args: `W H Y U V`):

```bash
cargo run -q -p wav1c-cli -- 320 240 128 128 128 -o gray.ivf
```

10-bit HDR10 encode from Y4M:

```bash
cargo run -q -p wav1c-cli -- input_10bit.y4m -o output_hdr.ivf \
  --bit-depth 10 \
  --hdr10 \
  --color-range full \
  --max-cll 203 \
  --max-fall 64
```

Custom color description + MDCV:

```bash
cargo run -q -p wav1c-cli -- input_10bit.y4m -o output_hdr.ivf \
  --bit-depth 10 \
  --color-range limited \
  --color-primaries 9 \
  --transfer 16 \
  --matrix 9 \
  --mdcv 34000,16000,13250,34500,7500,3000,15635,16450,10000000,1
```

CLI HDR flags:
- `--bit-depth <8|10>`
- `--hdr10`
- `--color-range <limited|full>`
- `--color-primaries <u8>`
- `--transfer <u8>`
- `--matrix <u8>`
- `--max-cll <u16>` and `--max-fall <u16>` (must be provided together)
- `--mdcv <rx,ry,gx,gy,bx,by,wx,wy,max_lum,min_lum>`

Notes:
- When input is Y4M and `--bit-depth` or `--color-range` are omitted, values are inferred from Y4M headers.
- `--hdr10` applies default color description (`primaries=9`, `transfer=16`, `matrix=9`).

## Rust API

### Simple 8-bit SDR frame (solid color)

```rust
use wav1c::y4m::FramePixels;
use wav1c::{encode_packets, EncodeConfig};

let frame = FramePixels::solid(320, 240, 128, 128, 128);
let packets = encode_packets(&[frame], &EncodeConfig::default());

assert!(!packets.is_empty());
assert!(!packets[0].data.is_empty());
```

### Simple Y4M file encode (8-bit or 10-bit input)

```rust
use std::path::Path;
use wav1c::y4m::FramePixels;
use wav1c::{encode_packets, EncodeConfig, Fps};

let frames = FramePixels::all_from_y4m_file(Path::new("input.y4m"))?;
let config = EncodeConfig {
    base_q_idx: 110,
    keyint: 60,
    fps: Fps::from_int(30).unwrap(),
    b_frames: true,
    gop_size: 4,
    ..EncodeConfig::default()
};

let packets = encode_packets(&frames, &config);
assert!(!packets.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### 10-bit HDR encode (signal + metadata)

```rust
use wav1c::y4m::FramePixels;
use wav1c::{
    encode_packets, BitDepth, ColorRange, ContentLightLevel, EncodeConfig, Fps,
    MasteringDisplayMetadata, VideoSignal,
};

let frame = FramePixels::solid_with_bit_depth(
    1920,
    1080,
    512,
    512,
    512,
    BitDepth::Ten,
    ColorRange::Full,
);

let config = EncodeConfig {
    keyint: 60,
    fps: Fps::from_int(30).unwrap(),
    b_frames: true,
    gop_size: 4,
    video_signal: VideoSignal::hdr10(ColorRange::Full),
    content_light: Some(ContentLightLevel {
        max_content_light_level: 203,
        max_frame_average_light_level: 64,
    }),
    mastering_display: Some(MasteringDisplayMetadata {
        primaries: [[34000, 16000], [13250, 34500], [7500, 3000]],
        white_point: [15635, 16450],
        max_luminance: 10_000_000,
        min_luminance: 1,
    }),
    ..EncodeConfig::default()
};

let packets = encode_packets(&[frame], &config);
assert!(!packets.is_empty());
```

### Key signal and metadata types

Exported from the crate root:
- `BitDepth`
- `ColorRange`
- `ColorDescription`
- `Fps`
- `VideoSignal`
- `ContentLightLevel`
- `MasteringDisplayMetadata`

## C FFI API (`wav1c-ffi`)

Header: `wav1c-ffi/include/wav1c.h`

Canonical API:
- `wav1c_default_config()`
- `wav1c_encoder_new(...)`
- `wav1c_encoder_send_frame(...)` (8-bit planes)
- `wav1c_encoder_send_frame_u16(...)` (10-bit planes)
- `wav1c_encoder_rate_control_stats(...)`
- `wav1c_last_error_message()`

`Wav1cConfig` fields:
- `fps_num`, `fps_den`: frame rate rational (`num/den`)
- `bit_depth`: `8` or `10`
- `color_range`: `0` limited, `1` full
- `color_primaries`, `transfer_characteristics`, `matrix_coefficients`: set to `-1` to omit color description
- `has_cll`, `max_cll`, `max_fall`
- `has_mdcv`, primaries/white-point/luminance fields

Simple 8-bit SDR usage:

```c
#include <stdio.h>
#include "wav1c.h"

Wav1cConfig cfg = wav1c_default_config();
cfg.base_q_idx = 128;
cfg.keyint = 60;
cfg.fps_num = 30;
cfg.fps_den = 1;
cfg.b_frames = 1;
cfg.gop_size = 4;

Wav1cEncoder *enc = wav1c_encoder_new(320, 240, &cfg);
if (!enc) {
    fprintf(stderr, "encoder_new failed: %s\n", wav1c_last_error_message());
    return -1;
}

// y/u/v are 4:2:0 8-bit planes. Lengths are sample counts, not byte counts.
if (wav1c_encoder_send_frame(enc, y, y_len, u, u_len, v, v_len, 0, 0) != WAV1C_STATUS_OK) {
    fprintf(stderr, "send_frame failed: %s\n", wav1c_last_error_message());
    wav1c_encoder_free(enc);
    return -1;
}

wav1c_encoder_flush(enc);
for (Wav1cPacket *pkt = NULL; (pkt = wav1c_encoder_receive_packet(enc)) != NULL; ) {
    // pkt->data / pkt->size contains AV1 OBU payload
    wav1c_packet_free(pkt);
}

wav1c_encoder_free(enc);
```

Simple rate-control stats query:

```c
#include <stdio.h>
#include "wav1c.h"

Wav1cConfig cfg = wav1c_default_config();
cfg.target_bitrate = 2 * 1000 * 1000; // 2 Mbps

Wav1cEncoder *enc = wav1c_encoder_new(1280, 720, &cfg);
if (!enc) return -1;

Wav1cRateControlStats stats;
if (wav1c_encoder_rate_control_stats(enc, &stats) == WAV1C_STATUS_OK) {
    printf("target=%llu, avg_qp=%u, buffer=%u%%\n",
           (unsigned long long)stats.target_bitrate,
           stats.avg_qp,
           stats.buffer_fullness_pct);
}

wav1c_encoder_free(enc);
```

10-bit HDR usage:

```c
#include <stdio.h>
#include "wav1c.h"

Wav1cConfig cfg = wav1c_default_config();
cfg.keyint = 60;
cfg.fps_num = 30;
cfg.fps_den = 1;
cfg.b_frames = 1;
cfg.gop_size = 4;
cfg.bit_depth = 10;
cfg.color_range = 1; // full
cfg.color_primaries = 9;
cfg.transfer_characteristics = 16;
cfg.matrix_coefficients = 9;
cfg.has_cll = 1;
cfg.max_cll = 203;
cfg.max_fall = 64;

Wav1cEncoder *enc = wav1c_encoder_new(1920, 1080, &cfg);
if (!enc) {
    fprintf(stderr, "encoder_new failed: %s\n", wav1c_last_error_message());
    return -1;
}

// y_len/u_len/v_len are sample counts, not byte counts.
if (wav1c_encoder_send_frame_u16(
        enc, y, y_len, u, u_len, v, v_len, y_stride, uv_stride) != WAV1C_STATUS_OK) {
    fprintf(stderr, "send_frame_u16 failed: %s\n", wav1c_last_error_message());
    wav1c_encoder_free(enc);
    return -1;
}

wav1c_encoder_flush(enc);
Wav1cPacket *pkt = wav1c_encoder_receive_packet(enc);

if (pkt) wav1c_packet_free(pkt);
wav1c_encoder_free(enc);
```

Build artifacts:
- `libwav1c_ffi.dylib` / `libwav1c_ffi.so`
- `libwav1c_ffi.a`

## WebAssembly API (`wav1c-wasm`)

Main entry point: `WasmEncoder`

Constructor:
- `new(width, height, base_q_idx, keyint, b_frames, gop_size, fps_num, fps_den, target_bitrate, bit_depth, color_range, cp, tc, mc, has_cll, max_cll, max_fall)`

10-bit/HDR methods:
- `encode_frame_10bit(y, u, v)`
- `set_hdr10(color_range)`
- `set_video_signal(bit_depth, color_range, cp, tc, mc)`
- `set_content_light_level(max_cll, max_fall)`
- `set_mastering_display_metadata(...)`
- `rate_control_stats()`

Important: signal and metadata mutators must be called before the first submitted frame.

## FFmpeg Integration (`libwav1c`)

This repository contains `ffmpeg-libwav1c.patch`, and we also maintain direct FFmpeg integration updates in `../FFmpeg` during active development.

Build `wav1c-ffi` first:

```bash
cargo build -p wav1c-ffi --release
```

Configure FFmpeg with `libwav1c`:

```bash
cd ../FFmpeg
./configure --enable-libwav1c \
  --extra-cflags="-I/absolute/path/to/wav1c/wav1c-ffi/include" \
  --extra-ldflags="-L/absolute/path/to/wav1c/target/release"
make -j$(sysctl -n hw.ncpu 2>/dev/null || nproc)
```

Run with dynamic library path (macOS example):

```bash
DYLD_LIBRARY_PATH=/absolute/path/to/wav1c/target/release ./ffmpeg \
  -i input.y4m \
  -pix_fmt yuv420p10le \
  -c:v libwav1c \
  -wav1c-hdr10 1 \
  -wav1c-max-cll 203 \
  -wav1c-max-fall 64 \
  -f ivf output.ivf
```

Supported FFmpeg `libwav1c` pixel formats:
- `yuv420p`
- `yuv420p10le`

Supported FFmpeg private options:
- `-wav1c-q`
- `-wav1c-b-frames`
- `-wav1c-gop-size`
- `-wav1c-hdr10`
- `-wav1c-max-cll`
- `-wav1c-max-fall`
- `-wav1c-mdcv`

## HEIC to HDR Workflow

For a full HEIC -> HDR AV1 workflow (CLI and FFmpeg), including verification with `ffprobe` and `dav1d`, see:

- `docs/HDR_HEIC_MANUAL.md`

Practical note for HEIC inputs:
- Some HEIC files decode as tile grids.
- Do not force-map an individual dependent tile if you want the full composed image.
- For very large outputs, prefer AVIF when IVF/MP4 16-bit container dimensions would be exceeded.

## Testing

```bash
cargo test --workspace
```

Integration tests decode encoded output with [dav1d](https://code.videolan.org/videolan/dav1d). If `dav1d` is unavailable, tests that require it are skipped.

## License

This project is licensed under the Mozilla Public License 2.0. See [LICENSE](LICENSE).
