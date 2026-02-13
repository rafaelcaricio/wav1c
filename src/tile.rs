use crate::cdf::CdfContext;
use crate::msac::MsacEncoder;
use std::cmp::min;

const DQ_DC_Q128: u32 = 140;

struct BlockParams {
    luma_t_dim_ctx: usize,
    luma_dq_shift: u32,
    luma_itx_shift: u32,
    chroma_t_dim_ctx: usize,
    chroma_dq_shift: u32,
    chroma_itx_shift: u32,
}

const BLOCK_PARAMS: [BlockParams; 5] = [
    BlockParams {
        luma_t_dim_ctx: 0,
        luma_dq_shift: 0,
        luma_itx_shift: 0,
        chroma_t_dim_ctx: 0,
        chroma_dq_shift: 0,
        chroma_itx_shift: 0,
    },
    BlockParams {
        luma_t_dim_ctx: 4,
        luma_dq_shift: 2,
        luma_itx_shift: 2,
        chroma_t_dim_ctx: 3,
        chroma_dq_shift: 1,
        chroma_itx_shift: 2,
    },
    BlockParams {
        luma_t_dim_ctx: 3,
        luma_dq_shift: 1,
        luma_itx_shift: 2,
        chroma_t_dim_ctx: 2,
        chroma_dq_shift: 0,
        chroma_itx_shift: 2,
    },
    BlockParams {
        luma_t_dim_ctx: 2,
        luma_dq_shift: 0,
        luma_itx_shift: 2,
        chroma_t_dim_ctx: 1,
        chroma_dq_shift: 0,
        chroma_itx_shift: 1,
    },
    BlockParams {
        luma_t_dim_ctx: 1,
        luma_dq_shift: 0,
        luma_itx_shift: 1,
        chroma_t_dim_ctx: 0,
        chroma_dq_shift: 0,
        chroma_itx_shift: 0,
    },
];

const PARTITION_CTX_NONE: [u8; 5] = [0, 0x10, 0x18, 0x1c, 0x1e];

const PARTITION_NSYMS: [u32; 5] = [9, 9, 9, 9, 3];

fn decoder_dc_residual(cf0: i32, itx_shift: u32) -> i32 {
    let rnd = (1i32 << itx_shift) >> 1;
    let dc = (cf0 * 181 + 128) >> 8;
    let dc = (dc + rnd) >> itx_shift;
    (dc * 181 + 128 + 2048) >> 12
}

fn compute_dc_tok(target_pixel: u8, dc_pred: u8, dq_shift: u32, itx_shift: u32) -> (u32, bool) {
    let residual = target_pixel as i32 - dc_pred as i32;
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

fn compute_reconstructed_dc(
    dc_pred: u8,
    dc_tok: u32,
    is_negative: bool,
    dq_shift: u32,
    itx_shift: u32,
) -> u8 {
    if dc_tok == 0 {
        return dc_pred;
    }
    let cf0 = ((DQ_DC_Q128 * dc_tok) >> dq_shift) as i32;
    let pixel_residual = decoder_dc_residual(cf0, itx_shift);
    let signed_residual = if is_negative {
        -pixel_residual
    } else {
        pixel_residual
    };
    (dc_pred as i32 + signed_residual).clamp(0, 255) as u8
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

#[derive(Clone, Copy)]
enum EobBin {
    Bin16,
    Bin64,
    Bin256,
    Bin1024,
}

fn encode_plane_coeffs(
    enc: &mut MsacEncoder,
    cdf: &mut CdfContext,
    dc_tok: u32,
    is_negative: bool,
    is_chroma: bool,
    t_dim_ctx: usize,
    eob_bin: EobBin,
) {
    let chroma_idx = if is_chroma { 1 } else { 0 };
    let sctx = if is_chroma { 7 } else { 0 };

    enc.encode_bool(false, &mut cdf.txb_skip[t_dim_ctx][sctx]);

    match eob_bin {
        EobBin::Bin16 => enc.encode_symbol(0, &mut cdf.eob_bin_16[chroma_idx][0], 4),
        EobBin::Bin64 => enc.encode_symbol(0, &mut cdf.eob_bin_64[chroma_idx][0], 6),
        EobBin::Bin256 => enc.encode_symbol(0, &mut cdf.eob_bin_256[chroma_idx][0], 8),
        EobBin::Bin1024 => enc.encode_symbol(0, &mut cdf.eob_bin_1024[chroma_idx], 10),
    }

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

fn luma_eob_bin(bl: usize) -> EobBin {
    match bl {
        3 => EobBin::Bin256,
        4 => EobBin::Bin64,
        _ => EobBin::Bin1024,
    }
}

fn chroma_eob_bin(bl: usize) -> EobBin {
    match bl {
        2 => EobBin::Bin256,
        3 => EobBin::Bin64,
        4 => EobBin::Bin16,
        _ => EobBin::Bin1024,
    }
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

#[derive(Clone, Copy)]
struct PixelTargets {
    y: u8,
    u: u8,
    v: u8,
}

struct TileEncoder {
    enc: MsacEncoder,
    cdf: CdfContext,
    ctx: TileContext,
    mi_cols: u32,
    mi_rows: u32,
    targets: PixelTargets,
}

struct TileContext {
    above_partition: Vec<u8>,
    above_skip: Vec<u8>,
    left_partition: [u8; 16],
    left_skip: [u8; 32],
    recon_y: u8,
    recon_u: u8,
    recon_v: u8,
    first_block: bool,
}

impl TileContext {
    fn new(mi_cols: u32) -> Self {
        let above_part_size = (mi_cols as usize / 2) + 16;
        let above_skip_size = mi_cols as usize + 32;
        Self {
            above_partition: vec![0u8; above_part_size],
            above_skip: vec![0u8; above_skip_size],
            left_partition: [0u8; 16],
            left_skip: [0u8; 32],
            recon_y: 128,
            recon_u: 128,
            recon_v: 128,
            first_block: true,
        }
    }

    fn reset_left_for_sb_row(&mut self) {
        self.left_partition = [0u8; 16];
        self.left_skip = [0u8; 32];
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

    fn update_partition_ctx(&mut self, bx: u32, by: u32, bl: usize) {
        let bx8 = (bx >> 1) as usize;
        let by8 = ((by & 31) >> 1) as usize;
        let hsz = 16usize >> bl;
        let above_val = PARTITION_CTX_NONE[bl];
        let left_val = PARTITION_CTX_NONE[bl];
        for i in 0..hsz {
            if bx8 + i < self.above_partition.len() {
                self.above_partition[bx8 + i] = above_val;
            }
        }
        for i in 0..hsz {
            if by8 + i < 16 {
                self.left_partition[by8 + i] = left_val;
            }
        }
    }

    fn update_skip_ctx(&mut self, bx: u32, by: u32, bl: usize, is_skip: bool) {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let bw4 = 2 * (16usize >> bl);
        let bh4 = bw4;
        let val = u8::from(is_skip);
        for i in 0..bw4 {
            if bx4 + i < self.above_skip.len() {
                self.above_skip[bx4 + i] = val;
            }
        }
        for i in 0..bh4 {
            if by4 + i < 32 {
                self.left_skip[by4 + i] = val;
            }
        }
    }
}

impl TileEncoder {
    fn new(width: u32, height: u32, targets: PixelTargets) -> Self {
        let mi_cols = 2 * width.div_ceil(8);
        let mi_rows = 2 * height.div_ceil(8);
        Self {
            enc: MsacEncoder::new(),
            cdf: CdfContext::default(),
            ctx: TileContext::new(mi_cols),
            mi_cols,
            mi_rows,
            targets,
        }
    }

    fn encode_block(&mut self, bx: u32, by: u32, bl: usize) {
        let params = &BLOCK_PARAMS[bl];
        let t = self.targets;

        let y_pred = if self.ctx.first_block { 128 } else { self.ctx.recon_y };
        let u_pred = if self.ctx.first_block { 128 } else { self.ctx.recon_u };
        let v_pred = if self.ctx.first_block { 128 } else { self.ctx.recon_v };

        let (y_tok, y_neg) =
            compute_dc_tok(t.y, y_pred, params.luma_dq_shift, params.luma_itx_shift);
        let (u_tok, u_neg) =
            compute_dc_tok(t.u, u_pred, params.chroma_dq_shift, params.chroma_itx_shift);
        let (v_tok, v_neg) =
            compute_dc_tok(t.v, v_pred, params.chroma_dq_shift, params.chroma_itx_shift);
        let is_skip = y_tok == 0 && u_tok == 0 && v_tok == 0;

        let skip_ctx = self.ctx.skip_ctx(bx, by);
        self.enc.encode_bool(is_skip, &mut self.cdf.skip[skip_ctx]);

        self.enc
            .encode_symbol(0, &mut self.cdf.kf_y_mode[0][0], 12);
        let cfl_allowed = bl >= 2;
        let uv_n_syms = if cfl_allowed { 13 } else { 12 };
        let cfl_idx = usize::from(cfl_allowed);
        self.enc
            .encode_symbol(0, &mut self.cdf.uv_mode[cfl_idx][0], uv_n_syms);

        if !is_skip {
            let l_eob = luma_eob_bin(bl);
            let c_eob = chroma_eob_bin(bl);

            if y_tok > 0 {
                encode_plane_coeffs(
                    &mut self.enc,
                    &mut self.cdf,
                    y_tok,
                    y_neg,
                    false,
                    params.luma_t_dim_ctx,
                    l_eob,
                );
            } else {
                self.enc
                    .encode_bool(true, &mut self.cdf.txb_skip[params.luma_t_dim_ctx][0]);
            }

            if u_tok > 0 {
                encode_plane_coeffs(
                    &mut self.enc,
                    &mut self.cdf,
                    u_tok,
                    u_neg,
                    true,
                    params.chroma_t_dim_ctx,
                    c_eob,
                );
            } else {
                self.enc
                    .encode_bool(true, &mut self.cdf.txb_skip[params.chroma_t_dim_ctx][7]);
            }

            if v_tok > 0 {
                encode_plane_coeffs(
                    &mut self.enc,
                    &mut self.cdf,
                    v_tok,
                    v_neg,
                    true,
                    params.chroma_t_dim_ctx,
                    c_eob,
                );
            } else {
                self.enc
                    .encode_bool(true, &mut self.cdf.txb_skip[params.chroma_t_dim_ctx][7]);
            }
        }

        self.ctx.recon_y = compute_reconstructed_dc(
            y_pred,
            y_tok,
            y_neg,
            params.luma_dq_shift,
            params.luma_itx_shift,
        );
        self.ctx.recon_u = compute_reconstructed_dc(
            u_pred,
            u_tok,
            u_neg,
            params.chroma_dq_shift,
            params.chroma_itx_shift,
        );
        self.ctx.recon_v = compute_reconstructed_dc(
            v_pred,
            v_tok,
            v_neg,
            params.chroma_dq_shift,
            params.chroma_itx_shift,
        );
        self.ctx.first_block = false;

        self.ctx.update_partition_ctx(bx, by, bl);
        self.ctx.update_skip_ctx(bx, by, bl, is_skip);
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
            self.enc.encode_symbol(
                0,
                &mut self.cdf.partition[bl][part_ctx],
                PARTITION_NSYMS[bl],
            );
            self.encode_block(bx, by, bl);
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

pub fn encode_tile(width: u32, height: u32, y: u8, u: u8, v: u8) -> Vec<u8> {
    let targets = PixelTargets { y, u, v };
    let mut tile = TileEncoder::new(width, height, targets);

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

    tile.enc.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_tok_with_dc_pred_128_matches_legacy() {
        let (tok, neg) = compute_dc_tok(0, 128, 2, 2);
        assert!(tok > 0);
        assert!(neg);

        let (tok, neg) = compute_dc_tok(128, 128, 2, 2);
        assert_eq!(tok, 0);
        assert!(!neg);
    }

    #[test]
    fn reconstructed_dc_roundtrips() {
        for target in [0u8, 64, 128, 192, 255] {
            let (tok, neg) = compute_dc_tok(target, 128, 2, 2);
            let recon = compute_reconstructed_dc(128, tok, neg, 2, 2);
            assert!(
                (recon as i32 - target as i32).abs() <= 1,
                "target={target} recon={recon}"
            );
        }
    }

    #[test]
    fn reconstructed_dc_with_nondefault_pred() {
        let (tok, neg) = compute_dc_tok(200, 190, 1, 2);
        let recon = compute_reconstructed_dc(190, tok, neg, 1, 2);
        assert!((recon as i32 - 200).abs() <= 1, "recon={recon}");
    }

    #[test]
    fn encode_tile_64x64_produces_bytes() {
        let bytes = encode_tile(64, 64, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_128x128_produces_bytes() {
        let bytes = encode_tile(128, 128, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_100x100_produces_bytes() {
        let bytes = encode_tile(100, 100, 64, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_320x240_produces_bytes() {
        let bytes = encode_tile(320, 240, 0, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_larger_than_64x64_differs() {
        let bytes_64 = encode_tile(64, 64, 128, 128, 128);
        let bytes_128 = encode_tile(128, 128, 128, 128, 128);
        assert_ne!(bytes_64, bytes_128);
    }

    #[test]
    fn encode_tile_different_colors_differ() {
        let bytes_gray = encode_tile(64, 64, 128, 128, 128);
        let bytes_black = encode_tile(64, 64, 0, 0, 0);
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
        ctx.update_partition_ctx(0, 0, 2);
        let ctx_at_bl1 = ctx.partition_ctx(0, 0, 1);
        assert_eq!(ctx_at_bl1, 3);
    }

    #[test]
    fn skip_ctx_updates() {
        let mut ctx = TileContext::new(32);
        assert_eq!(ctx.skip_ctx(0, 0), 0);
        ctx.update_skip_ctx(0, 0, 1, true);
        assert!(ctx.skip_ctx(0, 0) > 0);
    }

    #[test]
    fn gather_top_partition_prob_returns_valid() {
        let pc = [12631u16, 11221, 9690, 3202, 2931, 2507, 2244, 1876, 1044, 0, 0, 0, 0, 0, 0, 0];
        let prob = gather_top_partition_prob(&pc, 1);
        assert!(prob > 0);
    }

    #[test]
    fn gather_left_partition_prob_returns_valid() {
        let pc = [12631u16, 11221, 9690, 3202, 2931, 2507, 2244, 1876, 1044, 0, 0, 0, 0, 0, 0, 0];
        let prob = gather_left_partition_prob(&pc, 1);
        assert!(prob > 0);
    }

    #[test]
    fn encode_tile_8x8_produces_bytes() {
        let bytes = encode_tile(8, 8, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_odd_dimensions() {
        let bytes = encode_tile(17, 33, 100, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_wide_frame() {
        let bytes = encode_tile(256, 64, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_tile_tall_frame() {
        let bytes = encode_tile(64, 256, 128, 128, 128);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn reconstructed_dc_all_block_levels() {
        for &(dq_shift, itx_shift) in &[(2, 2), (1, 2), (0, 2), (0, 1), (0, 0)] {
            let (tok, neg) = compute_dc_tok(200, 128, dq_shift, itx_shift);
            let recon = compute_reconstructed_dc(128, tok, neg, dq_shift, itx_shift);
            assert!(
                (recon as i32 - 200).abs() <= 2,
                "dq_shift={dq_shift} itx_shift={itx_shift} recon={recon}"
            );
        }
    }
}
