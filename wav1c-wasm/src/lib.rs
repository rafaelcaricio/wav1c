#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;
use wav1c::packet::FrameType;
use wav1c::y4m::FramePixels;
use wav1c::{
    BitDepth, ColorDescription, ColorRange, ContentLightLevel, EncoderConfig,
    MasteringDisplayMetadata, VideoSignal,
};

#[wasm_bindgen]
pub struct WasmEncoder {
    encoder: wav1c::Encoder,
    config: EncoderConfig,
    width: u32,
    height: u32,
    frames_submitted: u64,
    last_keyframe: bool,
    last_frame_number: u64,
    last_packet_size: usize,
}

#[wasm_bindgen]
impl WasmEncoder {
    /// Simple constructor — no B-frames, gop_size=1, fps=30. Kept for backwards compatibility.
    #[wasm_bindgen(constructor)]
    pub fn new(
        width: u32,
        height: u32,
        base_q_idx: u8,
        keyint: usize,
    ) -> Result<WasmEncoder, JsError> {
        let config = EncoderConfig {
            base_q_idx,
            keyint,
            target_bitrate: None,
            fps: 30.0,
            b_frames: false,
            gop_size: 1,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        Self::create(width, height, config)
    }

    /// Full-featured constructor exposing all encoder parameters.
    ///
    /// - `b_frames`: enable B-frame encoding (requires `gop_size > 1`)
    /// - `gop_size`: mini-GOP size (number of frames per group, e.g. 3 = P + 2×B)
    /// - `fps`: frames per second (used for rate-control)
    /// - `target_bitrate`: optional CBR target in bits/s (pass 0 to disable)
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_config(
        width: u32,
        height: u32,
        base_q_idx: u8,
        keyint: usize,
        b_frames: bool,
        gop_size: usize,
        fps: f64,
        target_bitrate: u32,
    ) -> Result<WasmEncoder, JsError> {
        let config = EncoderConfig {
            base_q_idx,
            keyint,
            target_bitrate: if target_bitrate == 0 {
                None
            } else {
                Some(target_bitrate as u64)
            },
            fps,
            b_frames,
            gop_size,
            video_signal: VideoSignal::default(),
            content_light: None,
            mastering_display: None,
        };
        Self::create(width, height, config)
    }

    /// Extended constructor with explicit signal + optional HDR metadata.
    ///
    /// `color_range`: 0 = limited, 1 = full
    /// `color_primaries/transfer/matrix`: set all three to -1 to omit color description
    /// `max_cll/max_fall`: set both to 0 to omit CLL metadata
    #[allow(clippy::too_many_arguments)]
    pub fn new_ex(
        width: u32,
        height: u32,
        base_q_idx: u8,
        keyint: usize,
        b_frames: bool,
        gop_size: usize,
        fps: f64,
        target_bitrate: u32,
        bit_depth: u8,
        color_range: u8,
        color_primaries: i16,
        transfer: i16,
        matrix: i16,
        max_cll: u16,
        max_fall: u16,
    ) -> Result<WasmEncoder, JsError> {
        let mut signal = VideoSignal {
            bit_depth: parse_bit_depth(bit_depth)?,
            color_range: parse_color_range(color_range)?,
            color_description: None,
        };

        if color_primaries >= 0 || transfer >= 0 || matrix >= 0 {
            if color_primaries < 0 || transfer < 0 || matrix < 0 {
                return Err(JsError::new(
                    "color_primaries, transfer, and matrix must be all provided or all omitted",
                ));
            }
            signal.color_description = Some(ColorDescription {
                color_primaries: color_primaries as u8,
                transfer_characteristics: transfer as u8,
                matrix_coefficients: matrix as u8,
            });
        }

        let content_light = if max_cll == 0 && max_fall == 0 {
            None
        } else {
            Some(ContentLightLevel {
                max_content_light_level: max_cll,
                max_frame_average_light_level: max_fall,
            })
        };

        let config = EncoderConfig {
            base_q_idx,
            keyint,
            target_bitrate: if target_bitrate == 0 {
                None
            } else {
                Some(target_bitrate as u64)
            },
            fps,
            b_frames,
            gop_size,
            video_signal: signal,
            content_light,
            mastering_display: None,
        };
        Self::create(width, height, config)
    }

    fn create(width: u32, height: u32, config: EncoderConfig) -> Result<WasmEncoder, JsError> {
        let encoder = wav1c::Encoder::new(width, height, config.clone())
            .map_err(|e| JsError::new(&format!("{:?}", e)))?;
        Ok(WasmEncoder {
            encoder,
            config,
            width,
            height,
            frames_submitted: 0,
            last_keyframe: false,
            last_frame_number: 0,
            last_packet_size: 0,
        })
    }

    /// Send a raw 8-bit YUV 4:2:0 frame to the encoder.
    pub fn encode_frame(&mut self, y: &[u8], u: &[u8], v: &[u8]) -> Result<(), JsError> {
        self.validate_plane_lengths(y.len(), u.len(), v.len())?;

        let frame = FramePixels {
            y: y.iter().map(|&s| s as u16).collect(),
            u: u.iter().map(|&s| s as u16).collect(),
            v: v.iter().map(|&s| s as u16).collect(),
            width: self.width,
            height: self.height,
            bit_depth: BitDepth::Eight,
            color_range: self.config.video_signal.color_range,
        };
        self.encoder
            .send_frame(&frame)
            .map_err(|e| JsError::new(&format!("{:?}", e)))?;
        self.frames_submitted += 1;
        Ok(())
    }

    /// Send a raw 10-bit YUV 4:2:0 frame to the encoder.
    pub fn encode_frame_10bit(&mut self, y: &[u16], u: &[u16], v: &[u16]) -> Result<(), JsError> {
        self.validate_plane_lengths(y.len(), u.len(), v.len())?;

        let frame = FramePixels {
            y: y.to_vec(),
            u: u.to_vec(),
            v: v.to_vec(),
            width: self.width,
            height: self.height,
            bit_depth: BitDepth::Ten,
            color_range: self.config.video_signal.color_range,
        };
        self.encoder
            .send_frame(&frame)
            .map_err(|e| JsError::new(&format!("{:?}", e)))?;
        self.frames_submitted += 1;
        Ok(())
    }

    pub fn receive_packet(&mut self) -> Option<Vec<u8>> {
        let packet = self.encoder.receive_packet()?;
        self.last_keyframe = matches!(packet.frame_type, FrameType::Key);
        self.last_frame_number = packet.frame_number;
        self.last_packet_size = packet.data.len();
        Some(packet.data)
    }

    pub fn flush(&mut self) {
        self.encoder.flush();
    }

    /// Apply HDR10 defaults (BT.2020 + PQ + BT.2020NC) before first frame.
    pub fn set_hdr10(&mut self, color_range: u8) -> Result<(), JsError> {
        self.ensure_not_started()?;
        self.config.video_signal = VideoSignal::hdr10(parse_color_range(color_range)?);
        self.recreate_encoder()
    }

    /// Configure explicit signal fields before first frame.
    ///
    /// `color_range`: 0 = limited, 1 = full
    /// `color_primaries/transfer/matrix`: set all three to -1 to omit color description
    pub fn set_video_signal(
        &mut self,
        bit_depth: u8,
        color_range: u8,
        color_primaries: i16,
        transfer: i16,
        matrix: i16,
    ) -> Result<(), JsError> {
        self.ensure_not_started()?;
        let mut signal = VideoSignal {
            bit_depth: parse_bit_depth(bit_depth)?,
            color_range: parse_color_range(color_range)?,
            color_description: None,
        };

        if color_primaries >= 0 || transfer >= 0 || matrix >= 0 {
            if color_primaries < 0 || transfer < 0 || matrix < 0 {
                return Err(JsError::new(
                    "color_primaries, transfer, and matrix must be all provided or all omitted",
                ));
            }
            signal.color_description = Some(ColorDescription {
                color_primaries: color_primaries as u8,
                transfer_characteristics: transfer as u8,
                matrix_coefficients: matrix as u8,
            });
        }

        self.config.video_signal = signal;
        self.recreate_encoder()
    }

    pub fn set_content_light_level(&mut self, max_cll: u16, max_fall: u16) -> Result<(), JsError> {
        self.ensure_not_started()?;
        self.config.content_light = Some(ContentLightLevel {
            max_content_light_level: max_cll,
            max_frame_average_light_level: max_fall,
        });
        self.recreate_encoder()
    }

    pub fn clear_content_light_level(&mut self) -> Result<(), JsError> {
        self.ensure_not_started()?;
        self.config.content_light = None;
        self.recreate_encoder()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_mastering_display_metadata(
        &mut self,
        rx: u16,
        ry: u16,
        gx: u16,
        gy: u16,
        bx: u16,
        by: u16,
        wx: u16,
        wy: u16,
        max_luminance: u32,
        min_luminance: u32,
    ) -> Result<(), JsError> {
        self.ensure_not_started()?;
        self.config.mastering_display = Some(MasteringDisplayMetadata {
            primaries: [[rx, ry], [gx, gy], [bx, by]],
            white_point: [wx, wy],
            max_luminance,
            min_luminance,
        });
        self.recreate_encoder()
    }

    pub fn clear_mastering_display_metadata(&mut self) -> Result<(), JsError> {
        self.ensure_not_started()?;
        self.config.mastering_display = None;
        self.recreate_encoder()
    }

    pub fn is_keyframe(&self) -> bool {
        self.last_keyframe
    }

    pub fn frame_number(&self) -> u64 {
        self.last_frame_number
    }

    pub fn last_packet_size(&self) -> usize {
        self.last_packet_size
    }

    pub fn sequence_header(&self) -> Vec<u8> {
        self.encoder.headers()
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    fn recreate_encoder(&mut self) -> Result<(), JsError> {
        self.encoder = wav1c::Encoder::new(self.width, self.height, self.config.clone())
            .map_err(|e| JsError::new(&format!("{:?}", e)))?;
        self.last_keyframe = false;
        self.last_frame_number = 0;
        self.last_packet_size = 0;
        Ok(())
    }

    fn ensure_not_started(&self) -> Result<(), JsError> {
        if self.frames_submitted > 0 {
            Err(JsError::new(
                "signal/metadata can only be changed before the first frame",
            ))
        } else {
            Ok(())
        }
    }

    fn validate_plane_lengths(&self, y: usize, u: usize, v: usize) -> Result<(), JsError> {
        let expected_y = (self.width * self.height) as usize;
        let expected_uv = self.width.div_ceil(2) as usize * self.height.div_ceil(2) as usize;
        if y != expected_y || u != expected_uv || v != expected_uv {
            return Err(JsError::new(&format!(
                "invalid plane lengths: expected y={}, u={}, v={}, got y={}, u={}, v={}",
                expected_y, expected_uv, expected_uv, y, u, v
            )));
        }
        Ok(())
    }
}

fn parse_bit_depth(v: u8) -> Result<BitDepth, JsError> {
    BitDepth::from_u8(v).ok_or_else(|| JsError::new("bit_depth must be 8 or 10"))
}

fn parse_color_range(v: u8) -> Result<ColorRange, JsError> {
    match v {
        0 => Ok(ColorRange::Limited),
        1 => Ok(ColorRange::Full),
        _ => Err(JsError::new("color_range must be 0 (limited) or 1 (full)")),
    }
}
