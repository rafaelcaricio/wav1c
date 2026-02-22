use crate::EncodeConfig;
use crate::dequant;
use crate::error::EncoderError;
use crate::fps::Fps;
use crate::frame;
use crate::metadata;
use crate::obu;
use crate::packet::{FrameType, Packet};
use crate::rc::RateControl;
use crate::sequence;
use crate::video::{ContentLightLevel, MasteringDisplayMetadata, VideoSignal};
use crate::y4m::FramePixels;

const MAX_AV1_FRAME_DIMENSION: u32 = 1 << 16;

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub base_q_idx: u8,
    pub keyint: usize,
    pub target_bitrate: Option<u64>,
    pub fps: Fps,
    pub b_frames: bool,
    pub gop_size: usize,
    pub video_signal: VideoSignal,
    pub content_light: Option<ContentLightLevel>,
    pub mastering_display: Option<MasteringDisplayMetadata>,
}

impl From<&EncodeConfig> for EncoderConfig {
    fn from(c: &EncodeConfig) -> Self {
        Self {
            base_q_idx: c.base_q_idx,
            keyint: c.keyint,
            target_bitrate: c.target_bitrate,
            fps: c.fps,
            b_frames: c.b_frames,
            gop_size: c.gop_size,
            video_signal: c.video_signal,
            content_light: c.content_light,
            mastering_display: c.mastering_display,
        }
    }
}

#[derive(Debug)]
pub struct Encoder {
    config: EncoderConfig,
    width: u32,
    height: u32,
    sequence_level_idx: u8,
    frame_index: u64,
    rate_ctrl: Option<RateControl>,
    reference: Option<FramePixels>,

    // Tracks monotonically increasing IVF timestamps

    // Ping-pong buffer index for reference frames (0 and 1)
    base_slot: u8,

    // Mini-GOP Buffering
    // Stores (frame_index, frame_pixels)
    gop_queue: Vec<(u64, FramePixels)>,

    // Output queue
    pending_packets: std::collections::VecDeque<Packet>,
}

impl Encoder {
    pub fn new(width: u32, height: u32, config: EncoderConfig) -> Result<Self, EncoderError> {
        if width == 0
            || height == 0
            || width > MAX_AV1_FRAME_DIMENSION
            || height > MAX_AV1_FRAME_DIMENSION
        {
            return Err(EncoderError::InvalidDimensions { width, height });
        }

        preflight_frame_buffer_reserve(width, height)?;

        if (config.content_light.is_some() || config.mastering_display.is_some())
            && config.video_signal.bit_depth.bits() != 10
        {
            return Err(EncoderError::InvalidHdrMetadata {
                reason: "HDR metadata requires 10-bit signal",
            });
        }

        if (config.content_light.is_some() || config.mastering_display.is_some())
            && config.video_signal.color_description.is_none()
        {
            return Err(EncoderError::InvalidHdrMetadata {
                reason: "HDR metadata requires color description signaling",
            });
        }

        let rate_ctrl = config
            .target_bitrate
            .map(|bitrate| RateControl::new(bitrate, config.fps, width, height, config.keyint));

        Ok(Self {
            sequence_level_idx: sequence::derive_sequence_level_idx(width, height, config.fps),
            config,
            width,
            height,
            frame_index: 0,
            rate_ctrl,
            reference: None,
            base_slot: 0,
            gop_queue: Vec::with_capacity(4),
            pending_packets: std::collections::VecDeque::new(),
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn headers(&self) -> Vec<u8> {
        let seq = sequence::encode_sequence_header_with_level(
            self.width,
            self.height,
            &self.config.video_signal,
            self.sequence_level_idx,
        );
        let mut out = obu::obu_wrap(obu::ObuType::SequenceHeader, &seq);
        for m in self.metadata_obus() {
            out.extend_from_slice(&m);
        }
        out
    }

    fn metadata_obus(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if let Some(cll) = self.config.content_light {
            let payload = metadata::encode_hdr_cll(&cll);
            out.push(obu::obu_wrap(obu::ObuType::Metadata, &payload));
        }
        if let Some(mdcv) = self.config.mastering_display {
            let payload = metadata::encode_hdr_mdcv(&mdcv);
            out.push(obu::obu_wrap(obu::ObuType::Metadata, &payload));
        }
        out
    }

    fn temporal_unit_headers(&self) -> Vec<u8> {
        let td = obu::obu_wrap(obu::ObuType::TemporalDelimiter, &[]);
        let seq = obu::obu_wrap(
            obu::ObuType::SequenceHeader,
            &sequence::encode_sequence_header_with_level(
                self.width,
                self.height,
                &self.config.video_signal,
                self.sequence_level_idx,
            ),
        );
        let mut out = Vec::new();
        out.extend_from_slice(&td);
        out.extend_from_slice(&seq);
        for m in self.metadata_obus() {
            out.extend_from_slice(&m);
        }
        out
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
        let expected = self.config.video_signal.bit_depth.bits();
        let got = pixels.bit_depth.bits();
        if expected != got {
            return Err(EncoderError::FrameBitDepthMismatch { expected, got });
        }
        let max_value = self.config.video_signal.bit_depth.max_value();
        if let Some(sample) = pixels
            .y
            .iter()
            .chain(pixels.u.iter())
            .chain(pixels.v.iter())
            .copied()
            .find(|&s| s > max_value)
        {
            return Err(EncoderError::SampleOutOfRange {
                bit_depth: expected,
                sample,
            });
        }

        self.gop_queue.push((self.frame_index, pixels.clone()));
        self.frame_index += 1;

        // When B-frames are disabled, encode each frame immediately (lowest latency).
        // When B-frames are enabled, batch into mini-GOPs of gop_size.
        if !self.config.b_frames || self.gop_queue.len() >= self.config.gop_size {
            self.encode_gop();
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_single_frame(
        &mut self,
        index: u64,
        pixels: &FramePixels,
        fwd_ref: Option<&FramePixels>,
        refresh_frame_flags: u8,
        ref_slot: u8,
        bwd_ref_slot: u8,
        show_frame: bool,
    ) -> (Packet, FramePixels) {
        self.encode_single_frame_qidx(
            index,
            pixels,
            fwd_ref,
            refresh_frame_flags,
            ref_slot,
            bwd_ref_slot,
            show_frame,
            None,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_single_frame_qidx(
        &mut self,
        index: u64,
        pixels: &FramePixels,
        fwd_ref: Option<&FramePixels>,
        refresh_frame_flags: u8,
        ref_slot: u8,
        bwd_ref_slot: u8,
        show_frame: bool,
        override_q_idx: Option<u8>,
        emit_tu_headers: bool,
    ) -> (Packet, FramePixels) {
        let is_keyframe = index == 0
            || (self.config.keyint > 0 && index.is_multiple_of(self.config.keyint as u64))
            || self.reference.is_none();

        let base_q_idx = if let Some(q) = override_q_idx {
            q
        } else {
            match &mut self.rate_ctrl {
                Some(rc) => rc.compute_qp(is_keyframe),
                None => self.config.base_q_idx,
            }
        };
        let dq = dequant::lookup_dequant(base_q_idx, self.config.video_signal.bit_depth);

        let (frame_payload, recon) = if is_keyframe {
            frame::encode_frame_with_recon(pixels, base_q_idx, dq)
        } else {
            frame::encode_inter_frame_with_recon(
                pixels,
                self.reference.as_ref().unwrap(),
                fwd_ref,
                refresh_frame_flags,
                ref_slot,
                bwd_ref_slot,
                show_frame,
                base_q_idx,
                dq,
            )
        };
        let frm = obu::obu_wrap(obu::ObuType::Frame, &frame_payload);

        if let Some(rc) = &mut self.rate_ctrl {
            rc.update((frm.len() * 8) as u64, base_q_idx);
        }

        let mut data = Vec::new();
        if emit_tu_headers {
            data.extend_from_slice(&self.temporal_unit_headers());
        }
        data.extend_from_slice(&frm);

        let packet = Packet {
            data,
            frame_type: if is_keyframe {
                FrameType::Key
            } else {
                FrameType::Inter
            },
            frame_number: index,
        };

        (packet, recon)
    }

    fn encode_gop(&mut self) {
        if self.gop_queue.is_empty() {
            return;
        }

        // P-only fast path: when B-frames are disabled, encode each frame
        // as a standard shown P-frame (or keyframe) with refresh_frame_flags=0xFF
        if !self.config.b_frames {
            while !self.gop_queue.is_empty() {
                let (idx, pixels) = self.gop_queue.remove(0);
                let (mut pkt, recon) =
                    self.encode_single_frame(idx, &pixels, None, 0xFF, 0, 0, true);
                self.reference = Some(recon);

                // P-Only Output: Map exactly to the frame index (PTS)
                pkt.frame_number = idx;

                self.pending_packets.push_back(pkt);
            }
            return;
        }

        if self.gop_queue.len() == 1 {
            let (idx, pixels) = self.gop_queue.remove(0);
            self.base_slot = 0;
            let (mut pkt, recon) =
                self.encode_single_frame(idx, &pixels, None, 1 << self.base_slot, 0, 0, true);
            self.reference = Some(recon);

            // Single Fragment Output: Map exactly to the frame index (PTS)
            pkt.frame_number = idx;

            self.pending_packets.push_back(pkt);
            return;
        }

        // If the gop_queue begins with a keyframe or a frame where self.reference is missing,
        // we MUST encode it first to establish the baseline reference for the rest of the GOP!
        let mut base_packets = Vec::new();
        while !self.gop_queue.is_empty() {
            let (first_idx, _) = &self.gop_queue[0];
            let is_keyframe = *first_idx == 0
                || (self.config.keyint > 0 && first_idx.is_multiple_of(self.config.keyint as u64))
                || self.reference.is_none();

            if is_keyframe {
                let (idx, pixels) = self.gop_queue.remove(0);
                self.base_slot = 0; // Reset ping-pong on keyframe
                let (mut pkt, recon) =
                    self.encode_single_frame(idx, &pixels, None, 1 << self.base_slot, 0, 0, true);
                self.reference = Some(recon);

                // Keyframe Output: Map exactly to the frame index (PTS)
                pkt.frame_number = idx;

                base_packets.push(pkt);
            } else {
                break;
            }
        }

        // Output the base packets (e.g. keyframes) that were just encoded
        for pkt in base_packets {
            self.pending_packets.push_back(pkt);
        }

        if self.gop_queue.is_empty() {
            return;
        }

        // For the remaining GOP frames, encode the LAST frame (future reference) as a standard P-Frame
        let last_idx = self.gop_queue.len() - 1;
        let (f_idx, f_pixels) = self.gop_queue.remove(last_idx);

        // P-Frame writes to the alt slot
        let alt_slot = 1 - self.base_slot;
        // P-Frame is NOT shown immediately
        let (p_pkt, fwd_recon) = self.encode_single_frame(
            f_idx,
            &f_pixels,
            None,
            1 << alt_slot,
            self.base_slot,
            alt_slot,
            false,
        );

        // Encode intermediate frames as B-frames
        let mut b_packets = Vec::new();
        let b_frame_q_idx = self.config.base_q_idx.saturating_add(16); // Lower quality for B-frames to save bits
        while !self.gop_queue.is_empty() {
            let (idx, b_pixels) = self.gop_queue.remove(0);
            // They use the newly created fwd_recon as their future reference
            // B-Frames do not refresh any slots (0x00) and ARE shown immediately
            let (b_pkt, _) = self.encode_single_frame_qidx(
                idx,
                &b_pixels,
                Some(&fwd_recon),
                0x00,
                self.base_slot,
                alt_slot,
                true,
                Some(b_frame_q_idx),
                false,
            );
            b_packets.push(b_pkt);
        }

        // The decoder expects frames in display order to be reordered temporarily, but we are
        // writing IVF which strictly expects decode order.
        // So we output the P-frame *first*, then the B-frames that depend on it.
        // Note: Real AV1 uses `show_existing_frame` flags for display ordering. Our simple payload will write P then B.
        // Wait, IVF players expect presentation order to match file order.
        // Because we don't write `show_existing_frame` OBU packets yet, we cannot actually reorder the bitstream.
        // We will output them in display order (B then P) but pass the future reference into the B-frame encoder perfectly.

        // Actually, if we output B then P, the decoder hasn't seen P yet to decode B!
        // We *MUST* write P then B, and then write an empty `show_existing_frame=P` packet.
        // For now, to keep the test simple and valid, we will output in strict decode order.

        if !b_packets.is_empty() {
            let mut first_b = b_packets.remove(0);

            // The P-frame has TU headers [TD, SEQ].
            // The B-frame has NO TU headers (because we passed emit_tu_headers=false).
            // We concatenate P-frame data with B-frame data, creating a single chunk (TU) with TWO frames!
            let mut combined_data = p_pkt.data;
            combined_data.extend_from_slice(&first_b.data);

            first_b.data = combined_data;
            // The display order is first_b.frame_number. So this combined packet has the DTS/PTS of the B-frame!
            self.pending_packets.push_back(first_b);
        } else {
            // If no B-frames (e.g. gop size was reached exactly?), just push the P-frame.
            // But P-frames are only created if gop_queue.is_empty() is false, so B-frames exist.
            self.pending_packets.push_back(p_pkt);
        }

        // Then output remaining B-frames with their original display-order indices
        // Since they had emit_tu_headers=false, we MUST prepend TU headers to them!
        for mut b_pkt in b_packets {
            let mut tu_data = self.temporal_unit_headers();
            tu_data.extend_from_slice(&b_pkt.data);
            b_pkt.data = tu_data;
            self.pending_packets.push_back(b_pkt);
        }

        // Output show_existing_frame to display the hidden P-frame at its correct position
        let show_hdr = obu::obu_wrap(
            obu::ObuType::FrameHeader,
            &frame::encode_show_existing_frame(alt_slot),
        );

        let mut show_pkt_data = self.temporal_unit_headers();
        show_pkt_data.extend_from_slice(&show_hdr);

        let show_pkt = Packet {
            data: show_pkt_data,
            frame_type: FrameType::Inter,
            frame_number: f_idx, // Same display time as the P-frame it reveals
        };
        self.pending_packets.push_back(show_pkt);

        self.reference = Some(fwd_recon);
        // The newly encoded P-frame becomes the base for the next GOP
        self.base_slot = alt_slot;
    }

    pub fn receive_packet(&mut self) -> Option<Packet> {
        self.pending_packets.pop_front()
    }

    pub fn flush(&mut self) {
        self.encode_gop();
    }

    pub fn rate_control_stats(&self) -> Option<crate::rc::RateControlStats> {
        self.rate_ctrl.as_ref().map(|rc| rc.stats())
    }
}

fn preflight_frame_buffer_reserve(width: u32, height: u32) -> Result<(), EncoderError> {
    let fail = |reason: String| EncoderError::AllocationPreflightFailed {
        width,
        height,
        reason,
    };

    let luma_samples = width
        .checked_mul(height)
        .ok_or_else(|| fail("luma sample count overflow".to_owned()))?;
    let chroma_samples = width
        .div_ceil(2)
        .checked_mul(height.div_ceil(2))
        .ok_or_else(|| fail("chroma sample count overflow".to_owned()))?;
    let total_samples_per_frame = u64::from(luma_samples) + 2 * u64::from(chroma_samples);
    let total_samples_reserve = total_samples_per_frame
        .checked_mul(2)
        .ok_or_else(|| fail("frame reserve sample count overflow".to_owned()))?;
    let reserve_elems = usize::try_from(total_samples_reserve)
        .map_err(|_| fail("frame reserve sample count does not fit platform usize".to_owned()))?;

    let mut preflight = Vec::<u16>::new();
    preflight.try_reserve_exact(reserve_elems).map_err(|e| {
        fail(format!(
            "unable to reserve {} u16 samples for frame buffers: {}",
            reserve_elems, e
        ))
    })?;

    Ok(())
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
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
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
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        assert!(Encoder::new(1, 1, config).is_ok());
    }

    #[test]
    fn new_above_old_dimension_cap_is_valid() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        assert!(Encoder::new(4097, 2305, config).is_ok());
    }

    #[test]
    fn new_invalid_width_zero() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
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
    fn new_extremely_large_dimensions_fail_preflight() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        let result = Encoder::new(MAX_AV1_FRAME_DIMENSION, MAX_AV1_FRAME_DIMENSION, config);
        assert!(result.is_err());
        match result.unwrap_err() {
            EncoderError::AllocationPreflightFailed { .. } => {}
            other => panic!("expected AllocationPreflightFailed, got {other:?}"),
        }
    }

    #[test]
    fn new_width_above_av1_sequence_header_limit_is_invalid() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        let result = Encoder::new(MAX_AV1_FRAME_DIMENSION + 1, 64, config);
        assert!(result.is_err());
        match result.unwrap_err() {
            EncoderError::InvalidDimensions { width, height } => {
                assert_eq!(width, MAX_AV1_FRAME_DIMENSION + 1);
                assert_eq!(height, 64);
            }
            other => panic!("expected InvalidDimensions, got {other:?}"),
        }
    }

    #[test]
    fn new_height_above_av1_sequence_header_limit_is_invalid() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        let result = Encoder::new(64, MAX_AV1_FRAME_DIMENSION + 1, config);
        assert!(result.is_err());
        match result.unwrap_err() {
            EncoderError::InvalidDimensions { width, height } => {
                assert_eq!(width, 64);
                assert_eq!(height, MAX_AV1_FRAME_DIMENSION + 1);
            }
            other => panic!("expected InvalidDimensions, got {other:?}"),
        }
    }

    #[test]
    fn new_invalid_height_zero() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        assert!(Encoder::new(64, 0, config).is_err());
    }

    #[test]
    fn new_height_above_old_cap_is_valid() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        assert!(Encoder::new(64, 2305, config).is_ok());
    }

    #[test]
    fn send_frame_receive_packet_lifecycle() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        assert!(enc.receive_packet().is_none());

        enc.send_frame(&frame).unwrap();
        enc.flush();
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
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        enc.send_frame(&frame).unwrap();
        enc.flush();
        let packet = enc.receive_packet().unwrap();
        assert_eq!(packet.frame_type, FrameType::Key);
    }

    #[test]
    fn second_frame_is_inter() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        enc.send_frame(&frame).unwrap();
        enc.send_frame(&frame).unwrap();
        enc.flush();

        // We expect Frame 0 (Key), then Frame 1 (P-frame, emitted as end of GOP=1)
        let _key_packet = enc.receive_packet().unwrap();
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
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
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

        for _ in 0..expected_types.len() {
            enc.send_frame(&frame).unwrap();
        }
        enc.flush();

        // Drain the output block
        let mut actual_types = Vec::new();
        while let Some(packet) = enc.receive_packet() {
            actual_types.push((packet.frame_number, packet.frame_type));
        }
        // Since GOP emits out of order (P then B), we sort by frame number to verify correctness
        actual_types.sort_by_key(|a| a.0);

        for (i, expected) in expected_types.iter().enumerate() {
            assert_eq!(&actual_types[i].1, expected);
        }
    }

    #[test]
    fn dimension_mismatch_error() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
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
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
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
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
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
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        enc.send_frame(&frame).unwrap();
        enc.flush();
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
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        enc.send_frame(&frame).unwrap();
        enc.flush();
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
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        let mut enc = Encoder::new(64, 64, config).unwrap();
        let frame = FramePixels::solid(64, 64, 128, 128, 128);

        for _ in 0..5 {
            enc.send_frame(&frame).unwrap();
        }
        enc.flush();

        let mut actual_nums = Vec::new();
        while let Some(packet) = enc.receive_packet() {
            actual_nums.push(packet.frame_number);
        }
        actual_nums.sort_unstable();

        for expected_num in 0..5u64 {
            assert_eq!(actual_nums[expected_num as usize], expected_num);
        }
    }

    #[test]
    fn encoder_config_from_encode_config() {
        let ec = EncodeConfig {
            base_q_idx: 100,
            keyint: 10,
            target_bitrate: Some(1_000_000),
            fps: Fps::from_int(30).unwrap(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        let config: EncoderConfig = (&ec).into();
        assert_eq!(config.base_q_idx, 100);
        assert_eq!(config.keyint, 10);
        assert_eq!(config.target_bitrate, Some(1_000_000));
        assert_eq!(config.fps, Fps::from_int(30).unwrap());
    }

    #[test]
    fn hdr_metadata_requires_10bit_signal() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal::default(),
            content_light: Some(ContentLightLevel {
                max_content_light_level: 1000,
                max_frame_average_light_level: 400,
            }),
            mastering_display: None,
        };
        let err = Encoder::new(64, 64, config).unwrap_err();
        assert!(matches!(err, EncoderError::InvalidHdrMetadata { .. }));
    }

    #[test]
    fn hdr_metadata_requires_color_description() {
        let config = EncoderConfig {
            base_q_idx: 128,
            keyint: 25,
            target_bitrate: None,
            fps: Fps::default(),
            b_frames: false,
            gop_size: 3,
            video_signal: VideoSignal {
                bit_depth: crate::BitDepth::Ten,
                color_range: crate::ColorRange::Limited,
                color_description: None,
            },
            content_light: Some(ContentLightLevel {
                max_content_light_level: 1000,
                max_frame_average_light_level: 400,
            }),
            mastering_display: None,
        };
        let err = Encoder::new(64, 64, config).unwrap_err();
        assert!(matches!(err, EncoderError::InvalidHdrMetadata { .. }));
    }
}
