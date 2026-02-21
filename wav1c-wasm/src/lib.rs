#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;
use wav1c::EncoderConfig;
use wav1c::packet::FrameType;
use wav1c::y4m::FramePixels;

#[wasm_bindgen]
pub struct WasmEncoder {
    encoder: wav1c::Encoder,
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
        };
        Self::create(width, height, config)
    }

    /// Full-featured constructor exposing all encoder parameters.
    ///
    /// - `b_frames`: enable B-frame encoding (requires `gop_size > 1`)
    /// - `gop_size`: mini-GOP size (number of frames per group, e.g. 3 = P + 2×B)
    /// - `fps`: frames per second (used for rate-control)
    /// - `target_bitrate`: optional CBR target in bits/s (pass 0 to disable)
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
        };
        Self::create(width, height, config)
    }

    fn create(width: u32, height: u32, config: EncoderConfig) -> Result<WasmEncoder, JsError> {
        let encoder = wav1c::Encoder::new(width, height, config)
            .map_err(|e| JsError::new(&format!("{:?}", e)))?;
        Ok(WasmEncoder {
            encoder,
            last_keyframe: false,
            last_frame_number: 0,
            last_packet_size: 0,
        })
    }

    /// Send a raw YUV 4:2:0 frame to the encoder.
    ///
    /// When B-frames are **disabled** (default), a packet is available immediately
    /// after each `send_frame` call via `receive_packet`.
    ///
    /// When B-frames are **enabled**, packets are buffered until a full mini-GOP is
    /// complete. Call `flush` at end-of-stream, then drain with `receive_packet`.
    pub fn encode_frame(&mut self, y: &[u8], u: &[u8], v: &[u8]) -> Result<(), JsError> {
        let frame = FramePixels {
            y: y.to_vec(),
            u: u.to_vec(),
            v: v.to_vec(),
            width: self.encoder.width(),
            height: self.encoder.height(),
        };
        self.encoder
            .send_frame(&frame)
            .map_err(|e| JsError::new(&format!("{:?}", e)))?;
        Ok(())
    }

    /// Receive the next encoded packet, if one is available.
    /// Returns `None` (JS `null`) when no packet is ready yet.
    pub fn receive_packet(&mut self) -> Option<Vec<u8>> {
        let packet = self.encoder.receive_packet()?;
        self.last_keyframe = matches!(packet.frame_type, FrameType::Key);
        self.last_frame_number = packet.frame_number;
        self.last_packet_size = packet.data.len();
        Some(packet.data)
    }

    /// Flush any buffered frames (required when B-frames are enabled).
    /// After calling flush, drain all remaining packets with `receive_packet`.
    pub fn flush(&mut self) {
        self.encoder.flush();
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
        self.encoder.width()
    }

    pub fn height(&self) -> u32 {
        self.encoder.height()
    }
}
