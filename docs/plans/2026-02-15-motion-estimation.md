# Motion Estimation for Inter Frames Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add block-level motion estimation to inter frames, replacing zero-motion GLOBALMV with NEWMV when a non-zero motion vector reduces prediction error.

**Architecture:** For each 8x8 block, search a ±16 pixel window in the reference frame using SAD. If the best MV is non-zero and gives better RD cost, use NEWMV mode and encode the MV delta. Track per-block MVs for neighbor-based MV prediction matching dav1d's algorithm.

**Tech Stack:** Rust, existing MSAC/CDF/DCT infrastructure in wav1c

---

### Task 1: Add MV CDFs to cdf.rs

**Files:**
- Modify: `wav1c/src/cdf.rs`

**Step 1: Add MvComponentCdf struct and MV CDF defaults**

Add the MV component CDF struct and default values from dav1d (`dav1d/src/cdf.c` lines 612-638). The CDF arrays follow the same format as existing wav1c CDFs: `[value, 0, 0, 0]` for boolean CDFs, `[v0, v1, ..., 0, 0]` for multi-symbol CDFs with adaptation counter at the end.

```rust
pub struct MvComponentCdf {
    pub sign: [u16; 4],
    pub classes: [u16; 16],
    pub class0: [u16; 4],
    pub class0_fp: [[u16; 8]; 2],
    pub classN: [[u16; 4]; 10],
    pub classN_fp: [u16; 8],
}

pub struct MvCdf {
    pub joint: [u16; 8],
    pub comp: [MvComponentCdf; 2],
}
```

Default values (from dav1d `src/cdf.c:612-638`):
- `joint: [4096, 11264, 19328, 0, 0, 0, 0, 0]` (3 symbols for 4 values)
- `sign: [16384, 0, 0, 0]`
- `classes: [28672, 30976, 31858, 32320, 32551, 32656, 32740, 32757, 32762, 32767, 0, 0, 0, 0, 0, 0]` (10 symbols for 11 values)
- `class0: [27648, 0, 0, 0]`
- `class0_fp[0]: [16384, 24576, 26624, 0, 0, 0, 0, 0]` and `class0_fp[1]: [12288, 21248, 24128, 0, 0, 0, 0, 0]` (3 symbols for 4 values)
- `classN[0..10]`: `[17408,0,0,0], [17920,0,0,0], [18944,0,0,0], [20480,0,0,0], [22528,0,0,0], [24576,0,0,0], [28672,0,0,0], [29952,0,0,0], [29952,0,0,0], [30720,0,0,0]`
- `classN_fp: [8192, 17408, 21248, 0, 0, 0, 0, 0]`

Both comp[0] and comp[1] use the same default values.

**Step 2: Add DRL CDF defaults**

Add DRL bit CDFs (3 contexts, boolean):
```rust
pub const DEFAULT_DRL_CDF: [[u16; 4]; 3] = [
    [28160, 0, 0, 0],
    [30592, 0, 0, 0],
    [24320, 0, 0, 0],
];
```

**Step 3: Add fields to CdfContext**

Add to `CdfContext` struct:
```rust
pub mv: MvCdf,
pub drl: [[u16; 4]; 3],
```

Initialize in `for_qidx()` using the default values.

**Step 4: Run tests**

Run: `cargo test -p wav1c`
Expected: All existing tests PASS (no behavioral changes, only added CDF data)

---

### Task 2: Add MV component encoding functions

**Files:**
- Modify: `wav1c/src/tile.rs`

**Step 1: Write test for MV component decomposition**

Add to `#[cfg(test)] mod tests`:

```rust
#[test]
fn mv_diff_decompose_one_pixel() {
    let (cl, up, fp) = decompose_mv_diff(8);
    assert_eq!((cl, up, fp), (0, 0, 3));
}

#[test]
fn mv_diff_decompose_two_pixels() {
    let (cl, up, fp) = decompose_mv_diff(16);
    assert_eq!((cl, up, fp), (0, 1, 3));
}

#[test]
fn mv_diff_decompose_three_pixels() {
    let (cl, up, fp) = decompose_mv_diff(24);
    assert_eq!((cl, up, fp), (1, 2, 3));
}

#[test]
fn mv_diff_roundtrip() {
    for diff in 1..=128 {
        let (cl, up, fp) = decompose_mv_diff(diff);
        let reconstructed = ((up << 3) | (fp << 1) | 1) + 1;
        assert_eq!(reconstructed, diff as i32, "diff={diff}");
    }
}
```

**Step 2: Implement `decompose_mv_diff`**

This function converts an absolute MV diff value (in 1/8-pel units, always positive, ≥1) into (class, up_magnitude, fractional_position) components. `hp` is always 1 since `allow_high_precision_mv = 0`.

The formula: `diff = ((up << 3) | (fp << 1) | 1) + 1`, so we need to reverse it:
- `raw = diff - 1` (raw has hp=1 in bit 0)
- `fp = (raw >> 1) & 3`
- `up = raw >> 3`

Then determine class from `up`:
- class 0: `up` ∈ {0, 1}
- class N (N≥1): `up` ∈ [1<<N, (1<<(N+1))-1]

```rust
fn decompose_mv_diff(diff: u32) -> (u32, u32, u32) {
    let raw = diff - 1;
    let fp = (raw >> 1) & 3;
    let up = raw >> 3;

    if up < 2 {
        return (0, up, fp);
    }

    let cl = 32 - (up as u32).leading_zeros() - 1;
    (cl, up, fp)
}
```

**Step 3: Implement `encode_mv_component`**

Encode a single MV component (vertical or horizontal). The component value is a signed integer in 1/8-pel units. Follows dav1d `read_mv_component_diff` in reverse.

```rust
fn encode_mv_component(
    enc: &mut MsacEncoder,
    comp_cdf: &mut MvComponentCdf,
    value: i32,
) {
    let sign = value < 0;
    let abs_val = value.unsigned_abs();

    enc.encode_bool(sign, &mut comp_cdf.sign);

    let (cl, up, fp) = decompose_mv_diff(abs_val);

    enc.encode_symbol(cl, &mut comp_cdf.classes, 10);

    if cl == 0 {
        enc.encode_bool(up != 0, &mut comp_cdf.class0);
        enc.encode_symbol(fp, &mut comp_cdf.class0_fp[up as usize], 3);
    } else {
        for n in 0..cl {
            let bit = (up >> n) & 1;
            enc.encode_bool(bit != 0, &mut comp_cdf.classN[n as usize]);
        }
        enc.encode_symbol(fp, &mut comp_cdf.classN_fp, 3);
    }
}
```

**Step 4: Implement `encode_mv_residual`**

Encode the full MV residual (joint + components):

```rust
fn encode_mv_residual(
    enc: &mut MsacEncoder,
    mv_cdf: &mut MvCdf,
    dy: i32,
    dx: i32,
) {
    let joint = match (dy != 0, dx != 0) {
        (false, false) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (true, true) => 3,
    };

    enc.encode_symbol(joint, &mut mv_cdf.joint, 3);

    if dy != 0 {
        encode_mv_component(enc, &mut mv_cdf.comp[0], dy);
    }
    if dx != 0 {
        encode_mv_component(enc, &mut mv_cdf.comp[1], dx);
    }
}
```

**Step 5: Run tests**

Run: `cargo test -p wav1c mv_diff`
Expected: All 4 decomposition tests PASS

---

### Task 3: Add motion search function

**Files:**
- Modify: `wav1c/src/tile.rs`

**Step 1: Write test for motion search**

```rust
#[test]
fn motion_search_finds_shifted_block() {
    let mut reference = vec![128u8; 64 * 64];
    for r in 10..18 {
        for c in 14..22 {
            reference[r * 64 + c] = 200;
        }
    }
    let mut source = vec![128u8; 64 * 64];
    for r in 10..18 {
        for c in 10..18 {
            source[r * 64 + c] = 200;
        }
    }
    let (dx, dy) = motion_search_block(
        &source, &reference, 64, 64, 10, 10, 8,
    );
    assert_eq!((dx, dy), (4, 0));
}

#[test]
fn motion_search_zero_when_same() {
    let reference = vec![200u8; 64 * 64];
    let source = vec![200u8; 64 * 64];
    let (dx, dy) = motion_search_block(
        &source, &reference, 64, 64, 10, 10, 8,
    );
    assert_eq!((dx, dy), (0, 0));
}
```

**Step 2: Implement `motion_search_block`**

Full-pixel exhaustive search in a ±16 pixel window using SAD:

```rust
fn motion_search_block(
    source: &[u8],
    reference: &[u8],
    width: u32,
    height: u32,
    px_x: u32,
    px_y: u32,
    block_size: u32,
) -> (i32, i32) {
    let mut best_sad = u64::MAX;
    let mut best_dx: i32 = 0;
    let mut best_dy: i32 = 0;

    let search_range: i32 = 16;

    for dy in -search_range..=search_range {
        for dx in -search_range..=search_range {
            let ref_x = px_x as i32 + dx;
            let ref_y = px_y as i32 + dy;

            if ref_x < 0
                || ref_y < 0
                || (ref_x + block_size as i32) > width as i32
                || (ref_y + block_size as i32) > height as i32
            {
                continue;
            }

            let mut sad: u64 = 0;
            for r in 0..block_size {
                for c in 0..block_size {
                    let s = source[((px_y + r) * width + px_x + c) as usize] as i32;
                    let rf = reference[((ref_y as u32 + r) * width + ref_x as u32 + c) as usize] as i32;
                    sad += (s - rf).unsigned_abs() as u64;
                }
            }

            if sad < best_sad || (sad == best_sad && dx.abs() + dy.abs() < best_dx.abs() + best_dy.abs()) {
                best_sad = sad;
                best_dx = dx;
                best_dy = dy;
            }
        }
    }

    (best_dx, best_dy)
}
```

Note: returns motion vector in integer pixels. The caller converts to 1/8-pel units by multiplying by 8.

**Step 3: Run tests**

Run: `cargo test -p wav1c motion_search`
Expected: Both tests PASS

---

### Task 4: Add MV prediction and per-block MV storage

**Files:**
- Modify: `wav1c/src/tile.rs`

This is the critical task for correctness. The encoder must compute the same predicted MV that dav1d computes, so the residual encodes/decodes correctly.

**Step 1: Add MV storage to InterTileEncoder**

Add per-block MV storage in 4x4 units. Each entry stores the MV (in 1/8-pel units) and the reference frame index. For our case, all inter blocks use ref=0 (LAST_FRAME).

```rust
#[derive(Clone, Copy, Default)]
struct BlockMv {
    mv_x: i32,
    mv_y: i32,
    ref_frame: i8,
    is_newmv: bool,
}
```

Add to `InterTileEncoder`:
```rust
block_mvs: Vec<BlockMv>,  // indexed by (by4 * mi_cols + bx4) in 4x4 units
```

Initialize with `vec![BlockMv::default(); (mi_cols * mi_rows) as usize]` with `ref_frame = -1` (intra/unused).

After encoding each inter block, splat the chosen MV to all 4x4 positions within the block (2x2 for 8x8 blocks).

**Step 2: Implement simplified MV prediction matching dav1d**

For 8x8 blocks with single reference (LAST_FRAME), dav1d's `refmvs_find` scans spatial neighbors in this order:

1. Top row: 2 positions at (bx4, by4-1) and (bx4+1, by4-1)
2. Left column: 2 positions at (bx4-1, by4) and (bx4-1, by4+1)
3. Top-right: position (bx4+2, by4-1)

For each position, if the stored block has the same reference frame (0 = LAST_FRAME), its MV is added as a candidate. De-duplicate: if same MV already in list, add to weight instead of creating new entry.

After collecting, add 640 to all spatial candidate weights, then sort by weight descending. `mvstack[0]` is the NEWMV prediction base.

If no candidates exist, the prediction defaults to the global MV = (0, 0).

```rust
struct MvCandidate {
    mv_x: i32,
    mv_y: i32,
    weight: u32,
}

fn predict_mv(
    block_mvs: &[BlockMv],
    mi_cols: u32,
    bx4: u32,
    by4: u32,
) -> (i32, i32, u32) {
    // Returns (pred_mv_x, pred_mv_y, n_candidates)
    let mut candidates: Vec<MvCandidate> = Vec::new();

    // Scan top row
    if by4 > 0 {
        for col in bx4..bx4 + 2 {
            if col < mi_cols {
                let idx = ((by4 - 1) * mi_cols + col) as usize;
                let b = &block_mvs[idx];
                if b.ref_frame == 0 {
                    add_candidate(&mut candidates, b.mv_x, b.mv_y, 2);
                }
            }
        }
    }

    // Scan left column
    if bx4 > 0 {
        for row in by4..by4 + 2 {
            let idx = (row * mi_cols + bx4 - 1) as usize;
            let b = &block_mvs[idx];
            if b.ref_frame == 0 {
                add_candidate(&mut candidates, b.mv_x, b.mv_y, 2);
            }
        }
    }

    // Top-right
    if by4 > 0 && bx4 + 2 < mi_cols {
        let idx = ((by4 - 1) * mi_cols + bx4 + 2) as usize;
        let b = &block_mvs[idx];
        if b.ref_frame == 0 {
            add_candidate(&mut candidates, b.mv_x, b.mv_y, 2);
        }
    }

    if candidates.is_empty() {
        return (0, 0, 0);
    }

    // Add 640 to spatial weights
    for c in &mut candidates {
        c.weight += 640;
    }

    // Sort by weight descending
    candidates.sort_by(|a, b| b.weight.cmp(&a.weight));

    (candidates[0].mv_x, candidates[0].mv_y, candidates.len() as u32)
}

fn add_candidate(candidates: &mut Vec<MvCandidate>, mv_x: i32, mv_y: i32, weight: u32) {
    for c in candidates.iter_mut() {
        if c.mv_x == mv_x && c.mv_y == mv_y {
            c.weight += weight;
            return;
        }
    }
    candidates.push(MvCandidate { mv_x, mv_y, weight });
}
```

**Step 3: Implement DRL context computation**

Match dav1d's `get_drl_context`:

```rust
fn get_drl_context(candidates: &[MvCandidate], ref_idx: usize) -> usize {
    if candidates.len() <= ref_idx + 1 {
        return 2;
    }
    let cur_weight = candidates[ref_idx].weight;
    let next_weight = candidates[ref_idx + 1].weight;
    if cur_weight >= 640 {
        if next_weight < 640 { 1 } else { 0 }
    } else {
        if next_weight < 640 { 2 } else { 0 }
    }
}
```

**Step 4: Write test for MV prediction**

```rust
#[test]
fn mv_prediction_no_neighbors() {
    let mi_cols = 10u32;
    let mi_rows = 10u32;
    let block_mvs = vec![BlockMv::default(); (mi_cols * mi_rows) as usize];
    let (px, py, n) = predict_mv(&block_mvs, mi_cols, 0, 0);
    assert_eq!((px, py, n), (0, 0, 0));
}

#[test]
fn mv_prediction_from_left_neighbor() {
    let mi_cols = 10u32;
    let mi_rows = 10u32;
    let mut block_mvs = vec![BlockMv::default(); (mi_cols * mi_rows) as usize];
    // Set left neighbor (bx4=1, by4=2) with MV (16, 8)
    for row in 2..4 {
        for col in 0..2 {
            let idx = (row * mi_cols + col) as usize;
            block_mvs[idx] = BlockMv { mv_x: 16, mv_y: 8, ref_frame: 0, is_newmv: true };
        }
    }
    let (px, py, _n) = predict_mv(&block_mvs, mi_cols, 2, 2);
    assert_eq!((px, py), (16, 8));
}
```

**Step 5: Run tests**

Run: `cargo test -p wav1c mv_prediction`
Expected: Both tests PASS

---

### Task 5: Integrate motion estimation into encode_inter_block

**Files:**
- Modify: `wav1c/src/tile.rs`

This is the core integration task. Modify `encode_inter_block` to:
1. Run motion search for each block
2. Compute MV prediction from neighbors
3. Choose GLOBALMV or NEWMV based on which gives better results
4. If NEWMV, encode DRL + MV residual
5. Use the motion-compensated reference block for residual coding
6. Store the chosen MV for future neighbor reference

**Step 1: Update encode_inter_block**

The key changes to the mode decision tree:

Currently:
```rust
// Always GLOBALMV with zero motion
encode_bool(true, newmv);   // true = NOT newmv
encode_bool(false, zeromv);  // false = GLOBALMV
```

New logic:
```rust
// Motion search
let (dx_pixels, dy_pixels) = motion_search_block(
    &self.reference.y, &self.pixels.y, w, h, px_x, px_y, 8,
);
let mv_x = dx_pixels * 8;  // Convert to 1/8-pel
let mv_y = dy_pixels * 8;

// Get MV prediction from neighbors
let bx4 = bx;  // bx is already in 4x4 units? Check
let by4 = by;
let (pred_x, pred_y, n_mvs) = predict_mv(&self.block_mvs, self.mi_cols, bx4, by4);

// Decide GLOBALMV vs NEWMV
let use_newmv = mv_x != 0 || mv_y != 0;

if use_newmv {
    enc.encode_bool(false, &mut cdf.newmv[newmv_ctx]);  // false = NEWMV

    // DRL: always use index 0 (nearest candidate)
    if n_mvs > 1 {
        let drl_ctx = get_drl_context(&candidates, 0);
        enc.encode_bool(false, &mut cdf.drl[drl_ctx]);  // false = use index 0
    }

    // Encode MV residual
    let diff_x = mv_x - pred_x;
    let diff_y = mv_y - pred_y;
    encode_mv_residual(&mut enc, &mut cdf.mv, diff_y, diff_x);
} else {
    enc.encode_bool(true, &mut cdf.newmv[newmv_ctx]);   // true = NOT newmv
    enc.encode_bool(false, &mut cdf.zeromv[zeromv_ctx]); // false = GLOBALMV
}
```

**Step 2: Use motion-compensated reference block**

When NEWMV is selected, extract the reference block at the offset position:

```rust
let ref_px_x = (px_x as i32 + dx_pixels) as u32;
let ref_px_y = (px_y as i32 + dy_pixels) as u32;
let y_ref_block = extract_block(&self.reference.y, w, ref_px_x, ref_px_y, 8, w, h);
```

For chroma (4:2:0), the MV is halved:
```rust
let chroma_ref_x = (chroma_px_x as i32 + dx_pixels / 2) as u32;
let chroma_ref_y = (chroma_px_y as i32 + dy_pixels / 2) as u32;
let u_ref_block = extract_block(&self.reference.u, cw, chroma_ref_x, chroma_ref_y, 4, cw, ch);
let v_ref_block = extract_block(&self.reference.v, cw, chroma_ref_x, chroma_ref_y, 4, cw, ch);
```

**Step 3: Store MV after encoding**

After encoding the block, splat the MV to all 4x4 positions:

```rust
let stored_mv = BlockMv {
    mv_x: if use_newmv { mv_x } else { 0 },
    mv_y: if use_newmv { mv_y } else { 0 },
    ref_frame: 0,
    is_newmv: use_newmv,
};

// Splat to 2x2 grid of 4x4 blocks
for row in by..by + 2 {
    for col in bx..bx + 2 {
        if row < self.mi_rows && col < self.mi_cols {
            self.block_mvs[(row * self.mi_cols + col) as usize] = stored_mv;
        }
    }
}
```

**Step 4: Update reconstruction to use motion-compensated reference**

The reconstruction loop must also use the motion-compensated reference (not zero-offset):

```rust
let pixel = (y_ref_block[(r * 8 + c) as usize] as i32
    + y_recon_residual[(r * 8 + c) as usize])
    .clamp(0, 255) as u8;
```

This already works if `y_ref_block` was extracted at the correct offset.

**Step 5: Run all tests**

Run: `cargo test && cargo clippy --tests`
Expected: All tests PASS, no clippy warnings

**Step 6: Run integration tests**

Run: `cargo test --test integration`
Expected: All integration tests PASS (dav1d decodes the new NEWMV bitstream correctly)

---

### Task 6: Commit

```bash
git add wav1c/src/cdf.rs wav1c/src/tile.rs docs/plans/2026-02-15-motion-estimation.md
git commit -m "feat: add motion estimation with NEWMV for inter frames"
```
