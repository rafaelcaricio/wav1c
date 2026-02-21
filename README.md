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
- Y4M parsing:
  - `C420*` 8-bit and `C420p10`
  - `XCOLORRANGE=FULL|LIMITED` in stream and `FRAME` headers
  - Typed parse errors for malformed/truncated input
- Intra + inter coding pipeline with RD decisions, transforms, and entropy coding
- B-frame pipeline support
- Stream dimensions: `1..=4096` width and `1..=2304` height

Current scope limits:
- Chroma format: 4:2:0 only
- Bit depth: 8/10-bit only (no 12-bit)

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

### Backward-compatible convenience APIs (8-bit)

The existing helpers are still available:
- `encode_av1_ivf(...)`
- `encode_av1_ivf_y4m(...)`
- `encode_av1_ivf_multi(...)`
- `FramePixels::solid(...)`

### Streaming API with 10-bit/HDR config

```rust
use wav1c::y4m::FramePixels;
use wav1c::{
    BitDepth, ColorRange, ContentLightLevel, Encoder, EncoderConfig,
    MasteringDisplayMetadata, VideoSignal,
};

let config = EncoderConfig {
    base_q_idx: 128,
    keyint: 25,
    target_bitrate: None,
    fps: 25.0,
    b_frames: false,
    gop_size: 3,
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
};

let mut enc = Encoder::new(1920, 1080, config)?;
let frame = FramePixels::solid_with_bit_depth(
    1920,
    1080,
    512,
    512,
    512,
    BitDepth::Ten,
    ColorRange::Full,
);

enc.send_frame(&frame)?;
enc.flush();
while let Some(pkt) = enc.receive_packet() {
    // pkt.data contains AV1 OBUs
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Key signal and metadata types

Exported from the crate root:
- `BitDepth`
- `ColorRange`
- `ColorDescription`
- `VideoSignal`
- `ContentLightLevel`
- `MasteringDisplayMetadata`

## C FFI API (`wav1c-ffi`)

Header: `wav1c-ffi/include/wav1c.h`

Legacy 8-bit API (unchanged):
- `wav1c_encoder_new(...)`
- `wav1c_encoder_send_frame(...)`

Extended 10-bit/HDR API:
- `wav1c_encoder_new_ex(...)`
- `wav1c_encoder_send_frame_u16(...)`

`Wav1cConfigEx` fields:
- `bit_depth`: `8` or `10`
- `color_range`: `0` limited, `1` full
- `color_primaries`, `transfer_characteristics`, `matrix_coefficients`: set to `-1` to omit color description
- `has_cll`, `max_cll`, `max_fall`
- `has_mdcv`, primaries/white-point/luminance fields

10-bit C usage example:

```c
#include "wav1c.h"

Wav1cConfigEx cfg = {0};
cfg.base_q_idx = 128;
cfg.keyint = 25;
cfg.fps = 25.0;
cfg.bit_depth = 10;
cfg.color_range = 1; // full
cfg.color_primaries = 9;
cfg.transfer_characteristics = 16;
cfg.matrix_coefficients = 9;
cfg.has_cll = 1;
cfg.max_cll = 203;
cfg.max_fall = 64;

Wav1cEncoder *enc = wav1c_encoder_new_ex(1920, 1080, &cfg);
if (!enc) return -1;

// y_len/u_len/v_len are sample counts, not byte counts.
wav1c_encoder_send_frame_u16(enc, y, y_len, u, u_len, v, v_len, y_stride, uv_stride);
Wav1cPacket *pkt = wav1c_encoder_receive_packet(enc);

if (pkt) wav1c_packet_free(pkt);
wav1c_encoder_free(enc);
```

Build artifacts:
- `libwav1c_ffi.dylib` / `libwav1c_ffi.so`
- `libwav1c_ffi.a`

## WebAssembly API (`wav1c-wasm`)

Main entry point: `WasmEncoder`

Constructors:
- `new(width, height, base_q_idx, keyint)` (compat mode)
- `new_with_config(...)`
- `new_ex(...)` (explicit bit depth/signal + optional CLL)

10-bit/HDR methods:
- `encode_frame_10bit(y, u, v)`
- `set_hdr10(color_range)`
- `set_video_signal(bit_depth, color_range, cp, tc, mc)`
- `set_content_light_level(max_cll, max_fall)`
- `set_mastering_display_metadata(...)`

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
- If source height is above `2304`, resize before encoding (current encoder limit).

## Testing

```bash
cargo test --workspace
```

Integration tests decode encoded output with [dav1d](https://code.videolan.org/videolan/dav1d). If `dav1d` is unavailable, tests that require it are skipped.

## License

This project is licensed under the Mozilla Public License 2.0. See [LICENSE](LICENSE).
