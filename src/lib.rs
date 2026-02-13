pub mod bitwriter;
pub mod cdf;
pub mod frame;
pub mod ivf;
pub mod msac;
pub mod obu;
pub mod sequence;
pub mod tile;

pub fn encode_av1_ivf() -> Vec<u8> {
    let td = obu::obu_wrap(obu::ObuType::TemporalDelimiter, &[]);
    let seq = obu::obu_wrap(
        obu::ObuType::SequenceHeader,
        &sequence::encode_sequence_header(),
    );
    let frm = obu::obu_wrap(obu::ObuType::Frame, &frame::encode_frame());

    let mut frame_data = Vec::new();
    frame_data.extend_from_slice(&td);
    frame_data.extend_from_slice(&seq);
    frame_data.extend_from_slice(&frm);

    let mut output = Vec::new();
    ivf::write_ivf_header(&mut output, 64, 64, 1).unwrap();
    ivf::write_ivf_frame(&mut output, 0, &frame_data).unwrap();
    output
}

pub fn encode_av1_ivf_color(y: u8, u: u8, v: u8) -> Vec<u8> {
    let td = obu::obu_wrap(obu::ObuType::TemporalDelimiter, &[]);
    let seq = obu::obu_wrap(
        obu::ObuType::SequenceHeader,
        &sequence::encode_sequence_header(),
    );
    let frm = obu::obu_wrap(obu::ObuType::Frame, &frame::encode_frame_color(y, u, v));

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

    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn full_bitstream_frame_data_matches_reference() {
        let output = encode_av1_ivf();
        let frame_data = &output[44..];
        let expected = hex_to_bytes("12000a0618157ffc0008321018000000400a0579526e43d7e6426320");
        assert_eq!(frame_data, &expected[..]);
    }

    #[test]
    fn output_total_size() {
        let output = encode_av1_ivf();
        assert_eq!(output.len(), 72);
    }
}
