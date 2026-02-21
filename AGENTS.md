# AGENTS.md

This file defines expectations for humans and coding agents working in this repository.

## Project Goal

`wav1c` is a safe-Rust AV1 encoder that must produce bitstreams decodable by stock dav1d.

Primary deliverables:
- Correct AV1 bitstream generation
- Stable 8-bit SDR behavior (backward compatibility)
- End-to-end 10-bit HDR support across core, CLI, FFI, WASM, and FFmpeg integration

## Current Scope (Do Not Regress)

- Chroma: 4:2:0 only
- Bit depth: 8-bit and 10-bit
- Max dimensions: `1..=4096` width, `1..=2304` height
- Containers and I/O:
  - Input: Y4M (`C420*`, `C420p10`, `XCOLORRANGE`)
  - Output: IVF with AV1 OBUs
- Signal + metadata:
  - AV1 sequence header color signaling
  - Optional HDR metadata OBUs:
    - CLL (`max_cll`, `max_fall`)
    - MDCV (mastering display metadata)

Out of scope unless explicitly requested:
- 4:2:2 / 4:4:4
- 12-bit
- Tone-mapping or gamut conversion

## Authoritative References

- AV1 spec: `av1-spec.md`
- Decoder of record: `../dav1d/build/tools/dav1d` (or stock dav1d in `PATH`)
- Project architecture: `ARCHITECTURE.md`
- HDR workflow details: `docs/HDR_HEIC_MANUAL.md`

## Repository Layout

- `wav1c/` core encoder crate
- `wav1c-cli/` command-line app
- `wav1c-ffi/` C ABI surface
- `wav1c-wasm/` wasm-bindgen surface
- `docs/` implementation guides and plans

## Workflow Rules

1. Work autonomously and make the best technical decision without blocking on minor uncertainty.
2. Keep the build healthy:
   - `cargo test --workspace`
   - `cargo clippy --tests` (when touching Rust logic)
3. Treat tasks as incomplete until:
   - Code compiles
   - Relevant tests pass
   - Behavior is validated end-to-end when codec behavior changes
4. Add or update regression tests for behavior changes (especially bitstream-signaling changes).
5. Prefer clear naming and minimal complexity; keep implementation maintainable.

## Compatibility Rules

Preserve these invariants unless explicitly changing them:

1. Existing 8-bit APIs remain usable:
   - `encode_av1_ivf(...)`
   - `FramePixels::solid(...)`
   - legacy C FFI entrypoints
2. Default behavior remains SDR-safe unless 10-bit/HDR is explicitly configured.
3. New APIs should extend behavior, not break old callers.

## HDR-Specific Rules

1. HDR metadata is valid only with 10-bit signal.
2. CLL fields must be provided together.
3. Color description fields (`primaries`, `transfer`, `matrix`) should be treated as an all-or-none set.
4. Emit CLL/MDCV metadata OBUs only when configured.
5. Verify signaling with `ffprobe` whenever HDR behavior is changed.

## FFmpeg Integration Rules

Important project policy:
- Do not update `ffmpeg-libwav1c.patch` unless explicitly requested.
- Ongoing FFmpeg integration work is done directly in `../FFmpeg`.

When changing FFmpeg integration:
1. Keep `AV_PIX_FMT_YUV420P` path intact.
2. Validate `AV_PIX_FMT_YUV420P10LE` path when touched.
3. Verify encoder options map correctly to FFI config.

## Validation Checklist

Use this checklist for codec-affecting changes:

1. Unit and integration tests:
   - `cargo test --workspace`
2. Optional static checks:
   - `cargo clippy --tests`
3. End-to-end encode/decode sanity:
   - Encode with `wav1c-cli` or FFmpeg `libwav1c`
   - Decode with dav1d
4. HDR validation (if applicable):
   - `ffprobe -show_streams` confirms:
     - `pix_fmt=yuv420p10le`
     - expected `color_range`
     - expected `color_primaries`
     - expected `color_transfer`
     - expected `color_space`

## HEIC Validation Notes

For HEIC sources, avoid forcing a dependent tile stream if full-image composition is required.

- `-map 0:v:0` may select a tile (for some image-grid HEIC files).
- Use the composed decode path and resize when needed to satisfy wav1c limits (`<= 4096x2304`).

## Definition of Done

A change is done when all are true:

1. Implementation is complete and consistent across affected surfaces.
2. Tests and validation pass for the changed behavior.
3. Documentation is updated (`README.md` and/or docs under `docs/` as needed).
4. No unrelated files were modified or reverted accidentally.
