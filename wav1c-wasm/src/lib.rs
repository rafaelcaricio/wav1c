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
        };
        let encoder = wav1c::Encoder::new(width, height, config)
            .map_err(|e| JsError::new(&format!("{:?}", e)))?;
        Ok(WasmEncoder {
            encoder,
            last_keyframe: false,
            last_frame_number: 0,
            last_packet_size: 0,
        })
    }

    pub fn encode_frame(&mut self, y: &[u8], u: &[u8], v: &[u8]) -> Result<Vec<u8>, JsError> {
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
        let packet = self
            .encoder
            .receive_packet()
            .ok_or_else(|| JsError::new("no packet"))?;
        self.last_keyframe = matches!(packet.frame_type, FrameType::Key);
        self.last_frame_number = packet.frame_number;
        self.last_packet_size = packet.data.len();
        Ok(packet.data)
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
