use crate::cdf::CdfContext;
use crate::msac::MsacEncoder;
use crate::y4m::FramePixels;
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

#[allow(clippy::too_many_arguments)]
fn encode_plane_coeffs(
    enc: &mut MsacEncoder,
    cdf: &mut CdfContext,
    dc_tok: u32,
    is_negative: bool,
    is_chroma: bool,
    t_dim_ctx: usize,
    eob_bin: EobBin,
    txb_skip_ctx: usize,
    dc_sign_ctx: usize,
) {
    let chroma_idx = if is_chroma { 1 } else { 0 };

    enc.encode_bool(false, &mut cdf.txb_skip[t_dim_ctx][txb_skip_ctx]);

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

    enc.encode_bool(is_negative, &mut cdf.dc_sign[chroma_idx][dc_sign_ctx]);

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

fn coef_ctx_value(dc_tok: u32, is_negative: bool) -> u8 {
    let cul_level = min(dc_tok, 63) as u8;
    let dc_sign_level: u8 = if dc_tok == 0 {
        0x40
    } else if is_negative {
        0x00
    } else {
        0x80
    };
    cul_level | dc_sign_level
}

fn block_average(
    plane: &[u8],
    stride: u32,
    px_x: u32,
    px_y: u32,
    block_size: u32,
    frame_w: u32,
    frame_h: u32,
) -> u8 {
    let x_end = min(px_x + block_size, frame_w);
    let y_end = min(px_y + block_size, frame_h);
    let actual_w = x_end - px_x;
    let actual_h = y_end - px_y;

    if actual_w == 0 || actual_h == 0 {
        return 128;
    }

    let mut sum = 0u32;
    for row in px_y..y_end {
        let row_start = (row * stride + px_x) as usize;
        for col in 0..actual_w {
            sum += plane[row_start + col as usize] as u32;
        }
    }
    let count = actual_w * actual_h;
    ((sum + count / 2) / count) as u8
}

struct TileEncoder<'a> {
    enc: MsacEncoder,
    cdf: CdfContext,
    ctx: TileContext,
    mi_cols: u32,
    mi_rows: u32,
    pixels: &'a FramePixels,
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
    left_recon_y: [u8; 16],
    left_recon_u: [u8; 8],
    left_recon_v: [u8; 8],
    above_lcoef: Vec<u8>,
    left_lcoef: [u8; 32],
    above_ccoef: [Vec<u8>; 2],
    left_ccoef: [[u8; 16]; 2],
    above_intra: Vec<bool>,
    left_intra: [bool; 32],
}

impl TileContext {
    fn new(mi_cols: u32) -> Self {
        let above_part_size = (mi_cols as usize / 2) + 16;
        let above_skip_size = mi_cols as usize + 32;
        let above_recon_cols = (mi_cols as usize / 2) + 16;
        let above_coef_size = mi_cols as usize + 32;
        let above_ccoef_size = (mi_cols as usize / 2) + 16;
        let above_inter_size = mi_cols as usize + 32;
        Self {
            above_partition: vec![0u8; above_part_size],
            above_skip: vec![0u8; above_skip_size],
            left_partition: [0u8; 16],
            left_skip: [0u8; 32],
            above_recon_y: vec![128u8; above_recon_cols],
            above_recon_u: vec![128u8; above_recon_cols],
            above_recon_v: vec![128u8; above_recon_cols],
            left_recon_y: [128u8; 16],
            left_recon_u: [128u8; 8],
            left_recon_v: [128u8; 8],
            above_lcoef: vec![0x40u8; above_coef_size],
            left_lcoef: [0x40u8; 32],
            above_ccoef: [
                vec![0x40u8; above_ccoef_size],
                vec![0x40u8; above_ccoef_size],
            ],
            left_ccoef: [[0x40u8; 16]; 2],
            above_intra: vec![false; above_inter_size],
            left_intra: [false; 32],
        }
    }

    fn reset_left_for_sb_row(&mut self) {
        self.left_partition = [0u8; 16];
        self.left_skip = [0u8; 32];
        self.left_recon_y = [128u8; 16];
        self.left_recon_u = [128u8; 8];
        self.left_recon_v = [128u8; 8];
        self.left_lcoef = [0x40u8; 32];
        self.left_ccoef = [[0x40u8; 16]; 2];
        self.left_intra = [false; 32];
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
        y_tok: u32,
        y_neg: bool,
        u_tok: u32,
        u_neg: bool,
        v_tok: u32,
        v_neg: bool,
    ) {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let bw4 = 2 * (16usize >> bl);

        let aw = min(bw4, (mi_cols - bx) as usize);
        let lh = min(bw4, (mi_rows - by) as usize);

        let y_cf_ctx = coef_ctx_value(y_tok, y_neg);
        for i in 0..aw {
            if bx4 + i < self.above_lcoef.len() {
                self.above_lcoef[bx4 + i] = y_cf_ctx;
            }
        }
        for i in 0..lh {
            if by4 + i < self.left_lcoef.len() {
                self.left_lcoef[by4 + i] = y_cf_ctx;
            }
        }

        let cbx4 = (bx / 2) as usize;
        let cby4 = ((by & 31) / 2) as usize;
        let cw4 = (16usize >> bl).max(1);

        let caw = min(cw4, (mi_cols - bx).div_ceil(2) as usize);
        let clh = min(cw4, (mi_rows - by).div_ceil(2) as usize);

        let u_cf_ctx = coef_ctx_value(u_tok, u_neg);
        let v_cf_ctx = coef_ctx_value(v_tok, v_neg);

        for i in 0..caw {
            if cbx4 + i < self.above_ccoef[0].len() {
                self.above_ccoef[0][cbx4 + i] = u_cf_ctx;
                self.above_ccoef[1][cbx4 + i] = v_cf_ctx;
            }
        }
        for i in 0..clh {
            if cby4 + i < self.left_ccoef[0].len() {
                self.left_ccoef[0][cby4 + i] = u_cf_ctx;
                self.left_ccoef[1][cby4 + i] = v_cf_ctx;
            }
        }
    }

    fn dc_prediction(&self, bx: u32, by: u32, bl: usize, plane: usize) -> u8 {
        let have_top = by > 0;
        let have_left = bx > 0;

        if !have_top && !have_left {
            return 128;
        }

        let (above_recon, left_recon) = match plane {
            0 => (&self.above_recon_y[..], &self.left_recon_y[..]),
            1 => (&self.above_recon_u[..], &self.left_recon_u[..]),
            _ => (&self.above_recon_v[..], &self.left_recon_v[..]),
        };

        let (bx8, by8_local, n_entries) = if plane == 0 {
            let bx8 = (bx >> 1) as usize;
            let by8 = ((by & 31) >> 1) as usize;
            let n = 16usize >> bl;
            (bx8, by8, n)
        } else {
            let bx8 = (bx >> 2) as usize;
            let by8 = ((by & 31) >> 2) as usize;
            let n = (16usize >> bl).max(1);
            (bx8, by8, n)
        };

        if have_top && have_left {
            let above_val = above_recon[min(bx8, above_recon.len() - 1)];
            let left_val = left_recon[min(by8_local, left_recon.len() - 1)];
            ((above_val as u16 + left_val as u16 + 1) >> 1) as u8
        } else if have_top {
            let mut sum = 0u32;
            let mut count = 0u32;
            for i in 0..n_entries {
                let idx = bx8 + i;
                if idx < above_recon.len() {
                    sum += above_recon[idx] as u32;
                    count += 1;
                }
            }
            if count > 0 {
                ((sum + count / 2) / count) as u8
            } else {
                128
            }
        } else {
            let mut sum = 0u32;
            let mut count = 0u32;
            for i in 0..n_entries {
                let idx = by8_local + i;
                if idx < left_recon.len() {
                    sum += left_recon[idx] as u32;
                    count += 1;
                }
            }
            if count > 0 {
                ((sum + count / 2) / count) as u8
            } else {
                128
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn update_recon(
        &mut self,
        bx: u32,
        by: u32,
        bl: usize,
        mi_cols: u32,
        mi_rows: u32,
        recon_y: u8,
        recon_u: u8,
        recon_v: u8,
    ) {
        let bx8 = (bx >> 1) as usize;
        let by8 = ((by & 31) >> 1) as usize;
        let n_luma = 16usize >> bl;
        let aw = min(n_luma, (mi_cols - bx).div_ceil(2) as usize);
        let lh = min(n_luma, (mi_rows - by).div_ceil(2) as usize);

        for i in 0..aw {
            if bx8 + i < self.above_recon_y.len() {
                self.above_recon_y[bx8 + i] = recon_y;
            }
        }
        for i in 0..lh {
            if by8 + i < self.left_recon_y.len() {
                self.left_recon_y[by8 + i] = recon_y;
            }
        }

        let bx_c = (bx >> 2) as usize;
        let by_c = ((by & 31) >> 2) as usize;
        let n_chroma = (16usize >> bl).max(1);
        let caw = min(n_chroma, (mi_cols - bx).div_ceil(4) as usize);
        let clh = min(n_chroma, (mi_rows - by).div_ceil(4) as usize);

        for i in 0..caw {
            if bx_c + i < self.above_recon_u.len() {
                self.above_recon_u[bx_c + i] = recon_u;
                self.above_recon_v[bx_c + i] = recon_v;
            }
        }
        for i in 0..clh {
            if by_c + i < self.left_recon_u.len() {
                self.left_recon_u[by_c + i] = recon_u;
                self.left_recon_v[by_c + i] = recon_v;
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

    fn newmv_ctx(&self, bx: u32, by: u32) -> usize {
        let bx4 = bx as usize;
        let by4 = (by & 31) as usize;
        let have_top = by > 0;
        let have_left = bx > 0;

        let above_inter = have_top
            && bx4 < self.above_intra.len()
            && !self.above_intra[bx4];
        let left_inter = have_left && !self.left_intra[by4.min(31)];

        let nearest_match = above_inter as u32 + left_inter as u32;
        match nearest_match {
            0 => 0,
            1 => 3,
            2 => 5,
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
}

impl<'a> TileEncoder<'a> {
    fn new(pixels: &'a FramePixels) -> Self {
        let mi_cols = 2 * pixels.width.div_ceil(8);
        let mi_rows = 2 * pixels.height.div_ceil(8);
        let cw = pixels.width.div_ceil(2);
        let ch = pixels.height.div_ceil(2);
        Self {
            enc: MsacEncoder::new(),
            cdf: CdfContext::default(),
            ctx: TileContext::new(mi_cols),
            mi_cols,
            mi_rows,
            pixels,
            recon: FramePixels {
                width: pixels.width,
                height: pixels.height,
                y: vec![128u8; (pixels.width * pixels.height) as usize],
                u: vec![128u8; (cw * ch) as usize],
                v: vec![128u8; (cw * ch) as usize],
            },
        }
    }

    fn fill_recon(&mut self, bx: u32, by: u32, bl: usize, recon_y: u8, recon_u: u8, recon_v: u8) {
        let luma_px = 128u32 >> bl;
        let chroma_px = luma_px / 2;
        let px_x = bx * 4;
        let px_y = by * 4;
        let w = self.recon.width;
        let h = self.recon.height;
        let cw = w.div_ceil(2);
        let ch = h.div_ceil(2);

        let x_end = min(px_x + luma_px, w);
        let y_end = min(px_y + luma_px, h);
        for row in px_y..y_end {
            let start = (row * w + px_x) as usize;
            let end = (row * w + x_end) as usize;
            self.recon.y[start..end].fill(recon_y);
        }

        let cpx_x = px_x / 2;
        let cpx_y = px_y / 2;
        let cx_end = min(cpx_x + chroma_px, cw);
        let cy_end = min(cpx_y + chroma_px, ch);
        for row in cpx_y..cy_end {
            let start = (row * cw + cpx_x) as usize;
            let end = (row * cw + cx_end) as usize;
            self.recon.u[start..end].fill(recon_u);
            self.recon.v[start..end].fill(recon_v);
        }
    }

    fn encode_block(&mut self, bx: u32, by: u32, bl: usize) {
        let params = &BLOCK_PARAMS[bl];
        let luma_px = 128u32 >> bl;
        let chroma_px = luma_px / 2;
        let px_x = bx * 4;
        let px_y = by * 4;
        let chroma_px_x = px_x / 2;
        let chroma_px_y = px_y / 2;
        let chroma_w = self.pixels.width.div_ceil(2);
        let chroma_h = self.pixels.height.div_ceil(2);

        let y_avg = block_average(
            &self.pixels.y,
            self.pixels.width,
            px_x,
            px_y,
            luma_px,
            self.pixels.width,
            self.pixels.height,
        );
        let u_avg = block_average(
            &self.pixels.u,
            chroma_w,
            chroma_px_x,
            chroma_px_y,
            chroma_px,
            chroma_w,
            chroma_h,
        );
        let v_avg = block_average(
            &self.pixels.v,
            chroma_w,
            chroma_px_x,
            chroma_px_y,
            chroma_px,
            chroma_w,
            chroma_h,
        );

        let y_pred = self.ctx.dc_prediction(bx, by, bl, 0);
        let u_pred = self.ctx.dc_prediction(bx, by, bl, 1);
        let v_pred = self.ctx.dc_prediction(bx, by, bl, 2);

        let (y_tok, y_neg) =
            compute_dc_tok(y_avg, y_pred, params.luma_dq_shift, params.luma_itx_shift);
        let (u_tok, u_neg) = compute_dc_tok(
            u_avg,
            u_pred,
            params.chroma_dq_shift,
            params.chroma_itx_shift,
        );
        let (v_tok, v_neg) = compute_dc_tok(
            v_avg,
            v_pred,
            params.chroma_dq_shift,
            params.chroma_itx_shift,
        );
        let is_skip = y_tok == 0 && u_tok == 0 && v_tok == 0;

        let skip_ctx = self.ctx.skip_ctx(bx, by);
        self.enc.encode_bool(is_skip, &mut self.cdf.skip[skip_ctx]);

        self.enc.encode_symbol(0, &mut self.cdf.kf_y_mode[0][0], 12);

        let cfl_allowed = bl >= 2;
        let uv_n_syms = if cfl_allowed { 13 } else { 12 };
        let cfl_idx = usize::from(cfl_allowed);
        self.enc
            .encode_symbol(0, &mut self.cdf.uv_mode[cfl_idx][0], uv_n_syms);

        if !is_skip {
            let l_eob = luma_eob_bin(bl);
            let c_eob = chroma_eob_bin(bl);

            let y_txb_skip_ctx = 0;
            let y_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 0);

            if y_tok > 0 {
                encode_plane_coeffs(
                    &mut self.enc,
                    &mut self.cdf,
                    y_tok,
                    y_neg,
                    false,
                    params.luma_t_dim_ctx,
                    l_eob,
                    y_txb_skip_ctx,
                    y_dc_sign_ctx,
                );
            } else {
                self.enc.encode_bool(
                    true,
                    &mut self.cdf.txb_skip[params.luma_t_dim_ctx][y_txb_skip_ctx],
                );
            }

            let u_txb_skip_ctx = self.ctx.chroma_txb_skip_ctx(bx, by, bl, 1);
            let u_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 1);

            if u_tok > 0 {
                encode_plane_coeffs(
                    &mut self.enc,
                    &mut self.cdf,
                    u_tok,
                    u_neg,
                    true,
                    params.chroma_t_dim_ctx,
                    c_eob,
                    u_txb_skip_ctx,
                    u_dc_sign_ctx,
                );
            } else {
                self.enc.encode_bool(
                    true,
                    &mut self.cdf.txb_skip[params.chroma_t_dim_ctx][u_txb_skip_ctx],
                );
            }

            let v_txb_skip_ctx = self.ctx.chroma_txb_skip_ctx(bx, by, bl, 2);
            let v_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 2);

            if v_tok > 0 {
                encode_plane_coeffs(
                    &mut self.enc,
                    &mut self.cdf,
                    v_tok,
                    v_neg,
                    true,
                    params.chroma_t_dim_ctx,
                    c_eob,
                    v_txb_skip_ctx,
                    v_dc_sign_ctx,
                );
            } else {
                self.enc.encode_bool(
                    true,
                    &mut self.cdf.txb_skip[params.chroma_t_dim_ctx][v_txb_skip_ctx],
                );
            }
        }

        let recon_y = compute_reconstructed_dc(
            y_pred,
            y_tok,
            y_neg,
            params.luma_dq_shift,
            params.luma_itx_shift,
        );
        let recon_u = compute_reconstructed_dc(
            u_pred,
            u_tok,
            u_neg,
            params.chroma_dq_shift,
            params.chroma_itx_shift,
        );
        let recon_v = compute_reconstructed_dc(
            v_pred,
            v_tok,
            v_neg,
            params.chroma_dq_shift,
            params.chroma_itx_shift,
        );

        self.ctx.update_recon(
            bx,
            by,
            bl,
            self.mi_cols,
            self.mi_rows,
            recon_y,
            recon_u,
            recon_v,
        );
        self.fill_recon(bx, by, bl, recon_y, recon_u, recon_v);
        self.ctx.update_coef_ctx(
            bx,
            by,
            bl,
            self.mi_cols,
            self.mi_rows,
            y_tok,
            y_neg,
            u_tok,
            u_neg,
            v_tok,
            v_neg,
        );
        self.ctx
            .update_partition_ctx(bx, by, bl, self.mi_cols, self.mi_rows);
        self.ctx
            .update_skip_ctx(bx, by, bl, self.mi_cols, self.mi_rows, is_skip);
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

pub fn encode_tile(pixels: &FramePixels) -> Vec<u8> {
    encode_tile_with_recon(pixels).0
}

pub fn encode_tile_with_recon(pixels: &FramePixels) -> (Vec<u8>, FramePixels) {
    let mut tile = TileEncoder::new(pixels);

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
    recon: FramePixels,
}

impl<'a> InterTileEncoder<'a> {
    fn new(pixels: &'a FramePixels, reference: &'a FramePixels) -> Self {
        let mi_cols = 2 * pixels.width.div_ceil(8);
        let mi_rows = 2 * pixels.height.div_ceil(8);
        let cw = pixels.width.div_ceil(2);
        let ch = pixels.height.div_ceil(2);
        let mut enc = MsacEncoder::new();
        enc.allow_update_cdf = false;
        Self {
            enc,
            cdf: CdfContext::default(),
            ctx: TileContext::new(mi_cols),
            mi_cols,
            mi_rows,
            pixels,
            reference,
            recon: FramePixels {
                width: pixels.width,
                height: pixels.height,
                y: vec![128u8; (pixels.width * pixels.height) as usize],
                u: vec![128u8; (cw * ch) as usize],
                v: vec![128u8; (cw * ch) as usize],
            },
        }
    }

    fn fill_recon(&mut self, bx: u32, by: u32, bl: usize, recon_y: u8, recon_u: u8, recon_v: u8) {
        let luma_px = 128u32 >> bl;
        let chroma_px = luma_px / 2;
        let px_x = bx * 4;
        let px_y = by * 4;
        let w = self.recon.width;
        let h = self.recon.height;
        let cw = w.div_ceil(2);
        let ch = h.div_ceil(2);

        let x_end = min(px_x + luma_px, w);
        let y_end = min(px_y + luma_px, h);
        for row in px_y..y_end {
            let start = (row * w + px_x) as usize;
            let end = (row * w + x_end) as usize;
            self.recon.y[start..end].fill(recon_y);
        }

        let cpx_x = px_x / 2;
        let cpx_y = px_y / 2;
        let cx_end = min(cpx_x + chroma_px, cw);
        let cy_end = min(cpx_y + chroma_px, ch);
        for row in cpx_y..cy_end {
            let start = (row * cw + cpx_x) as usize;
            let end = (row * cw + cx_end) as usize;
            self.recon.u[start..end].fill(recon_u);
            self.recon.v[start..end].fill(recon_v);
        }
    }

    fn encode_inter_block(&mut self, bx: u32, by: u32, bl: usize) {
        let params = &BLOCK_PARAMS[bl];
        let luma_px = 128u32 >> bl;
        let chroma_px = luma_px / 2;
        let px_x = bx * 4;
        let px_y = by * 4;
        let chroma_px_x = px_x / 2;
        let chroma_px_y = px_y / 2;
        let chroma_w = self.pixels.width.div_ceil(2);
        let chroma_h = self.pixels.height.div_ceil(2);

        let y_avg = block_average(
            &self.pixels.y,
            self.pixels.width,
            px_x,
            px_y,
            luma_px,
            self.pixels.width,
            self.pixels.height,
        );
        let u_avg = block_average(
            &self.pixels.u,
            chroma_w,
            chroma_px_x,
            chroma_px_y,
            chroma_px,
            chroma_w,
            chroma_h,
        );
        let v_avg = block_average(
            &self.pixels.v,
            chroma_w,
            chroma_px_x,
            chroma_px_y,
            chroma_px,
            chroma_w,
            chroma_h,
        );

        let y_ref = block_average(
            &self.reference.y,
            self.reference.width,
            px_x,
            px_y,
            luma_px,
            self.reference.width,
            self.reference.height,
        );
        let u_ref = block_average(
            &self.reference.u,
            chroma_w,
            chroma_px_x,
            chroma_px_y,
            chroma_px,
            chroma_w,
            chroma_h,
        );
        let v_ref = block_average(
            &self.reference.v,
            chroma_w,
            chroma_px_x,
            chroma_px_y,
            chroma_px,
            chroma_w,
            chroma_h,
        );

        let (y_tok, y_neg) =
            compute_dc_tok(y_avg, y_ref, params.luma_dq_shift, params.luma_itx_shift);
        let (u_tok, u_neg) = compute_dc_tok(
            u_avg,
            u_ref,
            params.chroma_dq_shift,
            params.chroma_itx_shift,
        );
        let (v_tok, v_neg) = compute_dc_tok(
            v_avg,
            v_ref,
            params.chroma_dq_shift,
            params.chroma_itx_shift,
        );
        let is_skip = y_tok == 0 && u_tok == 0 && v_tok == 0;

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
        self.enc.encode_bool(true, &mut self.cdf.newmv[newmv_ctx]);

        let zeromv_ctx = 0usize;
        self.enc
            .encode_bool(false, &mut self.cdf.zeromv[zeromv_ctx]);

        if !is_skip {
            let l_eob = luma_eob_bin(bl);
            let c_eob = chroma_eob_bin(bl);

            let y_txb_skip_ctx = 0;
            let y_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 0);

            if y_tok > 0 {
                encode_plane_coeffs(
                    &mut self.enc,
                    &mut self.cdf,
                    y_tok,
                    y_neg,
                    false,
                    params.luma_t_dim_ctx,
                    l_eob,
                    y_txb_skip_ctx,
                    y_dc_sign_ctx,
                );
            } else {
                self.enc.encode_bool(
                    true,
                    &mut self.cdf.txb_skip[params.luma_t_dim_ctx][y_txb_skip_ctx],
                );
            }

            let u_txb_skip_ctx = self.ctx.chroma_txb_skip_ctx(bx, by, bl, 1);
            let u_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 1);

            if u_tok > 0 {
                encode_plane_coeffs(
                    &mut self.enc,
                    &mut self.cdf,
                    u_tok,
                    u_neg,
                    true,
                    params.chroma_t_dim_ctx,
                    c_eob,
                    u_txb_skip_ctx,
                    u_dc_sign_ctx,
                );
            } else {
                self.enc.encode_bool(
                    true,
                    &mut self.cdf.txb_skip[params.chroma_t_dim_ctx][u_txb_skip_ctx],
                );
            }

            let v_txb_skip_ctx = self.ctx.chroma_txb_skip_ctx(bx, by, bl, 2);
            let v_dc_sign_ctx = self.ctx.dc_sign_ctx(bx, by, bl, 2);

            if v_tok > 0 {
                encode_plane_coeffs(
                    &mut self.enc,
                    &mut self.cdf,
                    v_tok,
                    v_neg,
                    true,
                    params.chroma_t_dim_ctx,
                    c_eob,
                    v_txb_skip_ctx,
                    v_dc_sign_ctx,
                );
            } else {
                self.enc.encode_bool(
                    true,
                    &mut self.cdf.txb_skip[params.chroma_t_dim_ctx][v_txb_skip_ctx],
                );
            }
        }

        let recon_y = compute_reconstructed_dc(
            y_ref,
            y_tok,
            y_neg,
            params.luma_dq_shift,
            params.luma_itx_shift,
        );
        let recon_u = compute_reconstructed_dc(
            u_ref,
            u_tok,
            u_neg,
            params.chroma_dq_shift,
            params.chroma_itx_shift,
        );
        let recon_v = compute_reconstructed_dc(
            v_ref,
            v_tok,
            v_neg,
            params.chroma_dq_shift,
            params.chroma_itx_shift,
        );

        self.ctx.update_recon(
            bx,
            by,
            bl,
            self.mi_cols,
            self.mi_rows,
            recon_y,
            recon_u,
            recon_v,
        );
        self.fill_recon(bx, by, bl, recon_y, recon_u, recon_v);
        self.ctx.update_coef_ctx(
            bx,
            by,
            bl,
            self.mi_cols,
            self.mi_rows,
            y_tok,
            y_neg,
            u_tok,
            u_neg,
            v_tok,
            v_neg,
        );
        self.ctx
            .update_partition_ctx(bx, by, bl, self.mi_cols, self.mi_rows);
        self.ctx
            .update_skip_ctx(bx, by, bl, self.mi_cols, self.mi_rows, is_skip);
        self.ctx
            .update_intra_ctx(bx, by, bl, self.mi_cols, self.mi_rows, false);
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
            self.enc.encode_symbol(
                0,
                &mut self.cdf.partition[bl][part_ctx],
                PARTITION_NSYMS[bl],
            );
            self.encode_inter_block(bx, by, bl);
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
    encode_inter_tile_with_recon(pixels, reference).0
}

pub fn encode_inter_tile_with_recon(
    pixels: &FramePixels,
    reference: &FramePixels,
) -> (Vec<u8>, FramePixels) {
    assert_eq!(pixels.width, reference.width, "reference frame width mismatch");
    assert_eq!(pixels.height, reference.height, "reference frame height mismatch");
    let mut tile = InterTileEncoder::new(pixels, reference);

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

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_tile_solid(width: u32, height: u32, y: u8, u: u8, v: u8) -> Vec<u8> {
        let pixels = FramePixels::solid(width, height, y, u, v);
        encode_tile(&pixels)
    }

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

    #[test]
    fn block_average_full_block() {
        let plane = vec![100u8; 64 * 64];
        let avg = block_average(&plane, 64, 0, 0, 64, 64, 64);
        assert_eq!(avg, 100);
    }

    #[test]
    fn block_average_edge_clamp() {
        let plane = vec![200u8; 10 * 10];
        let avg = block_average(&plane, 10, 8, 8, 8, 10, 10);
        assert_eq!(avg, 200);
    }

    #[test]
    fn block_average_gradient() {
        let mut plane = vec![0u8; 8 * 8];
        for i in 0..64 {
            plane[i] = i as u8;
        }
        let avg = block_average(&plane, 8, 0, 0, 8, 8, 8);
        assert_eq!(avg, 32);
    }

    #[test]
    fn dc_prediction_no_neighbors() {
        let ctx = TileContext::new(32);
        assert_eq!(ctx.dc_prediction(0, 0, 1, 0), 128);
    }

    #[test]
    fn dc_prediction_top_only() {
        let mut ctx = TileContext::new(32);
        for i in 0..4 {
            ctx.above_recon_y[i] = 200;
        }
        assert_eq!(ctx.dc_prediction(0, 2, 2, 0), 200);
    }

    #[test]
    fn dc_prediction_both() {
        let mut ctx = TileContext::new(32);
        ctx.above_recon_y[1] = 200;
        ctx.left_recon_y[1] = 100;
        assert_eq!(ctx.dc_prediction(2, 2, 2, 0), 150);
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
}
