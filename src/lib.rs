pub mod bitwriter;
pub mod cdf;
pub mod cdf_coef;
pub mod dequant;
pub mod frame;
pub mod ivf;
pub mod msac;
pub mod obu;
pub mod sequence;
pub mod tile;
pub mod y4m;

pub const DEFAULT_BASE_Q_IDX: u8 = 128;

pub fn encode_av1_ivf_multi(frames: &[y4m::FramePixels]) -> Vec<u8> {
    encode_av1_ivf_multi_with_quality(frames, DEFAULT_BASE_Q_IDX)
}

pub fn encode_av1_ivf_multi_with_quality(
    frames: &[y4m::FramePixels],
    base_q_idx: u8,
) -> Vec<u8> {
    assert!(!frames.is_empty(), "frames must not be empty");

    let width = frames[0].width;
    let height = frames[0].height;

    for frame in &frames[1..] {
        assert!(
            frame.width == width && frame.height == height,
            "all frames must have the same dimensions"
        );
    }

    assert!((1..=4096).contains(&width), "width must be 1..=4096");
    assert!((1..=2304).contains(&height), "height must be 1..=2304");

    let dq = dequant::lookup_dequant(base_q_idx);
    let gop_size = 25usize;
    let mut output = Vec::new();
    ivf::write_ivf_header(&mut output, width as u16, height as u16, frames.len() as u32).unwrap();

    let mut reference: Option<y4m::FramePixels> = None;

    for (i, pixels) in frames.iter().enumerate() {
        let is_keyframe = i % gop_size == 0;

        let td = obu::obu_wrap(obu::ObuType::TemporalDelimiter, &[]);
        let seq = obu::obu_wrap(
            obu::ObuType::SequenceHeader,
            &sequence::encode_sequence_header(width, height),
        );

        let (frame_payload, recon) = if is_keyframe {
            frame::encode_frame_with_recon(pixels, base_q_idx, dq)
        } else {
            frame::encode_inter_frame_with_recon(
                pixels,
                reference.as_ref().unwrap(),
                0x01,
                0,
                base_q_idx,
                dq,
            )
        };

        reference = Some(recon);

        let frm = obu::obu_wrap(obu::ObuType::Frame, &frame_payload);

        let mut frame_data = Vec::new();
        frame_data.extend_from_slice(&td);
        frame_data.extend_from_slice(&seq);
        frame_data.extend_from_slice(&frm);

        ivf::write_ivf_frame(&mut output, i as u64, &frame_data).unwrap();
    }

    output
}

pub fn encode_av1_ivf_y4m(pixels: &y4m::FramePixels) -> Vec<u8> {
    encode_av1_ivf_multi(std::slice::from_ref(pixels))
}

pub fn encode_av1_ivf(width: u32, height: u32, y: u8, u: u8, v: u8) -> Vec<u8> {
    let pixels = y4m::FramePixels::solid(width, height, y, u, v);
    encode_av1_ivf_y4m(&pixels)
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

        assert_eq!(frame_data[0], temporal_delimiter_header);
        assert_eq!(frame_data[1], temporal_delimiter_size);
        assert_eq!(frame_data[2], sequence_header_obu_header);

        let seq_payload = sequence::encode_sequence_header(64, 64);
        let seq_size = seq_payload.len();
        assert_eq!(frame_data[3], seq_size as u8);

        let frame_obu_offset = 2 + 1 + 1 + seq_size;
        let frame_obu_header = 0x32;
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

    #[test]
    fn y4m_api_matches_solid_api() {
        let solid = encode_av1_ivf(64, 64, 128, 128, 128);
        let pixels = y4m::FramePixels::solid(64, 64, 128, 128, 128);
        let y4m_out = encode_av1_ivf_y4m(&pixels);
        assert_eq!(solid, y4m_out);
    }

    #[test]
    fn multi_frame_ivf_header_has_correct_count() {
        let frames: Vec<_> = (0..3)
            .map(|_| y4m::FramePixels::solid(64, 64, 128, 128, 128))
            .collect();
        let output = encode_av1_ivf_multi(&frames);
        let count = u32::from_le_bytes(output[24..28].try_into().unwrap());
        assert_eq!(count, 3);
    }

    #[test]
    fn multi_frame_produces_larger_output() {
        let one = encode_av1_ivf_multi(&[y4m::FramePixels::solid(64, 64, 128, 128, 128)]);
        let three: Vec<_> = (0..3)
            .map(|_| y4m::FramePixels::solid(64, 64, 128, 128, 128))
            .collect();
        let multi = encode_av1_ivf_multi(&three);
        assert!(multi.len() > one.len());
    }

    #[test]
    fn single_frame_multi_matches_single() {
        let pixels = y4m::FramePixels::solid(64, 64, 128, 128, 128);
        let single = encode_av1_ivf_y4m(&pixels);
        let multi = encode_av1_ivf_multi(&[y4m::FramePixels::solid(64, 64, 128, 128, 128)]);
        assert_eq!(single, multi);
    }

    #[test]
    fn multi_frame_different_content() {
        let frames = vec![
            y4m::FramePixels::solid(64, 64, 0, 0, 0),
            y4m::FramePixels::solid(64, 64, 255, 128, 128),
        ];
        let output = encode_av1_ivf_multi(&frames);
        let count = u32::from_le_bytes(output[24..28].try_into().unwrap());
        assert_eq!(count, 2);
        assert!(output.len() > 32);
    }
}
