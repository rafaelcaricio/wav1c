use crate::cdf::CdfContext;
use crate::dequant::DequantValues;
use crate::msac::MsacEncoder;
use crate::y4m::FramePixels;
use std::cmp::min;

#[path = "dct.rs"]
mod dct;
#[path = "scan.rs"]
mod scan;

use scan::{DEFAULT_SCAN_4X4, DEFAULT_SCAN_8X8, LO_CTX_OFFSETS_2D};

const PARTITION_CTX_NONE: [u8; 5] = [0, 0x10, 0x18, 0x1c, 0x1e];

const PARTITION_NSYMS: [u32; 5] = [9, 9, 9, 9, 3];

const INTRA_MODE_CONTEXT: [usize; 13] = [
    0, 1, 2, 3, 4, 4, 4, 4, 3, 0, 1, 2, 0,
];

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

#[rustfmt::skip]
const DR_INTRA_DERIVATIVE: [u16; 44] = [
       0,
    1023, 0,
     547,
     372, 0, 0,
     273,
     215, 0,
     178,
     151, 0,
     132,
     116, 0,
     102, 0,
      90,
      80, 0,
      71,
      64, 0,
      57,
      51, 0,
      45, 0,
      40,
      35, 0,
      31,
      27, 0,
      23,
      19, 0,
      15, 0,
      11, 0,
       7,
       3,
];

const TXTP_INTRA2_MAP: [dct::TxType; 5] = [
    dct::TxType::Idtx,
    dct::TxType::DctDct,
    dct::TxType::AdstAdst,
    dct::TxType::AdstDct,
    dct::TxType::DctAdst,
];

#[rustfmt::skip]
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

#[rustfmt::skip]
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

fn txtype_to_intra2_symbol(tx: dct::TxType) -> u32 {
    match tx {
        dct::TxType::Idtx => 0,
        dct::TxType::DctDct => 1,
        dct::TxType::AdstAdst => 2,
        dct::TxType::AdstDct => 3,
        dct::TxType::DctAdst => 4,
    }
}

fn encode_hi_tok(enc: &mut MsacEncoder, cdf: &mut [u16], dc_tok: u32) {
    let mut base = 3;
    for _ in 0..4 {
        let sym = min(dc_tok - base, 3);
        enc.encode_symbol(sym, cdf, 3);
        if sym < 3 {
            return;
        }
        base += 3;
    }
}

fn eob_to_bin(eob: usize) -> usize {
    match eob {
        0 => 0,
        1 => 1,
        2..=3 => 2,
        4..=7 => 3,
        8..=15 => 4,
        16..=31 => 5,
        32..=63 => 6,
        _ => 7,
    }
}

fn get_lo_ctx(level: &[u8], stride: usize, x: usize, y: usize) -> (usize, u32) {
    let mag = level[1] as u32 + level[stride] as u32;
    let hi_mag = mag + level[stride + 1] as u32;
    let full_mag = hi_mag + level[2] as u32 + level[2 * stride] as u32;

    let offset = LO_CTX_OFFSETS_2D[y.min(4)][x.min(4)] as usize;

    let mag_ctx = if full_mag > 512 { 4 } else { ((full_mag + 64) >> 7) as usize };
    (offset + mag_ctx, hi_mag)
}

fn level_tok(magnitude: u32) -> u8 {
    match magnitude {
        0 => 0,
        1 => 0x41,
        2 => 0x82,
        m => (m.min(15) as u8) + (3 << 6),
    }
}

fn get_hi_mag(level: &[u8], stride: usize) -> u32 {
    level[1] as u32 + level[stride] as u32 + level[stride + 1] as u32
}

fn coef_ctx_value(cul_level: u8, dc_sign_negative: bool, dc_is_zero: bool) -> u8 {
    let dc_sign_level: u8 = if dc_is_zero {
        0x40
    } else if dc_sign_negative {
        0x00
    } else {
        0x80
    };
    cul_level | dc_sign_level
}

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

fn predict_directional_z1(above: &[u8], w: usize, h: usize, dx: i32) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let max_base_x = (w + min(w, h) - 1).min(above.len().saturating_sub(1));
    for y in 0..h {
        let xpos_row = dx * (y as i32 + 1);
        let frac = xpos_row & 0x3E;
        let mut base = (xpos_row >> 6) as usize;
        for x in 0..w {
            if base < max_base_x {
                let v = above[base] as i32 * (64 - frac)
                    + above[base + 1] as i32 * frac;
                out[y * w + x] = ((v + 32) >> 6).clamp(0, 255) as u8;
            } else {
                for fill_x in x..w {
                    out[y * w + fill_x] = above[max_base_x];
                }
                break;
            }
            base += 1;
        }
    }
    out
}

fn predict_directional_z3(left: &[u8], w: usize, h: usize, dy: i32) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let max_base_y = (h + min(w, h) - 1).min(left.len().saturating_sub(1));
    for x in 0..w {
        let ypos_col = dy * (x as i32 + 1);
        let frac = ypos_col & 0x3E;
        let mut base = (ypos_col >> 6) as usize;
        for y in 0..h {
            if base < max_base_y {
                let v = left[base] as i32 * (64 - frac)
                    + left[base + 1] as i32 * frac;
                out[y * w + x] = ((v + 32) >> 6).clamp(0, 255) as u8;
            } else {
                for fill_y in y..h {
                    out[fill_y * w + x] = left[max_base_y];
                }
                break;
            }
            base += 1;
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn predict_directional_z2(
    above: &[u8], left: &[u8], top_left: u8,
    w: usize, h: usize, dx: i32, dy: i32,
) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let mut edge = vec![0u8; w + h + 1];
    let tl_idx = h;
    for i in 0..h {
        edge[h - 1 - i] = left[i];
    }
    edge[tl_idx] = top_left;
    for i in 0..w {
        edge[tl_idx + 1 + i] = above[i];
    }

    for y in 0..h {
        let xpos_row = 64_i32 - dx * (y as i32 + 1);
        let frac_x = xpos_row & 0x3E;
        let mut base_x = xpos_row >> 6;

        for x in 0..w {
            let v;
            if base_x >= 0 {
                let bx = base_x as usize;
                let idx = tl_idx + bx;
                if idx + 1 < edge.len() {
                    v = edge[idx] as i32 * (64 - frac_x)
                        + edge[idx + 1] as i32 * frac_x;
                } else {
                    v = edge[edge.len() - 1] as i32 * 64;
                }
            } else {
                let ypos = (y as i32 * 64) - dy * (x as i32 + 1);
                let base_y = ypos >> 6;
                let frac_y = ypos & 0x3E;
                if base_y >= 0 {
                    let by = base_y as usize;
                    let idx = tl_idx.wrapping_sub(1).wrapping_sub(by);
                    if idx < edge.len() && idx >= 1 {
                        v = edge[idx] as i32 * (64 - frac_y)
                            + edge[idx - 1] as i32 * frac_y;
                    } else if idx < edge.len() {
                        v = edge[idx] as i32 * 64;
                    } else {
                        v = top_left as i32 * 64;
                    }
                } else {
                    v = top_left as i32 * 64;
                }
            }
            out[y * w + x] = ((v + 32) >> 6).clamp(0, 255) as u8;
            base_x += 1;
        }
    }
    out
}

const MODE_TO_ANGLE: [i32; 8] = [90, 180, 45, 135, 113, 157, 203, 67];

#[allow(clippy::too_many_arguments)]
fn generate_directional_prediction(
    angle: i32, above: &[u8], left: &[u8], top_left: u8,
    have_above: bool, have_left: bool, w: usize, h: usize,
) -> Vec<u8> {
    if angle <= 90 {
        if angle < 90 && have_above {
            let dx = DR_INTRA_DERIVATIVE[(angle / 2) as usize] as i32;
            predict_directional_z1(above, w, h, dx)
        } else {
            predict_v(above, w, h)
        }
    } else if angle < 180 {
        let dx = DR_INTRA_DERIVATIVE[((180 - angle) / 2) as usize] as i32;
        let dy = DR_INTRA_DERIVATIVE[((angle - 90) / 2) as usize] as i32;
        predict_directional_z2(above, left, top_left, w, h, dx, dy)
    } else if angle > 180 && have_left {
        let dy = DR_INTRA_DERIVATIVE[((270 - angle) / 2) as usize] as i32;
        predict_directional_z3(left, w, h, dy)
    } else {
        predict_h(left, w, h)
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_prediction(
    mode: u8, delta: i8, above: &[u8], left: &[u8], top_left: u8,
    have_above: bool, have_left: bool, w: usize, h: usize,
) -> Vec<u8> {
    match mode {
        1..=8 => {
            let angle = MODE_TO_ANGLE[(mode - 1) as usize] + 3 * delta as i32;
            generate_directional_prediction(angle, above, left, top_left, have_above, have_left, w, h)
        }
        9 => predict_smooth(above, left, w, h),
        10 => predict_smooth_v(above, left, w, h),
        11 => predict_smooth_h(above, left, w, h),
        12 => predict_paeth(above, left, top_left, w, h),
        _ => predict_dc(above, left, have_above, have_left, w, h),
    }
}

#[allow(dead_code)]
fn compute_sad(source: &[u8], prediction: &[u8]) -> u32 {
    source.iter().zip(prediction.iter())
        .map(|(&s, &p)| (s as i32 - p as i32).unsigned_abs())
        .sum()
}

fn compute_rd_cost(source: &[u8], prediction: &[u8], dc_dq: u32, ac_dq: u32, tx_type: dct::TxType) -> u64 {
    let mut residual = [0i32; 64];
    for i in 0..64 {
        residual[i] = source[i] as i32 - prediction[i] as i32;
    }

    let dct_coeffs = dct::forward_transform_8x8(&residual, tx_type);
    let quant = quantize_coeffs(&dct_coeffs, 64, dc_dq, ac_dq);
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
) -> (u8, i8) {
    let dc = predict_dc(above, left, have_above, have_left, w, h);
    let mut best_mode = 0u8;
    let mut best_delta = 0i8;
    let mut best_cost = compute_rd_cost(source, &dc, dc_dq, ac_dq, dct::TxType::DctDct);

    if have_above {
        let v = predict_v(above, w, h);
        let cost = compute_rd_cost(source, &v, dc_dq, ac_dq, dct::TxType::DctDct);
        if cost < best_cost {
            best_cost = cost;
            best_mode = 1;
            best_delta = 0;
        }
    }

    if have_left {
        let hp = predict_h(left, w, h);
        let cost = compute_rd_cost(source, &hp, dc_dq, ac_dq, dct::TxType::DctDct);
        if cost < best_cost {
            best_cost = cost;
            best_mode = 2;
            best_delta = 0;
        }
    }

    if have_above && have_left {
        let smooth = predict_smooth(above, left, w, h);
        let cost = compute_rd_cost(source, &smooth, dc_dq, ac_dq, dct::TxType::DctDct);
        if cost < best_cost {
            best_cost = cost;
            best_mode = 9;
            best_delta = 0;
        }

        let sv = predict_smooth_v(above, left, w, h);
        let cost = compute_rd_cost(source, &sv, dc_dq, ac_dq, dct::TxType::DctDct);
        if cost < best_cost {
            best_cost = cost;
            best_mode = 10;
            best_delta = 0;
        }

        let sh = predict_smooth_h(above, left, w, h);
        let cost = compute_rd_cost(source, &sh, dc_dq, ac_dq, dct::TxType::DctDct);
        if cost < best_cost {
            best_cost = cost;
            best_mode = 11;
            best_delta = 0;
        }

        let paeth = predict_paeth(above, left, top_left, w, h);
        let cost = compute_rd_cost(source, &paeth, dc_dq, ac_dq, dct::TxType::DctDct);
        if cost < best_cost {
            best_cost = cost;
            best_mode = 12;
            best_delta = 0;
        }

        for mode in 1..=8u8 {
            for delta in -3..=3i8 {
                if delta == 0 && (mode == 1 || mode == 2) {
                    continue;
                }
                let pred = generate_prediction(mode, delta, above, left, top_left, true, true, w, h);
                let cost = compute_rd_cost(source, &pred, dc_dq, ac_dq, dct::TxType::DctDct);
                if cost < best_cost {
                    best_cost = cost;
                    best_mode = mode;
                    best_delta = delta;
                }
            }
        }
    }

    (best_mode, best_delta)
}

fn select_best_txtype(source: &[u8], prediction: &[u8], dc_dq: u32, ac_dq: u32) -> dct::TxType {
    let mut best_type = dct::TxType::DctDct;
    let mut best_cost = compute_rd_cost(source, prediction, dc_dq, ac_dq, dct::TxType::DctDct);

    for &tx in &TXTP_INTRA2_MAP {
        if tx == dct::TxType::DctDct {
            continue;
        }
        let cost = compute_rd_cost(source, prediction, dc_dq, ac_dq, tx);
        if cost < best_cost {
            best_cost = cost;
            best_type = tx;
        }
    }

    best_type
}

#[allow(clippy::too_many_arguments)]
fn encode_transform_block(
    enc: &mut MsacEncoder,
    cdf: &mut CdfContext,
    coeffs: &[i32],
    scan_table: &[u16],
    is_chroma: bool,
    is_inter: bool,
    t_dim_ctx: usize,
    txb_skip_ctx: usize,
    dc_sign_ctx: usize,
    y_mode: u8,
    tx_type: dct::TxType,
) -> (u8, bool, bool) {
    let chroma_idx = if is_chroma { 1 } else { 0 };
    let n = scan_table.len();
    let w = if n == 16 { 4usize } else { 8usize };

    let mut eob: i32 = -1;
    for (i, &sc) in scan_table[..n].iter().enumerate() {
        if coeffs[sc as usize] != 0 {
            eob = i as i32;
        }
    }

    if eob < 0 {
        enc.encode_bool(true, &mut cdf.txb_skip[t_dim_ctx][txb_skip_ctx]);
        return (0, false, true);
    }
    let eob = eob as usize;

    enc.encode_bool(false, &mut cdf.txb_skip[t_dim_ctx][txb_skip_ctx]);

    if !is_chroma {
        if is_inter {
            enc.encode_bool(true, &mut cdf.txtp_inter);
        } else {
            let t_dim_min = if scan_table.len() == 16 { 0usize } else { 1usize };
            enc.encode_symbol(txtype_to_intra2_symbol(tx_type), &mut cdf.txtp_intra2[t_dim_min][y_mode as usize], 4);
        }
    }

    let eob_bin = eob_to_bin(eob);
    let n_eob_syms = if n == 16 { 4u32 } else { 6u32 };
    let eob_cdf = if n == 16 {
        &mut cdf.eob_bin_16[chroma_idx][0]
    } else {
        &mut cdf.eob_bin_64[chroma_idx][0]
    };
    enc.encode_symbol(eob_bin as u32, eob_cdf, n_eob_syms);

    if eob_bin >= 2 {
        let extra_bits = eob_bin - 2;
        let hi_bit = (eob >> extra_bits) & 1;
        enc.encode_bool(hi_bit != 0, &mut cdf.eob_hi_bit[t_dim_ctx][chroma_idx][eob_bin - 2]);
        for bit_idx in (0..extra_bits).rev() {
            enc.encode_bool_equi((eob >> bit_idx) & 1 != 0);
        }
    }

    let stride = w;
    let levels_size = stride * (w + 2);
    let mut levels = vec![0u8; levels_size];

    let tx2dszctx = if n == 16 { 0usize } else { 2 };
    let eob_ctx = if eob == 0 {
        0
    } else {
        1 + usize::from(eob > (2 << tx2dszctx)) + usize::from(eob > (4 << tx2dszctx))
    };

    {
        let eob_rc = scan_table[eob] as usize;
        let eob_level = coeffs[eob_rc].unsigned_abs();
        let eob_tok = eob_level.min(3);
        let eob_base = if eob_tok >= 3 { 2u32 } else { eob_tok.saturating_sub(1) };
        enc.encode_symbol(eob_base, &mut cdf.eob_base_tok[t_dim_ctx][chroma_idx][eob_ctx], 2);

        if eob_level >= 3 {
            let eob_x = eob_rc % w;
            let eob_y = eob_rc / w;
            let mag = get_hi_mag(&levels[eob_rc..], stride) & 63;
            let hi_ctx = if eob == 0 {
                0
            } else {
                (if (eob_y | eob_x) > 1 { 14 } else { 7 })
                    + (if mag > 12 { 6 } else { ((mag + 1) >> 1) as usize })
            };
            encode_hi_tok(enc, &mut cdf.br_tok[t_dim_ctx.min(3)][chroma_idx][hi_ctx], eob_level);
        }

        levels[eob_rc] = level_tok(eob_level);
    }

    for i in (1..eob).rev() {
        let rc = scan_table[i] as usize;
        let x = rc % w;
        let y = rc / w;
        let level = coeffs[rc].unsigned_abs();

        let (ctx, _hi_mag) = get_lo_ctx(&levels[rc..], stride, x, y);
        let tok = level.min(3);
        enc.encode_symbol(tok, &mut cdf.base_tok[t_dim_ctx][chroma_idx][ctx], 3);

        if level >= 3 {
            let mag = get_hi_mag(&levels[rc..], stride) & 63;
            let hi_ctx = (if (y | x) > 1 { 14 } else { 7 })
                + (if mag > 12 { 6 } else { ((mag + 1) >> 1) as usize });
            encode_hi_tok(enc, &mut cdf.br_tok[t_dim_ctx.min(3)][chroma_idx][hi_ctx], level);
        }

        levels[rc] = level_tok(level);
    }

    if eob > 0 {
        let level = coeffs[0].unsigned_abs();

        let tok = level.min(3);
        enc.encode_symbol(tok, &mut cdf.base_tok[t_dim_ctx][chroma_idx][0], 3);

        if level >= 3 {
            let mag = get_hi_mag(&levels, stride) & 63;
            let hi_ctx = if mag > 12 { 6 } else { ((mag + 1) >> 1) as usize };
            encode_hi_tok(enc, &mut cdf.br_tok[t_dim_ctx.min(3)][chroma_idx][hi_ctx], level);
        }

        levels[0] = level_tok(level);
    }

    if coeffs[0] != 0 {
        let is_negative = coeffs[0] < 0;
        enc.encode_bool(is_negative, &mut cdf.dc_sign[chroma_idx][dc_sign_ctx]);
    }
    if coeffs[0].unsigned_abs() >= 15 {
        enc.encode_golomb(coeffs[0].unsigned_abs() - 15);
    }

    for &sc in &scan_table[1..=eob] {
        let rc = sc as usize;
        if coeffs[rc] != 0 {
            enc.encode_bool_equi(coeffs[rc] < 0);
            if coeffs[rc].unsigned_abs() >= 15 {
                enc.encode_golomb(coeffs[rc].unsigned_abs() - 15);
            }
        }
    }

    let cul_level: u32 = (0..=eob)
        .map(|i| {
            let rc = scan_table[i] as usize;
            coeffs[rc].unsigned_abs()
        })
        .sum();
    let cul_level = cul_level.min(63) as u8;
    let dc_is_zero = coeffs[0] == 0;
    let dc_negative = coeffs[0] < 0;

    (cul_level, dc_negative, dc_is_zero)
}

fn quantize_coeffs(dct_coeffs: &[i32], n: usize, dc_dq: u32, ac_dq: u32) -> Vec<i32> {
    let mut quantized = vec![0i32; n];
    for i in 0..n {
        let dq = if i == 0 { dc_dq } else { ac_dq };
        let abs_val = dct_coeffs[i].unsigned_abs();
        let tok = (abs_val + dq / 2) / dq;
        quantized[i] = if dct_coeffs[i] < 0 {
            -(tok as i32)
        } else {
            tok as i32
        };
    }
    quantized
}

fn dequantize_coeffs(quantized: &[i32], n: usize, dc_dq: u32, ac_dq: u32) -> Vec<i32> {
    let mut dequantized = vec![0i32; n];
    for i in 0..n {
        let dq = if i == 0 { dc_dq } else { ac_dq };
        dequantized[i] = quantized[i] * dq as i32;
    }
    dequantized
}

fn gather_top_partition_prob(pc: &[u16], bl: usize) -> u16 {
    let mut out = pc[1].wrapping_sub(pc[4]);
    out = out.wrapping_add(pc[5]);
    if bl != 0 {
        out = out.wrapping_add(pc[8].wrapping_sub(pc[7]));
    }
    out
}

fn gather_left_partition_prob(pc: &[u16], bl: usize) -> u16 {
    let mut out = pc[0].wrapping_sub(pc[1]);
    out = out.wrapping_add(pc[2].wrapping_sub(pc[6]));
    if bl != 0 {
        out = out.wrapping_add(pc[7].wrapping_sub(pc[8]));
    }
    out
}

fn extract_block(
    plane: &[u8],
    plane_stride: u32,
    px_x: u32,
    px_y: u32,
    block_size: usize,
    frame_w: u32,
    frame_h: u32,
) -> Vec<u8> {
    let mut block = vec![0u8; block_size * block_size];
    for r in 0..block_size {
        for c in 0..block_size {
            let sy = min(px_y + r as u32, frame_h - 1);
            let sx = min(px_x + c as u32, frame_w - 1);
            block[r * block_size + c] = plane[(sy * plane_stride + sx) as usize];
        }
    }
    block
}

#[allow(clippy::too_many_arguments)]
fn interpolate_block(
    reference: &[u8],
    width: u32,
    height: u32,
    int_x: i32,
    int_y: i32,
    phase_x: u32,
    phase_y: u32,
    block_size: u32,
) -> Vec<u8> {
    let bs = block_size as usize;
    let w = width as i32;
    let h = height as i32;
    let mut output = vec![0u8; bs * bs];

    let mx = phase_x * 2;
    let my = phase_y * 2;
    let filter_table = if block_size > 4 {
        &SUBPEL_FILTER_8TAP
    } else {
        &SUBPEL_FILTER_4TAP
    };

    let ref_pixel = |sx: i32, sy: i32| -> i32 {
        let cx = sx.clamp(0, w - 1) as u32;
        let cy = sy.clamp(0, h - 1) as u32;
        reference[(cy * width + cx) as usize] as i32
    };

    if mx == 0 && my == 0 {
        for r in 0..bs {
            for c in 0..bs {
                output[r * bs + c] = ref_pixel(int_x + c as i32, int_y + r as i32) as u8;
            }
        }
    } else if mx != 0 && my == 0 {
        let fh = &filter_table[(mx - 1) as usize];
        for r in 0..bs {
            let sy = int_y + r as i32;
            for c in 0..bs {
                let mut sum = 0i32;
                for t in 0..8i32 {
                    sum += fh[t as usize] as i32 * ref_pixel(int_x + c as i32 + t - 3, sy);
                }
                output[r * bs + c] = ((sum + 34) >> 6).clamp(0, 255) as u8;
            }
        }
    } else if mx == 0 {
        let fv = &filter_table[(my - 1) as usize];
        for r in 0..bs {
            for c in 0..bs {
                let sx = int_x + c as i32;
                let mut sum = 0i32;
                for t in 0..8i32 {
                    sum += fv[t as usize] as i32 * ref_pixel(sx, int_y + r as i32 + t - 3);
                }
                output[r * bs + c] = ((sum + 32) >> 6).clamp(0, 255) as u8;
            }
        }
    } else {
        let fh = &filter_table[(mx - 1) as usize];
        let fv = &filter_table[(my - 1) as usize];
        let mid_rows = bs + 7;
        let mut mid = vec![0i16; mid_rows * bs];

        for r in 0..mid_rows {
            let sy = int_y + r as i32 - 3;
            for c in 0..bs {
                let mut sum = 0i32;
                for t in 0..8i32 {
                    sum += fh[t as usize] as i32 * ref_pixel(int_x + c as i32 + t - 3, sy);
                }
                mid[r * bs + c] = ((sum + 2) >> 2) as i16;
            }
        }

        for r in 0..bs {
            for c in 0..bs {
                let mut sum = 0i32;
                for t in 0..8 {
                    sum += fv[t] as i32 * mid[(r + t) * bs + c] as i32;
                }
                output[r * bs + c] = ((sum + 512) >> 10).clamp(0, 255) as u8;
            }
        }
    }

    output
}

fn compute_block_sad(source: &[u8], predicted: &[u8]) -> u32 {
    source
        .iter()
        .zip(predicted.iter())
        .map(|(&s, &p)| (s as i32 - p as i32).unsigned_abs())
        .sum()
}

#[allow(clippy::too_many_arguments)]
fn subpel_refine(
    source: &[u8],
    reference: &[u8],
    width: u32,
    height: u32,
    px_x: u32,
    px_y: u32,
    block_size: u32,
    best_mv_x: i32,
    best_mv_y: i32,
) -> (i32, i32) {
    let bs = block_size as usize;
    let src_block: Vec<u8> = {
        let mut b = vec![0u8; bs * bs];
        for r in 0..bs {
            for c in 0..bs {
                let sy = min(px_y + r as u32, height - 1);
                let sx = min(px_x + c as u32, width - 1);
                b[r * bs + c] = source[(sy * width + sx) as usize];
            }
        }
        b
    };

    let eval = |mv_x: i32, mv_y: i32| -> u32 {
        let int_x = px_x as i32 + (mv_x >> 3);
        let int_y = px_y as i32 + (mv_y >> 3);
        let phase_x = (mv_x & 7) as u32;
        let phase_y = (mv_y & 7) as u32;
        let pred = interpolate_block(reference, width, height, int_x, int_y, phase_x, phase_y, block_size);
        compute_block_sad(&src_block, &pred)
    };

    let mut bx = best_mv_x;
    let mut by = best_mv_y;
    let mut best_sad = eval(bx, by);

    for &step in &[4i32, 2] {
        for &(dx, dy) in &[
            (-step, 0), (step, 0), (0, -step), (0, step),
            (-step, -step), (-step, step), (step, -step), (step, step),
        ] {
            let cx = bx + dx;
            let cy = by + dy;
            let sad = eval(cx, cy);
            let new_cost = cx.abs() + cy.abs();
            let old_cost = bx.abs() + by.abs();
            if sad < best_sad || (sad == best_sad && new_cost < old_cost) {
                best_sad = sad;
                bx = cx;
                by = cy;
            }
        }
    }

    (bx, by)
}

struct TileEncoder<'a> {
    enc: MsacEncoder,
    cdf: CdfContext,
    ctx: TileContext,
    mi_cols: u32,
    mi_rows: u32,
    pixels: &'a FramePixels,
    dq: DequantValues,
    recon: FramePixels,
}

struct TileContext {
    above_partition: Vec<u8>,
    above_skip: Vec<u8>,
    left_partition: [u8; 16],
    left_skip: [u8; 32],
    above_recon_y: Vec<u8>,
    above_recon_u: Vec<u8>,
    above_recon_v: Vec<u8>,
    left_recon_y: [u8; 64],
    left_recon_u: [u8; 32],
    left_recon_v: [u8; 32],
    above_lcoef: Vec<u8>,
    left_lcoef: [u8; 32],
    above_ccoef: [Vec<u8>; 2],
    left_ccoef: [[u8; 16]; 2],
    above_intra: Vec<bool>,
    left_intra: [bool; 32],
    above_mode: Vec<u8>,
    left_mode: [u8; 32],
    above_newmv: Vec<bool>,
    left_newmv: [bool; 32],
}

impl TileContext {
    fn new(mi_cols: u32) -> Self {
        let above_part_size = (mi_cols as usize / 2) + 16;
        let above_skip_size = mi_cols as usize + 32;
        let above_recon_y_size = mi_cols as usize * 4 + 32;
        let above_recon_uv_size = mi_cols as usize * 2 + 16;
        let above_coef_size = mi_cols as usize + 32;
        let above_ccoef_size = (mi_cols as usize / 2) + 16;
        let above_inter_size = mi_cols as usize + 32;
        Self {
            above_partition: vec![0u8; above_part_size],
            above_skip: vec![0u8; above_skip_size],
            left_partition: [0u8; 16],
            left_skip: [0u8; 32],
            above_recon_y: vec![128u8; above_recon_y_size],
            above_recon_u: vec![128u8; above_recon_uv_size],
            above_recon_v: vec![128u8; above_recon_uv_size],
            left_recon_y: [128u8; 64],
            left_recon_u: [128u8; 32],
            left_recon_v: [128u8; 32],
            above_lcoef: vec![0x40u8; above_coef_size],
            left_lcoef: [0x40u8; 32],
            above_ccoef: [
                vec![0x40u8; above_ccoef_size],
                vec![0x40u8; above_ccoef_size],
            ],
            left_ccoef: [[0x40u8; 16]; 2],
            above_intra: vec![false; above_inter_size],
            left_intra: [false; 32],
            above_mode: vec![0u8; mi_cols as usize + 32],
            left_mode: [0u8; 32],
            above_newmv: vec![false; above_inter_size],
            left_newmv: [false; 32],
        }
    }

    fn reset_left_for_sb_row(&mut self) {
        self.left_partition = [0u8; 16];
        self.left_skip = [0u8; 32];
        self.left_recon_y = [128u8; 64];
        self.left_recon_u = [128u8; 32];
        self.left_recon_v = [128u8; 32];
        self.left_lcoef = [0x40u8; 32];
        self.left_ccoef = [[0x40u8; 16]; 2];
        self.left_intra = [false; 32];
        self.left_mode = [0u8; 32];
        self.left_newmv = [false; 32];
    }

    fn partition_ctx(&self, bx: u32, by: u32, bl: usize) -> usize {
        let bx8 = (bx >> 1) as usize;
        let by8 = ((by & 31) >> 1) as usize;
        let above = (self.above_partition[bx8] >> (4 - bl)) & 1;
        let left = (self.left_partition[by8] >> (4 - bl)) & 1;
        above as usize | ((left as usize) << 1)
    }

    fn skip_ctx(&self, bx: u32, by: u32) -> usize {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        self.above_skip[bx4] as usize + self.left_skip[by4] as usize
    }

    fn update_partition_ctx(&mut self, bx: u32, by: u32, bl: usize, mi_cols: u32, mi_rows: u32) {
        let bx8 = (bx >> 1) as usize;
        let by8 = ((by & 31) >> 1) as usize;
        let hsz = 16usize >> bl;
        let aw = min(hsz, (mi_cols - bx).div_ceil(2) as usize);
        let lh = min(hsz, (mi_rows - by).div_ceil(2) as usize);
        let above_val = PARTITION_CTX_NONE[bl];
        let left_val = PARTITION_CTX_NONE[bl];
        for i in 0..aw {
            if bx8 + i < self.above_partition.len() {
                self.above_partition[bx8 + i] = above_val;
            }
        }
        for i in 0..lh {
            if by8 + i < 16 {
                self.left_partition[by8 + i] = left_val;
            }
        }
    }

    fn update_skip_ctx(
        &mut self,
        bx: u32,
        by: u32,
        bl: usize,
        mi_cols: u32,
        mi_rows: u32,
        is_skip: bool,
    ) {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let bw4 = 2 * (16usize >> bl);
        let aw = min(bw4, (mi_cols - bx) as usize);
        let lh = min(bw4, (mi_rows - by) as usize);
        let val = u8::from(is_skip);
        for i in 0..aw {
            if bx4 + i < self.above_skip.len() {
                self.above_skip[bx4 + i] = val;
            }
        }
        for i in 0..lh {
            if by4 + i < 32 {
                self.left_skip[by4 + i] = val;
            }
        }
    }

    fn dc_sign_ctx(&self, bx: u32, by: u32, bl: usize, plane: usize) -> usize {
        let (above_coef, left_coef, bx4, by4, n_above, n_left) = if plane == 0 {
            let bx4 = bx as usize;
            let by4 = (by & 31) as usize;
            let n = 2 * (16usize >> bl);
            (&self.above_lcoef[..], &self.left_lcoef[..], bx4, by4, n, n)
        } else {
            let pl = plane - 1;
            let bx4 = (bx / 2) as usize;
            let by4 = ((by & 31) / 2) as usize;
            let n = (16usize >> bl).max(1);
            (
                &self.above_ccoef[pl][..],
                &self.left_ccoef[pl][..],
                bx4,
                by4,
                n,
                n,
            )
        };

        let mut sum = 0i32;
        for i in 0..n_above {
            let idx = bx4 + i;
            if idx < above_coef.len() {
                sum += (above_coef[idx] >> 6) as i32;
            } else {
                sum += 1;
            }
        }
        for i in 0..n_left {
            let idx = by4 + i;
            if idx < left_coef.len() {
                sum += (left_coef[idx] >> 6) as i32;
            } else {
                sum += 1;
            }
        }

        let s = sum - (n_above as i32 + n_left as i32);
        if s < 0 {
            1
        } else if s > 0 {
            2
        } else {
            0
        }
    }

    fn chroma_txb_skip_ctx(&self, bx: u32, by: u32, bl: usize, plane: usize) -> usize {
        let pl = plane - 1;
        let bx4 = (bx / 2) as usize;
        let by4 = ((by & 31) / 2) as usize;
        let n = (16usize >> bl).max(1);

        let mut ca = false;
        for i in 0..n {
            let idx = bx4 + i;
            if idx < self.above_ccoef[pl].len() && self.above_ccoef[pl][idx] != 0x40 {
                ca = true;
                break;
            }
        }

        let mut cl = false;
        for i in 0..n {
            let idx = by4 + i;
            if idx < self.left_ccoef[pl].len() && self.left_ccoef[pl][idx] != 0x40 {
                cl = true;
                break;
            }
        }

        7 + ca as usize + cl as usize
    }

    #[allow(clippy::too_many_arguments)]
    fn update_coef_ctx(
        &mut self,
        bx: u32,
        by: u32,
        bl: usize,
        mi_cols: u32,
        mi_rows: u32,
        y_ctx: u8,
        u_ctx: u8,
        v_ctx: u8,
    ) {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let bw4 = 2 * (16usize >> bl);

        let aw = min(bw4, (mi_cols - bx) as usize);
        let lh = min(bw4, (mi_rows - by) as usize);

        for i in 0..aw {
            if bx4 + i < self.above_lcoef.len() {
                self.above_lcoef[bx4 + i] = y_ctx;
            }
        }
        for i in 0..lh {
            if by4 + i < self.left_lcoef.len() {
                self.left_lcoef[by4 + i] = y_ctx;
            }
        }

        let cbx4 = (bx / 2) as usize;
        let cby4 = ((by & 31) / 2) as usize;
        let cw4 = (16usize >> bl).max(1);

        let caw = min(cw4, (mi_cols - bx).div_ceil(2) as usize);
        let clh = min(cw4, (mi_rows - by).div_ceil(2) as usize);

        for i in 0..caw {
            if cbx4 + i < self.above_ccoef[0].len() {
                self.above_ccoef[0][cbx4 + i] = u_ctx;
                self.above_ccoef[1][cbx4 + i] = v_ctx;
            }
        }
        for i in 0..clh {
            if cby4 + i < self.left_ccoef[0].len() {
                self.left_ccoef[0][cby4 + i] = u_ctx;
                self.left_ccoef[1][cby4 + i] = v_ctx;
            }
        }
    }

    fn dc_prediction(&self, bx: u32, by: u32, bl: usize, plane: usize) -> u8 {
        let have_top = by > 0;
        let have_left = bx > 0;

        if !have_top && !have_left {
            return 128;
        }

        let (above_recon, left_recon, px_x, left_local_py, block_pixels) = if plane == 0 {
            let bp = 1usize << (7 - bl);
            (
                &self.above_recon_y[..],
                &self.left_recon_y[..],
                (bx * 4) as usize,
                ((by & 15) * 4) as usize,
                bp,
            )
        } else {
            let bp = 1usize << (6 - bl);
            let above = if plane == 1 {
                &self.above_recon_u[..]
            } else {
                &self.above_recon_v[..]
            };
            let left = if plane == 1 {
                &self.left_recon_u[..]
            } else {
                &self.left_recon_v[..]
            };
            (
                above,
                left,
                (bx * 2) as usize,
                ((by & 15) * 2) as usize,
                bp,
            )
        };

        if have_top && have_left {
            let mut sum = 0u32;
            for i in 0..block_pixels {
                let idx = px_x + i;
                if idx < above_recon.len() {
                    sum += above_recon[idx] as u32;
                }
            }
            for i in 0..block_pixels {
                let idx = left_local_py + i;
                if idx < left_recon.len() {
                    sum += left_recon[idx] as u32;
                }
            }
            let count = (2 * block_pixels) as u32;
            ((sum + count / 2) / count) as u8
        } else if have_top {
            let mut sum = 0u32;
            for i in 0..block_pixels {
                let idx = px_x + i;
                if idx < above_recon.len() {
                    sum += above_recon[idx] as u32;
                }
            }
            let count = block_pixels as u32;
            ((sum + count / 2) / count) as u8
        } else {
            let mut sum = 0u32;
            for i in 0..block_pixels {
                let idx = left_local_py + i;
                if idx < left_recon.len() {
                    sum += left_recon[idx] as u32;
                }
            }
            let count = block_pixels as u32;
            ((sum + count / 2) / count) as u8
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn update_recon(
        &mut self,
        bx: u32,
        by: u32,
        mi_cols: u32,
        mi_rows: u32,
        y_bottom_row: &[u8],
        y_right_col: &[u8],
        u_bottom_row: &[u8],
        u_right_col: &[u8],
        v_bottom_row: &[u8],
        v_right_col: &[u8],
    ) {
        let px_x = (bx * 4) as usize;
        let py_local = ((by & 15) * 4) as usize;
        let max_px_x = (mi_cols * 4) as usize;
        let max_py = (mi_rows * 4) as usize;
        let py_abs = (by * 4) as usize;

        for (i, &val) in y_bottom_row.iter().enumerate() {
            if px_x + i < max_px_x && px_x + i < self.above_recon_y.len() {
                self.above_recon_y[px_x + i] = val;
            }
        }
        for (i, &val) in y_right_col.iter().enumerate() {
            if py_abs + i < max_py && py_local + i < self.left_recon_y.len() {
                self.left_recon_y[py_local + i] = val;
            }
        }

        let cpx = (bx * 2) as usize;
        let cpy_local = ((by & 15) * 2) as usize;
        let max_cpx = (mi_cols * 2) as usize;
        let cpy_abs = (by * 2) as usize;
        let max_cpy = (mi_rows * 2) as usize;

        for i in 0..u_bottom_row.len() {
            if cpx + i < max_cpx && cpx + i < self.above_recon_u.len() {
                self.above_recon_u[cpx + i] = u_bottom_row[i];
                self.above_recon_v[cpx + i] = v_bottom_row[i];
            }
        }
        for i in 0..u_right_col.len() {
            if cpy_abs + i < max_cpy && cpy_local + i < self.left_recon_u.len() {
                self.left_recon_u[cpy_local + i] = u_right_col[i];
                self.left_recon_v[cpy_local + i] = v_right_col[i];
            }
        }
    }

    fn ref_ctx(&self, bx: u32, by: u32) -> usize {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let have_top = by > 0;
        let have_left = bx > 0;

        let above_inter = have_top
            && bx4 < self.above_intra.len()
            && !self.above_intra[bx4];
        let left_inter = have_left && !self.left_intra[by4.min(31)];

        if above_inter || left_inter {
            2
        } else {
            1
        }
    }

    fn has_inter_neighbor(&self, bx: u32, by: u32) -> bool {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let have_top = by > 0;
        let have_left = bx > 0;

        let above_inter = have_top
            && bx4 < self.above_intra.len()
            && !self.above_intra[bx4];
        let left_inter = have_left && !self.left_intra[by4.min(31)];

        above_inter || left_inter
    }

    fn newmv_ctx(&self, bx: u32, by: u32) -> usize {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let have_top = by > 0;
        let have_left = bx > 0;

        let above_inter = have_top
            && bx4 < self.above_intra.len()
            && !self.above_intra[bx4];
        let left_inter = have_left && !self.left_intra[by4.min(31)];

        let above_is_newmv = above_inter
            && bx4 < self.above_newmv.len()
            && self.above_newmv[bx4];
        let left_is_newmv = left_inter && self.left_newmv[by4.min(31)];
        let have_newmv = (above_is_newmv || left_is_newmv) as u32;

        let nearest_match = above_inter as u32 + left_inter as u32;
        match nearest_match {
            0 => 0,
            1 => (3 - have_newmv) as usize,
            2 => (5 - have_newmv) as usize,
            _ => unreachable!(),
        }
    }

    fn is_inter_ctx(&self, bx: u32, by: u32) -> usize {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let have_top = by > 0;
        let have_left = bx > 0;

        if have_left {
            if have_top {
                let l = self.left_intra[by4.min(31)] as usize;
                let a = if bx4 < self.above_intra.len() {
                    self.above_intra[bx4] as usize
                } else {
                    0
                };
                let ctx = l + a;
                ctx + usize::from(ctx == 2)
            } else {
                (self.left_intra[by4.min(31)] as usize) * 2
            }
        } else if have_top {
            let a = if bx4 < self.above_intra.len() {
                self.above_intra[bx4] as usize
            } else {
                0
            };
            a * 2
        } else {
            0
        }
    }

    fn update_intra_ctx(
        &mut self,
        bx: u32,
        by: u32,
        bl: usize,
        mi_cols: u32,
        mi_rows: u32,
        is_intra: bool,
    ) {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let bw4 = 2 * (16usize >> bl);
        let aw = min(bw4, (mi_cols - bx) as usize);
        let lh = min(bw4, (mi_rows - by) as usize);
        for i in 0..aw {
            if bx4 + i < self.above_intra.len() {
                self.above_intra[bx4 + i] = is_intra;
            }
        }
        for i in 0..lh {
            if by4 + i < 32 {
                self.left_intra[by4 + i] = is_intra;
            }
        }
    }

    fn update_newmv_flag(
        &mut self,
        bx: u32,
        by: u32,
        bl: usize,
        mi_cols: u32,
        mi_rows: u32,
        is_newmv: bool,
    ) {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let bw4 = 2 * (16usize >> bl);
        let aw = min(bw4, (mi_cols - bx) as usize);
        let lh = min(bw4, (mi_rows - by) as usize);
        for i in 0..aw {
            if bx4 + i < self.above_newmv.len() {
                self.above_newmv[bx4 + i] = is_newmv;
            }
        }
        for i in 0..lh {
            if by4 + i < 32 {
                self.left_newmv[by4 + i] = is_newmv;
            }
        }
    }

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
}

impl<'a> TileEncoder<'a> {
    fn new(pixels: &'a FramePixels, dq: DequantValues, base_q_idx: u8) -> Self {
        let mi_cols = 2 * pixels.width.div_ceil(8);
        let mi_rows = 2 * pixels.height.div_ceil(8);
        let cw = pixels.width.div_ceil(2);
        let ch = pixels.height.div_ceil(2);
        Self {
            enc: MsacEncoder::new(),
            cdf: CdfContext::for_qidx(base_q_idx),
            ctx: TileContext::new(mi_cols),
            mi_cols,
            mi_rows,
            pixels,
            dq,
            recon: FramePixels {
                width: pixels.width,
                height: pixels.height,
                y: vec![128u8; (pixels.width * pixels.height) as usize],
                u: vec![128u8; (cw * ch) as usize],
                v: vec![128u8; (cw * ch) as usize],
            },
        }
    }

    fn encode_block(&mut self, bx: u32, by: u32, bl: usize) {
        let px_x = bx * 4;
        let px_y = by * 4;
        let w = self.pixels.width;
        let h = self.pixels.height;
        let cw = w.div_ceil(2);
        let ch = h.div_ceil(2);
        let chroma_px_x = px_x / 2;
        let chroma_px_y = px_y / 2;

        let have_above = by > 0;
        let have_left = bx > 0;

        let above_y: Vec<u8> = (0..16)
            .map(|i| {
                let idx = px_x as usize + i;
                if have_above && idx < self.ctx.above_recon_y.len() {
                    self.ctx.above_recon_y[idx]
                } else if have_above && i < 8 {
                    self.ctx.above_recon_y[(px_x as usize + 7).min(self.ctx.above_recon_y.len() - 1)]
                } else {
                    128
                }
            })
            .collect();

        let left_local_py = ((by & 15) * 4) as usize;
        let left_y: Vec<u8> = (0..16)
            .map(|i| {
                let idx = left_local_py + i;
                if have_left && idx < self.ctx.left_recon_y.len() {
                    self.ctx.left_recon_y[idx]
                } else if have_left && i < 8 {
                    self.ctx.left_recon_y[(left_local_py + 7).min(self.ctx.left_recon_y.len() - 1)]
                } else {
                    128
                }
            })
            .collect();

        let top_left_y = if have_above && have_left {
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

        let y_block = extract_block(&self.pixels.y, w, px_x, px_y, 8, w, h);

        let (y_mode, y_angle_delta) = select_best_intra_mode(
            &y_block, &above_y, &left_y, top_left_y,
            have_above, have_left, 8, 8,
            self.dq.dc, self.dq.ac,
        );
        let y_pred_block = generate_prediction(
            y_mode, y_angle_delta, &above_y, &left_y, top_left_y,
            have_above, have_left, 8, 8,
        );
        let y_txtype = select_best_txtype(&y_block, &y_pred_block, self.dq.dc, self.dq.ac);

        let u_pred = self.ctx.dc_prediction(bx, by, bl, 1);
        let v_pred = self.ctx.dc_prediction(bx, by, bl, 2);

        let u_block = extract_block(&self.pixels.u, cw, chroma_px_x, chroma_px_y, 4, cw, ch);
        let v_block = extract_block(&self.pixels.v, cw, chroma_px_x, chroma_px_y, 4, cw, ch);

        let mut y_residual = [0i32; 64];
        for i in 0..64 {
            y_residual[i] = y_block[i] as i32 - y_pred_block[i] as i32;
        }
        let y_dct = dct::forward_transform_8x8(&y_residual, y_txtype);
        let y_quant = quantize_coeffs(&y_dct, 64, self.dq.dc, self.dq.ac);

        let mut u_residual = [0i32; 16];
        for i in 0..16 {
            u_residual[i] = u_block[i] as i32 - u_pred as i32;
        }
        let u_dct = dct::forward_dct_4x4(&u_residual);
        let u_quant = quantize_coeffs(&u_dct, 16, self.dq.dc, self.dq.ac);

        let mut v_residual = [0i32; 16];
        for i in 0..16 {
            v_residual[i] = v_block[i] as i32 - v_pred as i32;
        }
        let v_dct = dct::forward_dct_4x4(&v_residual);
        let v_quant = quantize_coeffs(&v_dct, 16, self.dq.dc, self.dq.ac);

        let is_skip = y_quant.iter().all(|&c| c == 0)
            && u_quant.iter().all(|&c| c == 0)
            && v_quant.iter().all(|&c| c == 0);

        let skip_ctx = self.ctx.skip_ctx(bx, by);
        self.enc.encode_bool(is_skip, &mut self.cdf.skip[skip_ctx]);

        let (above_mode_ctx, left_mode_ctx) = self.ctx.mode_ctx(bx, by);
        self.enc.encode_symbol(y_mode as u32, &mut self.cdf.kf_y_mode[above_mode_ctx][left_mode_ctx], 12);

        if (1..=8).contains(&y_mode) {
            self.enc.encode_symbol((y_angle_delta + 3) as u32, &mut self.cdf.angle_delta[(y_mode - 1) as usize], 6);
        }

        let cfl_allowed = bl >= 2;
        let uv_n_syms = if cfl_allowed { 13 } else { 12 };
        let cfl_idx = usize::from(cfl_allowed);
        self.enc
            .encode_symbol(0, &mut self.cdf.uv_mode[cfl_idx][y_mode as usize], uv_n_syms);

        let (y_cul, y_dc_neg, y_dc_zero);
        let (u_cul, u_dc_neg, u_dc_zero);
        let (v_cul, v_dc_neg, v_dc_zero);

        if !is_skip {
            let y_txb_skip_ctx = 0;
            let y_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 0);
            let y_result = encode_transform_block(
                &mut self.enc,
                &mut self.cdf,
                &y_quant,
                &DEFAULT_SCAN_8X8,
                false,
                false,
                1,
                y_txb_skip_ctx,
                y_dc_sign_ctx,
                y_mode,
                y_txtype,
            );
            y_cul = y_result.0;
            y_dc_neg = y_result.1;
            y_dc_zero = y_result.2;

            let u_txb_skip_ctx = self.ctx.chroma_txb_skip_ctx(bx, by, bl, 1);
            let u_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 1);
            let u_result = encode_transform_block(
                &mut self.enc,
                &mut self.cdf,
                &u_quant,
                &DEFAULT_SCAN_4X4,
                true,
                false,
                0,
                u_txb_skip_ctx,
                u_dc_sign_ctx,
                y_mode,
                dct::TxType::DctDct,
            );
            u_cul = u_result.0;
            u_dc_neg = u_result.1;
            u_dc_zero = u_result.2;

            let v_txb_skip_ctx = self.ctx.chroma_txb_skip_ctx(bx, by, bl, 2);
            let v_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 2);
            let v_result = encode_transform_block(
                &mut self.enc,
                &mut self.cdf,
                &v_quant,
                &DEFAULT_SCAN_4X4,
                true,
                false,
                0,
                v_txb_skip_ctx,
                v_dc_sign_ctx,
                y_mode,
                dct::TxType::DctDct,
            );
            v_cul = v_result.0;
            v_dc_neg = v_result.1;
            v_dc_zero = v_result.2;
        } else {
            y_cul = 0;
            y_dc_neg = false;
            y_dc_zero = true;
            u_cul = 0;
            u_dc_neg = false;
            u_dc_zero = true;
            v_cul = 0;
            v_dc_neg = false;
            v_dc_zero = true;
        }

        let y_deq = dequantize_coeffs(&y_quant, 64, self.dq.dc, self.dq.ac);
        let mut y_deq_arr = [0i32; 64];
        y_deq_arr.copy_from_slice(&y_deq);
        let y_recon_residual = dct::inverse_transform_8x8(&y_deq_arr, y_txtype);

        for r in 0..8u32 {
            for c in 0..8u32 {
                let dest_x = px_x + c;
                let dest_y = px_y + r;
                if dest_x < w && dest_y < h {
                    let pixel = (y_pred_block[(r * 8 + c) as usize] as i32 + y_recon_residual[(r * 8 + c) as usize]).clamp(0, 255) as u8;
                    self.recon.y[(dest_y * w + dest_x) as usize] = pixel;
                }
            }
        }

        let u_deq = dequantize_coeffs(&u_quant, 16, self.dq.dc, self.dq.ac);
        let mut u_deq_arr = [0i32; 16];
        u_deq_arr.copy_from_slice(&u_deq);
        let u_recon_residual = dct::inverse_dct_4x4(&u_deq_arr);

        for r in 0..4u32 {
            for c in 0..4u32 {
                let dest_x = chroma_px_x + c;
                let dest_y = chroma_px_y + r;
                if dest_x < cw && dest_y < ch {
                    let pixel = (u_pred as i32 + u_recon_residual[(r * 4 + c) as usize]).clamp(0, 255) as u8;
                    self.recon.u[(dest_y * cw + dest_x) as usize] = pixel;
                }
            }
        }

        let v_deq = dequantize_coeffs(&v_quant, 16, self.dq.dc, self.dq.ac);
        let mut v_deq_arr = [0i32; 16];
        v_deq_arr.copy_from_slice(&v_deq);
        let v_recon_residual = dct::inverse_dct_4x4(&v_deq_arr);

        for r in 0..4u32 {
            for c in 0..4u32 {
                let dest_x = chroma_px_x + c;
                let dest_y = chroma_px_y + r;
                if dest_x < cw && dest_y < ch {
                    let pixel = (v_pred as i32 + v_recon_residual[(r * 4 + c) as usize]).clamp(0, 255) as u8;
                    self.recon.v[(dest_y * cw + dest_x) as usize] = pixel;
                }
            }
        }

        let mut y_bottom_row = [128u8; 8];
        let mut y_right_col = [128u8; 8];
        for c in 0..8u32 {
            let dest_x = px_x + c;
            let dest_y = px_y + 7;
            if dest_x < w && dest_y < h {
                y_bottom_row[c as usize] = self.recon.y[(dest_y * w + dest_x) as usize];
            }
        }
        for r in 0..8u32 {
            let dest_x = px_x + 7;
            let dest_y = px_y + r;
            if dest_x < w && dest_y < h {
                y_right_col[r as usize] = self.recon.y[(dest_y * w + dest_x) as usize];
            }
        }

        let mut u_bottom_row = [128u8; 4];
        let mut u_right_col = [128u8; 4];
        for c in 0..4u32 {
            let dest_x = chroma_px_x + c;
            let dest_y = chroma_px_y + 3;
            if dest_x < cw && dest_y < ch {
                u_bottom_row[c as usize] = self.recon.u[(dest_y * cw + dest_x) as usize];
            }
        }
        for r in 0..4u32 {
            let dest_x = chroma_px_x + 3;
            let dest_y = chroma_px_y + r;
            if dest_x < cw && dest_y < ch {
                u_right_col[r as usize] = self.recon.u[(dest_y * cw + dest_x) as usize];
            }
        }

        let mut v_bottom_row = [128u8; 4];
        let mut v_right_col = [128u8; 4];
        for c in 0..4u32 {
            let dest_x = chroma_px_x + c;
            let dest_y = chroma_px_y + 3;
            if dest_x < cw && dest_y < ch {
                v_bottom_row[c as usize] = self.recon.v[(dest_y * cw + dest_x) as usize];
            }
        }
        for r in 0..4u32 {
            let dest_x = chroma_px_x + 3;
            let dest_y = chroma_px_y + r;
            if dest_x < cw && dest_y < ch {
                v_right_col[r as usize] = self.recon.v[(dest_y * cw + dest_x) as usize];
            }
        }

        self.ctx.update_recon(
            bx,
            by,
            self.mi_cols,
            self.mi_rows,
            &y_bottom_row,
            &y_right_col,
            &u_bottom_row,
            &u_right_col,
            &v_bottom_row,
            &v_right_col,
        );
        let y_cf_ctx = coef_ctx_value(y_cul, y_dc_neg, y_dc_zero);
        let u_cf_ctx = coef_ctx_value(u_cul, u_dc_neg, u_dc_zero);
        let v_cf_ctx = coef_ctx_value(v_cul, v_dc_neg, v_dc_zero);
        self.ctx.update_coef_ctx(
            bx,
            by,
            bl,
            self.mi_cols,
            self.mi_rows,
            y_cf_ctx,
            u_cf_ctx,
            v_cf_ctx,
        );
        self.ctx
            .update_partition_ctx(bx, by, bl, self.mi_cols, self.mi_rows);
        self.ctx
            .update_skip_ctx(bx, by, bl, self.mi_cols, self.mi_rows, is_skip);
        self.ctx.update_mode_ctx(bx, by, bl, self.mi_cols, self.mi_rows, y_mode);
    }

    fn skip_mse(&self, bx: u32, by: u32, bl: usize) -> u64 {
        let px_x = bx * 4;
        let px_y = by * 4;
        let block_size = 1u32 << (7 - bl);
        let w = self.pixels.width;
        let h = self.pixels.height;

        let y_pred = self.ctx.dc_prediction(bx, by, bl, 0) as i64;

        let mut sse = 0u64;
        let mut count = 0u64;
        for r in 0..block_size {
            for c in 0..block_size {
                let sy = min(px_y + r, h - 1);
                let sx = min(px_x + c, w - 1);
                let val = self.pixels.y[(sy * w + sx) as usize] as i64;
                let diff = val - y_pred;
                sse += (diff * diff) as u64;
                count += 1;
            }
        }

        sse / count.max(1)
    }

    fn should_use_partition_none(&self, bx: u32, by: u32, bl: usize) -> bool {
        let base = self.dq.ac as u64 * self.dq.ac as u64;
        let divisor = match bl {
            1 => 16,
            2 => 32,
            3 => 48,
            _ => 64,
        };
        self.skip_mse(bx, by, bl) <= base / divisor
    }

    fn encode_skip_block(&mut self, bx: u32, by: u32, bl: usize) {
        let px_x = bx * 4;
        let px_y = by * 4;
        let block_size = 1u32 << (7 - bl);
        let chroma_size = block_size / 2;
        let w = self.pixels.width;
        let h = self.pixels.height;
        let cw = w.div_ceil(2);
        let ch = h.div_ceil(2);
        let cpx = px_x / 2;
        let cpy = px_y / 2;

        let y_pred = self.ctx.dc_prediction(bx, by, bl, 0);
        let u_pred = self.ctx.dc_prediction(bx, by, bl, 1);
        let v_pred = self.ctx.dc_prediction(bx, by, bl, 2);

        let skip_ctx = self.ctx.skip_ctx(bx, by);
        self.enc.encode_bool(true, &mut self.cdf.skip[skip_ctx]);

        let (above_mode_ctx, left_mode_ctx) = self.ctx.mode_ctx(bx, by);
        self.enc.encode_symbol(0, &mut self.cdf.kf_y_mode[above_mode_ctx][left_mode_ctx], 12);

        let cfl_allowed = bl >= 2;
        let uv_n_syms = if cfl_allowed { 13 } else { 12 };
        let cfl_idx = usize::from(cfl_allowed);
        self.enc
            .encode_symbol(0, &mut self.cdf.uv_mode[cfl_idx][0], uv_n_syms);

        for r in 0..block_size {
            for c in 0..block_size {
                let dest_x = px_x + c;
                let dest_y = px_y + r;
                if dest_x < w && dest_y < h {
                    self.recon.y[(dest_y * w + dest_x) as usize] = y_pred;
                }
            }
        }
        for r in 0..chroma_size {
            for c in 0..chroma_size {
                let dest_x = cpx + c;
                let dest_y = cpy + r;
                if dest_x < cw && dest_y < ch {
                    self.recon.u[(dest_y * cw + dest_x) as usize] = u_pred;
                    self.recon.v[(dest_y * cw + dest_x) as usize] = v_pred;
                }
            }
        }

        let y_bp = block_size as usize;
        let c_bp = chroma_size as usize;
        let y_bottom = vec![y_pred; y_bp];
        let y_right = vec![y_pred; y_bp];
        let u_bottom = vec![u_pred; c_bp];
        let u_right = vec![u_pred; c_bp];
        let v_bottom = vec![v_pred; c_bp];
        let v_right = vec![v_pred; c_bp];

        self.ctx.update_recon(
            bx,
            by,
            self.mi_cols,
            self.mi_rows,
            &y_bottom,
            &y_right,
            &u_bottom,
            &u_right,
            &v_bottom,
            &v_right,
        );
        let skip_cf = coef_ctx_value(0, false, true);
        self.ctx.update_coef_ctx(
            bx,
            by,
            bl,
            self.mi_cols,
            self.mi_rows,
            skip_cf,
            skip_cf,
            skip_cf,
        );
        self.ctx
            .update_partition_ctx(bx, by, bl, self.mi_cols, self.mi_rows);
        self.ctx
            .update_skip_ctx(bx, by, bl, self.mi_cols, self.mi_rows, true);
        self.ctx.update_mode_ctx(bx, by, bl, self.mi_cols, self.mi_rows, 0);
    }

    fn encode_partition(&mut self, bl: usize, bx: u32, by: u32) {
        if bl > 4 {
            return;
        }

        let hsz = 16u32 >> bl;
        let have_h_split = self.mi_cols > bx + hsz;
        let have_v_split = self.mi_rows > by + hsz;

        if have_h_split && have_v_split {
            let part_ctx = self.ctx.partition_ctx(bx, by, bl);
            if bl < 4 {
                if self.should_use_partition_none(bx, by, bl) {
                    self.enc.encode_symbol(
                        0,
                        &mut self.cdf.partition[bl][part_ctx],
                        PARTITION_NSYMS[bl],
                    );
                    self.encode_skip_block(bx, by, bl);
                } else {
                    self.enc.encode_symbol(
                        3,
                        &mut self.cdf.partition[bl][part_ctx],
                        PARTITION_NSYMS[bl],
                    );
                    self.encode_partition(bl + 1, bx, by);
                    self.encode_partition(bl + 1, bx + hsz, by);
                    self.encode_partition(bl + 1, bx, by + hsz);
                    self.encode_partition(bl + 1, bx + hsz, by + hsz);
                }
            } else {
                self.enc.encode_symbol(
                    0,
                    &mut self.cdf.partition[bl][part_ctx],
                    PARTITION_NSYMS[bl],
                );
                self.encode_block(bx, by, bl);
            }
        } else if have_h_split {
            let part_ctx = self.ctx.partition_ctx(bx, by, bl);
            let prob = gather_top_partition_prob(&self.cdf.partition[bl][part_ctx], bl);
            self.enc.encode_bool_prob(true, prob);

            self.encode_partition(bl + 1, bx, by);
            self.encode_partition(bl + 1, bx + hsz, by);
        } else if have_v_split {
            let part_ctx = self.ctx.partition_ctx(bx, by, bl);
            let prob = gather_left_partition_prob(&self.cdf.partition[bl][part_ctx], bl);
            self.enc.encode_bool_prob(true, prob);

            self.encode_partition(bl + 1, bx, by);
            self.encode_partition(bl + 1, bx, by + hsz);
        } else {
            self.encode_partition(bl + 1, bx, by);
        }
    }
}

pub fn encode_tile(pixels: &FramePixels) -> Vec<u8> {
    let dq = crate::dequant::lookup_dequant(crate::DEFAULT_BASE_Q_IDX);
    encode_tile_with_recon(pixels, dq, crate::DEFAULT_BASE_Q_IDX).0
}

pub fn encode_tile_with_recon(pixels: &FramePixels, dq: DequantValues, base_q_idx: u8) -> (Vec<u8>, FramePixels) {
    let mut tile = TileEncoder::new(pixels, dq, base_q_idx);

    let sb_cols = tile.mi_cols.div_ceil(16);
    let sb_rows = tile.mi_rows.div_ceil(16);

    for sb_row in 0..sb_rows {
        tile.ctx.reset_left_for_sb_row();
        for sb_col in 0..sb_cols {
            let bx = sb_col * 16;
            let by = sb_row * 16;
            tile.encode_partition(1, bx, by);
        }
    }

    (tile.enc.finalize(), tile.recon)
}

struct InterTileEncoder<'a> {
    enc: MsacEncoder,
    cdf: CdfContext,
    ctx: TileContext,
    mi_cols: u32,
    mi_rows: u32,
    pixels: &'a FramePixels,
    reference: &'a FramePixels,
    dq: DequantValues,
    recon: FramePixels,
    block_mvs: Vec<BlockMv>,
}

impl<'a> InterTileEncoder<'a> {
    fn new(pixels: &'a FramePixels, reference: &'a FramePixels, dq: DequantValues, base_q_idx: u8) -> Self {
        let mi_cols = 2 * pixels.width.div_ceil(8);
        let mi_rows = 2 * pixels.height.div_ceil(8);
        let cw = pixels.width.div_ceil(2);
        let ch = pixels.height.div_ceil(2);
        let mut enc = MsacEncoder::new();
        enc.allow_update_cdf = false;
        Self {
            enc,
            cdf: CdfContext::for_qidx(base_q_idx),
            ctx: TileContext::new(mi_cols),
            mi_cols,
            mi_rows,
            pixels,
            reference,
            dq,
            recon: FramePixels {
                width: pixels.width,
                height: pixels.height,
                y: vec![128u8; (pixels.width * pixels.height) as usize],
                u: vec![128u8; (cw * ch) as usize],
                v: vec![128u8; (cw * ch) as usize],
            },
            block_mvs: vec![BlockMv::default(); (mi_cols * mi_rows) as usize],
        }
    }

    fn encode_inter_block(&mut self, bx: u32, by: u32, bl: usize) {
        let px_x = bx * 4;
        let px_y = by * 4;
        let w = self.pixels.width;
        let h = self.pixels.height;
        let cw = w.div_ceil(2);
        let ch = h.div_ceil(2);
        let chroma_px_x = px_x / 2;
        let chroma_px_y = px_y / 2;

        let y_src = extract_block(&self.pixels.y, w, px_x, px_y, 8, w, h);
        let u_src = extract_block(&self.pixels.u, cw, chroma_px_x, chroma_px_y, 4, cw, ch);
        let v_src = extract_block(&self.pixels.v, cw, chroma_px_x, chroma_px_y, 4, cw, ch);

        let (dx_pixels, dy_pixels) = motion_search_block(
            &self.pixels.y, &self.reference.y, w, h, px_x, px_y, 8,
        );

        let (refined_mv_x, refined_mv_y) = if dx_pixels != 0 || dy_pixels != 0 {
            subpel_refine(
                &self.pixels.y, &self.reference.y, w, h, px_x, px_y, 8,
                dx_pixels * 8, dy_pixels * 8,
            )
        } else {
            (0, 0)
        };

        let (pred_x, pred_y, mv_candidates) = predict_mv(
            &self.block_mvs, self.mi_cols, self.mi_rows, bx, by,
        );

        let zero_y_ref = extract_block(&self.reference.y, w, px_x, px_y, 8, w, h);
        let zero_u_ref = extract_block(&self.reference.u, cw, chroma_px_x, chroma_px_y, 4, cw, ch);
        let zero_v_ref = extract_block(&self.reference.v, cw, chroma_px_x, chroma_px_y, 4, cw, ch);

        let no_inter_neighbors = !self.ctx.has_inter_neighbor(bx, by);

        let use_newmv = if no_inter_neighbors && (refined_mv_x != 0 || refined_mv_y != 0) {
            let y_int_x = px_x as i32 + (refined_mv_x >> 3);
            let y_int_y = px_y as i32 + (refined_mv_y >> 3);
            let y_phase_x = (refined_mv_x & 7) as u32;
            let y_phase_y = (refined_mv_y & 7) as u32;
            let mc_y_ref = interpolate_block(
                &self.reference.y, w, h, y_int_x, y_int_y, y_phase_x, y_phase_y, 8,
            );

            let mut zero_energy = 0i64;
            let mut mc_energy = 0i64;
            for i in 0..64 {
                let zd = y_src[i] as i64 - zero_y_ref[i] as i64;
                let md = y_src[i] as i64 - mc_y_ref[i] as i64;
                zero_energy += zd * zd;
                mc_energy += md * md;
            }

            mc_energy < zero_energy
        } else {
            false
        };

        let (y_ref_block, u_ref_block, v_ref_block, final_mv_x, final_mv_y) = if use_newmv {
            let y_int_x = px_x as i32 + (refined_mv_x >> 3);
            let y_int_y = px_y as i32 + (refined_mv_y >> 3);
            let y_phase_x = (refined_mv_x & 7) as u32;
            let y_phase_y = (refined_mv_y & 7) as u32;

            let chroma_mv_x = refined_mv_x / 2;
            let chroma_mv_y = refined_mv_y / 2;
            let c_int_x = chroma_px_x as i32 + (chroma_mv_x >> 3);
            let c_int_y = chroma_px_y as i32 + (chroma_mv_y >> 3);
            let c_phase_x = (chroma_mv_x & 7) as u32;
            let c_phase_y = (chroma_mv_y & 7) as u32;

            (
                interpolate_block(&self.reference.y, w, h, y_int_x, y_int_y, y_phase_x, y_phase_y, 8),
                interpolate_block(&self.reference.u, cw, ch, c_int_x, c_int_y, c_phase_x, c_phase_y, 4),
                interpolate_block(&self.reference.v, cw, ch, c_int_x, c_int_y, c_phase_x, c_phase_y, 4),
                refined_mv_x,
                refined_mv_y,
            )
        } else {
            (zero_y_ref, zero_u_ref, zero_v_ref, 0, 0)
        };

        let mut y_residual = [0i32; 64];
        for i in 0..64 {
            y_residual[i] = y_src[i] as i32 - y_ref_block[i] as i32;
        }
        let y_dct = dct::forward_dct_8x8(&y_residual);
        let y_quant = quantize_coeffs(&y_dct, 64, self.dq.dc, self.dq.ac);

        let mut u_residual = [0i32; 16];
        for i in 0..16 {
            u_residual[i] = u_src[i] as i32 - u_ref_block[i] as i32;
        }
        let u_dct = dct::forward_dct_4x4(&u_residual);
        let u_quant = quantize_coeffs(&u_dct, 16, self.dq.dc, self.dq.ac);

        let mut v_residual = [0i32; 16];
        for i in 0..16 {
            v_residual[i] = v_src[i] as i32 - v_ref_block[i] as i32;
        }
        let v_dct = dct::forward_dct_4x4(&v_residual);
        let v_quant = quantize_coeffs(&v_dct, 16, self.dq.dc, self.dq.ac);

        let is_skip = y_quant.iter().all(|&c| c == 0)
            && u_quant.iter().all(|&c| c == 0)
            && v_quant.iter().all(|&c| c == 0);

        let skip_ctx = self.ctx.skip_ctx(bx, by);
        self.enc.encode_bool(is_skip, &mut self.cdf.skip[skip_ctx]);

        let is_inter_ctx = self.ctx.is_inter_ctx(bx, by);
        self.enc
            .encode_bool(true, &mut self.cdf.is_inter[is_inter_ctx]);

        let ref_ctx = self.ctx.ref_ctx(bx, by);
        self.enc
            .encode_bool(false, &mut self.cdf.single_ref[ref_ctx][0]);
        self.enc
            .encode_bool(false, &mut self.cdf.single_ref[ref_ctx][2]);
        self.enc
            .encode_bool(false, &mut self.cdf.single_ref[ref_ctx][3]);

        let newmv_ctx = self.ctx.newmv_ctx(bx, by);

        if use_newmv {
            self.enc.encode_bool(false, &mut self.cdf.newmv[newmv_ctx]);

            if mv_candidates.len() > 1 {
                let drl_ctx = get_drl_context(&mv_candidates, 0);
                self.enc.encode_bool(false, &mut self.cdf.drl[drl_ctx]);
            }

            let diff_x = final_mv_x - pred_x;
            let diff_y = final_mv_y - pred_y;
            encode_mv_residual(&mut self.enc, &mut self.cdf.mv, diff_y, diff_x);
        } else {
            self.enc.encode_bool(true, &mut self.cdf.newmv[newmv_ctx]);
            let zeromv_ctx = 0usize;
            self.enc
                .encode_bool(false, &mut self.cdf.zeromv[zeromv_ctx]);
        }

        let (y_cul, y_dc_neg, y_dc_zero);
        let (u_cul, u_dc_neg, u_dc_zero);
        let (v_cul, v_dc_neg, v_dc_zero);

        if !is_skip {
            let y_txb_skip_ctx = 0;
            let y_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 0);
            let y_result = encode_transform_block(
                &mut self.enc,
                &mut self.cdf,
                &y_quant,
                &DEFAULT_SCAN_8X8,
                false,
                true,
                1,
                y_txb_skip_ctx,
                y_dc_sign_ctx,
                0,
                dct::TxType::DctDct,
            );
            y_cul = y_result.0;
            y_dc_neg = y_result.1;
            y_dc_zero = y_result.2;

            let u_txb_skip_ctx = self.ctx.chroma_txb_skip_ctx(bx, by, bl, 1);
            let u_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 1);
            let u_result = encode_transform_block(
                &mut self.enc,
                &mut self.cdf,
                &u_quant,
                &DEFAULT_SCAN_4X4,
                true,
                true,
                0,
                u_txb_skip_ctx,
                u_dc_sign_ctx,
                0,
                dct::TxType::DctDct,
            );
            u_cul = u_result.0;
            u_dc_neg = u_result.1;
            u_dc_zero = u_result.2;

            let v_txb_skip_ctx = self.ctx.chroma_txb_skip_ctx(bx, by, bl, 2);
            let v_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 2);
            let v_result = encode_transform_block(
                &mut self.enc,
                &mut self.cdf,
                &v_quant,
                &DEFAULT_SCAN_4X4,
                true,
                true,
                0,
                v_txb_skip_ctx,
                v_dc_sign_ctx,
                0,
                dct::TxType::DctDct,
            );
            v_cul = v_result.0;
            v_dc_neg = v_result.1;
            v_dc_zero = v_result.2;
        } else {
            y_cul = 0;
            y_dc_neg = false;
            y_dc_zero = true;
            u_cul = 0;
            u_dc_neg = false;
            u_dc_zero = true;
            v_cul = 0;
            v_dc_neg = false;
            v_dc_zero = true;
        }

        let y_deq = dequantize_coeffs(&y_quant, 64, self.dq.dc, self.dq.ac);
        let mut y_deq_arr = [0i32; 64];
        y_deq_arr.copy_from_slice(&y_deq);
        let y_recon_residual = dct::inverse_dct_8x8(&y_deq_arr);

        for r in 0..8u32 {
            for c in 0..8u32 {
                let dest_x = px_x + c;
                let dest_y = px_y + r;
                if dest_x < w && dest_y < h {
                    let pixel = (y_ref_block[(r * 8 + c) as usize] as i32 + y_recon_residual[(r * 8 + c) as usize]).clamp(0, 255) as u8;
                    self.recon.y[(dest_y * w + dest_x) as usize] = pixel;
                }
            }
        }

        let u_deq = dequantize_coeffs(&u_quant, 16, self.dq.dc, self.dq.ac);
        let mut u_deq_arr = [0i32; 16];
        u_deq_arr.copy_from_slice(&u_deq);
        let u_recon_residual = dct::inverse_dct_4x4(&u_deq_arr);

        for r in 0..4u32 {
            for c in 0..4u32 {
                let dest_x = chroma_px_x + c;
                let dest_y = chroma_px_y + r;
                if dest_x < cw && dest_y < ch {
                    let pixel = (u_ref_block[(r * 4 + c) as usize] as i32 + u_recon_residual[(r * 4 + c) as usize]).clamp(0, 255) as u8;
                    self.recon.u[(dest_y * cw + dest_x) as usize] = pixel;
                }
            }
        }

        let v_deq = dequantize_coeffs(&v_quant, 16, self.dq.dc, self.dq.ac);
        let mut v_deq_arr = [0i32; 16];
        v_deq_arr.copy_from_slice(&v_deq);
        let v_recon_residual = dct::inverse_dct_4x4(&v_deq_arr);

        for r in 0..4u32 {
            for c in 0..4u32 {
                let dest_x = chroma_px_x + c;
                let dest_y = chroma_px_y + r;
                if dest_x < cw && dest_y < ch {
                    let pixel = (v_ref_block[(r * 4 + c) as usize] as i32 + v_recon_residual[(r * 4 + c) as usize]).clamp(0, 255) as u8;
                    self.recon.v[(dest_y * cw + dest_x) as usize] = pixel;
                }
            }
        }

        let mut y_bottom_row = [128u8; 8];
        let mut y_right_col = [128u8; 8];
        for c in 0..8u32 {
            let dest_x = px_x + c;
            let dest_y = px_y + 7;
            if dest_x < w && dest_y < h {
                y_bottom_row[c as usize] = self.recon.y[(dest_y * w + dest_x) as usize];
            }
        }
        for r in 0..8u32 {
            let dest_x = px_x + 7;
            let dest_y = px_y + r;
            if dest_x < w && dest_y < h {
                y_right_col[r as usize] = self.recon.y[(dest_y * w + dest_x) as usize];
            }
        }

        let mut u_bottom_row = [128u8; 4];
        let mut u_right_col = [128u8; 4];
        for c in 0..4u32 {
            let dest_x = chroma_px_x + c;
            let dest_y = chroma_px_y + 3;
            if dest_x < cw && dest_y < ch {
                u_bottom_row[c as usize] = self.recon.u[(dest_y * cw + dest_x) as usize];
            }
        }
        for r in 0..4u32 {
            let dest_x = chroma_px_x + 3;
            let dest_y = chroma_px_y + r;
            if dest_x < cw && dest_y < ch {
                u_right_col[r as usize] = self.recon.u[(dest_y * cw + dest_x) as usize];
            }
        }

        let mut v_bottom_row = [128u8; 4];
        let mut v_right_col = [128u8; 4];
        for c in 0..4u32 {
            let dest_x = chroma_px_x + c;
            let dest_y = chroma_px_y + 3;
            if dest_x < cw && dest_y < ch {
                v_bottom_row[c as usize] = self.recon.v[(dest_y * cw + dest_x) as usize];
            }
        }
        for r in 0..4u32 {
            let dest_x = chroma_px_x + 3;
            let dest_y = chroma_px_y + r;
            if dest_x < cw && dest_y < ch {
                v_right_col[r as usize] = self.recon.v[(dest_y * cw + dest_x) as usize];
            }
        }

        let stored_mv = BlockMv {
            mv_x: final_mv_x,
            mv_y: final_mv_y,
            ref_frame: 0,
        };
        for row in by..by.saturating_add(2).min(self.mi_rows) {
            for col in bx..bx.saturating_add(2).min(self.mi_cols) {
                self.block_mvs[(row * self.mi_cols + col) as usize] = stored_mv;
            }
        }

        self.ctx.update_recon(
            bx,
            by,
            self.mi_cols,
            self.mi_rows,
            &y_bottom_row,
            &y_right_col,
            &u_bottom_row,
            &u_right_col,
            &v_bottom_row,
            &v_right_col,
        );
        let y_cf_ctx = coef_ctx_value(y_cul, y_dc_neg, y_dc_zero);
        let u_cf_ctx = coef_ctx_value(u_cul, u_dc_neg, u_dc_zero);
        let v_cf_ctx = coef_ctx_value(v_cul, v_dc_neg, v_dc_zero);
        self.ctx.update_coef_ctx(
            bx,
            by,
            bl,
            self.mi_cols,
            self.mi_rows,
            y_cf_ctx,
            u_cf_ctx,
            v_cf_ctx,
        );
        self.ctx
            .update_partition_ctx(bx, by, bl, self.mi_cols, self.mi_rows);
        self.ctx
            .update_skip_ctx(bx, by, bl, self.mi_cols, self.mi_rows, is_skip);
        self.ctx
            .update_intra_ctx(bx, by, bl, self.mi_cols, self.mi_rows, false);
        self.ctx
            .update_newmv_flag(bx, by, bl, self.mi_cols, self.mi_rows, use_newmv);
    }

    fn inter_skip_mse(&self, bx: u32, by: u32, bl: usize) -> u64 {
        let px_x = bx * 4;
        let px_y = by * 4;
        let block_size = 1u32 << (7 - bl);
        let w = self.pixels.width;
        let h = self.pixels.height;

        let mut sse = 0u64;
        let mut count = 0u64;
        for r in 0..block_size {
            for c in 0..block_size {
                let sy = min(px_y + r, h - 1);
                let sx = min(px_x + c, w - 1);
                let idx = (sy * w + sx) as usize;
                let diff = self.pixels.y[idx] as i64 - self.reference.y[idx] as i64;
                sse += (diff * diff) as u64;
                count += 1;
            }
        }

        sse / count.max(1)
    }

    fn should_use_inter_partition_none(&self, bx: u32, by: u32, bl: usize) -> bool {
        let threshold = (self.dq.ac as u64 * self.dq.ac as u64) / 64;
        self.inter_skip_mse(bx, by, bl) <= threshold
    }

    fn encode_inter_skip_block(&mut self, bx: u32, by: u32, bl: usize) {
        let px_x = bx * 4;
        let px_y = by * 4;
        let block_size = 1u32 << (7 - bl);
        let chroma_size = block_size / 2;
        let w = self.pixels.width;
        let h = self.pixels.height;
        let cw = w.div_ceil(2);
        let ch = h.div_ceil(2);
        let cpx = px_x / 2;
        let cpy = px_y / 2;

        let skip_ctx = self.ctx.skip_ctx(bx, by);
        self.enc.encode_bool(true, &mut self.cdf.skip[skip_ctx]);

        let is_inter_ctx = self.ctx.is_inter_ctx(bx, by);
        self.enc
            .encode_bool(true, &mut self.cdf.is_inter[is_inter_ctx]);

        let ref_ctx = self.ctx.ref_ctx(bx, by);
        self.enc
            .encode_bool(false, &mut self.cdf.single_ref[ref_ctx][0]);
        self.enc
            .encode_bool(false, &mut self.cdf.single_ref[ref_ctx][2]);
        self.enc
            .encode_bool(false, &mut self.cdf.single_ref[ref_ctx][3]);

        let newmv_ctx = self.ctx.newmv_ctx(bx, by);
        self.enc.encode_bool(true, &mut self.cdf.newmv[newmv_ctx]);

        let zeromv_ctx = 0usize;
        self.enc
            .encode_bool(false, &mut self.cdf.zeromv[zeromv_ctx]);

        for r in 0..block_size {
            for c in 0..block_size {
                let dest_x = px_x + c;
                let dest_y = px_y + r;
                if dest_x < w && dest_y < h {
                    let idx = (dest_y * w + dest_x) as usize;
                    self.recon.y[idx] = self.reference.y[idx];
                }
            }
        }
        for r in 0..chroma_size {
            for c in 0..chroma_size {
                let dest_x = cpx + c;
                let dest_y = cpy + r;
                if dest_x < cw && dest_y < ch {
                    let idx = (dest_y * cw + dest_x) as usize;
                    self.recon.u[idx] = self.reference.u[idx];
                    self.recon.v[idx] = self.reference.v[idx];
                }
            }
        }

        let y_bp = block_size as usize;
        let c_bp = chroma_size as usize;
        let mut y_bottom = vec![128u8; y_bp];
        let mut y_right = vec![128u8; y_bp];
        let mut u_bottom = vec![128u8; c_bp];
        let mut u_right = vec![128u8; c_bp];
        let mut v_bottom = vec![128u8; c_bp];
        let mut v_right = vec![128u8; c_bp];

        for i in 0..y_bp {
            let dest_x = px_x + (block_size - 1);
            let dest_y = px_y + i as u32;
            if dest_x < w && dest_y < h {
                y_right[i] = self.recon.y[(dest_y * w + dest_x) as usize];
            }
            let dest_x2 = px_x + i as u32;
            let dest_y2 = px_y + (block_size - 1);
            if dest_x2 < w && dest_y2 < h {
                y_bottom[i] = self.recon.y[(dest_y2 * w + dest_x2) as usize];
            }
        }
        for i in 0..c_bp {
            let dest_x = cpx + (chroma_size - 1);
            let dest_y = cpy + i as u32;
            if dest_x < cw && dest_y < ch {
                u_right[i] = self.recon.u[(dest_y * cw + dest_x) as usize];
                v_right[i] = self.recon.v[(dest_y * cw + dest_x) as usize];
            }
            let dest_x2 = cpx + i as u32;
            let dest_y2 = cpy + (chroma_size - 1);
            if dest_x2 < cw && dest_y2 < ch {
                u_bottom[i] = self.recon.u[(dest_y2 * cw + dest_x2) as usize];
                v_bottom[i] = self.recon.v[(dest_y2 * cw + dest_x2) as usize];
            }
        }

        self.ctx.update_recon(
            bx,
            by,
            self.mi_cols,
            self.mi_rows,
            &y_bottom,
            &y_right,
            &u_bottom,
            &u_right,
            &v_bottom,
            &v_right,
        );
        let skip_cf = coef_ctx_value(0, false, true);
        self.ctx.update_coef_ctx(
            bx,
            by,
            bl,
            self.mi_cols,
            self.mi_rows,
            skip_cf,
            skip_cf,
            skip_cf,
        );
        self.ctx
            .update_partition_ctx(bx, by, bl, self.mi_cols, self.mi_rows);
        self.ctx
            .update_skip_ctx(bx, by, bl, self.mi_cols, self.mi_rows, true);
        self.ctx
            .update_intra_ctx(bx, by, bl, self.mi_cols, self.mi_rows, false);
        self.ctx
            .update_newmv_flag(bx, by, bl, self.mi_cols, self.mi_rows, false);

        let stored_mv = BlockMv {
            mv_x: 0,
            mv_y: 0,
            ref_frame: 0,
        };
        let mi_per_side = 2u32 << (4 - bl);
        for row in by..by.saturating_add(mi_per_side).min(self.mi_rows) {
            for col in bx..bx.saturating_add(mi_per_side).min(self.mi_cols) {
                self.block_mvs[(row * self.mi_cols + col) as usize] = stored_mv;
            }
        }
    }

    fn encode_inter_partition(&mut self, bl: usize, bx: u32, by: u32) {
        if bl > 4 {
            return;
        }

        let hsz = 16u32 >> bl;
        let have_h_split = self.mi_cols > bx + hsz;
        let have_v_split = self.mi_rows > by + hsz;

        if have_h_split && have_v_split {
            let part_ctx = self.ctx.partition_ctx(bx, by, bl);
            if bl < 4 {
                if bl >= 2 && self.should_use_inter_partition_none(bx, by, bl) {
                    self.enc.encode_symbol(
                        0,
                        &mut self.cdf.partition[bl][part_ctx],
                        PARTITION_NSYMS[bl],
                    );
                    self.encode_inter_skip_block(bx, by, bl);
                } else {
                    self.enc.encode_symbol(
                        3,
                        &mut self.cdf.partition[bl][part_ctx],
                        PARTITION_NSYMS[bl],
                    );
                    self.encode_inter_partition(bl + 1, bx, by);
                    self.encode_inter_partition(bl + 1, bx + hsz, by);
                    self.encode_inter_partition(bl + 1, bx, by + hsz);
                    self.encode_inter_partition(bl + 1, bx + hsz, by + hsz);
                }
            } else {
                self.enc.encode_symbol(
                    0,
                    &mut self.cdf.partition[bl][part_ctx],
                    PARTITION_NSYMS[bl],
                );
                self.encode_inter_block(bx, by, bl);
            }
        } else if have_h_split {
            let part_ctx = self.ctx.partition_ctx(bx, by, bl);
            let prob = gather_top_partition_prob(&self.cdf.partition[bl][part_ctx], bl);
            self.enc.encode_bool_prob(true, prob);

            self.encode_inter_partition(bl + 1, bx, by);
            self.encode_inter_partition(bl + 1, bx + hsz, by);
        } else if have_v_split {
            let part_ctx = self.ctx.partition_ctx(bx, by, bl);
            let prob = gather_left_partition_prob(&self.cdf.partition[bl][part_ctx], bl);
            self.enc.encode_bool_prob(true, prob);

            self.encode_inter_partition(bl + 1, bx, by);
            self.encode_inter_partition(bl + 1, bx, by + hsz);
        } else {
            self.encode_inter_partition(bl + 1, bx, by);
        }
    }
}

pub fn encode_inter_tile(pixels: &FramePixels, reference: &FramePixels) -> Vec<u8> {
    let dq = crate::dequant::lookup_dequant(crate::DEFAULT_BASE_Q_IDX);
    encode_inter_tile_with_recon(pixels, reference, dq, crate::DEFAULT_BASE_Q_IDX).0
}

pub fn encode_inter_tile_with_recon(
    pixels: &FramePixels,
    reference: &FramePixels,
    dq: DequantValues,
    base_q_idx: u8,
) -> (Vec<u8>, FramePixels) {
    assert_eq!(pixels.width, reference.width, "reference frame width mismatch");
    assert_eq!(pixels.height, reference.height, "reference frame height mismatch");
    let mut tile = InterTileEncoder::new(pixels, reference, dq, base_q_idx);

    let sb_cols = tile.mi_cols.div_ceil(16);
    let sb_rows = tile.mi_rows.div_ceil(16);

    for sb_row in 0..sb_rows {
        tile.ctx.reset_left_for_sb_row();
        for sb_col in 0..sb_cols {
            let bx = sb_col * 16;
            let by = sb_row * 16;
            tile.encode_inter_partition(1, bx, by);
        }
    }

    let tile_bytes = tile.enc.finalize();
    (tile_bytes, tile.recon)
}

fn decompose_mv_diff(diff: u32) -> (u32, u32, u32) {
    let raw = diff - 1;
    let fp = (raw >> 1) & 3;
    let up = raw >> 3;
    if up < 2 {
        (0, up, fp)
    } else {
        let class = 31 - up.leading_zeros();
        (class, up, fp)
    }
}

fn encode_mv_component(
    enc: &mut MsacEncoder,
    comp_cdf: &mut crate::cdf::MvComponentCdf,
    value: i32,
) {
    let sign = value < 0;
    let abs_val = value.unsigned_abs();
    let (cl, up, fp) = decompose_mv_diff(abs_val);

    enc.encode_bool(sign, &mut comp_cdf.sign);
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

fn encode_mv_residual(
    enc: &mut MsacEncoder,
    mv_cdf: &mut crate::cdf::MvCdf,
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

fn motion_search_block(
    source: &[u8],
    reference: &[u8],
    width: u32,
    height: u32,
    px_x: u32,
    px_y: u32,
    block_size: u32,
) -> (i32, i32) {
    if px_x + block_size > width || px_y + block_size > height {
        return (0, 0);
    }

    let mut best_dx: i32 = 0;
    let mut best_dy: i32 = 0;
    let mut best_sad: u32 = u32::MAX;
    let mut best_cost: i32 = 0;

    for dy in -16i32..=16 {
        for dx in -16i32..=16 {
            let ref_x = px_x as i32 + dx;
            let ref_y = px_y as i32 + dy;

            if ref_x < 0
                || ref_y < 0
                || ref_x + block_size as i32 > width as i32
                || ref_y + block_size as i32 > height as i32
            {
                continue;
            }

            let mut sad: u32 = 0;
            for row in 0..block_size {
                let src_off = ((px_y + row) * width + px_x) as usize;
                let ref_off = ((ref_y as u32 + row) * width + ref_x as u32) as usize;
                for col in 0..block_size as usize {
                    let s = source[src_off + col] as i32;
                    let r = reference[ref_off + col] as i32;
                    sad += (s - r).unsigned_abs();
                }
            }

            let cost = dx.abs() + dy.abs();

            if sad < best_sad || (sad == best_sad && cost < best_cost) {
                best_sad = sad;
                best_dx = dx;
                best_dy = dy;
                best_cost = cost;
            }
        }
    }

    (best_dx, best_dy)
}

#[derive(Clone, Copy)]
struct BlockMv {
    mv_x: i32,
    mv_y: i32,
    ref_frame: i8,
}

impl Default for BlockMv {
    fn default() -> Self {
        Self {
            mv_x: 0,
            mv_y: 0,
            ref_frame: -1,
        }
    }
}

struct MvCandidate {
    mv_x: i32,
    mv_y: i32,
    weight: u32,
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

fn predict_mv(
    block_mvs: &[BlockMv],
    mi_cols: u32,
    mi_rows: u32,
    bx4: u32,
    by4: u32,
) -> (i32, i32, Vec<MvCandidate>) {
    let mut candidates: Vec<MvCandidate> = Vec::new();

    if by4 > 0 {
        for col in bx4..bx4.saturating_add(2).min(mi_cols) {
            let idx = ((by4 - 1) * mi_cols + col) as usize;
            if idx < block_mvs.len() {
                let b = &block_mvs[idx];
                if b.ref_frame == 0 {
                    add_candidate(&mut candidates, b.mv_x, b.mv_y, 2);
                }
            }
        }
    }

    if bx4 > 0 {
        for row in by4..by4.saturating_add(2).min(mi_rows) {
            let idx = (row * mi_cols + bx4 - 1) as usize;
            if idx < block_mvs.len() {
                let b = &block_mvs[idx];
                if b.ref_frame == 0 {
                    add_candidate(&mut candidates, b.mv_x, b.mv_y, 2);
                }
            }
        }
    }

    if by4 > 0 && bx4 + 2 < mi_cols {
        let idx = ((by4 - 1) * mi_cols + bx4 + 2) as usize;
        if idx < block_mvs.len() {
            let b = &block_mvs[idx];
            if b.ref_frame == 0 {
                add_candidate(&mut candidates, b.mv_x, b.mv_y, 2);
            }
        }
    }

    if candidates.is_empty() {
        return (0, 0, candidates);
    }

    for c in &mut candidates {
        c.weight += 640;
    }

    candidates.sort_by(|a, b| b.weight.cmp(&a.weight));

    (candidates[0].mv_x, candidates[0].mv_y, candidates)
}

fn get_drl_context(candidates: &[MvCandidate], ref_idx: usize) -> usize {
    if candidates.len() <= ref_idx + 1 {
        return 2;
    }
    let cur_weight = candidates[ref_idx].weight;
    let next_weight = candidates[ref_idx + 1].weight;
    if cur_weight >= 640 {
        if next_weight < 640 {
            1
        } else {
            0
        }
    } else if next_weight < 640 {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_tile_solid(width: u32, height: u32, y: u8, u: u8, v: u8) -> Vec<u8> {
        let pixels = FramePixels::solid(width, height, y, u, v);
        encode_tile(&pixels)
    }

    #[test]
    fn encode_tile_64x64_produces_bytes() {
        let bytes = encode_tile_solid(64, 64, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_128x128_produces_bytes() {
        let bytes = encode_tile_solid(128, 128, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_100x100_produces_bytes() {
        let bytes = encode_tile_solid(100, 100, 64, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_320x240_produces_bytes() {
        let bytes = encode_tile_solid(320, 240, 0, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_larger_than_64x64_differs() {
        let bytes_64 = encode_tile_solid(64, 64, 128, 128, 128);
        let bytes_128 = encode_tile_solid(128, 128, 128, 128, 128);
        assert_ne!(bytes_64, bytes_128);
    }

    #[test]
    fn encode_tile_different_colors_differ() {
        let bytes_gray = encode_tile_solid(64, 64, 128, 128, 128);
        let bytes_black = encode_tile_solid(64, 64, 0, 0, 0);
        assert_ne!(bytes_gray, bytes_black);
    }

    #[test]
    fn sb_grid_calculations() {
        let w = 100u32;
        let h = 100u32;
        let mi_cols = 2 * w.div_ceil(8);
        let mi_rows = 2 * h.div_ceil(8);
        assert_eq!(mi_cols, 26);
        assert_eq!(mi_rows, 26);

        let sb_cols = mi_cols.div_ceil(16);
        let sb_rows = mi_rows.div_ceil(16);
        assert_eq!(sb_cols, 2);
        assert_eq!(sb_rows, 2);
    }

    #[test]
    fn partition_ctx_initial_is_zero() {
        let ctx = TileContext::new(16);
        assert_eq!(ctx.partition_ctx(0, 0, 1), 0);
    }

    #[test]
    fn partition_ctx_updates_correctly() {
        let mut ctx = TileContext::new(32);
        ctx.update_partition_ctx(0, 0, 2, 32, 32);
        let ctx_at_bl1 = ctx.partition_ctx(0, 0, 1);
        assert_eq!(ctx_at_bl1, 3);
    }

    #[test]
    fn skip_ctx_updates() {
        let mut ctx = TileContext::new(32);
        assert_eq!(ctx.skip_ctx(0, 0), 0);
        ctx.update_skip_ctx(0, 0, 1, 32, 32, true);
        assert!(ctx.skip_ctx(0, 0) > 0);
    }

    #[test]
    fn gather_top_partition_prob_returns_valid() {
        let pc = [
            12631u16, 11221, 9690, 3202, 2931, 2507, 2244, 1876, 1044, 0, 0, 0, 0, 0, 0, 0,
        ];
        let prob = gather_top_partition_prob(&pc, 1);
        assert!(prob > 0);
    }

    #[test]
    fn gather_left_partition_prob_returns_valid() {
        let pc = [
            12631u16, 11221, 9690, 3202, 2931, 2507, 2244, 1876, 1044, 0, 0, 0, 0, 0, 0, 0,
        ];
        let prob = gather_left_partition_prob(&pc, 1);
        assert!(prob > 0);
    }

    #[test]
    fn encode_tile_8x8_produces_bytes() {
        let bytes = encode_tile_solid(8, 8, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_odd_dimensions() {
        let bytes = encode_tile_solid(17, 33, 100, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_wide_frame() {
        let bytes = encode_tile_solid(256, 64, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_tall_frame() {
        let bytes = encode_tile_solid(64, 256, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn dc_prediction_no_neighbors() {
        let ctx = TileContext::new(32);
        assert_eq!(ctx.dc_prediction(0, 0, 1, 0), 128);
    }

    #[test]
    fn dc_prediction_top_only() {
        let mut ctx = TileContext::new(32);
        for i in 0..8 {
            ctx.above_recon_y[i] = 200;
        }
        assert_eq!(ctx.dc_prediction(0, 2, 4, 0), 200);
    }

    #[test]
    fn dc_prediction_both() {
        let mut ctx = TileContext::new(32);
        for i in 0..8 {
            ctx.above_recon_y[8 + i] = 200;
        }
        for i in 0..8 {
            ctx.left_recon_y[8 + i] = 100;
        }
        assert_eq!(ctx.dc_prediction(2, 2, 4, 0), 150);
    }

    #[test]
    fn encode_tile_with_gradient_pixels() {
        let mut pixels = FramePixels::solid(64, 64, 128, 128, 128);
        for row in 0..64u32 {
            for col in 0..64u32 {
                pixels.y[(row * 64 + col) as usize] = ((row * 4) as u8).min(255);
            }
        }
        let bytes = encode_tile(&pixels);
        assert!(!bytes.is_empty());
    }

    fn encode_inter_tile_solid(
        width: u32,
        height: u32,
        y: u8,
        u: u8,
        v: u8,
        ry: u8,
        ru: u8,
        rv: u8,
    ) -> Vec<u8> {
        let pixels = FramePixels::solid(width, height, y, u, v);
        let reference = FramePixels::solid(width, height, ry, ru, rv);
        encode_inter_tile(&pixels, &reference)
    }

    #[test]
    fn inter_tile_64x64_produces_bytes() {
        let bytes = encode_inter_tile_solid(64, 64, 128, 128, 128, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn inter_tile_128x128_produces_bytes() {
        let bytes = encode_inter_tile_solid(128, 128, 128, 128, 128, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn inter_tile_100x100_produces_bytes() {
        let bytes = encode_inter_tile_solid(100, 100, 64, 128, 128, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn inter_tile_320x240_produces_bytes() {
        let bytes = encode_inter_tile_solid(320, 240, 0, 128, 128, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn inter_tile_same_as_reference_is_small() {
        let same = encode_inter_tile_solid(64, 64, 128, 128, 128, 128, 128, 128);
        let diff = encode_inter_tile_solid(64, 64, 0, 0, 0, 255, 255, 255);
        assert!(diff.len() > same.len());
    }

    #[test]
    fn inter_tile_differs_from_intra_tile() {
        let intra = encode_tile_solid(64, 64, 128, 128, 128);
        let inter = encode_inter_tile_solid(64, 64, 128, 128, 128, 128, 128, 128);
        assert_ne!(intra, inter);
    }

    #[test]
    fn inter_tile_different_reference_produces_different_output() {
        let a = encode_inter_tile_solid(64, 64, 128, 128, 128, 0, 0, 0);
        let b = encode_inter_tile_solid(64, 64, 128, 128, 128, 255, 255, 255);
        assert_ne!(a, b);
    }

    #[test]
    fn inter_tile_8x8_produces_bytes() {
        let bytes = encode_inter_tile_solid(8, 8, 128, 128, 128, 100, 100, 100);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn inter_tile_odd_dimensions() {
        let bytes = encode_inter_tile_solid(17, 33, 100, 128, 128, 50, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn inter_tile_wide_frame() {
        let bytes = encode_inter_tile_solid(256, 64, 128, 128, 128, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn inter_tile_tall_frame() {
        let bytes = encode_inter_tile_solid(64, 256, 128, 128, 128, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn is_inter_ctx_no_neighbors() {
        let ctx = TileContext::new(32);
        assert_eq!(ctx.is_inter_ctx(0, 0), 0);
    }

    #[test]
    fn is_inter_ctx_both_intra_neighbors() {
        let mut ctx = TileContext::new(32);
        ctx.above_intra[2] = true;
        ctx.left_intra[2] = true;
        assert_eq!(ctx.is_inter_ctx(2, 2), 3);
    }

    #[test]
    fn is_inter_ctx_both_inter_neighbors() {
        let mut ctx = TileContext::new(32);
        ctx.above_intra[2] = false;
        ctx.left_intra[2] = false;
        assert_eq!(ctx.is_inter_ctx(2, 2), 0);
    }

    #[test]
    fn is_inter_ctx_one_intra_neighbor() {
        let mut ctx = TileContext::new(32);
        ctx.above_intra[2] = true;
        ctx.left_intra[2] = false;
        assert_eq!(ctx.is_inter_ctx(2, 2), 1);
    }

    #[test]
    fn is_inter_ctx_top_only_inter() {
        let ctx = TileContext::new(32);
        assert_eq!(ctx.is_inter_ctx(0, 2), 0);
    }

    #[test]
    fn is_inter_ctx_top_only_intra() {
        let mut ctx = TileContext::new(32);
        ctx.above_intra[0] = true;
        assert_eq!(ctx.is_inter_ctx(0, 2), 2);
    }

    #[test]
    fn newmv_ctx_no_neighbors() {
        let ctx = TileContext::new(32);
        assert_eq!(ctx.newmv_ctx(0, 0), 0);
    }

    #[test]
    fn newmv_ctx_left_only() {
        let mut ctx = TileContext::new(32);
        ctx.above_intra[16] = false;
        ctx.left_intra[0] = false;
        assert_eq!(ctx.newmv_ctx(16, 0), 3);
    }

    #[test]
    fn newmv_ctx_top_only() {
        let ctx = TileContext::new(32);
        assert_eq!(ctx.newmv_ctx(0, 16), 3);
    }

    #[test]
    fn newmv_ctx_both_neighbors() {
        let ctx = TileContext::new(64);
        assert_eq!(ctx.newmv_ctx(16, 16), 5);
    }

    #[test]
    fn newmv_ctx_intra_neighbor_not_counted() {
        let mut ctx = TileContext::new(64);
        ctx.above_intra[16] = true;
        assert_eq!(ctx.newmv_ctx(16, 16), 3);
    }

    #[test]
    fn newmv_ctx_neighbor_used_newmv_one_side() {
        let mut ctx = TileContext::new(64);
        ctx.above_newmv[16] = true;
        assert_eq!(ctx.newmv_ctx(16, 16), 4);
    }

    #[test]
    fn newmv_ctx_neighbor_used_newmv_both_sides() {
        let mut ctx = TileContext::new(64);
        ctx.above_newmv[16] = true;
        ctx.left_newmv[16] = true;
        assert_eq!(ctx.newmv_ctx(16, 16), 4);
    }

    #[test]
    fn newmv_ctx_neighbor_used_newmv_left_only() {
        let mut ctx = TileContext::new(32);
        ctx.above_intra[16] = false;
        ctx.left_intra[0] = false;
        ctx.left_newmv[0] = true;
        assert_eq!(ctx.newmv_ctx(16, 0), 2);
    }

    #[test]
    fn inter_tile_with_gradient() {
        let mut pixels = FramePixels::solid(64, 64, 128, 128, 128);
        for row in 0..64u32 {
            for col in 0..64u32 {
                pixels.y[(row * 64 + col) as usize] = ((row * 4) as u8).min(255);
            }
        }
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes = encode_inter_tile(&pixels, &reference);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn eob_to_bin_values() {
        assert_eq!(eob_to_bin(0), 0);
        assert_eq!(eob_to_bin(1), 1);
        assert_eq!(eob_to_bin(2), 2);
        assert_eq!(eob_to_bin(3), 2);
        assert_eq!(eob_to_bin(4), 3);
        assert_eq!(eob_to_bin(7), 3);
        assert_eq!(eob_to_bin(8), 4);
        assert_eq!(eob_to_bin(15), 4);
        assert_eq!(eob_to_bin(16), 5);
        assert_eq!(eob_to_bin(31), 5);
        assert_eq!(eob_to_bin(32), 6);
        assert_eq!(eob_to_bin(63), 6);
    }

    #[test]
    fn quantize_dequantize_roundtrip() {
        let coeffs = vec![280i32, -176, 88, 0, -352, 176, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let dq = crate::dequant::lookup_dequant(128);
        let quant = quantize_coeffs(&coeffs, 16, dq.dc, dq.ac);
        let deq = dequantize_coeffs(&quant, 16, dq.dc, dq.ac);
        assert_eq!(deq[0], 280);
        assert_eq!(deq[1], -176);
    }

    #[test]
    fn encode_transform_block_all_zero() {
        let mut enc = MsacEncoder::new();
        let mut cdf = CdfContext::default();
        let coeffs = vec![0i32; 16];
        let (cul, dc_neg, dc_zero) = encode_transform_block(
            &mut enc, &mut cdf, &coeffs, &DEFAULT_SCAN_4X4, false, false, 0, 0, 0, 0, dct::TxType::DctDct,
        );
        assert_eq!(cul, 0);
        assert!(!dc_neg);
        assert!(dc_zero);
    }

    #[test]
    fn encode_transform_block_dc_only() {
        let mut enc = MsacEncoder::new();
        let mut cdf = CdfContext::default();
        let mut coeffs = vec![0i32; 64];
        coeffs[0] = 2;
        let (cul, dc_neg, dc_zero) = encode_transform_block(
            &mut enc, &mut cdf, &coeffs, &DEFAULT_SCAN_8X8, false, false, 1, 0, 0, 0, dct::TxType::DctDct,
        );
        assert!(cul > 0);
        assert!(!dc_neg);
        assert!(!dc_zero);
    }

    #[test]
    fn encode_transform_block_with_ac() {
        let mut enc = MsacEncoder::new();
        let mut cdf = CdfContext::default();
        let mut coeffs = vec![0i32; 64];
        coeffs[0] = 5;
        coeffs[1] = -2;
        coeffs[8] = 1;
        let (cul, _dc_neg, dc_zero) = encode_transform_block(
            &mut enc, &mut cdf, &coeffs, &DEFAULT_SCAN_8X8, false, false, 1, 0, 0, 0, dct::TxType::DctDct,
        );
        assert!(cul > 0);
        assert!(!dc_zero);
        let bytes = enc.finalize();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn intra_mode_context_mapping() {
        assert_eq!(INTRA_MODE_CONTEXT[0], 0);
        assert_eq!(INTRA_MODE_CONTEXT[1], 1);
        assert_eq!(INTRA_MODE_CONTEXT[2], 2);
        assert_eq!(INTRA_MODE_CONTEXT[9], 0);
        assert_eq!(INTRA_MODE_CONTEXT[10], 1);
        assert_eq!(INTRA_MODE_CONTEXT[11], 2);
        assert_eq!(INTRA_MODE_CONTEXT[12], 0);
    }

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

    #[test]
    fn v_pred_copies_above_row() {
        let above = [10u8, 20, 30, 40];
        let result = predict_v(&above, 4, 4);
        for r in 0..4 {
            for c in 0..4 {
                assert_eq!(result[r * 4 + c], above[c]);
            }
        }
    }

    #[test]
    fn h_pred_copies_left_column() {
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
        for &p in &result {
            assert_eq!(p, 200);
        }
    }

    #[test]
    fn smooth_pred_corners() {
        let above = [255u8, 255, 255, 255];
        let left = [0u8, 0, 0, 0];
        let result = predict_smooth(&above, &left, 4, 4);
        assert_eq!(result[0], 128);
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

    #[test]
    fn mode_selection_picks_dc_for_solid() {
        let above = [128u8; 8];
        let left = [128u8; 8];
        let block = [128u8; 64];
        let dq = crate::dequant::lookup_dequant(128);
        let (mode, _) = select_best_intra_mode(&block, &above, &left, 128, true, true, 8, 8, dq.dc, dq.ac);
        assert_eq!(mode, 0);
    }

    #[test]
    fn mode_selection_picks_v_for_vertical_pattern() {
        let above = [10u8, 20, 30, 40, 50, 60, 70, 80];
        let left = [128u8; 8];
        let mut block = [0u8; 64];
        for r in 0..8 {
            for c in 0..8 {
                block[r * 8 + c] = above[c];
            }
        }
        let dq = crate::dequant::lookup_dequant(128);
        let (mode, _) = select_best_intra_mode(&block, &above, &left, 128, true, true, 8, 8, dq.dc, dq.ac);
        assert_eq!(mode, 1);
    }

    #[test]
    fn mode_selection_picks_h_for_horizontal_pattern() {
        let above = [128u8; 8];
        let left = [10u8, 20, 30, 40, 50, 60, 70, 80];
        let mut block = [0u8; 64];
        for r in 0..8 {
            for c in 0..8 {
                block[r * 8 + c] = left[r];
            }
        }
        let dq = crate::dequant::lookup_dequant(128);
        let (mode, _) = select_best_intra_mode(&block, &above, &left, 128, true, true, 8, 8, dq.dc, dq.ac);
        assert_eq!(mode, 2);
    }

    #[test]
    fn mode_context_initialized_to_dc() {
        let ctx = TileContext::new(16);
        for &m in &ctx.above_mode {
            assert_eq!(m, 0);
        }
        for &m in &ctx.left_mode {
            assert_eq!(m, 0);
        }
    }

    #[test]
    fn rd_cost_zero_for_perfect_prediction() {
        let source = [128u8; 64];
        let prediction = [128u8; 64];
        let dq = crate::dequant::lookup_dequant(128);
        let cost = compute_rd_cost(&source, &prediction, dq.dc, dq.ac, dct::TxType::DctDct);
        assert_eq!(cost, 0);
    }

    #[test]
    fn rd_cost_higher_for_worse_prediction() {
        let mut source = [0u8; 64];
        for r in 0..8 {
            for c in 0..8 {
                source[r * 8 + c] = (100 + r * 10 + c * 5) as u8;
            }
        }
        let good_pred = source;
        let mut bad_pred = [0u8; 64];
        for i in 0..64 {
            bad_pred[i] = 255 - source[i];
        }
        let dq = crate::dequant::lookup_dequant(128);
        let good_cost = compute_rd_cost(&source, &good_pred, dq.dc, dq.ac, dct::TxType::DctDct);
        let bad_cost = compute_rd_cost(&source, &bad_pred, dq.dc, dq.ac, dct::TxType::DctDct);
        assert!(good_cost < bad_cost);
    }

    #[test]
    fn dr_intra_derivative_key_values() {
        assert_eq!(DR_INTRA_DERIVATIVE[22], 64);
        assert_eq!(DR_INTRA_DERIVATIVE[33], 27);
        assert_eq!(DR_INTRA_DERIVATIVE[11], 151);
    }

    #[test]
    fn z1_d45_produces_diagonal() {
        let above = [10u8, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160];
        let dx = DR_INTRA_DERIVATIVE[22] as i32;
        let result = predict_directional_z1(&above, 8, 8, dx);
        assert_eq!(result.len(), 64);
        assert_eq!(result[0], above[1]);
        for &p in &result {
            assert!((10..=160).contains(&p));
        }
    }

    #[test]
    fn z1_d67_uses_above() {
        let above = [100u8; 16];
        let dx = DR_INTRA_DERIVATIVE[33] as i32;
        let result = predict_directional_z1(&above, 8, 8, dx);
        assert_eq!(result.len(), 64);
        for &p in &result {
            assert_eq!(p, 100);
        }
    }

    #[test]
    fn z3_d203_produces_output() {
        let left = [10u8, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160];
        let dy = DR_INTRA_DERIVATIVE[33] as i32;
        let result = predict_directional_z3(&left, 8, 8, dy);
        assert_eq!(result.len(), 64);
        for &p in &result {
            assert!((10..=160).contains(&p));
        }
    }

    #[test]
    fn z3_uniform_left() {
        let left = [128u8; 16];
        let dy = DR_INTRA_DERIVATIVE[33] as i32;
        let result = predict_directional_z3(&left, 8, 8, dy);
        for &p in &result {
            assert_eq!(p, 128);
        }
    }

    #[test]
    fn z2_d135_uniform_neighbors() {
        let above = [100u8; 16];
        let left = [100u8; 16];
        let dx = DR_INTRA_DERIVATIVE[22] as i32;
        let dy = DR_INTRA_DERIVATIVE[22] as i32;
        let result = predict_directional_z2(&above, &left, 100, 8, 8, dx, dy);
        assert_eq!(result.len(), 64);
        for &p in &result {
            assert_eq!(p, 100);
        }
    }

    #[test]
    fn z2_d113_produces_valid_output() {
        let above = [200u8; 16];
        let left = [50u8; 16];
        let dx = DR_INTRA_DERIVATIVE[33] as i32;
        let dy = DR_INTRA_DERIVATIVE[11] as i32;
        let result = predict_directional_z2(&above, &left, 200, 8, 8, dx, dy);
        assert_eq!(result.len(), 64);
        for &p in &result {
            assert!((50..=200).contains(&p));
        }
    }

    #[test]
    fn z2_d157_produces_valid_output() {
        let above = [200u8; 16];
        let left = [50u8; 16];
        let dx = DR_INTRA_DERIVATIVE[11] as i32;
        let dy = DR_INTRA_DERIVATIVE[33] as i32;
        let result = predict_directional_z2(&above, &left, 200, 8, 8, dx, dy);
        assert_eq!(result.len(), 64);
        for &p in &result {
            assert!((50..=200).contains(&p));
        }
    }

    #[test]
    fn generate_prediction_routes_directional_modes() {
        let above = [100u8; 16];
        let left = [100u8; 16];
        for mode in 3..=8u8 {
            let pred = generate_prediction(mode, 0, &above, &left, 100, true, true, 8, 8);
            assert_eq!(pred.len(), 64);
            for &p in &pred {
                assert_eq!(p, 100);
            }
        }
    }

    #[test]
    fn mode_selection_considers_directional() {
        let mut above = [128u8; 16];
        let left = [128u8; 16];
        for (i, pixel) in above.iter_mut().enumerate() {
            *pixel = (i * 16).min(255) as u8;
        }
        let mut block = [0u8; 64];
        let dx = DR_INTRA_DERIVATIVE[22] as i32;
        let d45_pred = predict_directional_z1(&above, 8, 8, dx);
        block.copy_from_slice(&d45_pred);
        let dq = crate::dequant::lookup_dequant(128);
        let (mode, _) = select_best_intra_mode(&block, &above, &left, 128, true, true, 8, 8, dq.dc, dq.ac);
        assert!((0..=12).contains(&mode));
    }

    #[test]
    fn directional_z1_output_varies() {
        let mut above = [0u8; 16];
        for (i, pixel) in above.iter_mut().enumerate() {
            *pixel = (i * 17).min(255) as u8;
        }
        let dx = DR_INTRA_DERIVATIVE[22] as i32;
        let result = predict_directional_z1(&above, 8, 8, dx);
        assert_eq!(result.len(), 64);
        let unique: std::collections::HashSet<u8> = result.iter().copied().collect();
        assert!(unique.len() > 1);
    }

    #[test]
    fn directional_z3_output_varies() {
        let mut left = [0u8; 16];
        for (i, pixel) in left.iter_mut().enumerate() {
            *pixel = (i * 17).min(255) as u8;
        }
        let dy = DR_INTRA_DERIVATIVE[33] as i32;
        let result = predict_directional_z3(&left, 8, 8, dy);
        assert_eq!(result.len(), 64);
        let unique: std::collections::HashSet<u8> = result.iter().copied().collect();
        assert!(unique.len() > 1);
    }

    #[test]
    fn encode_tile_with_diagonal_pixels() {
        let mut pixels = FramePixels::solid(64, 64, 128, 128, 128);
        for row in 0..64u32 {
            for col in 0..64u32 {
                pixels.y[(row * 64 + col) as usize] = ((row + col) * 2).min(255) as u8;
            }
        }
        let bytes = encode_tile(&pixels);
        assert!(!bytes.is_empty());
    }

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
        for diff in (2u32..=128).step_by(2) {
            let (cl, up, fp) = decompose_mv_diff(diff);
            let _ = cl;
            let reconstructed = ((up << 3) | (fp << 1) | 1) + 1;
            assert_eq!(reconstructed, diff, "diff={diff}");
        }
    }

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

    #[test]
    fn mv_prediction_no_neighbors() {
        let mi_cols = 10u32;
        let mi_rows = 10u32;
        let block_mvs = vec![BlockMv::default(); (mi_cols * mi_rows) as usize];
        let (px, py, cands) = predict_mv(&block_mvs, mi_cols, mi_rows, 0, 0);
        assert_eq!((px, py), (0, 0));
        assert!(cands.is_empty());
    }

    #[test]
    fn mv_prediction_from_left_neighbor() {
        let mi_cols = 10u32;
        let mi_rows = 10u32;
        let mut block_mvs = vec![BlockMv::default(); (mi_cols * mi_rows) as usize];
        for row in 2..4u32 {
            for col in 0..2u32 {
                let idx = (row * mi_cols + col) as usize;
                block_mvs[idx] = BlockMv { mv_x: 16, mv_y: 8, ref_frame: 0 };
            }
        }
        let (px, py, _) = predict_mv(&block_mvs, mi_cols, mi_rows, 2, 2);
        assert_eq!((px, py), (16, 8));
    }

    #[test]
    fn mv_prediction_from_above_neighbor() {
        let mi_cols = 10u32;
        let mi_rows = 10u32;
        let mut block_mvs = vec![BlockMv::default(); (mi_cols * mi_rows) as usize];
        for row in 0..2u32 {
            for col in 2..4u32 {
                let idx = (row * mi_cols + col) as usize;
                block_mvs[idx] = BlockMv { mv_x: 24, mv_y: -16, ref_frame: 0 };
            }
        }
        let (px, py, _) = predict_mv(&block_mvs, mi_cols, mi_rows, 2, 2);
        assert_eq!((px, py), (24, -16));
    }

    #[test]
    fn drl_context_computation() {
        let cands = vec![
            MvCandidate { mv_x: 8, mv_y: 0, weight: 644 },
            MvCandidate { mv_x: 16, mv_y: 0, weight: 642 },
        ];
        assert_eq!(get_drl_context(&cands, 0), 0);

        let cands2 = vec![
            MvCandidate { mv_x: 8, mv_y: 0, weight: 644 },
            MvCandidate { mv_x: 16, mv_y: 0, weight: 4 },
        ];
        assert_eq!(get_drl_context(&cands2, 0), 1);

        let single = vec![
            MvCandidate { mv_x: 8, mv_y: 0, weight: 644 },
        ];
        assert_eq!(get_drl_context(&single, 0), 2);
    }
}
