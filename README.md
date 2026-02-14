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

## Build

```bash
cargo build --release
```

## Usage

Encode a Y4M video:

```bash
wav1c input.y4m -o output.ivf
```

Encode a solid color frame:

```bash
wav1c 320 240 128 128 128 -o gray.ivf
```

Decode with dav1d:

```bash
dav1d -i output.ivf -o decoded.y4m
```

Generate test input with ffmpeg:

```bash
ffmpeg -f lavfi -i testsrc=duration=2:size=320x240:rate=25 -pix_fmt yuv420p input.y4m
```

## Test

```bash
cargo test
```

Integration tests decode output with dav1d and verify pixel accuracy. They require dav1d built at `../dav1d/build/tools/dav1d` and will skip gracefully if not found.

## License

This project is licensed under the Mozilla Public License 2.0. See [LICENSE](LICENSE) for details.
