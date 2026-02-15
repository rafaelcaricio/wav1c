# RD-Optimized Intra Mode Selection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace SAD-based intra mode selection with Rate-Distortion cost evaluation for better mode decisions.

**Architecture:** For each candidate mode, compute residual → DCT → quantize → dequantize → inverse DCT → SSE distortion. Estimate rate from non-zero coefficient count. Pick mode with lowest `SSE + λ * rate`. Lambda derived from quantizer step size.

**Tech Stack:** Rust, existing DCT/quantize/dequantize functions in wav1c

---

### Task 1: Add `compute_rd_cost` function

**Files:**
- Modify: `wav1c/src/tile.rs`

**Step 1: Write test for RD cost function**

Add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn rd_cost_zero_for_perfect_prediction() {
    let source = [128u8; 64];
    let prediction = [128u8; 64];
    let dq = crate::dequant::lookup_dequant(128);
    let cost = compute_rd_cost(&source, &prediction, dq.dc, dq.ac);
    assert_eq!(cost, 0);
}

#[test]
fn rd_cost_higher_for_worse_prediction() {
    let source = [200u8; 64];
    let good_pred = [190u8; 64];
    let bad_pred = [100u8; 64];
    let dq = crate::dequant::lookup_dequant(128);
    let good_cost = compute_rd_cost(&source, &good_pred, dq.dc, dq.ac);
    let bad_cost = compute_rd_cost(&source, &bad_pred, dq.dc, dq.ac);
    assert!(good_cost < bad_cost);
}
```

**Step 2: Implement `compute_rd_cost`**

Add as a free function near `compute_sad`:

```rust
fn compute_rd_cost(source: &[u8], prediction: &[u8], dc_dq: u32, ac_dq: u32) -> u64 {
    let mut residual = [0i32; 64];
    for i in 0..64 {
        residual[i] = source[i] as i32 - prediction[i] as i32;
    }

    let dct_coeffs = dct::forward_dct_8x8(&residual);
    let quant = quantize_coeffs(&dct_coeffs, 64, dc_dq, ac_dq);
    let deq = dequantize_coeffs(&quant, 64, dc_dq, ac_dq);
    let mut deq_arr = [0i32; 64];
    deq_arr.copy_from_slice(&deq);
    let recon_residual = dct::inverse_dct_8x8(&deq_arr);

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

**Step 3: Run tests**

Run: `cargo test -p wav1c rd_cost`
Expected: PASS

---

### Task 2: Replace SAD with RD cost in mode selection

**Files:**
- Modify: `wav1c/src/tile.rs`

**Step 1: Update `select_best_intra_mode` signature and body**

Add `dc_dq: u32, ac_dq: u32` parameters. Replace `compute_sad` calls with `compute_rd_cost`.

The function becomes:

```rust
#[allow(clippy::too_many_arguments)]
fn select_best_intra_mode(
    source: &[u8],
    above: &[u8],
    left: &[u8],
    top_left: u8,
    have_above: bool,
    have_left: bool,
    w: usize,
    h: usize,
    dc_dq: u32,
    ac_dq: u32,
) -> u8 {
    let dc = predict_dc(above, left, have_above, have_left, w, h);
    let mut best_mode = 0u8;
    let mut best_cost = compute_rd_cost(source, &dc, dc_dq, ac_dq);

    if have_above {
        let v = predict_v(above, w, h);
        let cost = compute_rd_cost(source, &v, dc_dq, ac_dq);
        if cost < best_cost {
            best_cost = cost;
            best_mode = 1;
        }
    }

    if have_left {
        let hp = predict_h(left, w, h);
        let cost = compute_rd_cost(source, &hp, dc_dq, ac_dq);
        if cost < best_cost {
            best_cost = cost;
            best_mode = 2;
        }
    }

    if have_above && have_left {
        for (mode_fn, mode_id) in [
            (predict_smooth as fn(&[u8], &[u8], usize, usize) -> Vec<u8>, 9u8),
            (predict_smooth_v, 10),
            (predict_smooth_h, 11),
        ] {
            let pred = mode_fn(above, left, w, h);
            let cost = compute_rd_cost(source, &pred, dc_dq, ac_dq);
            if cost < best_cost {
                best_cost = cost;
                best_mode = mode_id;
            }
        }

        let paeth = predict_paeth(above, left, top_left, w, h);
        let cost = compute_rd_cost(source, &paeth, dc_dq, ac_dq);
        if cost < best_cost {
            best_mode = 12;
        }
    }

    best_mode
}
```

**Step 2: Update call site in `encode_block`**

Pass `self.dq.dc, self.dq.ac` to the call at the `select_best_intra_mode` invocation.

**Step 3: Update existing mode selection tests**

Fix test calls to pass `dc_dq, ac_dq` parameters. Use `crate::dequant::lookup_dequant(128)`.

**Step 4: Run all tests**

Run: `cargo test && cargo clippy --tests`
Expected: All 208+ tests PASS, no new clippy warnings

**Step 5: Run integration tests with dav1d**

Run: `cargo test --test integration`
Expected: All 14 integration tests PASS

---

### Task 3: Commit

```bash
git add wav1c/src/tile.rs docs/plans/2026-02-15-rd-mode-selection.md
git commit -m "feat: replace SAD with RD cost for intra mode selection"
```
