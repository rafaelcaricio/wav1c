use crate::dequant;
use crate::error::EncoderError;
use crate::frame;
use crate::obu;
use crate::packet::{FrameType, Packet};
use crate::rc::RateControl;
use crate::sequence;
use crate::y4m::FramePixels;
use crate::EncodeConfig;

#[derive(Debug)]
pub struct EncoderConfig {
    pub base_q_idx: u8,
    pub keyint: usize,
    pub target_bitrate: Option<u64>,
    pub fps: f64,
}

impl From<&EncodeConfig> for EncoderConfig {
    fn from(c: &EncodeConfig) -> Self {
        Self {
            base_q_idx: c.base_q_idx,
            keyint: c.keyint,
            target_bitrate: c.target_bitrate,
            fps: c.fps,
        }
    }
}

#[derive(Debug)]
pub struct Encoder {
    config: EncoderConfig,
    width: u32,
    height: u32,
    frame_index: u64,
    rate_ctrl: Option<RateControl>,
    reference: Option<FramePixels>,
    pending_packet: Option<Packet>,
}

impl Encoder {
    pub fn new(width: u32, height: u32, config: EncoderConfig) -> Result<Self, EncoderError> {
        if !(1..=4096).contains(&width) || !(1..=2304).contains(&height) {
            return Err(EncoderError::InvalidDimensions { width, height });
        }

        let rate_ctrl = config.target_bitrate.map(|bitrate| {
            RateControl::new(bitrate, config.fps, width, height, config.keyint)
        });

        Ok(Self {
            config,
            width,
            height,
            frame_index: 0,
            rate_ctrl,
            reference: None,
            pending_packet: None,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn headers(&self) -> Vec<u8> {
        let seq = sequence::encode_sequence_header(self.width, self.height);
        obu::obu_wrap(obu::ObuType::SequenceHeader, &seq)
    }

    pub fn send_frame(&mut self, pixels: &FramePixels) -> Result<(), EncoderError> {
        if pixels.width != self.width || pixels.height != self.height {
            return Err(EncoderError::DimensionMismatch {
                expected_w: self.width,
                expected_h: self.height,
                got_w: pixels.width,
                got_h: pixels.height,
            });
        }

        let is_keyframe = self.frame_index.is_multiple_of(self.config.keyint as u64);

        let base_q_idx = match &mut self.rate_ctrl {
            Some(rc) => rc.compute_qp(is_keyframe),
            None => self.config.base_q_idx,
        };
        let dq = dequant::lookup_dequant(base_q_idx);

        let td = obu::obu_wrap(obu::ObuType::TemporalDelimiter, &[]);
        let seq = obu::obu_wrap(
            obu::ObuType::SequenceHeader,
            &sequence::encode_sequence_header(self.width, self.height),
        );

        let (frame_payload, recon) = if is_keyframe {
            frame::encode_frame_with_recon(pixels, base_q_idx, dq)
        } else {
            frame::encode_inter_frame_with_recon(
                pixels,
                self.reference.as_ref().unwrap(),
                0x01,
                0,
                base_q_idx,
                dq,
            )
        };

        self.reference = Some(recon);

        let frm = obu::obu_wrap(obu::ObuType::Frame, &frame_payload);

        if let Some(rc) = &mut self.rate_ctrl {
            rc.update((frm.len() * 8) as u64, base_q_idx);
        }

        let mut data = Vec::new();
        data.extend_from_slice(&td);
        data.extend_from_slice(&seq);
        data.extend_from_slice(&frm);

        let frame_type = if is_keyframe {
            FrameType::Key
        } else {
            FrameType::Inter
        };

        self.pending_packet = Some(Packet {
            data,
            frame_type,
            frame_number: self.frame_index,
        });

        self.frame_index += 1;

        Ok(())
    }

    pub fn receive_packet(&mut self) -> Option<Packet> {
        self.pending_packet.take()
    }

    pub fn flush(&mut self) {}

    pub fn rate_control_stats(&self) -> Option<crate::rc::RateControlStats> {
        self.rate_ctrl.as_ref().map(|rc| rc.stats())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_valid_dimensions() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        let enc = Encoder::new(64, 64, config);
        assert!(enc.is_ok());
        let enc = enc.unwrap();
        assert_eq!(enc.width(), 64);
        assert_eq!(enc.height(), 64);
    }

    #[test]
    fn new_min_dimensions() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        assert!(Encoder::new(1, 1, config).is_ok());
    }

    #[test]
    fn new_max_dimensions() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        assert!(Encoder::new(4096, 2304, config).is_ok());
    }

    #[test]
    fn new_invalid_width_zero() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        let result = Encoder::new(0, 64, config);
        assert!(result.is_err());
        match result.unwrap_err() {
            EncoderError::InvalidDimensions { width, height } => {
                assert_eq!(width, 0);
                assert_eq!(height, 64);
            }
            _ => panic!("expected InvalidDimensions"),
        }
    }

    #[test]
    fn new_invalid_width_too_large() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        assert!(Encoder::new(4097, 64, config).is_err());
    }

    #[test]
    fn new_invalid_height_zero() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        assert!(Encoder::new(64, 0, config).is_err());
    }

    #[test]
    fn new_invalid_height_too_large() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        assert!(Encoder::new(64, 2305, config).is_err());
    }

    #[test]
    fn send_frame_receive_packet_lifecycle() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        assert!(enc.receive_packet().is_none());

        enc.send_frame(&frame).unwrap();
        let packet = enc.receive_packet().unwrap();

        assert_eq!(packet.frame_type, FrameType::Key);
        assert_eq!(packet.frame_number, 0);
        assert!(!packet.data.is_empty());

        assert!(enc.receive_packet().is_none());
    }

    #[test]
    fn first_frame_is_keyframe() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        enc.send_frame(&frame).unwrap();
        let packet = enc.receive_packet().unwrap();
        assert_eq!(packet.frame_type, FrameType::Key);
    }

    #[test]
    fn second_frame_is_inter() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        enc.send_frame(&frame).unwrap();
        enc.receive_packet();

        enc.send_frame(&frame).unwrap();
        let packet = enc.receive_packet().unwrap();
        assert_eq!(packet.frame_type, FrameType::Inter);
        assert_eq!(packet.frame_number, 1);
    }

    #[test]
    fn keyint_triggers_new_keyframe() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 3,
            target_bitrate: None,
            fps: 25.0,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        let expected_types = [
            FrameType::Key,
            FrameType::Inter,
            FrameType::Inter,
            FrameType::Key,
            FrameType::Inter,
        ];

        for expected in &expected_types {
            enc.send_frame(&frame).unwrap();
            let packet = enc.receive_packet().unwrap();
            assert_eq!(&packet.frame_type, expected);
        }
    }

    #[test]
    fn dimension_mismatch_error() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let wrong_frame = FramePixels::solid(128, 128, 128, 128, 128);

        let result = enc.send_frame(&wrong_frame);
        assert!(result.is_err());
        match result.unwrap_err() {
            EncoderError::DimensionMismatch {
                expected_w,
                expected_h,
                got_w,
                got_h,
            } => {
                assert_eq!(expected_w, 64);
                assert_eq!(expected_h, 64);
                assert_eq!(got_w, 128);
                assert_eq!(got_h, 128);
            }
            _ => panic!("expected DimensionMismatch"),
        }
    }

    #[test]
    fn flush_is_callable() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        enc.flush();
        assert!(enc.receive_packet().is_none());
    }

    #[test]
    fn headers_returns_sequence_header_obu() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        let enc = Encoder::new(64, 64, config).unwrap();
        let headers = enc.headers();
        assert_eq!(headers[0], 0x0A);
        assert!(!headers.is_empty());
    }

    #[test]
    fn packet_data_starts_with_temporal_delimiter() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        enc.send_frame(&frame).unwrap();
        let packet = enc.receive_packet().unwrap();

        assert_eq!(packet.data[0], 0x12);
        assert_eq!(packet.data[1], 0x00);
    }

    #[test]
    fn encoder_with_rate_control() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: Some(500_000),
            fps: 25.0,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        enc.send_frame(&frame).unwrap();
        let packet = enc.receive_packet().unwrap();
        assert!(!packet.data.is_empty());

        let stats = enc.rate_control_stats();
        assert!(stats.is_some());
    }

    #[test]
    fn frame_numbers_increment() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: 25.0,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        for expected_num in 0..5u64 {
            enc.send_frame(&frame).unwrap();
            let packet = enc.receive_packet().unwrap();
            assert_eq!(packet.frame_number, expected_num);
        }
    }

    #[test]
    fn encoder_config_from_encode_config() {
        let ec = EncodeConfig {
            base_q_idx: 100,
            keyint: 10,
            target_bitrate: Some(1_000_000),
            fps: 30.0,
        };
        let config: EncoderConfig = (&ec).into();
        assert_eq!(config.base_q_idx, 100);
        assert_eq!(config.keyint, 10);
        assert_eq!(config.target_bitrate, Some(1_000_000));
        assert!((config.fps - 30.0).abs() < f64::EPSILON);
    }
}
