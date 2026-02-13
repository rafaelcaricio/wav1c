use crate::cdf::CdfContext;
use crate::msac::MsacEncoder;
use std::cmp::min;

const DQ_DC_Q128: u32 = 140;

fn decoder_dc_residual(cf0: i32, itx_shift: u32) -> i32 {
    let rnd = (1i32 << itx_shift) >> 1;
    let dc = (cf0 * 181 + 128) >> 8;
    let dc = (dc + rnd) >> itx_shift;
    (dc * 181 + 128 + 2048) >> 12
}

fn compute_dc_tok(target_pixel: u8, dq_shift: u32, itx_shift: u32) -> (u32, bool) {
    let residual = target_pixel as i32 - 128;
    if residual == 0 {
        return (0, false);
    }
    let is_negative = residual < 0;
    let abs_residual = residual.unsigned_abs() as i32;

    let mut best_tok: u32 = 0;
    let mut best_error = abs_residual;

    for tok in 1u32..=1024 {
        let cf0 = ((DQ_DC_Q128 * tok) >> dq_shift) as i32;
        let pixel_residual = decoder_dc_residual(cf0, itx_shift);
        let error = (pixel_residual - abs_residual).abs();
        if error < best_error {
            best_error = error;
            best_tok = tok;
        }
        if pixel_residual >= abs_residual {
            break;
        }
    }

    (best_tok, is_negative)
}

fn encode_hi_tok(enc: &mut MsacEncoder, cdf: &mut [u16], dc_tok: u32) {
    let tok = dc_tok;
    let mut base = 3;
    for _ in 0..4 {
        let sym = min(tok - base, 3);
        enc.encode_symbol(sym, cdf, 3);
        if sym < 3 {
            return;
        }
        base += 3;
    }
}

fn encode_plane_coeffs(
    enc: &mut MsacEncoder,
    cdf: &mut CdfContext,
    dc_tok: u32,
    is_negative: bool,
    is_chroma: bool,
    t_dim_ctx: usize,
) {
    let chroma_idx = if is_chroma { 1 } else { 0 };
    let sctx = if is_chroma { 7 } else { 0 };

    enc.encode_bool(false, &mut cdf.txb_skip[t_dim_ctx][sctx]);

    enc.encode_symbol(0, &mut cdf.eob_bin_1024[chroma_idx], 10);

    let tok_br = match dc_tok {
        1 => 0u32,
        2 => 1,
        _ => 2,
    };
    enc.encode_symbol(tok_br, &mut cdf.eob_base_tok[t_dim_ctx][chroma_idx][0], 2);

    if dc_tok >= 3 {
        let br_ctx = min(t_dim_ctx, 3);
        encode_hi_tok(enc, &mut cdf.br_tok[br_ctx][chroma_idx][0], dc_tok);
    }

    enc.encode_bool(is_negative, &mut cdf.dc_sign[chroma_idx][0]);

    if dc_tok >= 15 {
        enc.encode_golomb(dc_tok - 15);
    }
}

pub fn encode_tile(y: u8, u: u8, v: u8, base_q_idx: u8) -> Vec<u8> {
    let mut cdf = CdfContext::new(base_q_idx);
    let mut enc = MsacEncoder::new();

    let (y_tok, y_neg) = compute_dc_tok(y, 2, 2);
    let (u_tok, u_neg) = compute_dc_tok(u, 1, 2);
    let (v_tok, v_neg) = compute_dc_tok(v, 1, 2);
    let is_skip = y_tok == 0 && u_tok == 0 && v_tok == 0;

    enc.encode_symbol(0, &mut cdf.partition[1][0], 9);

    enc.encode_bool(is_skip, &mut cdf.skip[0]);

    enc.encode_symbol(0, &mut cdf.kf_y_mode[0][0], 12);

    enc.encode_symbol(0, &mut cdf.uv_mode[0][0], 12);

    if !is_skip {
        if y_tok > 0 {
            encode_plane_coeffs(&mut enc, &mut cdf, y_tok, y_neg, false, 4);
        } else {
            enc.encode_bool(true, &mut cdf.txb_skip[4][0]);
        }

        if u_tok > 0 {
            encode_plane_coeffs(&mut enc, &mut cdf, u_tok, u_neg, true, 3);
        } else {
            enc.encode_bool(true, &mut cdf.txb_skip[3][7]);
        }

        if v_tok > 0 {
            encode_plane_coeffs(&mut enc, &mut cdf, v_tok, v_neg, true, 3);
        } else {
            enc.encode_bool(true, &mut cdf.txb_skip[3][7]);
        }
    }

    enc.finalize()
}
