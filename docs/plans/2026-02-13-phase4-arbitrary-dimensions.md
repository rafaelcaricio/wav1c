# Phase 4: Arbitrary Frame Dimensions — Implementation Plan

## Goal
Parameterize the encoder to accept any frame width/height, encode multiple superblocks per frame, handle partial SBs at edges, and still produce valid AV1 bitstreams decodable by stock dav1d. Drop `reduced_still_picture_header` to prepare for multi-frame encoding.

## Key Design Decisions

1. **Keep SB size = 64x64** (sb128=0) — simpler partition tree
2. **still_picture=0** — prepares for multi-frame in Phase 6
3. **enable_order_hint=0** — avoid reference frame ordering complexity
4. **screen_content_tools=0** (fixed in seq header, not adaptive) — avoids per-frame bits
5. **seq_level_idx=13** (Level 5.1, tier=0) — supports up to 4K resolution
6. **Single tile** for all frame sizes — uniform tiling with log2_cols=0, log2_rows=0
7. **base_q_idx=128** — same as current
8. **frame_size_override=0** — always use max dimensions from sequence header

## Architecture

### API Change
```rust
pub fn encode_av1_ivf(width: u32, height: u32, y: u8, u: u8, v: u8) -> Vec<u8>
```

### SB Grid Iteration
For a WxH frame:
- mi_cols = 2 * ((width + 7) / 8)
- mi_rows = 2 * ((height + 7) / 8)
- sb_cols = (mi_cols + 15) / 16  (64px SB = 16 MI units)
- sb_rows = (mi_rows + 15) / 16

Iterate top-to-bottom, left-to-right (matching dav1d's decode_tile_sbrow).

### Partition at Edges
At each SB, recursively partition:
- Both halves fit → encode full partition symbol (PARTITION_NONE for simplest)
- Only one half fits → encode bool (split vs horz/vert)
- Neither half fits → forced split (no bits)
- Terminal: 8x8 block → PARTITION_NONE (no further splits)

For solid-color Phase 4, always choose PARTITION_NONE when both halves fit, and PARTITION_HORZ/VERT (not split) at edges. This minimizes partition depth.

### DC Prediction Context
- First SB (no neighbors): DC_pred = 128, residual = target - 128
- Subsequent SBs: DC_pred ≈ reconstructed value from first SB
- Track reconstructed pixel values per-plane to compute correct residuals

### Context Tracking
- `above_ctx`: partition context array for the "above" row, sized sb_cols
- `left_ctx`: partition context for current SB column, reset per row
- Partition context = (above_partition_bit << 1) | left_partition_bit

## Implementation Tasks

### Task 1: Parameterized Sequence Header (sequence.rs)
Rewrite `encode_sequence_header(width, height)` for full (non-reduced) format:
- seq_profile=0, still_picture=0, reduced_still_picture_header=0
- timing_info_present=0, initial_display_delay_present=0
- 1 operating point: idc=0, seq_level_idx=13, tier=0
- frame_width_bits/frame_height_bits computed from dimensions
- frame_id_numbers_present=0
- sb128=0, filter_intra=0, intra_edge_filter=0
- inter_intra=0, masked_compound=0, warped_motion=0, dual_filter=0
- order_hint=0, screen_content_tools=0/0, force_integer_mv=2(auto)
- super_res=0, cdef=0, restoration=0
- Color: 8-bit, not mono, no color desc, studio range, 4:2:0, chroma_pos=0
- separate_uv_delta_q=0, film_grain=0

### Task 2: Parameterized Frame Header (frame.rs)
Rewrite `encode_frame(width, height, y, u, v)` for full format:
- show_existing_frame=0
- frame_type=0 (KEY_FRAME), show_frame=1
- error_resilient_mode derived (=1 for keyframe+show)
- disable_cdf_update=0
- allow_screen_content_tools=0 (from seq_hdr fixed value)
- frame_size_override=0, have_render_size=0
- disable_frame_end_update_cdf=0
- Tiling: uniform=1, compute min/max log2_cols/rows from sbw/sbh, write 0-bits to stay at single tile
- Quantization: base_q_idx=128, all deltas=0, qm=0
- Segmentation: disabled
- Delta Q/LF: disabled
- Loop filter: level_y[0]=0, level_y[1]=0, sharpness=0, mode_ref_delta_enabled=1, update=0
- TX mode: TX_LARGEST (bit=0)

### Task 3: Multi-SB Tile Encoder (tile.rs)
Rewrite `encode_tile(width, height, y, u, v, base_q_idx)`:
- Compute mi_cols, mi_rows, sb_cols, sb_rows
- Maintain above_partition_ctx array (per SB column)
- For each SB row: reset left_partition_ctx
- For each SB in row: call encode_superblock()
- encode_superblock(): recursive partition tree with edge handling
- Track reconstruction state for DC prediction across SBs
- For solid-color: first SB gets full residual, subsequent SBs get near-zero

### Task 4: Update Public API (lib.rs, main.rs)
- `encode_av1_ivf(width, height, y, u, v)` — add width/height params
- CLI: `wav1c <width> <height> <Y> <U> <V> -o output.ivf`
- IVF header gets actual width/height

### Task 5: Integration Tests
- Keep 64x64 tests as regression (adapt API calls)
- Add dimension test cases: 1x1, 8x8, 32x32, 100x100, 128x128, 320x240, 640x480
- Validate all decode with dav1d
- Pixel-level verification for known solid colors
- Test non-SB-aligned dimensions (e.g., 100x100, 37x53)

## Sequence Header Bit Layout

```
seq_profile                     3 bits   = 0
still_picture                   1 bit    = 0
reduced_still_picture_header    1 bit    = 0
timing_info_present_flag        1 bit    = 0
initial_display_delay_present   1 bit    = 0
operating_points_cnt_minus_1    5 bits   = 0
operating_point_idc[0]         12 bits   = 0
seq_level_idx[0]                5 bits   = 13
seq_tier[0]                     1 bit    = 0 (since level > 7)
frame_width_bits_minus_1        4 bits   = computed
frame_height_bits_minus_1       4 bits   = computed
max_frame_width_minus_1         n bits   = width - 1
max_frame_height_minus_1        n bits   = height - 1
frame_id_numbers_present        1 bit    = 0
use_128x128_superblock          1 bit    = 0
enable_filter_intra             1 bit    = 0
enable_intra_edge_filter        1 bit    = 0
enable_interintra_compound      1 bit    = 0
enable_masked_compound          1 bit    = 0
enable_warped_motion            1 bit    = 0
enable_dual_filter              1 bit    = 0
enable_order_hint               1 bit    = 0
seq screen_content_tools: select=0 value=0  2 bits
seq force_integer_mv: (skipped, screen_content_tools=0)
enable_superres                 1 bit    = 0
enable_cdef                     1 bit    = 0
enable_restoration              1 bit    = 0
high_bitdepth                   1 bit    = 0
mono_chrome                     1 bit    = 0
color_description_present       1 bit    = 0
color_range                     1 bit    = 0
chroma_sample_position          2 bits   = 0
separate_uv_delta_q             1 bit    = 0
film_grain_params_present       1 bit    = 0
trailing bit                    1 bit    = 1
padding                         to byte boundary
```

## Frame Header Bit Layout

```
show_existing_frame             1 bit    = 0
frame_type                      2 bits   = 0 (KEY_FRAME)
show_frame                      1 bit    = 1
(error_resilient_mode derived = 1, not coded)
disable_cdf_update              1 bit    = 0
(allow_screen_content_tools = 0, from seq fixed, not coded)
(force_integer_mv derived for intra, not coded)
(frame_id not present)
frame_size_override_flag        1 bit    = 0
(order_hint: 0 bits since enable_order_hint=0)
(primary_ref_frame = PRIMARY_REF_NONE, derived)
(refresh_frame_flags = 0xFF, derived for KEY+show)
(frame_size from seq header, no bits)
have_render_size                1 bit    = 0
(allow_intrabc skipped, screen_content_tools=0)
disable_frame_end_update_cdf    1 bit    = 0
tiling.uniform                  1 bit    = 1
tiling cols loop               0-N bits  (0 bits to stop at min_log2=0)
tiling rows loop               0-N bits  (0 bits to stop at min_log2=0)
(tile update/size if log2_cols+log2_rows > 0)
base_q_idx                      8 bits   = 128
y_dc_delta_present              1 bit    = 0
diff_uv_delta (if separate_uv_delta_q, not set) = 0
u_dc_delta_present              1 bit    = 0
u_ac_delta_present              1 bit    = 0
using_qmatrix                   1 bit    = 0
segmentation_enabled            1 bit    = 0
delta_q_present                 1 bit    = 0
loop_filter_level_y[0]          6 bits   = 0
loop_filter_level_y[1]          6 bits   = 0
loop_filter_sharpness           3 bits   = 0
mode_ref_delta_enabled          1 bit    = 1
mode_ref_delta_update           1 bit    = 0
tx_mode                         1 bit    = 0 (TX_LARGEST)
byte-align
[tile data follows]
```

## Dependencies
- Task 1 (seq header) and Task 2 (frame header) are independent
- Task 3 (tile encoder) depends on understanding the frame dimensions
- Task 4 (API) depends on Tasks 1-3
- Task 5 (tests) depends on Task 4
