use std::ptr;

use wav1c::packet::FrameType;
use wav1c::y4m::FramePixels;
use wav1c::EncoderConfig;

pub struct Wav1cEncoder {
    inner: wav1c::Encoder,
    headers_cache: Vec<u8>,
}

#[repr(C)]
pub struct Wav1cPacket {
    pub data: *const u8,
    pub size: usize,
    pub frame_number: u64,
    pub is_keyframe: i32,
}

#[repr(C)]
pub struct Wav1cConfig {
    pub base_q_idx: u8,
    pub keyint: usize,
    pub target_bitrate: u64,
    pub fps: f64,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_new(
    width: u32,
    height: u32,
    cfg: *const Wav1cConfig,
) -> *mut Wav1cEncoder {
    if cfg.is_null() {
        return ptr::null_mut();
    }

    let cfg = unsafe { &*cfg };

    let config = EncoderConfig {
        base_q_idx: cfg.base_q_idx,
        keyint: cfg.keyint,
        target_bitrate: if cfg.target_bitrate == 0 {
            None
        } else {
            Some(cfg.target_bitrate)
        },
        fps: cfg.fps,
    };

    match wav1c::Encoder::new(width, height, config) {
        Ok(inner) => Box::into_raw(Box::new(Wav1cEncoder {
            inner,
            headers_cache: Vec::new(),
        })),
        Err(_) => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_free(enc: *mut Wav1cEncoder) {
    if !enc.is_null() {
        drop(unsafe { Box::from_raw(enc) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_headers(
    enc: *mut Wav1cEncoder,
    out_data: *mut *const u8,
) -> usize {
    if enc.is_null() || out_data.is_null() {
        return 0;
    }

    let enc = unsafe { &mut *enc };
    enc.headers_cache = enc.inner.headers();
    unsafe { *out_data = enc.headers_cache.as_ptr() };
    enc.headers_cache.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_send_frame(
    enc: *mut Wav1cEncoder,
    y: *const u8,
    y_len: usize,
    u: *const u8,
    u_len: usize,
    v: *const u8,
    v_len: usize,
) -> i32 {
    if enc.is_null() || y.is_null() || u.is_null() || v.is_null() {
        return -1;
    }

    let enc = unsafe { &mut *enc };

    let frame = unsafe {
        FramePixels {
            y: std::slice::from_raw_parts(y, y_len).to_vec(),
            u: std::slice::from_raw_parts(u, u_len).to_vec(),
            v: std::slice::from_raw_parts(v, v_len).to_vec(),
            width: enc.inner.width(),
            height: enc.inner.height(),
        }
    };

    match enc.inner.send_frame(&frame) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_receive_packet(
    enc: *mut Wav1cEncoder,
) -> *mut Wav1cPacket {
    if enc.is_null() {
        return ptr::null_mut();
    }

    let enc = unsafe { &mut *enc };

    match enc.inner.receive_packet() {
        Some(packet) => {
            let is_keyframe = match packet.frame_type {
                FrameType::Key => 1,
                FrameType::Inter => 0,
            };

            let data_boxed = packet.data.into_boxed_slice();
            let size = data_boxed.len();
            let data_ptr = Box::into_raw(data_boxed) as *const u8;

            Box::into_raw(Box::new(Wav1cPacket {
                data: data_ptr,
                size,
                frame_number: packet.frame_number,
                is_keyframe,
            }))
        }
        None => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_packet_free(pkt: *mut Wav1cPacket) {
    if pkt.is_null() {
        return;
    }

    let pkt = unsafe { Box::from_raw(pkt) };
    if !pkt.data.is_null() {
        unsafe {
            let slice_ptr = std::slice::from_raw_parts_mut(pkt.data as *mut u8, pkt.size);
            drop(Box::from_raw(slice_ptr as *mut [u8]));
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_flush(enc: *mut Wav1cEncoder) {
    if enc.is_null() {
        return;
    }

    let enc = unsafe { &mut *enc };
    enc.inner.flush();
}
