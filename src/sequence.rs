use crate::bitwriter::BitWriter;

pub fn encode_sequence_header() -> Vec<u8> {
    let mut w = BitWriter::new();

    w.write_bits(0, 3);
    w.write_bit(true);
    w.write_bit(true);
    w.write_bits(0, 5);

    w.write_bits(5, 4);
    w.write_bits(5, 4);
    w.write_bits(63, 6);
    w.write_bits(63, 6);

    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);

    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bits(0, 2);
    w.write_bit(false);
    w.write_bit(false);

    w.write_bit(true);
    w.write_bits(0, 3);

    w.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_header_matches_reference() {
        let bytes = encode_sequence_header();
        assert_eq!(bytes, vec![0x18, 0x15, 0x7f, 0xfc, 0x00, 0x08]);
    }

    #[test]
    fn sequence_header_is_6_bytes() {
        let bytes = encode_sequence_header();
        assert_eq!(bytes.len(), 6);
    }
}
