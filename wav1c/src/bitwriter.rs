#[derive(Default)]
pub struct BitWriter {
    buf: Vec<u8>,
    current_byte: u8,
    bits_in_current: u8,
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_bit(&mut self, bit: bool) {
        self.current_byte = (self.current_byte << 1) | (bit as u8);
        self.bits_in_current += 1;
        if self.bits_in_current == 8 {
            self.buf.push(self.current_byte);
            self.current_byte = 0;
            self.bits_in_current = 0;
        }
    }

    pub fn write_bits(&mut self, value: u64, n: u8) {
        debug_assert!(n <= 64);
        for i in (0..n).rev() {
            self.write_bit((value >> i) & 1 == 1);
        }
    }

    pub fn byte_align(&mut self) {
        if self.bits_in_current > 0 {
            self.current_byte <<= 8 - self.bits_in_current;
            self.buf.push(self.current_byte);
            self.current_byte = 0;
            self.bits_in_current = 0;
        }
    }

    pub fn finalize(mut self) -> Vec<u8> {
        self.byte_align();
        self.buf
    }

    pub fn trailing_bits(mut self) -> Vec<u8> {
        self.write_bit(true);
        self.byte_align();
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_bit_true() {
        let mut w = BitWriter::new();
        w.write_bit(true);
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0x80]);
    }

    #[test]
    fn single_bit_false() {
        let mut w = BitWriter::new();
        w.write_bit(false);
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0x00]);
    }

    #[test]
    fn write_byte_value() {
        let mut w = BitWriter::new();
        w.write_bits(0xAB, 8);
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0xAB]);
    }

    #[test]
    fn write_3_bits() {
        let mut w = BitWriter::new();
        w.write_bits(0b101, 3);
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0xA0]);
    }

    #[test]
    fn write_across_byte_boundary() {
        let mut w = BitWriter::new();
        w.write_bits(0b11111, 5);
        w.write_bits(0b11111, 5);
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0xFF, 0xC0]);
    }

    #[test]
    fn byte_align_no_op_when_aligned() {
        let mut w = BitWriter::new();
        w.write_bits(0xFF, 8);
        w.byte_align();
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0xFF]);
    }

    #[test]
    fn byte_align_pads_with_zeros() {
        let mut w = BitWriter::new();
        w.write_bits(0b111, 3);
        w.byte_align();
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0xE0]);
    }

    #[test]
    fn empty_writer() {
        let w = BitWriter::new();
        let bytes = w.finalize();
        assert_eq!(bytes, vec![]);
    }

    #[test]
    fn write_16_bits() {
        let mut w = BitWriter::new();
        w.write_bits(0xCAFE, 16);
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0xCA, 0xFE]);
    }
}
