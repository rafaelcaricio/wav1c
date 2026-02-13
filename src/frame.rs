use crate::bitwriter::BitWriter;

const MAX_TILE_COLS: u32 = 64;
const MAX_TILE_ROWS: u32 = 64;
const MAX_TILE_WIDTH_SB: u32 = 4096 / 64;
const MAX_TILE_AREA_SB: u32 = 4096 * 2304 / (64 * 64);

fn tile_log2(blk_size: u32, target: u32) -> u32 {
    let mut k = 0;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}

pub fn encode_frame(width: u32, height: u32, y: u8, u: u8, v: u8) -> Vec<u8> {
    let base_q_idx: u8 = 128;
    let mut w = BitWriter::new();

    let sbw = width.div_ceil(64);
    let sbh = height.div_ceil(64);

    w.write_bit(false);
    w.write_bits(0, 2);
    w.write_bit(true);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);

    w.write_bit(false);

    w.write_bit(true);

    let min_log2_cols = tile_log2(MAX_TILE_WIDTH_SB, sbw);
    let max_log2_cols = tile_log2(1, sbw.min(MAX_TILE_COLS));
    let log2_cols = min_log2_cols;

    if min_log2_cols < max_log2_cols {
        w.write_bit(false);
    }

    let min_log2_tiles = tile_log2(MAX_TILE_AREA_SB, sbw * sbh).max(min_log2_cols);
    let min_log2_rows = min_log2_tiles.saturating_sub(log2_cols);
    let max_log2_rows = tile_log2(1, sbh.min(MAX_TILE_ROWS));

    if min_log2_rows < max_log2_rows {
        w.write_bit(false);
    }

    w.write_bits(base_q_idx as u64, 8);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);

    w.write_bit(false);

    w.write_bit(false);

    w.write_bits(0, 6);
    w.write_bits(0, 6);
    w.write_bits(0, 3);
    w.write_bit(true);
    w.write_bit(false);

    w.write_bit(false);

    let mut header_bytes = w.finalize();
    let tile_data = crate::tile::encode_tile(width, height, y, u, v);
    header_bytes.extend_from_slice(&tile_data);
    header_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_log2_basic() {
        assert_eq!(tile_log2(64, 1), 0);
        assert_eq!(tile_log2(64, 64), 0);
        assert_eq!(tile_log2(64, 65), 1);
        assert_eq!(tile_log2(1, 1), 0);
        assert_eq!(tile_log2(1, 2), 1);
        assert_eq!(tile_log2(1, 64), 6);
    }

    #[test]
    fn frame_header_64x64_bit_layout() {
        let bytes = encode_frame(64, 64, 128, 128, 128);

        let mut expected = BitWriter::new();

        expected.write_bit(false);
        expected.write_bits(0, 2);
        expected.write_bit(true);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bit(true);

        expected.write_bits(128, 8);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bits(0, 6);
        expected.write_bits(0, 6);
        expected.write_bits(0, 3);
        expected.write_bit(true);
        expected.write_bit(false);

        expected.write_bit(false);

        let expected_header = expected.finalize();
        assert_eq!(&bytes[..expected_header.len()], &expected_header[..]);
    }

    #[test]
    fn frame_header_128x128_differs_from_64x64() {
        let bytes_64 = encode_frame(64, 64, 128, 128, 128);
        let bytes_128 = encode_frame(128, 128, 128, 128, 128);

        assert_ne!(bytes_64, bytes_128);
    }

    #[test]
    fn frame_header_starts_with_show_existing_frame_false() {
        let bytes = encode_frame(64, 64, 128, 128, 128);
        assert_eq!(bytes[0] & 0x80, 0);
    }

    #[test]
    fn frame_for_nonzero_residual_is_larger() {
        let skip_bytes = encode_frame(64, 64, 128, 128, 128);
        let color_bytes = encode_frame(64, 64, 0, 0, 0);
        assert!(color_bytes.len() > skip_bytes.len());
    }

    #[test]
    fn frame_header_320x240_has_tile_bits() {
        let mut expected = BitWriter::new();

        expected.write_bit(false);
        expected.write_bits(0, 2);
        expected.write_bit(true);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bit(true);

        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bits(128, 8);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bits(0, 6);
        expected.write_bits(0, 6);
        expected.write_bits(0, 3);
        expected.write_bit(true);
        expected.write_bit(false);

        expected.write_bit(false);

        let expected_header = expected.finalize();
        let bytes = encode_frame(320, 240, 128, 128, 128);
        assert_eq!(&bytes[..expected_header.len()], &expected_header[..]);
    }
}
