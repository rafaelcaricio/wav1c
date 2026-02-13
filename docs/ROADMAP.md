# wav1c AV1 Encoder Roadmap

**Goal:** Encode a 2-second raw video with 1-second GoP to AV1, decodable by stock dav1d.

## Phase 1: Minimal Valid Bitstream [DONE]

- IVF container, OBU framing, sequence header, frame header
- Hardcoded tile data from aomenc reference
- 64x64 single-frame key frame, decodable by dav1d

## Phase 2+3: MSAC Encoder + Solid-Color Tile Encoding [DONE]

- MSAC arithmetic encoder (precarry buffer approach)
- Default CDF tables extracted from dav1d
- Real tile encoding: partition, mode, DC coefficients
- Any solid Y/U/V color, validated with dav1d (540+ combinations)

## Phase 4: Arbitrary Frame Dimensions [NEXT]

- Parameterize sequence/frame headers for any width/height
- Multiple superblocks per frame (SB grid)
- Handle partial superblocks at frame edges
- Still solid-color per frame, but now at any resolution
- Drop reduced_still_picture_header (need full headers for multi-frame later)

## Phase 5: Real Image Input (Single Intra Frame)

- Y4M input file parsing
- Per-block DC prediction with neighbor context
- DC-only coefficients per block (quantized residuals)
- Single intra frame of arbitrary image content

## Phase 6: Multi-Frame Encoding (All-Intra)

- Multiple frames (all keyframes)
- Temporal delimiters between frames
- IVF with correct frame count and timestamps
- CLI: wav1c input.y4m -o output.ivf

## Phase 7: Inter Prediction & GoP

- Reference frame buffer management
- ZERO_MV inter prediction (simplest inter mode)
- GoP structure: keyframe every N frames, inter frames between
- 2-second video at 25fps with 1-second GoP = 50 frames total
