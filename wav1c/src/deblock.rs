use crate::y4m::FramePixels;

/// Loop filter parameters for a frame.
#[derive(Debug, Clone, Copy)]
pub struct LoopFilterParams {
    pub y_vert_level: u8,
    pub y_horiz_level: u8,
    pub uv_vert_level: u8,
    pub uv_horiz_level: u8,
    pub sharpness: u8,
    pub delta_enabled: bool,
    pub delta_update: bool,
}

/// Derive loop filter params from the quantization index.
pub fn loop_filter_params_for_qidx(base_q_idx: u8) -> LoopFilterParams {
    let y_level = qp_to_filter_level(base_q_idx);
    LoopFilterParams {
        y_vert_level: y_level,
        y_horiz_level: y_level,
        uv_vert_level: y_level,
        uv_horiz_level: y_level,
        sharpness: 0,
        delta_enabled: true,
        delta_update: false,
    }
}

/// Maps quantization index (0–255) to a loop filter level (0–63).
fn qp_to_filter_level(q: u8) -> u8 {
    if q <= 4 {
        return 0;
    }
    let qf = q as f64;
    let level = (40.0 * (qf / 4.0).ln() / (64.0_f64).ln()).round() as i32;
    level.clamp(0, 63) as u8
}

// ---------------------------------------------------------------------------
// E/I/H computation — exact port of dav1d `dav1d_calc_eih`.
// ---------------------------------------------------------------------------

fn calc_e(level: u8, sharpness: u8) -> i32 {
    let limit = calc_limit(level as i32, sharpness);
    2 * (level as i32 + 2) + limit
}

fn calc_i(level: u8, sharpness: u8) -> i32 {
    calc_limit(level as i32, sharpness)
}

fn calc_limit(level: i32, sharpness: u8) -> i32 {
    let sharp = sharpness as i32;
    let mut limit = level;
    if sharp > 0 {
        limit >>= (sharp + 3) >> 2;
        limit = limit.min(9 - sharp);
    }
    limit.max(1)
}

// ---------------------------------------------------------------------------
// Core loop filter — exact port of dav1d `loop_filter` (src/loopfilter_tmpl.c)
// ---------------------------------------------------------------------------

/// Apply the loop filter to one edge pixel row/column.
///
/// * `plane` – pixel buffer
/// * `stride` – row stride (image width for the plane)
/// * `x`, `y` – pixel position of the Q-side edge start
/// * `horiz` – true for horizontal edges (stride along x), false for vertical
/// * `e`, `i`, `h` – filter thresholds (pre-bit-depth-scaled, see calc_*)
/// * `wd` – filter width: 4 (narrow), 6 (flat-6), or 8 (flat-8)
/// * `bd_max` – max pixel value for the bit depth
/// * `bd_min_8` – bit_depth - 8 (0 for 8-bit, 2 for 10-bit)
#[allow(clippy::too_many_arguments)]
fn loop_filter_edge(
    plane: &mut [u16],
    stride: usize,
    x: usize,
    y: usize,
    horiz: bool,
    e: i32,
    i: i32,
    h: i32,
    wd: u8,
    bd_max: u16,
    bd_min_8: i32,
) {
    let e = e << bd_min_8;
    let i = i << bd_min_8;
    let h_val = h << bd_min_8;
    let f_val: i32 = 1 << bd_min_8;
    let clip_max = (128i32 << bd_min_8) - 1;

    let plane_h = plane.len() / stride;
    let radius = if wd > 6 { 4 } else if wd > 4 { 3 } else { 2 };

    // We process 4 rows/columns per call (dav1d does the same).
    for k in 0..4usize {
        // Pixel coordinates of the Q-side edge pixel.
        let (px, py) = if horiz {
            // Horizontal edge: iterate along x, strideb = stride (y direction)
            (x + k, y)
        } else {
            // Vertical edge: iterate along y, strideb = 1 (x direction)
            (x, y + k)
        };

        if px >= stride || py >= plane_h {
            continue;
        }
        if horiz {
            if py < radius || py + radius > plane_h {
                continue;
            }
        } else if px < radius || px + radius > stride {
            continue;
        }

        let p1 = if horiz {
            plane[(py - 2) * stride + px] as i32
        } else {
            plane[py * stride + (px - 2)] as i32
        };

        let read = |dy: isize, dx: isize| -> i32 {
            let ry = (py as isize + dy) as usize;
            let rx = (px as isize + dx) as usize;
            plane[ry * stride + rx] as i32
        };

        let p0 = if horiz { read(-1, 0) } else { read(0, -1) };
        let q0 = read(0, 0);
        let q1 = if horiz { read(1, 0) } else { read(0, 1) };

        // Filter mask (fm)
        let mut fm = (p1 - p0).abs() <= i
            && (q1 - q0).abs() <= i
            && (p0 - q0).abs() * 2 + ((p1 - q1).abs() >> 1) <= e;

        let (mut p2, mut p3, mut q2, mut q3) = (0i32, 0i32, 0i32, 0i32);

        if wd > 4 {
            p2 = if horiz { read(-3, 0) } else { read(0, -3) };
            q2 = if horiz { read(2, 0) } else { read(0, 2) };
            fm &= (p2 - p1).abs() <= i && (q2 - q1).abs() <= i;
        }

        if wd > 6 {
            p3 = if horiz { read(-4, 0) } else { read(0, -4) };
            q3 = if horiz { read(3, 0) } else { read(0, 3) };
            fm &= (p3 - p2).abs() <= i && (q3 - q2).abs() <= i;
        }

        if !fm {
            continue;
        }

        // Flat-8 inner check
        let flat8in = if wd >= 6 {
            let mut flat = (p2 - p0).abs() <= f_val
                && (p1 - p0).abs() <= f_val
                && (q1 - q0).abs() <= f_val
                && (q2 - q0).abs() <= f_val;
            if wd >= 8 {
                flat &= (p3 - p0).abs() <= f_val && (q3 - q0).abs() <= f_val;
            }
            flat
        } else {
            false
        };

        let mut write = |off: isize, val: i32| {
            let (wy, wx) = if horiz {
                ((py as isize + off) as usize, px)
            } else {
                (py, (px as isize + off) as usize)
            };
            plane[wy * stride + wx] = val.clamp(0, bd_max as i32) as u16;
        };

        if wd >= 8 && flat8in {
            // Flat 8-tap filter
            write(-3, (p3 * 3 + p2 * 2 + p1 + p0 + q0 + 4) >> 3);
            write(-2, (p3 * 2 + p2 * 2 + p1 * 2 + p0 + q0 + q1 + 4) >> 3);
            write(-1, (p3 + p2 + p1 + p0 * 2 + q0 + q1 + q2 + 4) >> 3);
            write(0, (p2 + p1 + p0 + q0 * 2 + q1 + q2 + q3 + 4) >> 3);
            write(1, (p1 + p0 + q0 + q1 * 2 + q2 + q3 * 2 + 4) >> 3);
            write(2, (p0 + q0 + q1 + q2 * 2 + q3 * 3 + 4) >> 3);
        } else if wd == 6 && flat8in {
            // Flat 6-tap filter
            write(-2, (p2 * 3 + p1 * 2 + p0 * 2 + q0 + 4) >> 3);
            write(-1, (p2 + p1 * 2 + p0 * 2 + q0 * 2 + q1 + 4) >> 3);
            write(0, (p1 + p0 * 2 + q0 * 2 + q1 * 2 + q2 + 4) >> 3);
            write(1, (p0 + q0 + q1 + q2 * 2 + q2 + 4) >> 3);
        } else {
            // Narrow (4-tap) filter
            let hev = (p1 - p0).abs() > h_val || (q1 - q0).abs() > h_val;

            if hev {
                let f = clip3(-clip_max, clip_max, p1 - q1);
                let f = clip3(-clip_max, clip_max, 3 * (q0 - p0) + f);
                let f1 = (f + 4).min(clip_max) >> 3;
                let f2 = (f + 3).min(clip_max) >> 3;
                write(-1, p0 + f2);
                write(0, q0 - f1);
            } else {
                let f = clip3(-clip_max, clip_max, 3 * (q0 - p0));
                let f1 = (f + 4).min(clip_max) >> 3;
                let f2 = (f + 3).min(clip_max) >> 3;
                write(-1, p0 + f2);
                write(0, q0 - f1);
                let f = (f1 + 1) >> 1;
                write(-2, p1 + f);
                write(1, q1 - f);
            }
        }
    }
}

fn clip3(lo: i32, hi: i32, val: i32) -> i32 {
    val.clamp(lo, hi)
}

// ---------------------------------------------------------------------------
// Public entry: apply the AV1 loop filter to a reconstructed frame.
// ---------------------------------------------------------------------------

pub fn apply_loop_filter(pixels: &mut FramePixels, lfp: &LoopFilterParams) {
    if lfp.y_vert_level == 0
        && lfp.y_horiz_level == 0
        && lfp.uv_vert_level == 0
        && lfp.uv_horiz_level == 0
    {
        return;
    }

    let w = pixels.width as usize;
    let h = pixels.height as usize;
    let bd = pixels.bit_depth.bits();
    let max_val = pixels.bit_depth.max_value();
    let bd_min_8 = (bd as i32) - 8;

    // Filter width for luma edges: dav1d uses wd = 4 << idx.
    // TX_8X8 (idx=1) → wd=8; TX_16X16+ (idx=2) → wd=16.
    // Our common luma case is TX_8X8, so use wd=8 here.
    let y_wd = 8u8;
    // Chroma blocks are TX_4X4 in our common path → wd=4.
    let uv_wd = 4u8;

    // --- Luma: vertical edges first (iterate over x positions on edge) ---
    if lfp.y_vert_level > 0 {
        let e = calc_e(lfp.y_vert_level, lfp.sharpness);
        let i_level = calc_i(lfp.y_vert_level, lfp.sharpness);
        let h_level = (lfp.y_vert_level as i32) >> 4;
        for y_start in (0..h).step_by(4) {
            let block_h = (h - y_start).min(4);
            // Vertical edges at 8-pixel aligned x positions (x >= 8)
            for x in (8..w).step_by(8) {
                // Process `block_h` rows in chunks of 4
                for row_off in (0..block_h).step_by(4) {
                    loop_filter_edge(
                        &mut pixels.y, w, x, y_start + row_off, false,
                        e, i_level, h_level, y_wd, max_val, bd_min_8,
                    );
                }
            }
        }
    }

    // --- Luma: horizontal edges ---
    if lfp.y_horiz_level > 0 {
        let e = calc_e(lfp.y_horiz_level, lfp.sharpness);
        let i_level = calc_i(lfp.y_horiz_level, lfp.sharpness);
        let h_level = (lfp.y_horiz_level as i32) >> 4;
        for y in (8..h).step_by(8) {
            for x_start in (0..w).step_by(4) {
                loop_filter_edge(
                    &mut pixels.y, w, x_start, y, true,
                    e, i_level, h_level, y_wd, max_val, bd_min_8,
                );
            }
        }
    }

    // --- Chroma ---
    let uv_w = w.div_ceil(2);
    let uv_h = h.div_ceil(2);

    for plane_idx in 0..2 {
        let plane = if plane_idx == 0 {
            &mut pixels.u
        } else {
            &mut pixels.v
        };

        // Vertical edges
        if lfp.uv_vert_level > 0 && uv_w > 4 {
            let e = calc_e(lfp.uv_vert_level, lfp.sharpness);
            let i_level = calc_i(lfp.uv_vert_level, lfp.sharpness);
            let h_level = (lfp.uv_vert_level as i32) >> 4;
            for y_start in (0..uv_h).step_by(4) {
                for x in (8..uv_w).step_by(8) {
                    loop_filter_edge(
                        plane, uv_w, x, y_start, false,
                        e, i_level, h_level, uv_wd, max_val, bd_min_8,
                    );
                }
            }
        }

        // Horizontal edges
        if lfp.uv_horiz_level > 0 && uv_h > 8 {
            let e = calc_e(lfp.uv_horiz_level, lfp.sharpness);
            let i_level = calc_i(lfp.uv_horiz_level, lfp.sharpness);
            let h_level = (lfp.uv_horiz_level as i32) >> 4;
            for y in (8..uv_h).step_by(8) {
                for x_start in (0..uv_w).step_by(4) {
                    loop_filter_edge(
                        plane, uv_w, x_start, y, true,
                        e, i_level, h_level, uv_wd, max_val, bd_min_8,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_qp_returns_zero_level() {
        let lfp = loop_filter_params_for_qidx(0);
        assert_eq!(lfp.y_vert_level, 0);
        assert_eq!(lfp.y_horiz_level, 0);
    }

    #[test]
    fn high_qp_returns_nonzero_level() {
        let lfp = loop_filter_params_for_qidx(128);
        assert!(lfp.y_vert_level > 0, "level should be non-zero for QP=128");
        assert!(lfp.y_vert_level <= 63);
    }

    #[test]
    fn level_increases_with_qp() {
        let l1 = loop_filter_params_for_qidx(64).y_vert_level;
        let l2 = loop_filter_params_for_qidx(128).y_vert_level;
        let l3 = loop_filter_params_for_qidx(200).y_vert_level;
        assert!(l2 > l1, "level should increase QP=64→128");
        assert!(l3 > l2, "level should increase QP=128→200");
    }

    #[test]
    fn eih_matches_dav1d_for_sharpness_zero() {
        // For sharpness=0 and level L: I = max(L, 1), E = 2*(L+2) + max(L, 1)
        // For L=37: I=37, E=2*39+37=115
        assert_eq!(calc_i(37, 0), 37);
        assert_eq!(calc_e(37, 0), 115);
        assert_eq!(calc_i(1, 0), 1);
        assert_eq!(calc_e(1, 0), 2 * 3 + 1);
    }

    #[test]
    fn apply_loop_filter_no_op_for_zero_levels() {
        let mut frame = FramePixels::solid(64, 64, 128, 128, 128);
        let copy = frame.clone();
        let lfp = LoopFilterParams {
            y_vert_level: 0,
            y_horiz_level: 0,
            uv_vert_level: 0,
            uv_horiz_level: 0,
            sharpness: 0,
            delta_enabled: false,
            delta_update: false,
        };
        apply_loop_filter(&mut frame, &lfp);
        assert_eq!(frame.y, copy.y);
    }

    #[test]
    fn apply_loop_filter_modifies_pixels_near_edge() {
        let mut frame = FramePixels::solid(64, 64, 128, 128, 128);
        for y in 0..64 {
            for x in 0..32 {
                frame.y[y * 64 + x] = 64;
            }
        }
        let original: Vec<u16> = frame.y.clone();
        let lfp = LoopFilterParams {
            y_vert_level: 30,
            y_horiz_level: 30,
            uv_vert_level: 30,
            uv_horiz_level: 30,
            sharpness: 0,
            delta_enabled: true,
            delta_update: false,
        };
        apply_loop_filter(&mut frame, &lfp);
        assert_ne!(
            frame.y, original,
            "filter should modify pixels near a block edge"
        );
    }

    #[test]
    fn qp_to_filter_level_boundaries() {
        assert_eq!(qp_to_filter_level(0), 0);
        assert_eq!(qp_to_filter_level(4), 0);
        assert!(qp_to_filter_level(8) > 0);
        assert!(qp_to_filter_level(255) <= 63);
    }
}
