# Intra Prediction Modes — Visual Quality Enhancement

## Goal

Replace the single DC_PRED mode with per-pixel prediction using 7 intra modes (DC, V, H, SMOOTH, SMOOTH_V, SMOOTH_H, PAETH) with SAD-based mode selection. This is the single highest-impact quality improvement for keyframes.

## Current State

- All intra blocks use `dc_prediction()` which returns a **single u8 value** for the entire block
- Residual = `source_pixel - flat_dc_value` for every pixel
- Mode is hardcoded to 0 (DC_PRED) in the bitstream
- Natural images with edges, gradients, and textures produce large residuals

## Design

### Prediction Functions

Each function takes reconstructed neighbor pixels and produces a full prediction block:

- **DC_PRED**: Average of above + left neighbors, fill entire block (existing behavior, but per-pixel output)
- **V_PRED**: Copy above row into every row of the block
- **H_PRED**: Copy left column into every column of the block
- **SMOOTH_PRED**: Weighted average blending toward above-right and below-left corners
- **SMOOTH_V_PRED**: Vertical-only smooth (blend top to bottom)
- **SMOOTH_H_PRED**: Horizontal-only smooth (blend left to right)
- **PAETH_PRED**: For each pixel, pick closest of (above, left, top-left) based on gradient

### Mode Selection

For each block, try all 7 modes and pick the one with lowest SAD (Sum of Absolute Differences) against the source pixels. SAD is chosen over SATD because it's fast and doesn't require a transform.

### Bitstream Encoding

- `kf_y_mode[above_mode][left_mode]` — encode chosen Y mode (0-12, using AV1 mode indices)
- `uv_mode[cfl_idx][y_mode]` — encode chosen UV mode (initially same as Y)
- Mode context arrays: track above/left modes per MI column/row

### AV1 Mode Index Mapping

| Mode | AV1 Index |
|------|-----------|
| DC_PRED | 0 |
| V_PRED | 1 |
| H_PRED | 2 |
| SMOOTH_PRED | 9 |
| SMOOTH_V_PRED | 10 |
| SMOOTH_H_PRED | 11 |
| PAETH_PRED | 12 |

Directional modes (3-8) are not included in this phase.

### Files Changed

- `src/tile.rs` — prediction functions, mode selection, encode_block, encode_skip_block, context tracking
- `src/cdf.rs` — possibly adjust mode CDF context indexing (kf_y_mode already has 13x13 context)

### What This Does NOT Change

- Transform type (stays DCT_DCT)
- Partition decisions (stays variance-based skip + forced 8x8 split)
- Inter frames (stays GLOBALMV)
- Chroma prediction (UV follows Y mode, no CFL)
- Frame/sequence headers (no new flags needed)

### Validation

- All existing tests must pass (solid colors still pick DC_PRED)
- Encode gradient/edge test patterns, decode with dav1d
- Measure PSNR improvement on real image content
