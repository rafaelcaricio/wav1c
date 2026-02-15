# Sub-Pixel Motion Estimation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add half-pel and quarter-pel motion refinement to inter frames using AV1's 8-tap interpolation filters.

**Architecture:** After integer-pel search, refine at half-pel (±4 MV units) then quarter-pel (±2 MV units) using interpolated SAD. Use EIGHTTAP_REGULAR 8-tap filter for luma, 4-tap for chroma. MV encoding already supports fractional values.

**Tech Stack:** Rust, existing motion search and MV encoding in wav1c

**Key technical details:**
- MVs are in 1/8-pel units. Integer pixel offset = `mv >> 3`, sub-pixel phase = `mv & 7`
- Filter table index = `(mv & 7) * 2`. If 0 → integer (no filter), else → `table[index - 1]`
- With `allow_high_precision_mv=0`: quarter-pel precision, phase ∈ {0, 2, 4, 6}
- 2-pass filtering: horizontal → intermediate i16 → vertical → output u8
- 8-tap filter reads pixels at offsets -3..+4 relative to center (need clamped edge access)
- Luma (w=8): 8-tap EIGHTTAP_REGULAR (dav1d table index 0)
- Chroma (w=4): 4-tap REGULAR (dav1d table index 3)
- `interpolation_filter=0` already set in frame header

---

### Task 1: Add filter coefficient tables

**Files:** `wav1c/src/tile.rs`

Add two constant arrays from dav1d `src/tables.c:445`:

```rust
const SUBPEL_FILTER_8TAP: [[i8; 8]; 15] = [
    [  0,  1, -3, 63,  4, -1,  0,  0],
    [  0,  1, -5, 61,  9, -2,  0,  0],
    [  0,  1, -6, 58, 14, -4,  1,  0],
    [  0,  1, -7, 55, 19, -5,  1,  0],
    [  0,  1, -7, 51, 24, -6,  1,  0],
    [  0,  1, -8, 47, 29, -6,  1,  0],
    [  0,  1, -7, 42, 33, -6,  1,  0],
    [  0,  1, -7, 38, 38, -7,  1,  0],
    [  0,  1, -6, 33, 42, -7,  1,  0],
    [  0,  1, -6, 29, 47, -8,  1,  0],
    [  0,  1, -6, 24, 51, -7,  1,  0],
    [  0,  1, -5, 19, 55, -7,  1,  0],
    [  0,  1, -4, 14, 58, -6,  1,  0],
    [  0,  0, -2,  9, 61, -5,  1,  0],
    [  0,  0, -1,  4, 63, -3,  1,  0],
];

const SUBPEL_FILTER_4TAP: [[i8; 8]; 15] = [
    [  0,  0, -2, 63,  4, -1,  0,  0],
    [  0,  0, -4, 61,  9, -2,  0,  0],
    [  0,  0, -5, 58, 14, -3,  0,  0],
    [  0,  0, -6, 55, 19, -4,  0,  0],
    [  0,  0, -6, 51, 24, -5,  0,  0],
    [  0,  0, -7, 47, 29, -5,  0,  0],
    [  0,  0, -6, 42, 33, -5,  0,  0],
    [  0,  0, -6, 38, 38, -6,  0,  0],
    [  0,  0, -5, 33, 42, -6,  0,  0],
    [  0,  0, -5, 29, 47, -7,  0,  0],
    [  0,  0, -5, 24, 51, -6,  0,  0],
    [  0,  0, -4, 19, 55, -6,  0,  0],
    [  0,  0, -3, 14, 58, -5,  0,  0],
    [  0,  0, -2,  9, 61, -4,  0,  0],
    [  0,  0, -1,  4, 63, -2,  0,  0],
];
```

Run: `cargo test -p wav1c`

---

### Task 2: Implement sub-pixel block interpolation

**Files:** `wav1c/src/tile.rs`

Implement `interpolate_block` — given a reference plane, integer pixel position, sub-pixel phase (0-7), and block size, produce an interpolated block using 2-pass filtering.

```rust
fn interpolate_block(
    reference: &[u8],
    width: u32,
    height: u32,
    int_x: i32,
    int_y: i32,
    phase_x: u32,   // 0-7 (1/8-pel fractional part)
    phase_y: u32,
    block_size: u32, // 8 for luma, 4 for chroma
) -> Vec<u8>
```

Algorithm:
1. Compute filter indices: `fidx_h = phase_x * 2`, `fidx_v = phase_y * 2`
2. Select filter table: 8-tap for block_size > 4, 4-tap for block_size <= 4
3. If both phases are 0: just copy pixels (integer position)
4. If only horizontal phase non-zero: apply horizontal filter only
5. If only vertical phase non-zero: apply vertical filter only
6. If both non-zero: 2-pass (H then V)

For horizontal pass (per row):
- Read 8 reference pixels centered around the target pixel (offsets -3..+4 for 8-tap)
- Clamp source coordinates to [0, width-1] and [0, height-1]
- Compute: `sum = Σ filter[i] * pixel[offset+i-3]` for i=0..7
- Store intermediate: `(sum + 4) >> 3` (keep extra precision as i16)

For vertical pass (per column on intermediate):
- Same 8-tap filter applied vertically
- Final: `((sum + 4) >> 3 + offset) >> shift`, clamp to [0, 255]

Rounding: For 2-pass, horizontal produces i16 with `(sum + (1 << (FILTER_BITS-1))) >> FILTER_BITS` where FILTER_BITS depends on intermediate precision. For single-pass, `(sum + 64) >> 7`.

**Tests:**

```rust
#[test]
fn interpolate_integer_position_copies_pixels() {
    let reference = vec![100u8; 64 * 64];
    let result = interpolate_block(&reference, 64, 64, 10, 10, 0, 0, 8);
    assert_eq!(result.len(), 64);
    assert!(result.iter().all(|&p| p == 100));
}

#[test]
fn interpolate_half_pel_is_symmetric() {
    // Half-pel filter [7] is symmetric: {0,1,-7,38,38,-7,1,0}
    // Two identical rows should produce same value at half-pel
    let mut reference = vec![128u8; 64 * 64];
    for c in 0..64 {
        reference[10 * 64 + c] = 200;
        reference[11 * 64 + c] = 200;
    }
    let result = interpolate_block(&reference, 64, 64, 0, 10, 0, 4, 8);
    // At half-pel between two identical rows, output should be ~200
    assert!((result[0] as i32 - 200).unsigned_abs() <= 1);
}
```

Run: `cargo test -p wav1c interpolate`

---

### Task 3: Add sub-pixel refinement to motion search

**Files:** `wav1c/src/tile.rs`

Add `subpel_refine` function that takes the best integer-pel MV and refines it at half-pel then quarter-pel.

```rust
fn subpel_refine(
    source: &[u8],
    reference: &[u8],
    width: u32,
    height: u32,
    px_x: u32,
    px_y: u32,
    block_size: u32,
    best_mv_x: i32,  // in 1/8-pel units (from integer search * 8)
    best_mv_y: i32,
) -> (i32, i32)  // refined MV in 1/8-pel units
```

Algorithm:
1. **Half-pel refinement** (step=4 in 1/8-pel units):
   - Check 8 positions around best: (±4, 0), (0, ±4), (±4, ±4)
   - For each, compute interpolated SAD
   - Update best if lower SAD

2. **Quarter-pel refinement** (step=2):
   - Check 8 positions around best half-pel result
   - Same SAD comparison

For computing SAD at sub-pixel positions:
- Split MV into integer + fractional: `int_offset = mv >> 3`, `phase = mv & 7`
- Call `interpolate_block` to get predicted block
- Compute SAD between source and interpolated block
- The candidate with lowest SAD (and preferring smaller MV on ties) wins

**Tests:**

```rust
#[test]
fn subpel_refine_returns_integer_when_best() {
    // Uniform blocks — no sub-pixel position should be better
    let source = vec![128u8; 64 * 64];
    let reference = vec![128u8; 64 * 64];
    let (mx, my) = subpel_refine(&source, &reference, 64, 64, 10, 10, 8, 0, 0);
    assert_eq!((mx, my), (0, 0));
}
```

Run: `cargo test -p wav1c subpel`

---

### Task 4: Integrate sub-pixel motion into encode_inter_block

**Files:** `wav1c/src/tile.rs`

Modify `encode_inter_block` to:
1. After `motion_search_block` returns integer (dx, dy), convert to 1/8-pel: `(dx*8, dy*8)`
2. Call `subpel_refine` to get refined MV in 1/8-pel units
3. Use `interpolate_block` with the refined MV to produce the reference block
4. Use this interpolated reference for residual, DCT, quantize, etc.
5. For chroma: MV is halved (`mv_x/2, mv_y/2`), use 4-tap interpolation
6. MV encoding already handles fractional values (decompose_mv_diff works for all even diffs)
7. Store the refined MV (in 1/8-pel) in block_mvs

**Key change**: Replace `extract_block(&self.reference.y, w, ref_px_x, ref_px_y, 8, w, h)` with `interpolate_block(...)` for the motion-compensated reference.

Run: `cargo test && cargo test --test integration`

---

### Task 5: Commit

```bash
git add wav1c/src/tile.rs docs/plans/2026-02-15-subpel-motion.md
git commit -m "feat: add sub-pixel motion refinement with 8-tap interpolation"
```
