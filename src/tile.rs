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
) -> (u8, bool, bool) {
    let chroma_idx = if is_chroma { 1 } else { 0 };
    let n = scan_table.len();
    let w = if n == 16 { 4usize } else { 8usize };

    let mut eob: i32 = -1;
    for i in 0..n {
        let rc = scan_table[i] as usize;
        if coeffs[rc] != 0 {
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
            enc.encode_symbol(1, &mut cdf.txtp_intra, 4);
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
        let tok = level.min(3) as u32;
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

        let tok = level.min(3) as u32;
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

    for i in 1..=eob {
        let rc = scan_table[i] as usize;
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

    fn dc_prediction(&self, bx: u32, by: u32, _bl: usize, plane: usize) -> u8 {
        let have_top = by > 0;
        let have_left = bx > 0;

        if !have_top && !have_left {
            return 128;
        }

        let (above_recon, left_recon, px_x, left_local_py, block_pixels) = if plane == 0 {
            (
                &self.above_recon_y[..],
                &self.left_recon_y[..],
                (bx * 4) as usize,
                ((by & 15) * 4) as usize,
                8usize,
            )
        } else {
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
                4usize,
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

        for i in 0..8 {
            if px_x + i < max_px_x && px_x + i < self.above_recon_y.len() {
                self.above_recon_y[px_x + i] = y_bottom_row[i];
            }
        }
        for i in 0..8 {
            if py_abs + i < max_py && py_local + i < self.left_recon_y.len() {
                self.left_recon_y[py_local + i] = y_right_col[i];
            }
        }

        let cpx = (bx * 2) as usize;
        let cpy_local = ((by & 15) * 2) as usize;
        let max_cpx = (mi_cols * 2) as usize;
        let cpy_abs = (by * 2) as usize;
        let max_cpy = (mi_rows * 2) as usize;

        for i in 0..4 {
            if cpx + i < max_cpx && cpx + i < self.above_recon_u.len() {
                self.above_recon_u[cpx + i] = u_bottom_row[i];
                self.above_recon_v[cpx + i] = v_bottom_row[i];
            }
        }
        for i in 0..4 {
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

        let y_pred = self.ctx.dc_prediction(bx, by, bl, 0);
        let u_pred = self.ctx.dc_prediction(bx, by, bl, 1);
        let v_pred = self.ctx.dc_prediction(bx, by, bl, 2);

        let y_block = extract_block(&self.pixels.y, w, px_x, px_y, 8, w, h);
        let u_block = extract_block(&self.pixels.u, cw, chroma_px_x, chroma_px_y, 4, cw, ch);
        let v_block = extract_block(&self.pixels.v, cw, chroma_px_x, chroma_px_y, 4, cw, ch);

        let mut y_residual = [0i32; 64];
        for i in 0..64 {
            y_residual[i] = y_block[i] as i32 - y_pred as i32;
        }
        let y_dct = dct::forward_dct_8x8(&y_residual);
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

        self.enc.encode_symbol(0, &mut self.cdf.kf_y_mode[0][0], 12);

        let cfl_allowed = bl >= 2;
        let uv_n_syms = if cfl_allowed { 13 } else { 12 };
        let cfl_idx = usize::from(cfl_allowed);
        self.enc
            .encode_symbol(0, &mut self.cdf.uv_mode[cfl_idx][0], uv_n_syms);

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
                    let pixel = (y_pred as i32 + y_recon_residual[(r * 8 + c) as usize]).clamp(0, 255) as u8;
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
                self.enc.encode_symbol(
                    3,
                    &mut self.cdf.partition[bl][part_ctx],
                    PARTITION_NSYMS[bl],
                );
                self.encode_partition(bl + 1, bx, by);
                self.encode_partition(bl + 1, bx + hsz, by);
                self.encode_partition(bl + 1, bx, by + hsz);
                self.encode_partition(bl + 1, bx + hsz, by + hsz);
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

        let y_ref_block = extract_block(&self.reference.y, w, px_x, px_y, 8, w, h);
        let u_ref_block = extract_block(&self.reference.u, cw, chroma_px_x, chroma_px_y, 4, cw, ch);
        let v_ref_block = extract_block(&self.reference.v, cw, chroma_px_x, chroma_px_y, 4, cw, ch);

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
        self.enc.encode_bool(true, &mut self.cdf.newmv[newmv_ctx]);

        let zeromv_ctx = 0usize;
        self.enc
            .encode_bool(false, &mut self.cdf.zeromv[zeromv_ctx]);

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
                self.enc.encode_symbol(
                    3,
                    &mut self.cdf.partition[bl][part_ctx],
                    PARTITION_NSYMS[bl],
                );
                self.encode_inter_partition(bl + 1, bx, by);
                self.encode_inter_partition(bl + 1, bx + hsz, by);
                self.encode_inter_partition(bl + 1, bx, by + hsz);
                self.encode_inter_partition(bl + 1, bx + hsz, by + hsz);
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
            &mut enc, &mut cdf, &coeffs, &DEFAULT_SCAN_4X4, false, false, 0, 0, 0,
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
            &mut enc, &mut cdf, &coeffs, &DEFAULT_SCAN_8X8, false, false, 1, 0, 0,
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
            &mut enc, &mut cdf, &coeffs, &DEFAULT_SCAN_8X8, false, false, 1, 0, 0,
        );
        assert!(cul > 0);
        assert!(!dc_zero);
        let bytes = enc.finalize();
        assert!(!bytes.is_empty());
    }
}
