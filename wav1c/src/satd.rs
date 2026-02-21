/// Computes a fast 4x4 Hadamard transform of the given residual
#[inline]
fn hadamard_4x4(residual: &[i32], stride: usize) -> [i32; 16] {
    let mut temp = [0i32; 16];
    let mut out = [0i32; 16];

    // Horizontal pass
    for i in 0..4 {
        let src_idx = i * stride;
        let t0 = residual[src_idx] + residual[src_idx + 1];
        let t1 = residual[src_idx] - residual[src_idx + 1];
        let t2 = residual[src_idx + 2] + residual[src_idx + 3];
        let t3 = residual[src_idx + 2] - residual[src_idx + 3];

        temp[i * 4] = t0 + t2;
        temp[i * 4 + 1] = t1 + t3;
        temp[i * 4 + 2] = t0 - t2;
        temp[i * 4 + 3] = t1 - t3;
    }

    // Vertical pass
    for j in 0..4 {
        let t0 = temp[j] + temp[4 + j];
        let t1 = temp[j] - temp[4 + j];
        let t2 = temp[8 + j] + temp[12 + j];
        let t3 = temp[8 + j] - temp[12 + j];

        out[j] = t0 + t2;
        out[4 + j] = t1 + t3;
        out[8 + j] = t0 - t2;
        out[12 + j] = t1 - t3;
    }

    out
}

/// Computes SATD (Sum of Absolute Transformed Differences) for a block
/// Uses 4x4 Hadamard transforms as the base unit to approximate the energy.
pub fn compute_satd(
    source: &[u8],
    prediction: &[u8],
    width: usize,
    height: usize,
    src_stride: usize,
    pred_stride: usize,
) -> u32 {
    let mut satd = 0u32;
    // For small blocks, just sum SAD (e.g. 4x4, 8x4 etc.)
    // We break everything into 4x4 chunks. If a block is not a multiple of 4, we use SAD.
    if !width.is_multiple_of(4) || !height.is_multiple_of(4) {
        for y in 0..height {
            for x in 0..width {
                let diff =
                    (source[y * src_stride + x] as i32) - (prediction[y * pred_stride + x] as i32);
                satd += diff.unsigned_abs();
            }
        }
        return satd;
    }

    let mut residual = [0i32; 16];
    for by in (0..height).step_by(4) {
        for bx in (0..width).step_by(4) {
            for y in 0..4 {
                for x in 0..4 {
                    residual[y * 4 + x] = (source[(by + y) * src_stride + (bx + x)] as i32)
                        - (prediction[(by + y) * pred_stride + (bx + x)] as i32);
                }
            }

            let transformed = hadamard_4x4(&residual, 4);
            let mut chunk_satd = 0u32;
            for &coeff in transformed.iter() {
                chunk_satd += coeff.unsigned_abs();
            }
            // Scale down to match SD range roughly
            satd += chunk_satd / 2;
        }
    }

    satd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_satd_identical() {
        let src = [0u8; 64];
        let pred = [0u8; 64];
        assert_eq!(compute_satd(&src, &pred, 8, 8, 8, 8), 0);
    }

    #[test]
    fn test_compute_satd_diff() {
        let mut src = [0u8; 16];
        let pred = [0u8; 16];
        src[0] = 10;
        src[1] = 10;

        let satd = compute_satd(&src, &pred, 4, 4, 4, 4);
        assert!(satd > 0);
    }
}
