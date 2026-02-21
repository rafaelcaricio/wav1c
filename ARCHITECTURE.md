# wav1c Project Architecture & Overview

## Project Overview

`wav1c` is a lightweight, dependency-free Rust implementation of an AV1 video encoder. It focuses on conceptual clarity, real-time potential, and adherence to the AV1 bitstream specification, rather than competing with production encoders like `svt-av1` or `aomenc` on sheer compression efficiency.

The repository is structured into two main components:
1. **`wav1c` (Core Rust Library):** The main encoder implementation.
2. **`wav1c-ffi` (C API Bindings):** Exposes the Rust encoder via C headers, enabling integration into tools like FFmpeg.

## Core Architecture

The encoder is structured around the fundamental AV1 coding tools:

### Bitstream Structure
- **OBUs (Open Bitstream Units):** The encoder correctly structures the bitstream using OBUs for Sequence Headers, Frame Headers, Tile Data, and Temporal Delimiters.
- **IVF Output:** A simple file format wrapper used for testing and validation.

### Video Encoding Pipeline Phase
1. **Partitioning:** Recursive division of 64x64 Superblocks (SBs) down to 4x4 blocks.
2. **Prediction:**
   - **Intra Prediction:** Spatial prediction based on neighboring pixels (DC, V, H, Paeth, Smooth, Directional).
   - **Inter Prediction:** Temporal prediction using motion vectors pointing to reference frames. Includes support for global motion estimation using downscaled frame comparisons to anchor sub-pixel searches.
3. **Transforms:** DCT/ADST transformations of residual data to the frequency domain to isolate important visual data.
4. **Quantization:** Standard AV1 scalar quantization with lookup tables (`dq`), controllable via a single `base_q_idx` (0-255).
5. **Entropy Coding (MSAC):** Multi-Symbol Arithmetic Coding for bit-level compression. Requires strict probability synchronization (CDF) between encoder and decoder to prevent decoding panics.

### Temporal Structure & B-Frames
- **GOP (Group of Pictures):** Supports regular IDR/Keyframe insertion via a configurable `keyint`.
- **B-Frames (Bi-Directional):** Features a mini-GOP architecture where the "future" reference (P-frame) is encoded first but marked locally with `show_frame=false`. The intermediate B-frames are encoded bi-directionally using both the past and future references, marked with `show_frame=true`. The hidden P-frame is eventually displayed exactly at its temporal presentation time using a `show_existing_frame` OBU.
- **Reference Buffer Management:** Employs ping-pong reference slot rotation (`base_slot`/`alt_slot`) to continually refresh future references accurately.

### Rate-Distortion Optimization (RDO)
- **Fast SATD:** Utilizes Sum of Absolute Transformed Differences (using Hadamard transforms) to quickly and efficiently decide block partitioning structure and perform broad motion searches.
- **Hybrid Refinement:** Performs exhaustive True Bit Cost + SSE (Sum of Squared Errors) scoring only on the top-K candidates returned by the fast SATD search, optimizing visual quality while maintaining encode speed.

### In-Loop Filters
- **CDEF (Constrained Directional Enhancement Filter):** Preserves sharp edges and textures while blurring blocking artifacts generated naturally from DCT/ADST transforms and quantization. Default strength is implicitly derived from `base_q_idx` but can be manually constrained.

## Quality Validation Pipeline (VMAF)

To ensure optimizations do not inadvertently degrade visual quality, testing is conducted using procedural Y4M generated sequences directly against the `vmaf` FFmpeg filter:

```bash
ffmpeg -i encoded_output.ivf \
       -i original_source.y4m \
       -lavfi "[0:v]settb=1/25,setpts=PTS-STARTPTS[dec];[1:v]settb=1/25,setpts=PTS-STARTPTS[ref];[dec][ref]libvmaf=log_fmt=json:log_path=vmaf.json" \
       -f null -
```

Using container-stripped timelines (`setpts=PTS-STARTPTS`) ensures frames align 1:1 specifically validating B-frame temporal reconstruction properly outputs the right sequence frame-for-frame against the procedural inputs (`zoom_test.y4m`, `pan_test.y4m`).
