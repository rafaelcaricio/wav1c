pub const DEFAULT_SCAN_4X4: [u16; 16] = [
     0,  4,  1,  2,
     5,  8, 12,  9,
     6,  3,  7, 10,
    13, 14, 11, 15,
];

pub const DEFAULT_SCAN_8X8: [u16; 64] = [
     0,  8,  1,  2,  9, 16, 24, 17,
    10,  3,  4, 11, 18, 25, 32, 40,
    33, 26, 19, 12,  5,  6, 13, 20,
    27, 34, 41, 48, 56, 49, 42, 35,
    28, 21, 14,  7, 15, 22, 29, 36,
    43, 50, 57, 58, 51, 44, 37, 30,
    23, 31, 38, 45, 52, 59, 60, 53,
    46, 39, 47, 54, 61, 62, 55, 63,
];

pub const LO_CTX_OFFSETS_2D: [[u8; 5]; 5] = [
    [ 0,  1,  6,  6, 21],
    [ 1,  6,  6, 21, 21],
    [ 6,  6, 21, 21, 21],
    [ 6, 21, 21, 21, 21],
    [21, 21, 21, 21, 21],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_4x4_dc_is_first() {
        assert_eq!(DEFAULT_SCAN_4X4[0], 0);
    }

    #[test]
    fn scan_8x8_dc_is_first() {
        assert_eq!(DEFAULT_SCAN_8X8[0], 0);
    }

    #[test]
    fn scan_4x4_has_correct_length() {
        assert_eq!(DEFAULT_SCAN_4X4.len(), 16);
    }

    #[test]
    fn scan_8x8_has_correct_length() {
        assert_eq!(DEFAULT_SCAN_8X8.len(), 64);
    }

    #[test]
    fn scan_4x4_covers_all_positions() {
        let mut seen = [false; 16];
        for &pos in &DEFAULT_SCAN_4X4 {
            assert!((pos as usize) < 16);
            assert!(!seen[pos as usize]);
            seen[pos as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn scan_8x8_covers_all_positions() {
        let mut seen = [false; 64];
        for &pos in &DEFAULT_SCAN_8X8 {
            assert!((pos as usize) < 64);
            assert!(!seen[pos as usize]);
            seen[pos as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }
}
