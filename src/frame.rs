use crate::bitwriter::BitWriter;

const TILE_DATA: [u8; 12] = [
    0x40, 0x0a, 0x05, 0x79, 0x52, 0x6e, 0x43, 0xd7, 0xe6, 0x42, 0x63, 0x20,
];

pub fn encode_frame() -> Vec<u8> {
    let mut w = BitWriter::new();

    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);

    w.write_bits(192, 8);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);

    w.write_bits(0, 6);
    w.write_bits(0, 6);

    w.write_bit(false);
    w.write_bit(false);

    let mut header_bytes = w.finalize();
    header_bytes.extend_from_slice(&TILE_DATA);
    header_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_payload_matches_reference() {
        let bytes = encode_frame();
        let expected = vec![
            0x18, 0x00, 0x00, 0x00, 0x40, 0x0a, 0x05, 0x79, 0x52, 0x6e, 0x43, 0xd7, 0xe6, 0x42,
            0x63, 0x20,
        ];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn frame_payload_is_16_bytes() {
        let bytes = encode_frame();
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn frame_header_is_first_4_bytes() {
        let bytes = encode_frame();
        assert_eq!(&bytes[..4], &[0x18, 0x00, 0x00, 0x00]);
    }
}
