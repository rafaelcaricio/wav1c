#![forbid(unsafe_code)]

pub mod bitwriter;
pub mod cdef;
pub mod cdf;
pub mod cdf_coef;
pub mod dequant;
pub mod encoder;
pub mod error;
pub mod frame;
pub mod ivf;
pub mod msac;
pub mod obu;
pub mod packet;
pub mod rc;
pub mod rdo;
pub mod satd;
pub mod sequence;
pub mod tile;
pub mod y4m;

pub use encoder::{Encoder, EncoderConfig};
pub use error::EncoderError;
pub use packet::{FrameType, Packet};

pub const DEFAULT_BASE_Q_IDX: u8 = 128;
pub const DEFAULT_KEYINT: usize = 25;

#[derive(Clone)]
pub struct EncodeConfig {
    pub base_q_idx: u8,
    pub keyint: usize,
    pub target_bitrate: Option<u64>,
    pub fps: f64,
    pub b_frames: bool,
    pub gop_size: usize,
}

impl Default for EncodeConfig {
    fn default() -> Self {
        Self {
            base_q_idx: DEFAULT_BASE_Q_IDX,
            keyint: DEFAULT_KEYINT,
            target_bitrate: None,
            fps: 25.0,
            b_frames: false,
            gop_size: 3,
        }
    }
}

pub fn encode_av1_ivf_multi(frames: &[y4m::FramePixels]) -> Vec<u8> {
    encode(frames, &EncodeConfig::default())
}

pub fn encode_av1_ivf_multi_with_quality(frames: &[y4m::FramePixels], base_q_idx: u8) -> Vec<u8> {
    encode(
        frames,
        &EncodeConfig {
            base_q_idx,
            ..Default::default()
        },
    )
}

pub fn encode(frames: &[y4m::FramePixels], config: &EncodeConfig) -> Vec<u8> {
    assert!(!frames.is_empty(), "frames must not be empty");

    let width = frames[0].width;
    let height = frames[0].height;

    for frame in &frames[1..] {
        assert!(
            frame.width == width && frame.height == height,
            "all frames must have the same dimensions"
        );
    }

    let mut enc = Encoder::new(width, height, EncoderConfig::from(config))
        .expect("invalid encoder dimensions");

    let mut output = Vec::new();
    ivf::write_ivf_header(
        &mut output,
        width as u16,
        height as u16,
        frames.len() as u32,
    )
    .unwrap();

    for pixels in frames {
        enc.send_frame(pixels).expect("send_frame failed");
        while let Some(packet) = enc.receive_packet() {
            ivf::write_ivf_frame(&mut output, packet.frame_number, &packet.data).unwrap();
        }
    }

    enc.flush();
    while let Some(packet) = enc.receive_packet() {
        ivf::write_ivf_frame(&mut output, packet.frame_number, &packet.data).unwrap();
    }

    if let Some(stats) = enc.rate_control_stats() {
        eprintln!(
            "Rate control: target={}kbps, avg_qp={}, buffer={}%",
            stats.target_bitrate / 1000,
            stats.avg_qp,
            stats.buffer_fullness_pct
        );
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
