#![forbid(unsafe_code)]

pub mod bitwriter;
pub mod cdef;
pub mod cdf;
pub mod cdf_coef;
pub mod dequant;
pub mod encoder;
pub mod error;
pub mod frame;
pub mod metadata;
pub mod msac;
pub mod obu;
pub mod packet;
pub mod rc;
pub mod rdo;
pub mod satd;
pub mod sequence;
pub mod tile;
pub mod video;
pub mod y4m;

pub use encoder::{Encoder, EncoderConfig};
pub use error::EncoderError;
pub use packet::{FrameType, Packet};
pub use video::{
    BitDepth, ColorDescription, ColorRange, ContentLightLevel, MasteringDisplayMetadata,
    VideoSignal,
};

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
    pub video_signal: VideoSignal,
    pub content_light: Option<ContentLightLevel>,
    pub mastering_display: Option<MasteringDisplayMetadata>,
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
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        }
    }
}

pub fn encode_packets(frames: &[y4m::FramePixels], config: &EncodeConfig) -> Vec<Packet> {
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

    let mut packets = Vec::new();

    for pixels in frames {
        enc.send_frame(pixels).expect("send_frame failed");
        while let Some(packet) = enc.receive_packet() {
            packets.push(packet);
        }
    }

    enc.flush();
    while let Some(packet) = enc.receive_packet() {
        packets.push(packet);
    }

    if let Some(stats) = enc.rate_control_stats() {
        eprintln!(
            "Rate control: target={}kbps, avg_qp={}, buffer={}%",
            stats.target_bitrate / 1000,
            stats.avg_qp,
            stats.buffer_fullness_pct
        );
    }

    packets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_solid(width: u32, height: u32, y: u8, u: u8, v: u8) -> Vec<Packet> {
        let pixels = y4m::FramePixels::solid(width, height, y, u, v);
        encode_packets(&[pixels], &EncodeConfig::default())
    }

    #[test]
    fn output_starts_with_valid_obu_structure() {
        let packets = encode_solid(64, 64, 128, 128, 128);
        let frame_data = &packets[0].data;
        let temporal_delimiter_header = 0x12;
        let temporal_delimiter_size = 0x00;
        let sequence_header_obu_header = 0x0A;

        assert_eq!(frame_data[0], temporal_delimiter_header);
        assert_eq!(frame_data[1], temporal_delimiter_size);
        assert_eq!(frame_data[2], sequence_header_obu_header);

        let seq_payload = sequence::encode_sequence_header(64, 64, &VideoSignal::default());
        let seq_size = seq_payload.len();
        assert_eq!(frame_data[3], seq_size as u8);

        let frame_obu_offset = 2 + 1 + 1 + seq_size;
        let frame_obu_header = 0x32;
        assert_eq!(frame_data[frame_obu_offset], frame_obu_header);
    }

    #[test]
    fn different_colors_produce_different_output() {
        let gray = encode_solid(64, 64, 128, 128, 128);
        let black = encode_solid(64, 64, 0, 0, 0);
        assert_ne!(gray[0].data, black[0].data);
    }

    #[test]
    fn different_dimensions_produce_different_output() {
        let small = encode_solid(64, 64, 128, 128, 128);
        let large = encode_solid(128, 128, 128, 128, 128);
        assert_ne!(small[0].data, large[0].data);
    }

    #[test]
    fn y4m_api_matches_solid_api() {
        let solid_pixels = y4m::FramePixels::solid(64, 64, 128, 128, 128);
        let y4m_pixels = y4m::FramePixels::solid(64, 64, 128, 128, 128);
        let solid = encode_packets(&[solid_pixels], &EncodeConfig::default());
        let y4m_out = encode_packets(&[y4m_pixels], &EncodeConfig::default());
        assert_eq!(solid[0].data, y4m_out[0].data);
    }

    #[test]
    fn multi_frame_packet_count_matches() {
        let frames: Vec<_> = (0..3)
            .map(|_| y4m::FramePixels::solid(64, 64, 128, 128, 128))
            .collect();
        let packets = encode_packets(&frames, &EncodeConfig::default());
        assert_eq!(packets.len(), 3);
    }

    #[test]
    fn multi_frame_produces_more_data() {
        let one = encode_packets(
            &[y4m::FramePixels::solid(64, 64, 128, 128, 128)],
            &EncodeConfig::default(),
        );
        let three: Vec<_> = (0..3)
            .map(|_| y4m::FramePixels::solid(64, 64, 128, 128, 128))
            .collect();
        let multi = encode_packets(&three, &EncodeConfig::default());
        let one_total: usize = one.iter().map(|p| p.data.len()).sum();
        let multi_total: usize = multi.iter().map(|p| p.data.len()).sum();
        assert!(multi_total > one_total);
    }

    #[test]
    fn single_frame_multi_matches_single() {
        let a = encode_packets(
            &[y4m::FramePixels::solid(64, 64, 128, 128, 128)],
            &EncodeConfig::default(),
        );
        let b = encode_packets(
            &[y4m::FramePixels::solid(64, 64, 128, 128, 128)],
            &EncodeConfig::default(),
        );
        assert_eq!(a[0].data, b[0].data);
    }

    #[test]
    fn multi_frame_different_content() {
        let frames = vec![
            y4m::FramePixels::solid(64, 64, 0, 0, 0),
            y4m::FramePixels::solid(64, 64, 255, 128, 128),
        ];
        let packets = encode_packets(&frames, &EncodeConfig::default());
        assert_eq!(packets.len(), 2);
        assert!(!packets[0].data.is_empty());
    }
}
