# Phase 5: Real Image Input — Implementation Plan

## Goal
Accept Y4M video files as input and encode the first frame as a single AV1 intra frame with per-block DC prediction. Each block gets its own DC value computed from the actual pixel data, producing a blocky but recognizable output decodable by stock dav1d.

## Key Design Decisions

1. **Minimal Y4M parser** — no external dependency, ~60 lines, 4:2:0 only
2. **DC-only encoding** — same as Phase 4 but with per-block target values from actual pixels
3. **Keep largest blocks** — PARTITION_NONE when both halves fit (same as Phase 4)
4. **Per-block DC prediction** — average of reconstructed top and left edge pixels
5. **Track per-block reconstructed DC** — above-row array + left-column value per plane
6. **Keep existing solid-color API** — add new `encode_av1_ivf_y4m()` alongside existing API
7. **base_q_idx=128** — same quantizer, DC dequant=140

## DC Prediction for DC-Only Blocks

Since we only encode DC coefficients, each reconstructed block is a flat fill of one value. This means neighbor "edge pixels" are uniform, simplifying the DC prediction:

```
No neighbors:  DC_pred = 128
Top only:      DC_pred = above_recon_dc
Left only:     DC_pred = left_recon_dc
Both:          DC_pred = (above_recon_dc + left_recon_dc + 1) >> 1
```

The AV1 spec formula with uniform edges:
```
dc = (rounding + N*above + N*left) >> log2(2*N)
   = (N + N*above + N*left) / (2*N)
   = (1 + above + left) / 2
   = (above + left + 1) >> 1
```

## Per-Block Target Computation

For each leaf block at position (bx, by) in MI units with block level `bl`:
- Block pixel size: `px_size = 64 >> bl` (luma), `px_size/2` (chroma in 4:2:0)
- Pixel position: `px_x = bx * 4`, `px_y = by * 4`
- Average over all pixels in the block region (clamped to frame bounds)
- This average becomes the "target" for DC token computation

## Data Structures

### FramePixels
```rust
struct FramePixels {
    y: Vec<u8>,   // width * height
    u: Vec<u8>,   // (width/2) * (height/2)
    v: Vec<u8>,   // (width/2) * (height/2)
    width: u32,
    height: u32,
}
```

### Updated TileContext
```rust
struct TileContext {
    // Existing fields...
    above_recon_y: Vec<u8>,  // one per MI-column / 2 (per block-column at finest level)
    above_recon_u: Vec<u8>,
    above_recon_v: Vec<u8>,
    left_recon_y: [u8; 16],  // one SB height worth of block-rows
    left_recon_u: [u8; 16],
    left_recon_v: [u8; 16],
}
```

Wait — actually simpler. Since blocks at different levels have different sizes, and we always use PARTITION_NONE at the largest possible level, we know the block sizes at each position. For DC-only flat blocks, we can track reconstructed DC per MI-block-column for above and per MI-block-row for left. But the block sizes vary (64x64 interior, potentially 32x32 or 8x8 at edges), so we need finer granularity.

Simplest approach: track at 8x8 granularity (MI_SIZE = 4, so MI_COLS/2 entries for above). Each 8x8 region within a reconstructed block has the same DC value. When we reconstruct a 64x64 block, fill all 8 entries in above/left arrays.

### Refined TileContext additions
```
above_recon_y: Vec<u8>,   // size = mi_cols / 2 (one per 8x8 column)
above_recon_u: Vec<u8>,   // size = mi_cols / 4 (one per 8x8 chroma column)
above_recon_v: Vec<u8>,
left_recon_y: [u8; 8],    // one SB = 8 blocks of 8x8
left_recon_u: [u8; 4],
left_recon_v: [u8; 4],
```

## DC Prediction Logic per Block

```
fn dc_predict(above: &[u8], left: &[u8], have_top: bool, have_left: bool) -> u8 {
    match (have_top, have_left) {
        (false, false) => 128,
        (true, false) => average(above),
        (false, true) => average(left),
        (true, true) => {
            let sum: u32 = above.iter().chain(left.iter()).map(|&x| x as u32).sum();
            ((sum + (above.len() + left.len()) as u32 / 2)
                / (above.len() + left.len()) as u32) as u8
        }
    }
}
```

For DC-only blocks where all above/left edge pixels equal the same reconstructed value:
- `average(above) = above[0]` (all same)
- `(above + left + 1) >> 1` when both present

## Implementation Tasks

### Task 1: Y4M Parser (y4m.rs)
New module for minimal Y4M parsing:
- Parse header: `YUV4MPEG2 W<w> H<h> F<n>:<d> C420...`
- Extract first frame data after `FRAME\n` marker
- Return `FramePixels { y, u, v, width, height }`
- Only support 4:2:0, 8-bit
- Parse from file path or byte slice

### Task 2: Per-Block DC Prediction (tile.rs)
Refactor TileEncoder to support per-block targets:
- Add `FramePixels` reference to TileEncoder
- Add per-block reconstructed DC tracking (above row + left column arrays)
- In `encode_block`: compute block average from pixel data
- In `encode_block`: compute DC prediction from neighbor reconstructed values
- Update reconstruction tracking after each block
- When FramePixels is None, fall back to solid-color behavior (backward compat)

### Task 3: Block Average Computation (tile.rs)
Helper to compute average pixel value for a block region:
- `fn block_average(plane: &[u8], stride: u32, px_x: u32, px_y: u32, px_size: u32, frame_w: u32, frame_h: u32) -> u8`
- Handle edge blocks that extend past frame boundary (clamp to available pixels)
- Separate computation for luma and chroma planes

### Task 4: Public API Update (lib.rs, main.rs)
- Add `encode_av1_ivf_y4m(pixels: &FramePixels) -> Vec<u8>`
- Keep existing `encode_av1_ivf(width, height, y, u, v)` for solid color
- CLI: `wav1c input.y4m -o output.ivf` (detect Y4M input vs solid-color mode)
- CLI: Keep `wav1c <W> <H> <Y> <U> <V> -o output.ivf` for solid-color mode

### Task 5: Integration Tests
- Create a test Y4M file programmatically (gradient or pattern)
- Encode with wav1c, decode with dav1d, verify no crash
- Pixel comparison: decoded output should approximately match input (within DC-only quality limits)
- Test with solid-color Y4M to verify backward compatibility
- Test various dimensions with Y4M input

## File Changes Summary

| File | Change |
|------|--------|
| src/y4m.rs | NEW — Y4M parser |
| src/tile.rs | Refactor for per-block targets and DC prediction |
| src/lib.rs | Add `encode_av1_ivf_y4m()`, register y4m module |
| src/main.rs | Detect Y4M input, update CLI |
| tests/integration.rs | Add Y4M input tests |

## Quality Expectations

With DC-only encoding at PARTITION_NONE (largest blocks):
- 64x64 blocks produce very blocky output
- Each block is a flat fill of one color
- Recognizable but low quality (like extreme JPEG compression)
- PSNR will be low but image structure should be visible
- This is expected — AC coefficients and smaller partitions come in later phases

## Dependencies
- Task 1 (Y4M parser) is independent
- Task 2 (DC prediction) is independent of Task 1 but the main architectural change
- Task 3 (block average) depends on Task 2's data structures
- Task 4 (API) depends on Tasks 1-3
- Task 5 (tests) depends on Task 4
