# Phase 8: DCT Transform Encoding

## Goal
Replace DC-only coefficient encoding with full DCT transform encoding, enabling real image encoding with per-pixel fidelity.

## Current State
- Partition always uses PARTITION_NONE at the first valid level (bl=1 for interior SBs)
- Each block encodes a single DC coefficient (block average)
- No AC coefficients encoded
- DC dequant = 140 at base_q_idx=128

## Phase 8 Changes

### 1. Force 8x8 Partition
- Always split to bl=4 (8x8 luma, 4x4 chroma)
- Encode PARTITION_SPLIT (symbol 3) at bl=1,2,3
- Encode PARTITION_NONE (symbol 0) at bl=4
- This avoids needing DCT transforms larger than 8x8

### 2. Forward/Inverse DCT Transforms (`src/dct.rs`)

**Forward DCT4 (for chroma 4x4 blocks):**
- Input: 4x4 residual block (pixel - prediction)
- Output: 4x4 coefficient block
- Uses same integer constants as dav1d inverse (transposed operation)

**Forward DCT8 (for luma 8x8 blocks):**
- Input: 8x8 residual block
- Output: 8x8 coefficient block
- Hierarchical: DCT4 on even indices, then odd butterfly

**Inverse DCT4/DCT8 (for reconstruction):**
- Must match dav1d exactly (same integer constants, same rounding)
- Used to compute reconstructed pixels for context tracking

**2D Transform:**
- Row-first, then column (separable)
- For 4x4: no intermediate shift
- For 8x8: intermediate shift of 1

### 3. Quantization
- DC dequant = 140 (existing)
- AC dequant = 176 (new, at base_q_idx=128)
- For TX_4X4 (t_dim_ctx=0): dq_shift=0
- For TX_8X8 (t_dim_ctx=1): dq_shift=0
- Forward quantize: token = round(coef / dq_factor)
- Inverse quantize: coef = (token * dq_factor) & 0xffffff >> dq_shift

### 4. Scan Order Tables (`src/scan.rs`)
- DEFAULT_SCAN_4X4: 16 entries, diagonal zigzag pattern
- DEFAULT_SCAN_8X8: 64 entries, diagonal zigzag pattern
- Maps scan index → raster position

### 5. New CDF Tables (`src/cdf.rs`)
- `base_tok[t_dim_ctx][chroma][ctx]` - 4 symbols, 41 contexts
- `eob_hi_bit[t_dim_ctx][chroma][eob_bin]` - boolean, ~10 contexts

### 6. Multi-Coefficient Encoding

**Encoding a transform block with eob > 0:**

1. `txb_skip = 0` (not all-zero)
2. Encode eob_bin (which bin contains eob)
   - For TX_4X4: eob_bin_16 with 5 symbols
   - For TX_8X8: eob_bin_64 with 7 symbols
3. If eob_bin >= 2: encode eob_hi_bit + extra raw bits
4. For EOB coefficient: encode eob_base_tok (2 symbols: 0,1,2)
5. For coefficients eob-1 down to 1: encode base_tok with context (4 symbols)
6. For DC (coefficient 0): encode base_tok with context
7. For all coefficients with level >= 3: encode hi_tok
8. Encode signs: DC uses context-adaptive, AC uses equiprobable
9. For tok >= 15: encode Golomb(tok - 15)

**Context computation for base_tok (get_lo_ctx):**
- Uses levels array (previously encoded coefficient magnitudes)
- Position-based offsets from context offset table
- Magnitude-based offset: mag > 512 ? 4 : (mag + 64) >> 7

### 7. Per-Pixel Encoding Path

For each 8x8 luma block:
1. Extract 8x8 pixel block from source
2. Compute DC prediction (same as current)
3. Compute residual: residual[y][x] = pixel[y][x] - prediction
4. Forward DCT 8x8 on residual
5. Quantize each coefficient: tok = round(dct_coef / dq_factor)
6. Find eob (last non-zero in scan order)
7. Encode using multi-coefficient syntax
8. Reconstruct: dequantize → inverse DCT → add prediction → clamp
9. Store reconstructed pixels for context tracking

For each 4x4 chroma block: same flow with DCT4

### 8. Block Params for bl=4

- Luma: TX_8X8, t_dim_ctx=1, dq_shift=0, itx_shift=1
- Chroma: TX_4X4, t_dim_ctx=0, dq_shift=0, itx_shift=0
- DC dequant = 140, AC dequant = 176

## Implementation Tasks

1. Create `src/dct.rs` with forward/inverse DCT4 and DCT8
2. Create `src/scan.rs` with scan tables and context offset tables
3. Extract base_tok and eob_hi_bit CDFs from dav1d, add to cdf.rs
4. Rewrite encode_block to use per-pixel DCT encoding
5. Change encode_partition to always split to bl=4
6. Update reconstruction to use inverse DCT
7. Add integration tests with dav1d validation
8. Update existing tests for new partition behavior

## Validation
- cargo test + cargo clippy --tests
- Generate gradient test frame with ffmpeg
- Encode with wav1c, decode with dav1d
- Compare output pixels (PSNR check)
