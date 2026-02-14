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

## Phase 6: Multi-Frame Encoding (All-Intra) [DONE]

- Multi-frame Y4M parsing (all FRAME markers)
- All-keyframe encoding (TD + SEQ + FRAME per frame)
- IVF with correct frame count and sequential timestamps
- CLI: wav1c input.y4m -o output.ivf (multi-frame)
- Validated 3-frame and 5-frame sequences up to 320x240

## Phase 7: Inter Prediction & GoP [DONE]

- GLOBALMV inter prediction with IDENTITY transform
- GoP encoding: keyframe + inter frames with LAST_FRAME reference
- Reference frame buffer management (slot 0)
- disable_cdf_update=1, error_resilient_mode=1 for inter frames
- Neighbor-based newmv_ctx computation
- Validated multi-frame GoP at various dimensions

## Phase 8: DCT Transform Encoding [DONE]

- Forward/Inverse 4x4 and 8x8 DCT integer transforms (matching dav1d)
- Scan order tables (DEFAULT_SCAN_4X4, DEFAULT_SCAN_8X8)
- Full multi-coefficient encoding: txb_skip → txtp → eob_bin → eob_hi_bit → eob_base_tok → base_tok → br_tok/hi_tok → signs → Golomb
- Complete CDF tables for all coefficient coding symbols
- Packed level_tok storage matching dav1d's flat levels layout
- Quantization/dequantization with DC and AC dequant values
- Per-pixel DC prediction with reconstructed border tracking
- Validated with real images up to 640x480 (ffmpeg testsrc)

## Phase 9: Rate Control & Quality [IN PROGRESS]

- Configurable quantizer index (base_q_idx 0-255) with -q CLI flag [DONE]
- Per-qcat default coefficient CDF selection (4 qcat table sets) [DONE]
- Dequantization lookup table from dav1d (8-bit, 256 entries) [DONE]
- Adaptive loop filter levels based on QP [DONE]
- CDEF with adaptive strength based on QP [DONE]
- Adaptive quantization (per-block delta_q)
- Rate control for target bitrate
