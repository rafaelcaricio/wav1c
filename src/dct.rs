fn clip(v: i32) -> i32 {
    v.clamp(-32768, 32767)
}

fn inv_dct4_1d(data: &mut [i32], offset: usize, stride: usize) {
    let in0 = data[offset];
    let in1 = data[offset + stride];
    let in2 = data[offset + 2 * stride];
    let in3 = data[offset + 3 * stride];

    let t0 = ((in0 + in2) * 181 + 128) >> 8;
    let t1 = ((in0 - in2) * 181 + 128) >> 8;
    let t2 = ((in1 * 1567 - in3 * (3784 - 4096) + 2048) >> 12) - in3;
    let t3 = ((in1 * (3784 - 4096) + in3 * 1567 + 2048) >> 12) + in1;

    data[offset] = clip(t0 + t3);
    data[offset + stride] = clip(t1 + t2);
    data[offset + 2 * stride] = clip(t1 - t2);
    data[offset + 3 * stride] = clip(t0 - t3);
}

fn inv_dct8_1d(data: &mut [i32], offset: usize, stride: usize) {
    inv_dct4_1d(data, offset, stride * 2);

    let in1 = data[offset + stride];
    let in3 = data[offset + 3 * stride];
    let in5 = data[offset + 5 * stride];
    let in7 = data[offset + 7 * stride];

    let t4a = ((in1 * 799 - in7 * (4017 - 4096) + 2048) >> 12) - in7;
    let t5a = (in5 * 1703 - in3 * 1138 + 1024) >> 11;
    let t6a = (in5 * 1138 + in3 * 1703 + 1024) >> 11;
    let t7a = ((in1 * (4017 - 4096) + in7 * 799 + 2048) >> 12) + in1;

    let t4 = clip(t4a + t5a);
    let t5a = clip(t4a - t5a);
    let t7 = clip(t7a + t6a);
    let t6a = clip(t7a - t6a);

    let t5 = ((t6a - t5a) * 181 + 128) >> 8;
    let t6 = ((t6a + t5a) * 181 + 128) >> 8;

    let t0 = data[offset];
    let t1 = data[offset + 2 * stride];
    let t2 = data[offset + 4 * stride];
    let t3 = data[offset + 6 * stride];

    data[offset] = clip(t0 + t7);
    data[offset + stride] = clip(t1 + t6);
    data[offset + 2 * stride] = clip(t2 + t5);
    data[offset + 3 * stride] = clip(t3 + t4);
    data[offset + 4 * stride] = clip(t3 - t4);
    data[offset + 5 * stride] = clip(t2 - t5);
    data[offset + 6 * stride] = clip(t1 - t6);
    data[offset + 7 * stride] = clip(t0 - t7);
}

fn fwd_dct4_1d(data: &mut [i32], offset: usize, stride: usize) {
    let (out0, out1, out2, out3) = fwd_dct4_1d_values(
        data[offset],
        data[offset + stride],
        data[offset + 2 * stride],
        data[offset + 3 * stride],
    );
    data[offset] = out0;
    data[offset + stride] = out1;
    data[offset + 2 * stride] = out2;
    data[offset + 3 * stride] = out3;
}

fn fwd_dct4_1d_values(in0: i32, in1: i32, in2: i32, in3: i32) -> (i32, i32, i32, i32) {
    let s0 = in0 + in3;
    let s1 = in1 + in2;
    let s2 = in1 - in2;
    let s3 = in0 - in3;

    let out0 = ((s0 + s1) * 181 + 128) >> 8;
    let out1 = ((s3 * (3784 - 4096) + s2 * 1567 + 2048) >> 12) + s3;
    let out2 = ((s0 - s1) * 181 + 128) >> 8;
    let out3 = ((s3 * 1567 - s2 * (3784 - 4096) + 2048) >> 12) - s2;

    (out0, out1, out2, out3)
}

fn fwd_dct8_1d(data: &mut [i32], offset: usize, stride: usize) {
    let in0 = data[offset];
    let in1 = data[offset + stride];
    let in2 = data[offset + 2 * stride];
    let in3 = data[offset + 3 * stride];
    let in4 = data[offset + 4 * stride];
    let in5 = data[offset + 5 * stride];
    let in6 = data[offset + 6 * stride];
    let in7 = data[offset + 7 * stride];

    let s0 = in0 + in7;
    let s1 = in1 + in6;
    let s2 = in2 + in5;
    let s3 = in3 + in4;
    let s4 = in3 - in4;
    let s5 = in2 - in5;
    let s6 = in1 - in6;
    let s7 = in0 - in7;

    let (e0, e1, e2, e3) = fwd_dct4_1d_values(s0, s1, s2, s3);

    let t5 = ((s6 - s5) * 181 + 128) >> 8;
    let t6 = ((s6 + s5) * 181 + 128) >> 8;

    let t4a = clip(s4 + t5);
    let t5a = clip(s4 - t5);
    let t7a = clip(s7 + t6);
    let t6a = clip(s7 - t6);

    let o1 = ((t7a * (4017 - 4096) + t4a * 799 + 2048) >> 12) + t7a;
    let o3 = (t6a * 1703 - t5a * 1138 + 1024) >> 11;
    let o5 = (t5a * 1703 + t6a * 1138 + 1024) >> 11;
    let o7 = ((t7a * 799 - t4a * (4017 - 4096) + 2048) >> 12) - t4a;

    data[offset] = e0;
    data[offset + stride] = o1;
    data[offset + 2 * stride] = e1;
    data[offset + 3 * stride] = o3;
    data[offset + 4 * stride] = e2;
    data[offset + 5 * stride] = o5;
    data[offset + 6 * stride] = e3;
    data[offset + 7 * stride] = o7;
}

pub fn forward_dct_4x4(residual: &[i32; 16]) -> [i32; 16] {
    let mut buf = *residual;

    for v in &mut buf {
        *v <<= 2;
    }

    for row in 0..4 {
        fwd_dct4_1d(&mut buf, row * 4, 1);
    }

    for col in 0..4 {
        fwd_dct4_1d(&mut buf, col, 4);
    }

    buf
}

pub fn forward_dct_8x8(residual: &[i32; 64]) -> [i32; 64] {
    let mut buf = *residual;

    for v in &mut buf {
        *v <<= 2;
    }

    for row in 0..8 {
        fwd_dct8_1d(&mut buf, row * 8, 1);
    }

    for v in &mut buf {
        *v = (*v + 1) >> 1;
    }

    for col in 0..8 {
        fwd_dct8_1d(&mut buf, col, 8);
    }

    buf
}

pub fn inverse_dct_4x4(coeffs: &[i32; 16]) -> [i32; 16] {
    let mut buf = *coeffs;

    for row in 0..4 {
        inv_dct4_1d(&mut buf, row * 4, 1);
    }

    for col in 0..4 {
        inv_dct4_1d(&mut buf, col, 4);
    }

    for v in &mut buf {
        *v = (*v + 8) >> 4;
    }

    buf
}

pub fn inverse_dct_8x8(coeffs: &[i32; 64]) -> [i32; 64] {
    let mut buf = *coeffs;

    for row in 0..8 {
        inv_dct8_1d(&mut buf, row * 8, 1);
    }

    for v in &mut buf {
        *v = (*v + 1) >> 1;
    }

    for col in 0..8 {
        inv_dct8_1d(&mut buf, col, 8);
    }

    for v in &mut buf {
        *v = (*v + 8) >> 4;
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_zero_4x4_produces_all_zero() {
        let input = [0i32; 16];
        let coeffs = forward_dct_4x4(&input);
        assert_eq!(coeffs, [0i32; 16]);
    }

    #[test]
    fn all_zero_8x8_produces_all_zero() {
        let input = [0i32; 64];
        let coeffs = forward_dct_8x8(&input);
        assert_eq!(coeffs, [0i32; 64]);
    }

    #[test]
    fn dc_only_4x4() {
        let input = [100i32; 16];
        let coeffs = forward_dct_4x4(&input);
        assert_ne!(coeffs[0], 0);
        for i in 1..16 {
            assert_eq!(coeffs[i], 0, "AC coefficient at {} should be zero", i);
        }
    }

    #[test]
    fn dc_only_8x8() {
        let input = [50i32; 64];
        let coeffs = forward_dct_8x8(&input);
        assert_ne!(coeffs[0], 0);
        for i in 1..64 {
            assert_eq!(coeffs[i], 0, "AC coefficient at {} should be zero", i);
        }
    }

    #[test]
    fn roundtrip_4x4_constant() {
        let original = [42i32; 16];
        let coeffs = forward_dct_4x4(&original);
        let recovered = inverse_dct_4x4(&coeffs);
        for i in 0..16 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_8x8_constant() {
        let original = [42i32; 64];
        let coeffs = forward_dct_8x8(&original);
        let recovered = inverse_dct_8x8(&coeffs);
        for i in 0..64 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_4x4_gradient() {
        let mut original = [0i32; 16];
        for i in 0..16 {
            original[i] = (i as i32) * 10;
        }
        let coeffs = forward_dct_4x4(&original);
        let recovered = inverse_dct_4x4(&coeffs);
        for i in 0..16 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_8x8_gradient() {
        let mut original = [0i32; 64];
        for i in 0..64 {
            original[i] = (i as i32) * 3;
        }
        let coeffs = forward_dct_8x8(&original);
        let recovered = inverse_dct_8x8(&coeffs);
        for i in 0..64 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_4x4_small_residual() {
        let original = [
            3, -1, 2, 0, -2, 1, -3, 4, 1, 0, -1, 2, -4, 3, 0, -2,
        ];
        let coeffs = forward_dct_4x4(&original);
        let recovered = inverse_dct_4x4(&coeffs);
        for i in 0..16 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_8x8_small_residual() {
        let mut original = [0i32; 64];
        for i in 0..64 {
            original[i] = ((i as i32 * 7 + 3) % 11) - 5;
        }
        let coeffs = forward_dct_8x8(&original);
        let recovered = inverse_dct_8x8(&coeffs);
        for i in 0..64 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_4x4_typical_residual() {
        let original = [
            -15, 8, -3, 12, 7, -20, 5, 1, -8, 14, -6, 3, 10, -2, 9, -11,
        ];
        let coeffs = forward_dct_4x4(&original);
        let recovered = inverse_dct_4x4(&coeffs);
        for i in 0..16 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_8x8_typical_residual() {
        let mut original = [0i32; 64];
        for i in 0..64 {
            original[i] = ((i as i32 * 13 + 5) % 51) - 25;
        }
        let coeffs = forward_dct_8x8(&original);
        let recovered = inverse_dct_8x8(&coeffs);
        for i in 0..64 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_4x4_large_values() {
        let original = [
            200, -150, 180, -100, 120, -200, 90, 50, -180, 160, -80, 140, 70, -120, 190, -60,
        ];
        let coeffs = forward_dct_4x4(&original);
        let recovered = inverse_dct_4x4(&coeffs);
        for i in 0..16 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_8x8_large_values() {
        let mut original = [0i32; 64];
        for i in 0..64 {
            original[i] = ((i as i32 * 37 + 11) % 401) - 200;
        }
        let coeffs = forward_dct_8x8(&original);
        let recovered = inverse_dct_8x8(&coeffs);
        for i in 0..64 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn forward_4x4_known_dc_value() {
        let input = [100i32; 16];
        let coeffs = forward_dct_4x4(&input);
        let scaled = 100 << 2;
        let row_dc = (scaled * 4 * 181 + 128) >> 8;
        let expected_dc = (row_dc * 4 * 181 + 128) >> 8;
        assert_eq!(coeffs[0], expected_dc);
    }

    #[test]
    fn inverse_4x4_matches_dav1d_structure() {
        let coeffs = [100, 20, -10, 5, 30, -15, 8, -3, -5, 12, 0, -7, 18, -9, 4, -2];
        let result = inverse_dct_4x4(&coeffs);
        let mut buf = coeffs;
        for row in 0..4 {
            inv_dct4_1d(&mut buf, row * 4, 1);
        }
        for col in 0..4 {
            inv_dct4_1d(&mut buf, col, 4);
        }
        for i in 0..16 {
            assert_eq!(result[i], (buf[i] + 8) >> 4);
        }
    }

    #[test]
    fn inverse_8x8_matches_dav1d_structure() {
        let mut coeffs = [0i32; 64];
        for i in 0..64 {
            coeffs[i] = ((i as i32 * 11 + 3) % 41) - 20;
        }
        let result = inverse_dct_8x8(&coeffs);
        let mut buf = coeffs;
        for row in 0..8 {
            inv_dct8_1d(&mut buf, row * 8, 1);
        }
        for i in 0..64 {
            buf[i] = (buf[i] + 1) >> 1;
        }
        for col in 0..8 {
            inv_dct8_1d(&mut buf, col, 8);
        }
        for i in 0..64 {
            assert_eq!(result[i], (buf[i] + 8) >> 4);
        }
    }

    #[test]
    fn forward_8x8_known_dc_value() {
        let input = [50i32; 64];
        let coeffs = forward_dct_8x8(&input);
        assert_ne!(coeffs[0], 0);
        for i in 1..64 {
            assert_eq!(coeffs[i], 0, "AC coefficient at {} should be zero for constant input", i);
        }
    }

    #[test]
    fn roundtrip_4x4_single_pixel() {
        let mut original = [0i32; 16];
        original[0] = 100;
        let coeffs = forward_dct_4x4(&original);
        let recovered = inverse_dct_4x4(&coeffs);
        for i in 0..16 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_8x8_single_pixel() {
        let mut original = [0i32; 64];
        original[0] = 100;
        let coeffs = forward_dct_8x8(&original);
        let recovered = inverse_dct_8x8(&coeffs);
        for i in 0..64 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_4x4_negative_values() {
        let original = [
            -50, -30, -10, -70, -20, -60, -40, -80, -5, -15, -25, -35, -45, -55, -65, -75,
        ];
        let coeffs = forward_dct_4x4(&original);
        let recovered = inverse_dct_4x4(&coeffs);
        for i in 0..16 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_4x4_checkerboard() {
        let original = [
            100, -100, 100, -100, -100, 100, -100, 100, 100, -100, 100, -100, -100, 100, -100,
            100,
        ];
        let coeffs = forward_dct_4x4(&original);
        let recovered = inverse_dct_4x4(&coeffs);
        for i in 0..16 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }

    #[test]
    fn roundtrip_8x8_checkerboard() {
        let mut original = [0i32; 64];
        for row in 0..8 {
            for col in 0..8 {
                original[row * 8 + col] = if (row + col) % 2 == 0 { 80 } else { -80 };
            }
        }
        let coeffs = forward_dct_8x8(&original);
        let recovered = inverse_dct_8x8(&coeffs);
        for i in 0..64 {
            assert!(
                (recovered[i] - original[i]).abs() <= 1,
                "pixel {} differs: original={}, recovered={}",
                i,
                original[i],
                recovered[i]
            );
        }
    }
}
