#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;
use wav1c::packet::FrameType;
use wav1c::y4m::FramePixels;
use wav1c::{
    BitDepth, ColorDescription, ColorRange, ContentLightLevel, EncoderConfig,
    MasteringDisplayMetadata, VideoSignal,
};

#[wasm_bindgen]
pub struct WasmRateControlStats {
    target_bitrate: u64,
    frames_encoded: u64,
    buffer_fullness_pct: u32,
    avg_qp: u8,
}

#[wasm_bindgen]
impl WasmRateControlStats {
    #[wasm_bindgen(getter)]
    pub fn target_bitrate(&self) -> u64 {
        self.target_bitrate
    }

    #[wasm_bindgen(getter)]
    pub fn frames_encoded(&self) -> u64 {
        self.frames_encoded
    }

    #[wasm_bindgen(getter)]
    pub fn buffer_fullness_pct(&self) -> u32 {
        self.buffer_fullness_pct
    }

    #[wasm_bindgen(getter)]
    pub fn avg_qp(&self) -> u8 {
        self.avg_qp
    }
}

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
    /// Canonical constructor.
    ///
    /// - `target_bitrate`: bits/s (`0` disables rate control)
    /// - `color_range`: `0` limited, `1` full
    /// - `color_primaries/transfer/matrix`: set all three to `-1` to omit color description
    /// - `has_cll`: when false, `max_cll/max_fall` must both be zero
    #[wasm_bindgen(constructor)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        width: u32,
        height: u32,
        base_q_idx: u8,
        keyint: usize,
        b_frames: bool,
        gop_size: usize,
        fps: f64,
        target_bitrate: u64,
        bit_depth: u8,
        color_range: u8,
        color_primaries: i16,
        transfer: i16,
        matrix: i16,
        has_cll: bool,
        max_cll: u16,
        max_fall: u16,
    ) -> Result<WasmEncoder, JsError> {
        let signal = VideoSignal {
            bit_depth: parse_bit_depth(bit_depth)?,
            color_range: parse_color_range(color_range)?,
            color_description: parse_color_description(color_primaries, transfer, matrix)?,
        };
        let content_light = parse_content_light(has_cll, max_cll, max_fall)?;

        let config = EncoderConfig {
            base_q_idx,
            keyint,
            target_bitrate: if target_bitrate == 0 {
                None
            } else {
                Some(target_bitrate)
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
            .map_err(|e| JsError::new(&e.to_string()))?;
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
            .map_err(|e| JsError::new(&e.to_string()))?;
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
            .map_err(|e| JsError::new(&e.to_string()))?;
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
        self.config.video_signal = VideoSignal {
            bit_depth: parse_bit_depth(bit_depth)?,
            color_range: parse_color_range(color_range)?,
            color_description: parse_color_description(color_primaries, transfer, matrix)?,
        };
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

    pub fn rate_control_stats(&self) -> Option<WasmRateControlStats> {
        self.encoder
            .rate_control_stats()
            .map(|stats| WasmRateControlStats {
                target_bitrate: stats.target_bitrate,
                frames_encoded: stats.frames_encoded,
                buffer_fullness_pct: stats.buffer_fullness_pct,
                avg_qp: stats.avg_qp,
            })
    }

    fn recreate_encoder(&mut self) -> Result<(), JsError> {
        self.encoder = wav1c::Encoder::new(self.width, self.height, self.config.clone())
            .map_err(|e| JsError::new(&e.to_string()))?;
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
        let expected_y = (self.width as usize)
            .checked_mul(self.height as usize)
            .ok_or_else(|| JsError::new("plane dimensions overflowed"))?;
        let expected_uv = (self.width.div_ceil(2) as usize)
            .checked_mul(self.height.div_ceil(2) as usize)
            .ok_or_else(|| JsError::new("plane dimensions overflowed"))?;
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

fn parse_code_point(name: &str, v: i16) -> Result<u8, JsError> {
    if (0..=u8::MAX as i16).contains(&v) {
        Ok(v as u8)
    } else {
        Err(JsError::new(&format!("{name} must be in 0..=255")))
    }
}

fn parse_color_description(
    color_primaries: i16,
    transfer: i16,
    matrix: i16,
) -> Result<Option<ColorDescription>, JsError> {
    let values = [color_primaries, transfer, matrix];
    if values.iter().any(|&v| v < -1) {
        return Err(JsError::new(
            "color_primaries, transfer, and matrix must be -1 or in 0..=255",
        ));
    }
    let provided = values.iter().filter(|&&v| v >= 0).count();
    match provided {
        0 => Ok(None),
        3 => Ok(Some(ColorDescription {
            color_primaries: parse_code_point("color_primaries", color_primaries)?,
            transfer_characteristics: parse_code_point("transfer", transfer)?,
            matrix_coefficients: parse_code_point("matrix", matrix)?,
        })),
        _ => Err(JsError::new(
            "color_primaries, transfer, and matrix must be all provided or all omitted",
        )),
    }
}

fn parse_content_light(
    has_cll: bool,
    max_cll: u16,
    max_fall: u16,
) -> Result<Option<ContentLightLevel>, JsError> {
    if !has_cll {
        if max_cll != 0 || max_fall != 0 {
            return Err(JsError::new("max_cll/max_fall require has_cll=true"));
        }
        return Ok(None);
    }
    Ok(Some(ContentLightLevel {
        max_content_light_level: max_cll,
        max_frame_average_light_level: max_fall,
    }))
}
