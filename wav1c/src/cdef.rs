use crate::y4m::FramePixels;

const CDEF_PRI_TAPS: [[i32; 2]; 2] = [[4, 2], [3, 3]];
const CDEF_SEC_TAPS: [[i32; 2]; 2] = [[2, 1], [2, 1]];

const CDEF_DIRECTIONS: [[(i32, i32); 2]; 8] = [
    [(-1, 1), (-2, 2)],
    [(0, 1), (-1, 2)],
    [(0, 1), (0, 2)],
    [(0, 1), (1, 2)],
    [(1, 1), (2, 2)],
    [(1, 0), (2, 1)],
    [(1, 0), (2, 0)],
    [(1, 0), (2, -1)],
];

#[inline]
fn constrain(diff: i32, strength: i32, damping: i32) -> i32 {
    if strength == 0 {
        return 0;
    }
    let shift = 0.max(damping - (31 - strength.leading_zeros() as i32));
    let abs_diff = diff.abs();
    let val = 0.max(strength - (abs_diff >> shift));
    if diff < 0 {
        -abs_diff.min(val)
    } else {
        abs_diff.min(val)
    }
}

pub fn cdef_analyze_direction(src: &[u8], stride: usize, bw: usize, bh: usize) -> u8 {
    let mut cost = [0i32; 8];
    for y in 0..bh {
        for x in 0..bw {
            let p = src[y * stride + x] as i32;
            for dir in 0..8 {
                let (dy, dx) = CDEF_DIRECTIONS[dir][0];
                let ny1 = y as i32 + dy;
                let nx1 = x as i32 + dx;
                let ny2 = y as i32 - dy;
                let nx2 = x as i32 - dx;
                
                if ny1 >= 0 && ny1 < bh as i32 && nx1 >= 0 && nx1 < bw as i32 {
                    let p1 = src[ny1 as usize * stride + nx1 as usize] as i32;
                    cost[dir] += (p - p1).pow(2);
                }
                if ny2 >= 0 && ny2 < bh as i32 && nx2 >= 0 && nx2 < bw as i32 {
                    let p2 = src[ny2 as usize * stride + nx2 as usize] as i32;
                    cost[dir] += (p - p2).pow(2);
                }
            }
        }
    }
    
    let mut best_dir = 0;
    let mut min_cost = i32::MAX;
    for (dir, &c) in cost.iter().enumerate() {
        if c < min_cost {
            min_cost = c;
            best_dir = dir as u8;
        }
    }
    best_dir
}

pub fn cdef_filter_block(
    src: &[u8],
    stride: usize,
    dst: &mut [u8],
    dst_stride: usize,
    width: usize,
    height: usize,
    pri_strength: i32,
    sec_strength: i32,
    damping: i32,
) {
    if pri_strength == 0 && sec_strength == 0 {
        for y in 0..height {
            for x in 0..width {
                dst[y * dst_stride + x] = src[y * stride + x];
            }
        }
        return;
    }

    let dir = cdef_analyze_direction(src, stride, width, height);
    let pri_taps = &CDEF_PRI_TAPS[(pri_strength == 0) as usize];
    let sec_taps = &CDEF_SEC_TAPS[(pri_strength == 0) as usize];
    
    let dir1 = dir as usize;
    let dir2 = (dir as usize + 2) & 7;
    let dir3 = (dir as usize + 6) & 7;

    for y in 0..height {
        for x in 0..width {
            let p = src[y * stride + x] as i32;
            let mut sum = 0;

            // Primary direction
            for k in 0..2 {
                let (dy, dx) = CDEF_DIRECTIONS[dir1][k];
                for sign in [-1, 1] {
                    let ny = y as i32 + sign * dy;
                    let nx = x as i32 + sign * dx;
                    if ny >= 0 && ny < height as i32 && nx >= 0 && nx < width as i32 {
                        let p1 = src[ny as usize * stride + nx as usize] as i32;
                        let c = constrain(p1 - p, pri_strength, damping);
                        sum += pri_taps[k] * c;
                    }
                }
            }

            // Secondary direction 1
            for k in 0..2 {
                let (dy, dx) = CDEF_DIRECTIONS[dir2][k];
                for sign in [-1, 1] {
                    let ny = y as i32 + sign * dy;
                    let nx = x as i32 + sign * dx;
                    if ny >= 0 && ny < height as i32 && nx >= 0 && nx < width as i32 {
                        let p1 = src[ny as usize * stride + nx as usize] as i32;
                        let c = constrain(p1 - p, sec_strength, damping);
                        sum += sec_taps[k] * c;
                    }
                }
            }

            // Secondary direction 2
            for k in 0..2 {
                let (dy, dx) = CDEF_DIRECTIONS[dir3][k];
                for sign in [-1, 1] {
                    let ny = y as i32 + sign * dy;
                    let nx = x as i32 + sign * dx;
                    if ny >= 0 && ny < height as i32 && nx >= 0 && nx < width as i32 {
                        let p1 = src[ny as usize * stride + nx as usize] as i32;
                        let c = constrain(p1 - p, sec_strength, damping);
                        sum += sec_taps[k] * c;
                    }
                }
            }

            let filtered = p + ((8 + sum - (sum < 0) as i32) >> 4);
            dst[y * dst_stride + x] = filtered.clamp(0, 255) as u8;
        }
    }
}

pub fn apply_cdef_frame(
    pixels: &mut FramePixels,
    pri_strength: i32,
    sec_strength: i32,
    damping: i32,
) {
    if pri_strength == 0 && sec_strength == 0 {
        return;
    }

    let mut filtered_y = vec![0u8; pixels.y.len()];
    let mut filtered_u = vec![0u8; pixels.u.len()];
    let mut filtered_v = vec![0u8; pixels.v.len()];

    let width = pixels.width as usize;
    let height = pixels.height as usize;
    let uv_w = width.div_ceil(2);
    let uv_h = height.div_ceil(2);

    for by in (0..height).step_by(8) {
        for bx in (0..width).step_by(8) {
            let bw = (8).min(width - bx);
            let bh = (8).min(height - by);
            
            cdef_filter_block(
                &pixels.y[by * width + bx..],
                width,
                &mut filtered_y[by * width + bx..],
                width,
                bw,
                bh,
                pri_strength,
                sec_strength,
                damping,
            );
        }
    }

    for by in (0..uv_h).step_by(4) {
        for bx in (0..uv_w).step_by(4) {
            let bw = (4).min(uv_w - bx);
            let bh = (4).min(uv_h - by);
            
            cdef_filter_block(
                &pixels.u[by * uv_w + bx..],
                uv_w,
                &mut filtered_u[by * uv_w + bx..],
                uv_w,
                bw,
                bh,
                pri_strength,
                sec_strength,
                damping,
            );
            
            cdef_filter_block(
                &pixels.v[by * uv_w + bx..],
                uv_w,
                &mut filtered_v[by * uv_w + bx..],
                uv_w,
                bw,
                bh,
                pri_strength,
                sec_strength,
                damping,
            );
        }
    }

    pixels.y = filtered_y;
    pixels.u = filtered_u;
    pixels.v = filtered_v;
}
