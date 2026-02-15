# wav1c - Wondrous AV1 Encoder

[![crates.io](https://img.shields.io/crates/v/wav1c.svg)](https://crates.io/crates/wav1c)
[![license](https://img.shields.io/crates/l/wav1c.svg)](LICENSE)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

An AV1 video encoder written from scratch in safe Rust with zero dependencies. Produces IVF files decodable by [dav1d](https://code.videolan.org/videolan/dav1d).

## Features

- YUV 4:2:0 encoding (8-bit)
- Intra prediction (DC, V, H, Smooth, Paeth) with RD-cost mode selection
- Inter prediction (GLOBALMV with LAST_FRAME reference)
- Forward DCT/ADST transforms with RD-cost type selection (4x4 chroma, 8x8/16x16 luma)
- MSAC arithmetic coding with full coefficient encoding
- GoP structure (keyframe + inter frames)
- Arbitrary frame dimensions up to 4096x2304
- Y4M input, IVF output
- Streaming encoder API (frame-by-frame encoding)
- C FFI shared library for embedding in non-Rust applications

## Workspace Structure

```
wav1c/          Core library with batch and streaming APIs
wav1c-cli/      Command-line encoder
wav1c-ffi/      C FFI shared library (cdylib + staticlib)
wav1c-wasm/     WebAssembly bindings (wasm-bindgen)
```

## Build

```bash
cargo build --workspace --release
```

## CLI Usage

Encode a Y4M video:

```bash
wav1c input.y4m -o output.ivf
```

Encode a solid color frame:

```bash
wav1c 320 240 128 128 128 -o gray.ivf
```

With options:

```bash
wav1c input.y4m -o output.ivf -q 100 --keyint 10 --bitrate 500k
```

Decode with dav1d:

```bash
dav1d -i output.ivf -o decoded.y4m
```

Generate test input with ffmpeg:

```bash
ffmpeg -f lavfi -i testsrc=duration=2:size=320x240:rate=25 -pix_fmt yuv420p input.y4m
```

## Rust Streaming API

```rust
use wav1c::{Encoder, EncoderConfig, FrameType};
use wav1c::y4m::FramePixels;

let config = EncoderConfig {
    base_q_idx: 128,
    keyint: 25,
    target_bitrate: None,
    fps: 25.0,
};

let mut encoder = Encoder::new(320, 240, config).unwrap();
let frame = FramePixels::solid(320, 240, 128, 128, 128);

encoder.send_frame(&frame).unwrap();
let packet = encoder.receive_packet().unwrap();

assert_eq!(packet.frame_type, FrameType::Key);
// packet.data contains raw AV1 OBUs (TD + SequenceHeader + Frame)
```

## C FFI

The `wav1c-ffi` crate produces `libwav1c_ffi.dylib` (macOS) / `libwav1c_ffi.so` (Linux) and `libwav1c_ffi.a`.

```c
#include "wav1c.h"

Wav1cConfig cfg = { .base_q_idx = 128, .keyint = 25, .fps = 25.0 };
Wav1cEncoder *enc = wav1c_encoder_new(320, 240, &cfg);

wav1c_encoder_send_frame(enc, y, y_len, u, u_len, v, v_len, width, width / 2);
Wav1cPacket *pkt = wav1c_encoder_receive_packet(enc);
// pkt->data, pkt->size contain raw AV1 OBUs

wav1c_packet_free(pkt);
wav1c_encoder_free(enc);
```

Build the C example:

```bash
cargo build -p wav1c-ffi --release
cc -o encode -I wav1c-ffi/include wav1c-ffi/examples/encode.c \
   -L target/release -lwav1c_ffi
```

## FFmpeg Integration

A patch is included to add wav1c as an FFmpeg external encoder (`-c:v libwav1c`).

```bash
# Build the wav1c static library
cargo build -p wav1c-ffi --release

# Clone and patch FFmpeg
git clone https://git.ffmpeg.org/ffmpeg.git
cd ffmpeg
git apply /path/to/wav1c/ffmpeg-libwav1c.patch

# Configure with libwav1c (adjust library path as needed)
./configure --enable-libwav1c \
  --extra-cflags="-I/path/to/wav1c/wav1c-ffi/include" \
  --extra-ldflags="-L/path/to/wav1c/target/release"

make -j$(sysctl -n hw.ncpu 2>/dev/null || nproc)

# Encode
./ffmpeg -i input.y4m -c:v libwav1c -wav1c-q 128 output.mp4
```

## Test

```bash
cargo test --workspace
```

Integration tests decode output with [dav1d](https://code.videolan.org/videolan/dav1d) and verify pixel accuracy. Install dav1d and ensure it is available in your `PATH`, or set the `DAV1D` environment variable to point to the binary. Tests will skip gracefully if dav1d is not found.

## License

This project is licensed under the Mozilla Public License 2.0. See [LICENSE](LICENSE) for details.
