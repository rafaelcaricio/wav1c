# ADST/Identity Transform Type Selection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add ADST and Identity transforms with per-block RD-based transform type selection for intra frames.

**Architecture:** Implement forward/inverse ADST4, ADST8, Identity4, Identity8 in dct.rs. Add generic 2D transform dispatching by TxType enum. In encode_block, try all 5 allowed transform types (reduced_txtp_set=1: IDTX, DCT_DCT, ADST_ADST, ADST_DCT, DCT_ADST) and pick the one with lowest RD cost. Encode the selected txtp symbol via txtp_intra2 CDF.

**Tech Stack:** Rust, existing DCT/quantize/dequantize in wav1c, dav1d for validation

**Key Reference:**
- txtp_intra2 symbol mapping: `[IDTX=0, DCT_DCT=1, ADST_ADST=2, ADST_DCT=3, DCT_ADST=4]` (from `dav1d_tx_types_per_set[0..5]`)
- All 5 types in TX_SET_INTRA_2 have `TX_CLASS_2D` — no coefficient context changes needed
- Chroma txtp derived from uv_mode by decoder (CFL → DCT_DCT), no encoder change needed
- Inverse ADST4 constants (12-bit): SINPI = 1321, 2482, 3344, 3803
- Inverse Identity4: `out = in + ((in * 1697 + 2048) >> 12)`, Identity8: `out = in * 2`

---

### Task 1: Add TxType enum and 1D ADST forward/inverse transforms

**Files:**
- Modify: `wav1c/src/dct.rs`

**Step 1: Add TxType enum**

Add at top of file after `clip` fn:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxType {
    DctDct = 0,
    AdstDct = 1,
    DctAdst = 2,
    AdstAdst = 3,
    Idtx = 9,
}
```

**Step 2: Write round-trip test for forward/inverse ADST4**

Add to `#[cfg(test)] mod tests`:

```rust
#[test]
fn adst4_round_trip() {
    let input = [0i32; 16];
    let fwd = forward_transform_4x4(&input, TxType::AdstAdst);
    assert_eq!(fwd, [0i32; 16]);

    let signal: [i32; 16] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160];
    let fwd = forward_transform_4x4(&signal, TxType::AdstAdst);
    let inv = inverse_transform_4x4(&fwd, TxType::AdstAdst);
    for i in 0..16 {
        assert!((signal[i] - inv[i]).abs() <= 1, "mismatch at {i}: {} vs {}", signal[i], inv[i]);
    }
}
```

**Step 3: Implement 1D forward ADST4**

The forward ADST4 is the transpose of dav1d's inverse ADST4. Uses SINPI constants at 12-bit precision (1321, 2482, 3344, 3803):

```rust
fn fwd_adst4_1d(data: &mut [i32], offset: usize, stride: usize) {
    let in0 = data[offset];
    let in1 = data[offset + stride];
    let in2 = data[offset + 2 * stride];
    let in3 = data[offset + 3 * stride];

    let s0 = 1321 * in0 + 2482 * in1 + 3344 * in2 + 3803 * in3;
    let s1 = 3344 * (in0 + in1 - in3);
    let s2 = 3803 * in0 - 1321 * in1 - 3344 * in2 + 2482 * in3;
    let s3 = 2482 * in0 - 3803 * in1 + 3344 * in2 - 1321 * in3;

    data[offset] = (s0 + 2048) >> 12;
    data[offset + stride] = (s1 + 2048) >> 12;
    data[offset + 2 * stride] = (s2 + 2048) >> 12;
    data[offset + 3 * stride] = (s3 + 2048) >> 12;
}
```

**Step 4: Implement 1D inverse ADST4 (port from dav1d)**

```rust
fn inv_adst4_1d(data: &mut [i32], offset: usize, stride: usize) {
    let in0 = data[offset];
    let in1 = data[offset + stride];
    let in2 = data[offset + 2 * stride];
    let in3 = data[offset + 3 * stride];

    let o0 = ((1321 * in0 + (3803 - 4096) * in2 + (2482 - 4096) * in3 + (3344 - 4096) * in1 + 2048) >> 12)
        + in2 + in3 + in1;
    let o1 = (((2482 - 4096) * in0 - 1321 * in2 - (3803 - 4096) * in3 + (3344 - 4096) * in1 + 2048) >> 12)
        + in0 - in3 + in1;
    let o2 = (209 * (in0 - in2 + in3) + 128) >> 8;
    let o3 = (((3803 - 4096) * in0 + (2482 - 4096) * in2 - 1321 * in3 - (3344 - 4096) * in1 + 2048) >> 12)
        + in0 + in2 - in1;

    data[offset] = clip(o0);
    data[offset + stride] = clip(o1);
    data[offset + 2 * stride] = clip(o2);
    data[offset + 3 * stride] = clip(o3);
}
```

**Step 5: Run tests**

Run: `cargo test -p wav1c adst4`
Expected: PASS

---

### Task 2: Add 1D ADST8 forward/inverse transforms

**Files:**
- Modify: `wav1c/src/dct.rs`

**Step 1: Write round-trip test for ADST8**

```rust
#[test]
fn adst8_round_trip() {
    let input = [0i32; 64];
    let fwd = forward_transform_8x8(&input, TxType::AdstAdst);
    assert_eq!(fwd, [0i32; 64]);

    let mut signal = [0i32; 64];
    for i in 0..64 { signal[i] = (i as i32) * 3 - 90; }
    let fwd = forward_transform_8x8(&signal, TxType::AdstAdst);
    let inv = inverse_transform_8x8(&fwd, TxType::AdstAdst);
    for i in 0..64 {
        assert!((signal[i] - inv[i]).abs() <= 2, "mismatch at {i}: {} vs {}", signal[i], inv[i]);
    }
}
```

**Step 2: Implement 1D forward ADST8**

Port from dav1d's inverse ADST8 (transposed). Uses rotation constants from dav1d at 12-bit precision:

```rust
fn fwd_adst8_1d(data: &mut [i32], offset: usize, stride: usize) {
    let in0 = data[offset + 7 * stride];
    let in1 = data[offset];
    let in2 = data[offset + 5 * stride];
    let in3 = data[offset + 2 * stride];
    let in4 = data[offset + 3 * stride];
    let in5 = data[offset + 4 * stride];
    let in6 = data[offset + 1 * stride];
    let in7 = data[offset + 6 * stride];

    let t0a = (((4076 - 4096) * in0 + 401 * in1 + 2048) >> 12) + in0;
    let t1a = ((401 * in0 - (4076 - 4096) * in1 + 2048) >> 12) - in1;
    let t2a = (((3612 - 4096) * in2 + 1931 * in3 + 2048) >> 12) + in2;
    let t3a = ((1931 * in2 - (3612 - 4096) * in3 + 2048) >> 12) - in3;
    let t4a = (1299 * in4 + 1583 * in5 + 1024) >> 11;
    let t5a = (1583 * in4 - 1299 * in5 + 1024) >> 11;
    let t6a = ((1189 * in6 + (3920 - 4096) * in7 + 2048) >> 12) + in7;
    let t7a = (((3920 - 4096) * in6 - 1189 * in7 + 2048) >> 12) + in6;

    let t0 = clip(t0a + t4a);
    let t1 = clip(t1a + t5a);
    let t2 = clip(t2a + t6a);
    let t3 = clip(t3a + t7a);
    let t4 = clip(t0a - t4a);
    let t5 = clip(t1a - t5a);
    let t6 = clip(t2a - t6a);
    let t7 = clip(t3a - t7a);

    let t4b = (((3784 - 4096) * t4 + 1567 * t5 + 2048) >> 12) + t4;
    let t5b = ((1567 * t4 - (3784 - 4096) * t5 + 2048) >> 12) - t5;
    let t6b = (((3784 - 4096) * t7 - 1567 * t6 + 2048) >> 12) + t7;
    let t7b = ((1567 * t7 + (3784 - 4096) * t6 + 2048) >> 12) + t6;

    let o0 = clip(t0 + t2);
    let o7 = clip(t1 + t3);
    let t2f = clip(t0 - t2);
    let t3f = clip(t1 - t3);
    let o1 = clip(t4b + t6b);
    let o6 = clip(t5b + t7b);
    let t6f = clip(t4b - t6b);
    let t7f = clip(t5b - t7b);

    data[offset] = o0;
    data[offset + stride] = -o1;
    data[offset + 2 * stride] = ((t6f + t7f) * 181 + 128) >> 8;
    data[offset + 3 * stride] = -(((t2f + t3f) * 181 + 128) >> 8);
    data[offset + 4 * stride] = ((t2f - t3f) * 181 + 128) >> 8;
    data[offset + 5 * stride] = -(((t6f - t7f) * 181 + 128) >> 8);
    data[offset + 6 * stride] = o6;
    data[offset + 7 * stride] = -o7;
}
```

**Step 3: Implement 1D inverse ADST8 (port from dav1d)**

```rust
fn inv_adst8_1d(data: &mut [i32], offset: usize, stride: usize) {
    let in0 = data[offset];
    let in1 = data[offset + stride];
    let in2 = data[offset + 2 * stride];
    let in3 = data[offset + 3 * stride];
    let in4 = data[offset + 4 * stride];
    let in5 = data[offset + 5 * stride];
    let in6 = data[offset + 6 * stride];
    let in7 = data[offset + 7 * stride];

    let t0a = (((4076 - 4096) * in7 + 401 * in0 + 2048) >> 12) + in7;
    let t1a = ((401 * in7 - (4076 - 4096) * in0 + 2048) >> 12) - in0;
    let t2a = (((3612 - 4096) * in5 + 1931 * in2 + 2048) >> 12) + in5;
    let t3a = ((1931 * in5 - (3612 - 4096) * in2 + 2048) >> 12) - in2;
    let t4a = (1299 * in3 + 1583 * in4 + 1024) >> 11;
    let t5a = (1583 * in3 - 1299 * in4 + 1024) >> 11;
    let t6a = ((1189 * in1 + (3920 - 4096) * in6 + 2048) >> 12) + in6;
    let t7a = (((3920 - 4096) * in1 - 1189 * in6 + 2048) >> 12) + in1;

    let t0 = clip(t0a + t4a);
    let t1 = clip(t1a + t5a);
    let mut t2 = clip(t2a + t6a);
    let mut t3 = clip(t3a + t7a);
    let t4 = clip(t0a - t4a);
    let t5 = clip(t1a - t5a);
    let mut t6 = clip(t2a - t6a);
    let mut t7 = clip(t3a - t7a);

    let t4b = (((3784 - 4096) * t4 + 1567 * t5 + 2048) >> 12) + t4;
    let t5b = ((1567 * t4 - (3784 - 4096) * t5 + 2048) >> 12) - t5;
    let t6b = (((3784 - 4096) * t7 - 1567 * t6 + 2048) >> 12) + t7;
    let t7b = ((1567 * t7 + (3784 - 4096) * t6 + 2048) >> 12) + t6;

    data[offset] = clip(t0 + t2);
    data[offset + 7 * stride] = -clip(t1 + t3);
    t2 = clip(t0 - t2);
    t3 = clip(t1 - t3);
    data[offset + stride] = -clip(t4b + t6b);
    data[offset + 6 * stride] = clip(t5b + t7b);
    t6 = clip(t4b - t6b);
    t7 = clip(t5b - t7b);

    data[offset + 3 * stride] = -(((t2 + t3) * 181 + 128) >> 8);
    data[offset + 4 * stride] = ((t2 - t3) * 181 + 128) >> 8;
    data[offset + 2 * stride] = ((t6 + t7) * 181 + 128) >> 8;
    data[offset + 5 * stride] = -(((t6 - t7) * 181 + 128) >> 8);
}
```

**Step 4: Run tests**

Run: `cargo test -p wav1c adst8`
Expected: PASS

---

### Task 3: Add 1D Identity transforms and 2D dispatch functions

**Files:**
- Modify: `wav1c/src/dct.rs`

**Step 1: Write round-trip tests for Identity**

```rust
#[test]
fn identity4_round_trip() {
    let signal: [i32; 16] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160];
    let fwd = forward_transform_4x4(&signal, TxType::Idtx);
    let inv = inverse_transform_4x4(&fwd, TxType::Idtx);
    for i in 0..16 {
        assert!((signal[i] - inv[i]).abs() <= 1, "mismatch at {i}: {} vs {}", signal[i], inv[i]);
    }
}

#[test]
fn identity8_round_trip() {
    let mut signal = [0i32; 64];
    for i in 0..64 { signal[i] = (i as i32) * 2 - 60; }
    let fwd = forward_transform_8x8(&signal, TxType::Idtx);
    let inv = inverse_transform_8x8(&fwd, TxType::Idtx);
    for i in 0..64 {
        assert!((signal[i] - inv[i]).abs() <= 2, "mismatch at {i}: {} vs {}", signal[i], inv[i]);
    }
}

#[test]
fn mixed_adst_dct_round_trip() {
    let mut signal = [0i32; 64];
    for i in 0..64 { signal[i] = (i as i32) * 3 - 90; }

    for tx in [TxType::AdstDct, TxType::DctAdst] {
        let fwd = forward_transform_8x8(&signal, tx);
        let inv = inverse_transform_8x8(&fwd, tx);
        for i in 0..64 {
            assert!((signal[i] - inv[i]).abs() <= 2, "mismatch at {i} for {:?}: {} vs {}", tx, signal[i], inv[i]);
        }
    }
}
```

**Step 2: Implement 1D Identity transforms**

```rust
fn fwd_identity4_1d(data: &mut [i32], offset: usize, stride: usize) {
    for i in 0..4 {
        let v = data[offset + i * stride];
        data[offset + i * stride] = v + ((v * 1697 + 2048) >> 12);
    }
}

fn inv_identity4_1d(data: &mut [i32], offset: usize, stride: usize) {
    for i in 0..4 {
        let v = data[offset + i * stride];
        data[offset + i * stride] = v + ((v * 1697 + 2048) >> 12);
    }
}

fn fwd_identity8_1d(data: &mut [i32], offset: usize, stride: usize) {
    for i in 0..8 {
        data[offset + i * stride] *= 2;
    }
}

fn inv_identity8_1d(data: &mut [i32], offset: usize, stride: usize) {
    for i in 0..8 {
        data[offset + i * stride] *= 2;
    }
}
```

**Step 3: Implement 2D forward/inverse dispatch functions**

The 2D functions use the same pipeline as existing DCT but dispatch to the correct 1D function per dimension. Each TxType decomposes into `(row_1d, col_1d)`:
- DctDct → (DCT, DCT)
- AdstDct → (DCT, ADST)  *note: row=col, col=row in AV1*
- DctAdst → (ADST, DCT)
- AdstAdst → (ADST, ADST)
- Idtx → (Identity, Identity)

```rust
type Transform1dFn4 = fn(&mut [i32], usize, usize);
type Transform1dFn8 = fn(&mut [i32], usize, usize);

fn get_1d_fns_4(tx_type: TxType) -> (Transform1dFn4, Transform1dFn4) {
    match tx_type {
        TxType::DctDct => (fwd_dct4_1d, fwd_dct4_1d),
        TxType::AdstDct => (fwd_dct4_1d, fwd_adst4_1d),
        TxType::DctAdst => (fwd_adst4_1d, fwd_dct4_1d),
        TxType::AdstAdst => (fwd_adst4_1d, fwd_adst4_1d),
        TxType::Idtx => (fwd_identity4_1d, fwd_identity4_1d),
    }
}

fn get_1d_fns_8(tx_type: TxType) -> (Transform1dFn8, Transform1dFn8) {
    match tx_type {
        TxType::DctDct => (fwd_dct8_1d, fwd_dct8_1d),
        TxType::AdstDct => (fwd_dct8_1d, fwd_adst8_1d),
        TxType::DctAdst => (fwd_adst8_1d, fwd_dct8_1d),
        TxType::AdstAdst => (fwd_adst8_1d, fwd_adst8_1d),
        TxType::Idtx => (fwd_identity8_1d, fwd_identity8_1d),
    }
}

fn get_inv_1d_fns_4(tx_type: TxType) -> (Transform1dFn4, Transform1dFn4) {
    match tx_type {
        TxType::DctDct => (inv_dct4_1d, inv_dct4_1d),
        TxType::AdstDct => (inv_dct4_1d, inv_adst4_1d),
        TxType::DctAdst => (inv_adst4_1d, inv_dct4_1d),
        TxType::AdstAdst => (inv_adst4_1d, inv_adst4_1d),
        TxType::Idtx => (inv_identity4_1d, inv_identity4_1d),
    }
}

fn get_inv_1d_fns_8(tx_type: TxType) -> (Transform1dFn8, Transform1dFn8) {
    match tx_type {
        TxType::DctDct => (inv_dct8_1d, inv_dct8_1d),
        TxType::AdstDct => (inv_dct8_1d, inv_adst8_1d),
        TxType::DctAdst => (inv_adst8_1d, inv_dct8_1d),
        TxType::AdstAdst => (inv_adst8_1d, inv_adst8_1d),
        TxType::Idtx => (inv_identity8_1d, inv_identity8_1d),
    }
}

pub fn forward_transform_4x4(residual: &[i32; 16], tx_type: TxType) -> [i32; 16] {
    let mut buf = *residual;
    for v in &mut buf { *v <<= 2; }
    let (row_fn, col_fn) = get_1d_fns_4(tx_type);
    for row in 0..4 { row_fn(&mut buf, row * 4, 1); }
    for col in 0..4 { col_fn(&mut buf, col, 4); }
    transpose_4x4(&mut buf);
    buf
}

pub fn forward_transform_8x8(residual: &[i32; 64], tx_type: TxType) -> [i32; 64] {
    let mut buf = *residual;
    for v in &mut buf { *v <<= 2; }
    let (row_fn, col_fn) = get_1d_fns_8(tx_type);
    for row in 0..8 { row_fn(&mut buf, row * 8, 1); }
    for v in &mut buf { *v = (*v + 1) >> 1; }
    for col in 0..8 { col_fn(&mut buf, col, 8); }
    transpose_8x8(&mut buf);
    buf
}

pub fn inverse_transform_4x4(coeffs: &[i32; 16], tx_type: TxType) -> [i32; 16] {
    let mut buf = *coeffs;
    transpose_4x4(&mut buf);
    let (row_fn, col_fn) = get_inv_1d_fns_4(tx_type);
    for row in 0..4 { row_fn(&mut buf, row * 4, 1); }
    for col in 0..4 { col_fn(&mut buf, col, 4); }
    for v in &mut buf { *v = (*v + 8) >> 4; }
    buf
}

pub fn inverse_transform_8x8(coeffs: &[i32; 64], tx_type: TxType) -> [i32; 64] {
    let mut buf = *coeffs;
    transpose_8x8(&mut buf);
    let (row_fn, col_fn) = get_inv_1d_fns_8(tx_type);
    for row in 0..8 { row_fn(&mut buf, row * 8, 1); }
    for v in &mut buf { *v = (*v + 1) >> 1; }
    for col in 0..8 { col_fn(&mut buf, col, 8); }
    for v in &mut buf { *v = (*v + 8) >> 4; }
    buf
}
```

**Step 4: Update existing DCT functions to delegate**

Change `forward_dct_4x4`, `forward_dct_8x8`, `inverse_dct_4x4`, `inverse_dct_8x8` to call the new generic functions:

```rust
pub fn forward_dct_4x4(residual: &[i32; 16]) -> [i32; 16] {
    forward_transform_4x4(residual, TxType::DctDct)
}

pub fn forward_dct_8x8(residual: &[i32; 64]) -> [i32; 64] {
    forward_transform_8x8(residual, TxType::DctDct)
}

pub fn inverse_dct_4x4(coeffs: &[i32; 16]) -> [i32; 16] {
    inverse_transform_4x4(coeffs, TxType::DctDct)
}

pub fn inverse_dct_8x8(coeffs: &[i32; 64]) -> [i32; 64] {
    inverse_transform_8x8(coeffs, TxType::DctDct)
}
```

**Step 5: Run all tests**

Run: `cargo test -p wav1c`
Expected: All existing DCT tests + new ADST/Identity tests PASS. Existing code using `forward_dct_8x8` etc. is unchanged.

---

### Task 4: Add txtp selection and encoding in tile.rs

**Files:**
- Modify: `wav1c/src/tile.rs`

**Step 1: Add TXTP_INTRA2_SYMBOLS constant and txtype→symbol mapping**

Add near top of file:

```rust
const TXTP_INTRA2_MAP: [dct::TxType; 5] = [
    dct::TxType::Idtx,
    dct::TxType::DctDct,
    dct::TxType::AdstAdst,
    dct::TxType::AdstDct,
    dct::TxType::DctAdst,
];

fn txtype_to_intra2_symbol(tx: dct::TxType) -> u32 {
    match tx {
        dct::TxType::Idtx => 0,
        dct::TxType::DctDct => 1,
        dct::TxType::AdstAdst => 2,
        dct::TxType::AdstDct => 3,
        dct::TxType::DctAdst => 4,
    }
}
```

**Step 2: Update `compute_rd_cost` to accept TxType**

Change signature to include `tx_type: dct::TxType`. Replace `dct::forward_dct_8x8` and `dct::inverse_dct_8x8` with the generic versions:

```rust
fn compute_rd_cost(source: &[u8], prediction: &[u8], dc_dq: u32, ac_dq: u32, tx_type: dct::TxType) -> u64 {
    let mut residual = [0i32; 64];
    for i in 0..64 {
        residual[i] = source[i] as i32 - prediction[i] as i32;
    }

    let coeffs = dct::forward_transform_8x8(&residual, tx_type);
    let quant = quantize_coeffs(&coeffs, 64, dc_dq, ac_dq);
    let deq = dequantize_coeffs(&quant, 64, dc_dq, ac_dq);
    let mut deq_arr = [0i32; 64];
    deq_arr.copy_from_slice(&deq);
    let recon_residual = dct::inverse_transform_8x8(&deq_arr, tx_type);

    let mut sse: u64 = 0;
    for i in 0..64 {
        let recon = (prediction[i] as i32 + recon_residual[i]).clamp(0, 255);
        let diff = source[i] as i32 - recon;
        sse += (diff * diff) as u64;
    }

    let nz_count: u64 = quant.iter().filter(|&&c| c != 0).count() as u64;
    let lambda = (ac_dq as u64 * ac_dq as u64) >> 2;

    sse + lambda * nz_count
}
```

**Step 3: Update `select_best_intra_mode` calls to pass `DctDct`**

All existing calls to `compute_rd_cost` inside `select_best_intra_mode` should pass `dct::TxType::DctDct` as the last argument — mode selection still uses DCT for consistency (txtype selection happens separately after mode is chosen).

**Step 4: Add `select_best_txtype` function**

Add after `select_best_intra_mode`:

```rust
fn select_best_txtype(source: &[u8], prediction: &[u8], dc_dq: u32, ac_dq: u32) -> dct::TxType {
    let mut best_type = dct::TxType::DctDct;
    let mut best_cost = compute_rd_cost(source, prediction, dc_dq, ac_dq, dct::TxType::DctDct);

    for &tx in &TXTP_INTRA2_MAP {
        if tx == dct::TxType::DctDct { continue; }
        let cost = compute_rd_cost(source, prediction, dc_dq, ac_dq, tx);
        if cost < best_cost {
            best_cost = cost;
            best_type = tx;
        }
    }

    best_type
}
```

**Step 5: Update `encode_block` to select and use txtype**

In `encode_block`, after computing `y_pred_block` and before the residual/transform code:

Replace:
```rust
let y_dct = dct::forward_dct_8x8(&y_residual);
```
With:
```rust
let y_txtype = select_best_txtype(&y_block, &y_pred_block, self.dq.dc, self.dq.ac);
let y_dct = dct::forward_transform_8x8(&y_residual, y_txtype);
```

And update the reconstruction inverse to match:
Replace:
```rust
let y_recon_residual = dct::inverse_dct_8x8(&y_deq_arr);
```
With:
```rust
let y_recon_residual = dct::inverse_transform_8x8(&y_deq_arr, y_txtype);
```

**Step 6: Update txtp_intra2 symbol encoding**

In `encode_transform_block`, replace the hardcoded symbol 1 with the actual txtype symbol. This requires passing `tx_type` to `encode_transform_block`.

Add `tx_type: dct::TxType` parameter to `encode_transform_block` signature.

Replace:
```rust
enc.encode_symbol(1, &mut cdf.txtp_intra2[t_dim_min][y_mode as usize], 4);
```
With:
```rust
enc.encode_symbol(txtype_to_intra2_symbol(tx_type), &mut cdf.txtp_intra2[t_dim_min][y_mode as usize], 4);
```

Pass `y_txtype` from `encode_block` for luma, and `dct::TxType::DctDct` for chroma (chroma txtype derived from uv_mode by decoder, CFL→DCT_DCT, no encoder choice).

Update all call sites of `encode_transform_block` (in `encode_block` and `encode_inter_block`) to pass the new `tx_type` parameter.

**Step 7: Update inter blocks**

For inter blocks with `reduced_txtp_set=1`, the choice is just DCT_DCT or IDTX (bool). Keep encoding `true` (DCT_DCT) for now — unchanged.

Pass `dct::TxType::DctDct` for inter `encode_transform_block` calls.

**Step 8: Update test helper calls**

Update direct `encode_transform_block` calls in unit tests to pass `dct::TxType::DctDct`.

Update `compute_rd_cost` test calls to pass `dct::TxType::DctDct`.

**Step 9: Run all tests**

Run: `cargo test -p wav1c && cargo clippy -p wav1c --tests`
Expected: All tests PASS, no new warnings

---

### Task 5: Integration test and commit

**Files:**
- Modify: none (test existing)

**Step 1: Run integration tests with dav1d**

Run: `cargo test --test integration`
Expected: All 14 integration tests PASS (including dav1d decode verification)

**Step 2: Rebuild FFmpeg and verify**

```bash
cd wav1c-ffi && cargo build --release
touch ../FFmpeg/libavcodec/libwav1c.c && cd ../FFmpeg && make -j8
./ffmpeg -y -i ../wav1c/examples/mandelbrot_160x90.y4m -c:v libwav1c -frames:v 5 /tmp/claude/ffmpeg_test.ivf 2>&1 | tail -5
../dav1d/build/tools/dav1d -i /tmp/claude/ffmpeg_test.ivf -o /dev/null 2>&1
```
Expected: encode + decode succeeds without errors

**Step 3: Commit**

```bash
git add wav1c/src/dct.rs wav1c/src/tile.rs docs/plans/2026-02-15-transform-type-selection.md
git commit -m "feat: add ADST/Identity transforms with RD-based txtype selection"
```
