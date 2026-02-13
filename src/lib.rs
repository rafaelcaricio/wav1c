pub mod bitwriter;
pub mod cdf;
pub mod frame;
pub mod ivf;
pub mod msac;
pub mod obu;
pub mod sequence;
pub mod tile;

pub fn encode_av1_ivf(y: u8, u: u8, v: u8) -> Vec<u8> {
    let td = obu::obu_wrap(obu::ObuType::TemporalDelimiter, &[]);
    let seq = obu::obu_wrap(
        obu::ObuType::SequenceHeader,
        &sequence::encode_sequence_header(),
    );
    let frm = obu::obu_wrap(obu::ObuType::Frame, &frame::encode_frame(y, u, v));

    let mut frame_data = Vec::new();
    frame_data.extend_from_slice(&td);
    frame_data.extend_from_slice(&seq);
    frame_data.extend_from_slice(&frm);

    let mut output = Vec::new();
    ivf::write_ivf_header(&mut output, 64, 64, 1).unwrap();
    ivf::write_ivf_frame(&mut output, 0, &frame_data).unwrap();
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_starts_with_valid_obu_structure() {
        let output = encode_av1_ivf(128, 128, 128);
        let frame_data = &output[44..];
        assert_eq!(frame_data[0], 0x12);
        assert_eq!(frame_data[1], 0x00);
        assert_eq!(frame_data[2], 0x0A);
        assert_eq!(frame_data[3], 0x06);
        assert_eq!(
            &frame_data[4..10],
            &[0x18, 0x15, 0x7f, 0xfc, 0x00, 0x08]
        );
        assert_eq!(frame_data[10], 0x32);
    }

    #[test]
    fn different_colors_produce_different_output() {
        let gray = encode_av1_ivf(128, 128, 128);
        let black = encode_av1_ivf(0, 0, 0);
        assert_ne!(gray, black);
    }
}
