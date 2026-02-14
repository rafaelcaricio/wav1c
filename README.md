# wav1c - Wondrous AV1 Encoder

An AV1 video encoder written from scratch in safe Rust with zero dependencies. Produces IVF files decodable by [dav1d](https://code.videolan.org/videolan/dav1d).

## Features

- YUV 4:2:0 encoding (8-bit)
- Intra prediction (DC mode) with per-pixel reconstructed context
- Inter prediction (GLOBALMV with LAST_FRAME reference)
- Forward DCT transforms (4x4 chroma, 8x8 luma)
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

wav1c_encoder_send_frame(enc, y, y_len, u, u_len, v, v_len);
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

## Test

```bash
cargo test --workspace
```

Integration tests decode output with dav1d and verify pixel accuracy. They require dav1d built at `../dav1d/build/tools/dav1d` and will skip gracefully if not found.

## License

This project is licensed under the Mozilla Public License 2.0. See [LICENSE](LICENSE) for details.
