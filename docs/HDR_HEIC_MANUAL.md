# HDR HEIC Manual (wav1c + FFmpeg libwav1c)

This guide shows how to encode an HDR HEIC image into AV1 with proper 10-bit HDR signaling.

## 1. Prerequisites

- `ffmpeg` and `ffprobe` in `PATH`
- `dav1d` decoder in `PATH` (or at `../dav1d/build/tools/dav1d`)
- `wav1c` built from this repository
- HEIC source file (example):
  - `/Users/rafaelcaricio/development/wav1c/photo_hdr.heic`

## 2. HEIC -> 10-bit Y4M

Convert one HEIC frame to 10-bit 4:2:0 Y4M:

```bash
ffmpeg -y -v error \
  -i /Users/rafaelcaricio/development/wav1c/photo_hdr.heic \
  -frames:v 1 \
  -pix_fmt yuv420p10le \
  -strict -1 \
  -f yuv4mpegpipe /tmp/wav1c_photo_hdr_10bit.y4m
```

If your HEIC is image-grid based and this outputs `512x512`, see section 6.

## 3. Encode with wav1c-cli (HDR10)

Use HDR10 defaults and optional CLL metadata:

```bash
cargo run -q -p wav1c-cli -- \
  /tmp/wav1c_photo_hdr_10bit.y4m \
  -o /tmp/wav1c_photo_hdr_hdr.ivf \
  --bit-depth 10 \
  --hdr10 \
  --color-range full \
  --max-cll 203 \
  --max-fall 64
```

Optional mastering display metadata (MDCV):

```bash
--mdcv 34000,16000,13250,34500,7500,3000,15635,16450,10000000,1
```

MDCV field order is:

`rx,ry,gx,gy,bx,by,wx,wy,max_lum,min_lum`

## 4. Verify stream signaling

Check stream-level HDR fields:

```bash
ffprobe -v error -show_streams /tmp/wav1c_photo_hdr_hdr.ivf
```

Expected key lines (HDR10 full-range example):

- `pix_fmt=yuv420p10le`
- `color_primaries=bt2020`
- `color_transfer=smpte2084`
- `color_space=bt2020nc`
- `color_range=pc`

## 5. Verify decode path

Decode with dav1d:

```bash
../dav1d/build/tools/dav1d \
  -i /tmp/wav1c_photo_hdr_hdr.ivf \
  -o /tmp/wav1c_photo_hdr_hdr_decoded.y4m
```

Check decoded Y4M header:

```bash
head -n 1 /tmp/wav1c_photo_hdr_hdr_decoded.y4m
```

Expected colorspace tag: `C420p10`.

## 6. Encode directly with FFmpeg libwav1c (updated integration)

After building FFmpeg with `--enable-libwav1c`, you can encode directly:

```bash
ffmpeg -y -v error \
  -i /Users/rafaelcaricio/development/wav1c/photo_hdr.heic \
  -frames:v 1 \
  -pix_fmt yuv420p10le \
  -color_range pc \
  -color_primaries bt2020 \
  -color_trc smpte2084 \
  -colorspace bt2020nc \
  -c:v libwav1c \
  -wav1c-hdr10 1 \
  -wav1c-max-cll 203 \
  -wav1c-max-fall 64 \
  -f ivf /tmp/ffmpeg_libwav1c_hdr.ivf
```

Optional MDCV from private option:

```bash
-wav1c-mdcv 34000,16000,13250,34500,7500,3000,15635,16450,10000000,1
```

Then verify with:

```bash
ffprobe -v error -show_streams /tmp/ffmpeg_libwav1c_hdr.ivf
```

## 7. Notes

- `-map 0:v:0` on this HEIC picks one dependent tile (512x512), not the full
  composed image.
- Use the section 6 command (without `-map 0:v:0`) to keep the composed image
  path in FFmpeg's `libwav1c` integration.
- Container limits still apply in CLI workflows:
  - IVF/MP4 require `width <= 65535` and `height <= 65535`.
  - AVIF supports larger dimensions.
- This implementation is 4:2:0 only (`yuv420p`, `yuv420p10le`).
- 8-bit SDR behavior remains available and backward compatible.
- HDR metadata OBUs (CLL/MDCV) are emitted only when provided.
