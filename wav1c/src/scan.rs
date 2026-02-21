pub const DEFAULT_SCAN_4X4: [u16; 16] = [0, 4, 1, 2, 5, 8, 12, 9, 6, 3, 7, 10, 13, 14, 11, 15];

pub const DEFAULT_SCAN_8X8: [u16; 64] = [
    0, 8, 1, 2, 9, 16, 24, 17, 10, 3, 4, 11, 18, 25, 32, 40, 33, 26, 19, 12, 5, 6, 13, 20, 27, 34,
    41, 48, 56, 49, 42, 35, 28, 21, 14, 7, 15, 22, 29, 36, 43, 50, 57, 58, 51, 44, 37, 30, 23, 31,
    38, 45, 52, 59, 60, 53, 46, 39, 47, 54, 61, 62, 55, 63,
];

pub const DEFAULT_SCAN_16X16: [u16; 256] = [
    0, 16, 1, 2, 17, 32, 48, 33, 18, 3, 4, 19, 34, 49, 64, 80, 65, 50, 35, 20, 5, 6, 21, 36, 51,
    66, 81, 96, 112, 97, 82, 67, 52, 37, 22, 7, 8, 23, 38, 53, 68, 83, 98, 113, 128, 144, 129, 114,
    99, 84, 69, 54, 39, 24, 9, 10, 25, 40, 55, 70, 85, 100, 115, 130, 145, 160, 176, 161, 146, 131,
    116, 101, 86, 71, 56, 41, 26, 11, 12, 27, 42, 57, 72, 87, 102, 117, 132, 147, 162, 177, 192,
    208, 193, 178, 163, 148, 133, 118, 103, 88, 73, 58, 43, 28, 13, 14, 29, 44, 59, 74, 89, 104,
    119, 134, 149, 164, 179, 194, 209, 224, 240, 225, 210, 195, 180, 165, 150, 135, 120, 105, 90,
    75, 60, 45, 30, 15, 31, 46, 61, 76, 91, 106, 121, 136, 151, 166, 181, 196, 211, 226, 241, 242,
    227, 212, 197, 182, 167, 152, 137, 122, 107, 92, 77, 62, 47, 63, 78, 93, 108, 123, 138, 153,
    168, 183, 198, 213, 228, 243, 244, 229, 214, 199, 184, 169, 154, 139, 124, 109, 94, 79, 95,
    110, 125, 140, 155, 170, 185, 200, 215, 230, 245, 246, 231, 216, 201, 186, 171, 156, 141, 126,
    111, 127, 142, 157, 172, 187, 202, 217, 232, 247, 248, 233, 218, 203, 188, 173, 158, 143, 159,
    174, 189, 204, 219, 234, 249, 250, 235, 220, 205, 190, 175, 191, 206, 221, 236, 251, 252, 237,
    222, 207, 223, 238, 253, 254, 239, 255,
];

pub const LO_CTX_OFFSETS_2D: [[u8; 5]; 5] = [
    [0, 1, 6, 6, 21],
    [1, 6, 6, 21, 21],
    [6, 6, 21, 21, 21],
    [6, 21, 21, 21, 21],
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

    #[test]
    fn scan_16x16_dc_is_first() {
        assert_eq!(DEFAULT_SCAN_16X16[0], 0);
    }

    #[test]
    fn scan_16x16_has_correct_length() {
        assert_eq!(DEFAULT_SCAN_16X16.len(), 256);
    }

    #[test]
    fn scan_16x16_covers_all_positions() {
        let mut seen = [false; 256];
        for &pos in &DEFAULT_SCAN_16X16 {
            assert!((pos as usize) < 256);
            assert!(!seen[pos as usize]);
            seen[pos as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }
}
