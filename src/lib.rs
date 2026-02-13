pub mod bitwriter;
pub mod cdf;
pub mod frame;
pub mod ivf;
pub mod msac;
pub mod obu;
pub mod sequence;
pub mod tile;

pub fn encode_av1_ivf(width: u32, height: u32, y: u8, u: u8, v: u8) -> Vec<u8> {
    assert!((1..=4096).contains(&width), "width must be 1..=4096");
    assert!((1..=2304).contains(&height), "height must be 1..=2304");

    let td = obu::obu_wrap(obu::ObuType::TemporalDelimiter, &[]);
    let seq = obu::obu_wrap(
        obu::ObuType::SequenceHeader,
        &sequence::encode_sequence_header(width, height),
    );
    let frm = obu::obu_wrap(obu::ObuType::Frame, &frame::encode_frame(width, height, y, u, v));

    let mut frame_data = Vec::new();
    frame_data.extend_from_slice(&td);
    frame_data.extend_from_slice(&seq);
    frame_data.extend_from_slice(&frm);

    let mut output = Vec::new();
    ivf::write_ivf_header(&mut output, width as u16, height as u16, 1).unwrap();
    ivf::write_ivf_frame(&mut output, 0, &frame_data).unwrap();
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_starts_with_valid_obu_structure() {
        let output = encode_av1_ivf(64, 64, 128, 128, 128);
        let frame_data = &output[44..];
        let temporal_delimiter_header = 0x12;
        let temporal_delimiter_size = 0x00;
        let sequence_header_obu_header = 0x0A;
        let frame_obu_header = 0x32;

        assert_eq!(frame_data[0], temporal_delimiter_header);
        assert_eq!(frame_data[1], temporal_delimiter_size);
        assert_eq!(frame_data[2], sequence_header_obu_header);

        let seq_payload = sequence::encode_sequence_header(64, 64);
        let seq_size = seq_payload.len();
        assert_eq!(frame_data[3], seq_size as u8);

        let frame_obu_offset = 2 + 1 + 1 + seq_size;
        assert_eq!(frame_data[frame_obu_offset], frame_obu_header);
    }

    #[test]
    fn different_colors_produce_different_output() {
        let gray = encode_av1_ivf(64, 64, 128, 128, 128);
        let black = encode_av1_ivf(64, 64, 0, 0, 0);
        assert_ne!(gray, black);
    }

    #[test]
    fn different_dimensions_produce_different_output() {
        let small = encode_av1_ivf(64, 64, 128, 128, 128);
        let large = encode_av1_ivf(128, 128, 128, 128, 128);
        assert_ne!(small, large);
    }
}
