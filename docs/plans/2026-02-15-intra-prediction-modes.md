# Intra Prediction Modes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add 7 intra prediction modes (DC, V, H, SMOOTH, SMOOTH_V, SMOOTH_H, PAETH) with SAD-based mode selection to improve keyframe visual quality.

**Architecture:** Refactor the prediction pipeline from scalar `u8` output to per-pixel `Vec<u8>` blocks. Add prediction functions for each mode, a mode selection loop that picks the lowest-SAD mode, and mode context tracking arrays for correct bitstream encoding. UV mode follows Y mode.

**Tech Stack:** Rust, AV1 bitstream (MSAC arithmetic coding), dav1d decoder for validation

---

### Task 1: Add Smooth Weight Table

**Files:**
- Modify: `wav1c/src/tile.rs` (top of file, after constants)

**Step 1: Write the failing test**

Add to the `#[cfg(test)]` module at the bottom of `wav1c/src/tile.rs`:

```rust
#[test]
fn smooth_weights_correct_for_size_4() {
    assert_eq!(SM_WEIGHTS[4], 255);
    assert_eq!(SM_WEIGHTS[5], 149);
    assert_eq!(SM_WEIGHTS[6], 85);
    assert_eq!(SM_WEIGHTS[7], 64);
}

#[test]
fn smooth_weights_correct_for_size_8() {
    assert_eq!(SM_WEIGHTS[8], 255);
    assert_eq!(SM_WEIGHTS[9], 197);
    assert_eq!(SM_WEIGHTS[14], 37);
    assert_eq!(SM_WEIGHTS[15], 32);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p wav1c smooth_weights -- --nocapture`
Expected: FAIL — `SM_WEIGHTS` not found

**Step 3: Write minimal implementation**

Add the smooth weight table from the AV1 spec / dav1d `tables.c` at the top of `tile.rs`, after the existing constants:

```rust
#[rustfmt::skip]
const SM_WEIGHTS: [u8; 128] = [
      0,   0,
    255, 128,
    255, 149,  85,  64,
    255, 197, 146, 105,  73,  50,  37,  32,
    255, 225, 196, 170, 145, 123, 102,  84,
     68,  54,  43,  33,  26,  20,  17,  16,
    255, 240, 225, 210, 196, 182, 169, 157,
    145, 133, 122, 111, 101,  92,  83,  74,
     66,  59,  52,  45,  39,  34,  29,  25,
     21,  17,  14,  12,  10,   9,   8,   8,
    255, 248, 240, 233, 225, 218, 210, 203,
    196, 189, 182, 176, 169, 163, 156, 150,
    144, 138, 133, 127, 121, 116, 111, 106,
    101,  96,  91,  86,  82,  77,  73,  69,
     65,  61,  57,  54,  50,  47,  44,  41,
     38,  35,  32,  29,  27,  25,  22,  20,
     18,  16,  15,  13,  12,  10,   9,   8,
      7,   6,   6,   5,   5,   4,   4,   4,
];
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p wav1c smooth_weights -- --nocapture`
Expected: PASS

**Step 5: Commit**

```
feat: add AV1 smooth weight table for intra prediction
```

---

### Task 2: Add Intra Mode Context Mapping Table

The AV1 spec maps 13 intra modes to 5 context values for the `kf_y_mode` CDF lookup.

**Files:**
- Modify: `wav1c/src/tile.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn intra_mode_context_mapping() {
    assert_eq!(INTRA_MODE_CONTEXT[0], 0);  // DC_PRED
    assert_eq!(INTRA_MODE_CONTEXT[1], 1);  // V_PRED
    assert_eq!(INTRA_MODE_CONTEXT[2], 2);  // H_PRED
    assert_eq!(INTRA_MODE_CONTEXT[9], 0);  // SMOOTH_PRED
    assert_eq!(INTRA_MODE_CONTEXT[10], 1); // SMOOTH_V_PRED
    assert_eq!(INTRA_MODE_CONTEXT[11], 2); // SMOOTH_H_PRED
    assert_eq!(INTRA_MODE_CONTEXT[12], 0); // PAETH_PRED
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p wav1c intra_mode_context -- --nocapture`
Expected: FAIL — `INTRA_MODE_CONTEXT` not found

**Step 3: Write minimal implementation**

```rust
const INTRA_MODE_CONTEXT: [usize; 13] = [
    0, // DC_PRED
    1, // V_PRED
    2, // H_PRED
    3, // D45_PRED
    4, // D135_PRED
    4, // D113_PRED
    4, // D157_PRED
    4, // D203_PRED
    3, // D67_PRED
    0, // SMOOTH_PRED
    1, // SMOOTH_V_PRED
    2, // SMOOTH_H_PRED
    0, // PAETH_PRED
];
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p wav1c intra_mode_context -- --nocapture`
Expected: PASS

**Step 5: Commit**

```
feat: add AV1 intra mode context mapping table
```

---

### Task 3: Add Per-Pixel Prediction Functions

Implement the 7 prediction modes as standalone functions that return pixel arrays.

**Files:**
- Modify: `wav1c/src/tile.rs`

**Step 1: Write the failing tests**

```rust
#[test]
fn v_pred_copies_above_row() {
    let above = [10u8, 20, 30, 40];
    let left = [50u8, 60, 70, 80];
    let result = predict_v(&above, 4, 4);
    for r in 0..4 {
        for c in 0..4 {
            assert_eq!(result[r * 4 + c], above[c]);
        }
    }
}

#[test]
fn h_pred_copies_left_column() {
    let above = [10u8, 20, 30, 40];
    let left = [50u8, 60, 70, 80];
    let result = predict_h(&left, 4, 4);
    for r in 0..4 {
        for c in 0..4 {
            assert_eq!(result[r * 4 + c], left[r]);
        }
    }
}

#[test]
fn paeth_pred_uniform_neighbors() {
    let above = [100u8; 8];
    let left = [100u8; 8];
    let result = predict_paeth(&above, &left, 100, 8, 8);
    for &p in &result {
        assert_eq!(p, 100);
    }
}

#[test]
fn paeth_pred_vertical_edge() {
    let above = [200u8; 4];
    let left = [100u8; 4];
    let result = predict_paeth(&above, &left, 100, 4, 4);
    // base = 200 + 100 - 100 = 200
    // pLeft = |200 - 100| = 100, pTop = |200 - 200| = 0, pTL = |200 - 100| = 100
    // top wins -> 200
    for &p in &result {
        assert_eq!(p, 200);
    }
}

#[test]
fn smooth_pred_corners() {
    let above = [255u8, 255, 255, 255];
    let left = [0u8, 0, 0, 0];
    let result = predict_smooth(&above, &left, 4, 4);
    // top-left pixel: wY=255, wX=255
    // pred = 255*255 + (256-255)*0 + 255*0 + (256-255)*255 = 65025 + 0 + 0 + 255 = 65280
    // (65280 + 256) >> 9 = 128
    assert_eq!(result[0], 128);
}

#[test]
fn smooth_v_pred_top_row_matches_above() {
    let above = [200u8, 150, 100, 50];
    let left = [200u8, 128, 64, 0];
    let result = predict_smooth_v(&above, &left, 4, 4);
    // first row: weight[0]=255, pred = 255*above[j] + 1*bottom / 256
    // bottom = left[3] = 0
    // pred[0][0] = (255*200 + 1*0 + 128) >> 8 = (51000+128)/256 = 199
    assert!((result[0] as i32 - 200).abs() <= 1);
}

#[test]
fn smooth_h_pred_left_col_matches_left() {
    let above = [200u8, 150, 100, 50];
    let left = [200u8, 128, 64, 0];
    let result = predict_smooth_h(&above, &left, 4, 4);
    // first col: weight[0]=255, pred = 255*left[i] + 1*right / 256
    // right = above[3] = 50
    // pred[0][0] = (255*200 + 1*50 + 128) >> 8 = (51000+50+128)/256 = 199
    assert!((result[0] as i32 - 200).abs() <= 1);
}

#[test]
fn dc_pred_block_matches_scalar() {
    let above = [100u8, 120, 140, 160, 100, 120, 140, 160];
    let left = [80u8, 90, 100, 110, 80, 90, 100, 110];
    let result = predict_dc(&above, &left, true, true, 8, 8);
    let expected = {
        let sum: u32 = above.iter().chain(left.iter()).map(|&x| x as u32).sum();
        ((sum + 8) / 16) as u8
    };
    for &p in &result {
        assert_eq!(p, expected);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p wav1c pred -- --nocapture`
Expected: FAIL — functions not found

**Step 3: Write the prediction functions**

Add these functions to `tile.rs` (outside the impl blocks, as free functions or inside a prediction section):

```rust
fn predict_dc(above: &[u8], left: &[u8], have_above: bool, have_left: bool, w: usize, h: usize) -> Vec<u8> {
    let val = if have_above && have_left {
        let sum: u32 = above[..w].iter().chain(left[..h].iter()).map(|&x| x as u32).sum();
        ((sum + (w + h) as u32 / 2) / (w + h) as u32) as u8
    } else if have_above {
        let sum: u32 = above[..w].iter().map(|&x| x as u32).sum();
        ((sum + w as u32 / 2) / w as u32) as u8
    } else if have_left {
        let sum: u32 = left[..h].iter().map(|&x| x as u32).sum();
        ((sum + h as u32 / 2) / h as u32) as u8
    } else {
        128
    };
    vec![val; w * h]
}

fn predict_v(above: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    for r in 0..h {
        out[r * w..r * w + w].copy_from_slice(&above[..w]);
    }
    out
}

fn predict_h(left: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    for r in 0..h {
        for c in 0..w {
            out[r * w + c] = left[r];
        }
    }
    out
}

fn predict_paeth(above: &[u8], left: &[u8], top_left: u8, w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let tl = top_left as i32;
    for r in 0..h {
        let l = left[r] as i32;
        for c in 0..w {
            let t = above[c] as i32;
            let base = l + t - tl;
            let p_left = (base - l).abs();
            let p_top = (base - t).abs();
            let p_tl = (base - tl).abs();
            out[r * w + c] = if p_left <= p_top && p_left <= p_tl {
                left[r]
            } else if p_top <= p_tl {
                above[c]
            } else {
                top_left
            };
        }
    }
    out
}

fn predict_smooth(above: &[u8], left: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let weights_x = &SM_WEIGHTS[w..w * 2];
    let weights_y = &SM_WEIGHTS[h..h * 2];
    let right = above[w - 1] as i32;
    let bottom = left[h - 1] as i32;
    for r in 0..h {
        for c in 0..w {
            let wy = weights_y[r] as i32;
            let wx = weights_x[c] as i32;
            let pred = wy * above[c] as i32
                + (256 - wy) * bottom
                + wx * left[r] as i32
                + (256 - wx) * right;
            out[r * w + c] = ((pred + 256) >> 9) as u8;
        }
    }
    out
}

fn predict_smooth_v(above: &[u8], left: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let weights = &SM_WEIGHTS[h..h * 2];
    let bottom = left[h - 1] as i32;
    for r in 0..h {
        let wy = weights[r] as i32;
        for c in 0..w {
            let pred = wy * above[c] as i32 + (256 - wy) * bottom;
            out[r * w + c] = ((pred + 128) >> 8) as u8;
        }
    }
    out
}

fn predict_smooth_h(above: &[u8], left: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let weights = &SM_WEIGHTS[w..w * 2];
    let right = above[w - 1] as i32;
    for r in 0..h {
        let l = left[r] as i32;
        for c in 0..w {
            let wx = weights[c] as i32;
            let pred = wx * l + (256 - wx) * right;
            out[r * w + c] = ((pred + 128) >> 8) as u8;
        }
    }
    out
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p wav1c pred -- --nocapture`
Expected: PASS

**Step 5: Commit**

```
feat: add per-pixel intra prediction functions (DC, V, H, SMOOTH, PAETH)
```

---

### Task 4: Add Mode Context Tracking to TileContext

Add `above_mode` and `left_mode` arrays to track which intra mode each block used, for the `kf_y_mode` CDF context.

**Files:**
- Modify: `wav1c/src/tile.rs` (TileContext struct, new(), reset_left_for_sb_row())

**Step 1: Write the failing test**

```rust
#[test]
fn mode_context_initialized_to_dc() {
    let ctx = TileContext::new(16);
    for &m in &ctx.above_mode {
        assert_eq!(m, 0); // DC_PRED
    }
    for &m in &ctx.left_mode {
        assert_eq!(m, 0); // DC_PRED
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p wav1c mode_context_init -- --nocapture`
Expected: FAIL — no field `above_mode`

**Step 3: Write the implementation**

Add to `TileContext` struct:
```rust
above_mode: Vec<u8>,
left_mode: [u8; 32],
```

In `TileContext::new()`:
```rust
above_mode: vec![0u8; mi_cols as usize + 32],
left_mode: [0u8; 32],
```

In `reset_left_for_sb_row()`:
```rust
self.left_mode = [0u8; 32];
```

Add an `update_mode_ctx` method to `TileContext`:
```rust
fn update_mode_ctx(&mut self, bx: u32, by: u32, bl: usize, mi_cols: u32, mi_rows: u32, mode: u8) {
    let bx4 = bx as usize;
    let by4 = (by & 31) as usize;
    let bw4 = 2 * (16usize >> bl);
    let aw = min(bw4, (mi_cols - bx) as usize);
    let lh = min(bw4, (mi_rows - by) as usize);
    for i in 0..aw {
        if bx4 + i < self.above_mode.len() {
            self.above_mode[bx4 + i] = mode;
        }
    }
    for i in 0..lh {
        if by4 + i < 32 {
            self.left_mode[by4 + i] = mode;
        }
    }
}
```

Add a `mode_ctx` method to get above/left context for `kf_y_mode` CDF:
```rust
fn mode_ctx(&self, bx: u32, by: u32) -> (usize, usize) {
    let have_above = by > 0;
    let have_left = bx > 0;
    let above_mode = if have_above {
        let bx4 = bx as usize;
        if bx4 < self.above_mode.len() {
            self.above_mode[bx4] as usize
        } else {
            0
        }
    } else {
        0
    };
    let left_mode = if have_left {
        let by4 = (by & 31) as usize;
        self.left_mode[by4.min(31)] as usize
    } else {
        0
    };
    let above_ctx = INTRA_MODE_CONTEXT[above_mode.min(12)];
    let left_ctx = INTRA_MODE_CONTEXT[left_mode.min(12)];
    (above_ctx, left_ctx)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p wav1c mode_context -- --nocapture`
Expected: PASS

**Step 5: Commit**

```
feat: add intra mode context tracking to TileContext
```

---

### Task 5: Add SAD-Based Mode Selection

Add a method to the TileEncoder that tries all 7 modes and picks the one with the lowest SAD.

**Files:**
- Modify: `wav1c/src/tile.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn mode_selection_picks_dc_for_solid() {
    let above = [128u8; 8];
    let left = [128u8; 8];
    let block = [128u8; 64]; // solid 8x8
    let mode = select_best_intra_mode(&block, &above, &left, 128, true, true, 8, 8);
    assert_eq!(mode, 0); // DC_PRED
}

#[test]
fn mode_selection_picks_v_for_vertical_pattern() {
    let above = [10u8, 20, 30, 40, 50, 60, 70, 80];
    let left = [128u8; 8];
    let mut block = [0u8; 64];
    for r in 0..8 {
        for c in 0..8 {
            block[r * 8 + c] = above[c]; // vertical pattern
        }
    }
    let mode = select_best_intra_mode(&block, &above, &left, 128, true, true, 8, 8);
    assert_eq!(mode, 1); // V_PRED
}

#[test]
fn mode_selection_picks_h_for_horizontal_pattern() {
    let above = [128u8; 8];
    let left = [10u8, 20, 30, 40, 50, 60, 70, 80];
    let mut block = [0u8; 64];
    for r in 0..8 {
        for c in 0..8 {
            block[r * 8 + c] = left[r]; // horizontal pattern
        }
    }
    let mode = select_best_intra_mode(&block, &above, &left, 128, true, true, 8, 8);
    assert_eq!(mode, 2); // H_PRED
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p wav1c mode_selection -- --nocapture`
Expected: FAIL — `select_best_intra_mode` not found

**Step 3: Write the implementation**

```rust
fn compute_sad(source: &[u8], prediction: &[u8]) -> u32 {
    source.iter().zip(prediction.iter())
        .map(|(&s, &p)| (s as i32 - p as i32).unsigned_abs())
        .sum()
}

fn select_best_intra_mode(
    source: &[u8],
    above: &[u8],
    left: &[u8],
    top_left: u8,
    have_above: bool,
    have_left: bool,
    w: usize,
    h: usize,
) -> u8 {
    let dc = predict_dc(above, left, have_above, have_left, w, h);
    let mut best_mode = 0u8;
    let mut best_sad = compute_sad(source, &dc);

    if have_above {
        let v = predict_v(above, w, h);
        let sad = compute_sad(source, &v);
        if sad < best_sad {
            best_sad = sad;
            best_mode = 1;
        }
    }

    if have_left {
        let hp = predict_h(left, w, h);
        let sad = compute_sad(source, &hp);
        if sad < best_sad {
            best_sad = sad;
            best_mode = 2;
        }
    }

    if have_above && have_left {
        let smooth = predict_smooth(above, left, w, h);
        let sad = compute_sad(source, &smooth);
        if sad < best_sad {
            best_sad = sad;
            best_mode = 9;
        }

        let sv = predict_smooth_v(above, left, w, h);
        let sad = compute_sad(source, &sv);
        if sad < best_sad {
            best_sad = sad;
            best_mode = 10;
        }

        let sh = predict_smooth_h(above, left, w, h);
        let sad = compute_sad(source, &sh);
        if sad < best_sad {
            best_sad = sad;
            best_mode = 11;
        }

        let paeth = predict_paeth(above, left, top_left, w, h);
        let sad = compute_sad(source, &paeth);
        if sad < best_sad {
            best_mode = 12;
        }
    }

    best_mode
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p wav1c mode_selection -- --nocapture`
Expected: PASS

**Step 5: Commit**

```
feat: add SAD-based intra mode selection
```

---

### Task 6: Refactor encode_block to Use Per-Pixel Prediction and Mode Selection

This is the core change. Replace the scalar DC prediction in `encode_block` with per-pixel prediction using the selected best mode.

**Files:**
- Modify: `wav1c/src/tile.rs` — `TileEncoder::encode_block()`

**Step 1: No new unit test — this is wiring existing tested components. We'll validate with integration tests.**

**Step 2: Modify `encode_block`**

The key changes in `encode_block(bx, by, bl)`:

1. **Extract neighbor context** — get above/left pixel arrays and top-left pixel for prediction:

```rust
// For luma (plane 0):
let luma_size = 8; // always 8 at bl=4
let px_x = bx * 4;
let px_y = by * 4;
let have_above = by > 0;
let have_left = bx > 0;

let above_y: Vec<u8> = (0..luma_size)
    .map(|i| {
        let idx = px_x as usize + i;
        if have_above && idx < self.ctx.above_recon_y.len() {
            self.ctx.above_recon_y[idx]
        } else {
            128
        }
    })
    .collect();

let left_local_py = ((by & 15) * 4) as usize;
let left_y: Vec<u8> = (0..luma_size)
    .map(|i| {
        let idx = left_local_py + i;
        if have_left && idx < self.ctx.left_recon_y.len() {
            self.ctx.left_recon_y[idx]
        } else {
            128
        }
    })
    .collect();

let top_left_y = if have_above && have_left {
    // The pixel at (bx*4-1, by*4-1) in reconstructed frame
    // Approximate: use left_recon at local position above current block
    if left_local_py > 0 {
        self.ctx.left_recon_y[left_local_py - 1]
    } else if (px_x as usize) > 0 {
        self.ctx.above_recon_y[px_x as usize - 1]
    } else {
        128
    }
} else {
    128
};
```

2. **Select best Y mode and generate prediction block:**

```rust
let y_block = extract_block(&self.pixels.y, w, px_x, px_y, 8, w, h);
let y_mode = select_best_intra_mode(
    &y_block, &above_y, &left_y, top_left_y,
    have_above, have_left, 8, 8,
);
let y_pred_block = generate_prediction(y_mode, &above_y, &left_y, top_left_y, have_above, have_left, 8, 8);
```

Add a helper function:
```rust
fn generate_prediction(
    mode: u8, above: &[u8], left: &[u8], top_left: u8,
    have_above: bool, have_left: bool, w: usize, h: usize,
) -> Vec<u8> {
    match mode {
        0 => predict_dc(above, left, have_above, have_left, w, h),
        1 => predict_v(above, w, h),
        2 => predict_h(left, w, h),
        9 => predict_smooth(above, left, w, h),
        10 => predict_smooth_v(above, left, w, h),
        11 => predict_smooth_h(above, left, w, h),
        12 => predict_paeth(above, left, top_left, w, h),
        _ => predict_dc(above, left, have_above, have_left, w, h),
    }
}
```

3. **Compute per-pixel residual:**

```rust
let mut y_residual = [0i32; 64];
for i in 0..64 {
    y_residual[i] = y_block[i] as i32 - y_pred_block[i] as i32;
}
```

4. **Chroma — use DC for UV (match Y mode later as improvement):**

For chroma, use DC prediction for now (simplest correct approach):
```rust
let u_pred = self.ctx.dc_prediction(bx, by, bl, 1);
let v_pred = self.ctx.dc_prediction(bx, by, bl, 2);
// ... residual same as before using scalar u_pred/v_pred
```

5. **Encode the mode in bitstream:**

Replace:
```rust
self.enc.encode_symbol(0, &mut self.cdf.kf_y_mode[0][0], 12);
```

With:
```rust
let (above_mode_ctx, left_mode_ctx) = self.ctx.mode_ctx(bx, by);
self.enc.encode_symbol(y_mode as u32, &mut self.cdf.kf_y_mode[above_mode_ctx][left_mode_ctx], 12);
```

Note: `kf_y_mode` has 12 symbols (modes 0-11 in CDF terms), but the AV1 spec encodes 13 modes (0-12). Looking at the CDF tables: `DEFAULT_KF_Y_MODE_CDF: [[[u16; 16]; 5]; 5]` — the CDFs have 12 non-zero values, so there are actually 13 symbols (N symbols = N-1 CDF entries + implicit last). Actually `encode_symbol` with nsyms=12 only handles symbols 0-11. We need nsyms=13 for 13 modes (DC=0 through PAETH=12).

**IMPORTANT**: Check current code uses `12` as nsyms. The CDF has 12 entries (indices 0-11 are CDF values), which means 13 symbols (0-12). The `encode_symbol` function needs `nsyms` = number of symbols. Looking at the CDF layout: 12 non-zero values in each CDF means 13 symbols. So we need to change `12` to `13`:

```rust
self.enc.encode_symbol(y_mode as u32, &mut self.cdf.kf_y_mode[above_mode_ctx][left_mode_ctx], 13);
```

Wait — examine the existing code: `encode_symbol(0, &mut self.cdf.kf_y_mode[0][0], 12)`. This encodes symbol 0 with 12 possible symbols. But AV1 has 13 intra modes. Let's verify: the CDF array has 16 u16 values, with the last 4 being 0/padding. The non-zero count is 12 (look at line 5: 12 non-zero values before the zeros). In AV1, nsyms for kf_y_mode is 13 (INTRA_MODES = 13). So **the existing code is wrong** — it uses 12 when it should be 13. This is fine because it only ever encodes symbol 0 (DC_PRED), but now we need to fix it to 13.

```rust
self.enc.encode_symbol(y_mode as u32, &mut self.cdf.kf_y_mode[above_mode_ctx][left_mode_ctx], 13);
```

6. **UV mode — keep DC (mode 0):**

The uv_mode encoding stays as-is for now:
```rust
let cfl_allowed = bl >= 2;
let uv_n_syms = if cfl_allowed { 13 } else { 12 };
let cfl_idx = usize::from(cfl_allowed);
self.enc.encode_symbol(0, &mut self.cdf.uv_mode[cfl_idx][y_mode as usize], uv_n_syms);
```

Note the change: `uv_mode[cfl_idx][y_mode as usize]` instead of `uv_mode[cfl_idx][0]` — the UV CDF is indexed by the Y mode.

7. **Reconstruct per-pixel:**

Replace:
```rust
let pixel = (y_pred as i32 + y_recon_residual[(r * 8 + c) as usize]).clamp(0, 255) as u8;
```

With:
```rust
let pixel = (y_pred_block[(r * 8 + c) as usize] as i32 + y_recon_residual[(r * 8 + c) as usize]).clamp(0, 255) as u8;
```

8. **Update mode context after block:**

Add after the existing context updates:
```rust
self.ctx.update_mode_ctx(bx, by, bl, self.mi_cols, self.mi_rows, y_mode);
```

**Step 3: Run unit tests**

Run: `cargo test -p wav1c -- --nocapture`
Expected: PASS (existing tests should still work since solid blocks will pick DC)

**Step 4: Commit**

```
feat: wire per-pixel prediction and mode selection into encode_block
```

---

### Task 7: Refactor encode_skip_block for Mode Context

The skip block also needs to encode the correct mode and update mode context.

**Files:**
- Modify: `wav1c/src/tile.rs` — `TileEncoder::encode_skip_block()`

**Step 1: Modify encode_skip_block**

For skip blocks, we still use DC prediction (mode 0) since the block is flat enough to skip. But we need to:

1. Use proper mode context for kf_y_mode CDF:

Replace:
```rust
self.enc.encode_symbol(0, &mut self.cdf.kf_y_mode[0][0], 12);
```

With:
```rust
let (above_mode_ctx, left_mode_ctx) = self.ctx.mode_ctx(bx, by);
self.enc.encode_symbol(0, &mut self.cdf.kf_y_mode[above_mode_ctx][left_mode_ctx], 13);
```

2. Use proper UV mode context:

Replace:
```rust
self.enc.encode_symbol(0, &mut self.cdf.uv_mode[cfl_idx][0], uv_n_syms);
```

With:
```rust
self.enc.encode_symbol(0, &mut self.cdf.uv_mode[cfl_idx][0], uv_n_syms);
```

(This stays the same — y_mode=0=DC_PRED, so the UV CDF index is still [0].)

3. Update mode context:

Add before the existing context updates:
```rust
self.ctx.update_mode_ctx(bx, by, bl, self.mi_cols, self.mi_rows, 0);
```

**Step 2: Run unit tests**

Run: `cargo test -p wav1c -- --nocapture`
Expected: PASS

**Step 3: Commit**

```
feat: update encode_skip_block for mode context tracking
```

---

### Task 8: Run Full Test Suite and Validate with dav1d

**Step 1: Run clippy**

Run: `cargo clippy -p wav1c --tests -- -D warnings`
Expected: No warnings

**Step 2: Run all unit tests**

Run: `cargo test -p wav1c -- --nocapture`
Expected: All pass

**Step 3: Run integration tests with dav1d**

Run: `cargo test -p wav1c --test integration -- --nocapture`
Expected: All pass. The critical tests:
- `dav1d_decodes_default_gray` — solid gray still works (DC mode)
- `dav1d_decodes_all_colors` — all solid colors still work
- `decoded_pixels_match_input` — pixel accuracy maintained for solid colors
- `dav1d_decodes_gradient_y4m` — gradients decode correctly (may now use V/H modes)
- `dav1d_decodes_various_dimensions` — all dimensions still work
- `dav1d_decodes_multi_frame_solid` — multi-frame still works
- `dav1d_decodes_multi_frame_gradient` — multi-frame gradients still work

**Step 4: If any integration test fails, debug**

The most likely failure point is the mode/CDF encoding. If dav1d fails to decode:
1. Check nsyms (must be 13 for kf_y_mode)
2. Check mode context (above_mode_ctx/left_mode_ctx must be valid 0-4 indices)
3. Check uv_mode CDF index (y_mode must be 0-12)
4. Temporarily force y_mode=0 (DC only) to verify the refactoring didn't break reconstruction

**Step 5: Commit**

```
test: verify all intra prediction modes pass dav1d decoding
```

---

### Task 9: Add Quality Validation Test

Add a test that encodes a gradient image and verifies that multiple prediction modes are actually being selected.

**Files:**
- Modify: `wav1c/tests/integration.rs`

**Step 1: Write the test**

```rust
#[test]
fn dav1d_decodes_edge_patterns() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    // Vertical stripes — should benefit from V_PRED
    let v_stripes = create_test_y4m(128, 128, |col, _row| {
        let y = if col % 16 < 8 { 50 } else { 200 };
        (y, 128, 128)
    });
    let pixels = FramePixels::from_y4m(&v_stripes);
    let output = wav1c::encode_av1_ivf_y4m(&pixels);
    let (success, stderr, _) = decode_to_y4m(&dav1d, &output, "v_stripes");
    assert!(success, "dav1d failed for vertical stripes: {}", stderr);

    // Horizontal stripes — should benefit from H_PRED
    let h_stripes = create_test_y4m(128, 128, |_col, row| {
        let y = if row % 16 < 8 { 50 } else { 200 };
        (y, 128, 128)
    });
    let pixels = FramePixels::from_y4m(&h_stripes);
    let output = wav1c::encode_av1_ivf_y4m(&pixels);
    let (success, stderr, _) = decode_to_y4m(&dav1d, &output, "h_stripes");
    assert!(success, "dav1d failed for horizontal stripes: {}", stderr);

    // Diagonal gradient — should benefit from SMOOTH/PAETH
    let diag = create_test_y4m(128, 128, |col, row| {
        let y = ((col + row) * 255 / 254).min(255) as u8;
        (y, 128, 128)
    });
    let pixels = FramePixels::from_y4m(&diag);
    let output = wav1c::encode_av1_ivf_y4m(&pixels);
    let (success, stderr, _) = decode_to_y4m(&dav1d, &output, "diagonal");
    assert!(success, "dav1d failed for diagonal gradient: {}", stderr);
}
```

**Step 2: Run the test**

Run: `cargo test -p wav1c --test integration dav1d_decodes_edge_patterns -- --nocapture`
Expected: PASS

**Step 3: Commit**

```
test: add edge pattern validation for intra prediction modes
```

---

### Task 10: Add PSNR Measurement Utility Test

Add a test that measures PSNR before and after the mode selection improvement on a gradient image to quantify the quality gain.

**Files:**
- Modify: `wav1c/tests/integration.rs`

**Step 1: Write the PSNR helper and test**

```rust
fn compute_psnr(original: &[u8], decoded: &[u8]) -> f64 {
    let mse: f64 = original.iter().zip(decoded.iter())
        .map(|(&o, &d)| {
            let diff = o as f64 - d as f64;
            diff * diff
        })
        .sum::<f64>() / original.len() as f64;

    if mse == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0_f64 * 255.0 / mse).log10()
}

#[test]
fn gradient_psnr_above_threshold() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let y4m_data = create_test_y4m(128, 128, |col, _row| {
        let y = (col * 255 / 127).min(255) as u8;
        (y, 128, 128)
    });
    let pixels = FramePixels::from_y4m(&y4m_data);
    let output = wav1c::encode_av1_ivf_y4m(&pixels);
    let (success, stderr, decoded_y4m) = decode_to_y4m(&dav1d, &output, "psnr_gradient");
    assert!(success, "dav1d failed: {}", stderr);

    let (y_decoded, _, _) = extract_y4m_planes(&decoded_y4m, 128, 128);
    let psnr = compute_psnr(&pixels.y, &y_decoded);
    eprintln!("Gradient PSNR: {:.2} dB", psnr);
    assert!(psnr > 25.0, "PSNR too low: {:.2} dB", psnr);
}
```

**Step 2: Run the test**

Run: `cargo test -p wav1c --test integration gradient_psnr -- --nocapture`
Expected: PASS with PSNR printed

**Step 3: Commit**

```
test: add PSNR measurement for gradient quality validation
```

---

## Summary

| Task | Description | Key Risk |
|------|-------------|----------|
| 1 | Smooth weight table | None — data only |
| 2 | Mode context mapping table | None — data only |
| 3 | Per-pixel prediction functions | Algorithm correctness |
| 4 | Mode context tracking in TileContext | Array indexing bugs |
| 5 | SAD-based mode selection | None — pure function |
| 6 | Wire into encode_block | **Highest risk** — bitstream changes |
| 7 | Update encode_skip_block | Mode context consistency |
| 8 | Full test suite validation | dav1d decode compatibility |
| 9 | Edge pattern validation tests | None — test only |
| 10 | PSNR measurement | None — test only |

Tasks 1-5 are safe (no bitstream changes). Task 6 is the critical integration point where bitstream encoding changes. Task 8 validates everything end-to-end.
