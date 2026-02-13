use crate::bitwriter::BitWriter;

pub fn encode_frame(y: u8, u: u8, v: u8) -> Vec<u8> {
    let base_q_idx: u8 = 128;
    let mut w = BitWriter::new();

    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);

    w.write_bit(true);
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
    w.write_bit(false);

    let mut header_bytes = w.finalize();
    let tile_data = crate::tile::encode_tile(y, u, v, base_q_idx);
    header_bytes.extend_from_slice(&tile_data);
    header_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_header_starts_correctly() {
        let bytes = encode_frame(128, 128, 128);
        assert!(bytes.len() > 5);
    }

    #[test]
    fn frame_for_skip_color_is_compact() {
        let bytes = encode_frame(128, 128, 128);
        assert!(bytes.len() < 20);
    }

    #[test]
    fn frame_for_nonzero_residual_is_larger() {
        let skip_bytes = encode_frame(128, 128, 128);
        let color_bytes = encode_frame(0, 0, 0);
        assert!(color_bytes.len() > skip_bytes.len());
    }
}
