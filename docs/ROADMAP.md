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

## Phase 4: Arbitrary Frame Dimensions [DONE]

- Full (non-reduced) sequence/frame headers, Level 5.1
- Multi-SB tile encoder with recursive partition tree
- Edge partition handling (forced splits at frame boundaries)
- DC prediction context tracking across superblocks
- Validated 1x1 to 1920x1080, including non-SB-aligned dimensions

## Phase 5: Real Image Input (Single Intra Frame) [DONE]

- Y4M input file parsing (minimal parser, 4:2:0 only)
- Per-block DC prediction with reconstructed neighbor context
- Coefficient context tracking (dc_sign_ctx, txb_skip_ctx)
- Frame boundary clamping for all context arrays
- Validated with gradient patterns at various dimensions

## Phase 6: Multi-Frame Encoding (All-Intra) [NEXT]

- Multiple frames (all keyframes)
- Temporal delimiters between frames
- IVF with correct frame count and timestamps
- CLI: wav1c input.y4m -o output.ivf

## Phase 7: Inter Prediction & GoP

- Reference frame buffer management
- ZERO_MV inter prediction (simplest inter mode)
- GoP structure: keyframe every N frames, inter frames between
- 2-second video at 25fps with 1-second GoP = 50 frames total
