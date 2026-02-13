#[repr(u8)]
#[derive(Clone, Copy)]
pub enum ObuType {
    SequenceHeader = 1,
    TemporalDelimiter = 2,
    Frame = 6,
}

pub fn leb128_encode(mut value: u64) -> Vec<u8> {
    let mut result = Vec::new();
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        result.push(byte);
        if value == 0 {
            break;
        }
    }
    result
}

pub fn obu_wrap(obu_type: ObuType, payload: &[u8]) -> Vec<u8> {
    let header_byte = (obu_type as u8) << 3 | (1 << 1);
    let size_bytes = leb128_encode(payload.len() as u64);
    let mut result = Vec::with_capacity(1 + size_bytes.len() + payload.len());
    result.push(header_byte);
    result.extend_from_slice(&size_bytes);
    result.extend_from_slice(payload);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb128_zero() {
        assert_eq!(leb128_encode(0), vec![0x00]);
    }

    #[test]
    fn leb128_small() {
        assert_eq!(leb128_encode(6), vec![0x06]);
    }

    #[test]
    fn leb128_127() {
        assert_eq!(leb128_encode(127), vec![0x7F]);
    }

    #[test]
    fn leb128_128() {
        assert_eq!(leb128_encode(128), vec![0x80, 0x01]);
    }

    #[test]
    fn leb128_300() {
        assert_eq!(leb128_encode(300), vec![0xAC, 0x02]);
    }

    #[test]
    fn obu_temporal_delimiter() {
        let result = obu_wrap(ObuType::TemporalDelimiter, &[]);
        assert_eq!(result, vec![0x12, 0x00]);
    }

    #[test]
    fn obu_sequence_header_6bytes() {
        let payload = vec![0x18, 0x15, 0x7f, 0xfc, 0x00, 0x08];
        let result = obu_wrap(ObuType::SequenceHeader, &payload);
        assert_eq!(result[0], 0x0A);
        assert_eq!(result[1], 0x06);
        assert_eq!(&result[2..], &payload[..]);
    }

    #[test]
    fn obu_frame_16bytes() {
        let payload = vec![0u8; 16];
        let result = obu_wrap(ObuType::Frame, &payload);
        assert_eq!(result[0], 0x32);
        assert_eq!(result[1], 0x10);
        assert_eq!(result.len(), 2 + 16);
    }
}
