#![allow(clippy::missing_safety_doc)]

use std::ptr;

use wav1c::EncoderConfig;
use wav1c::packet::FrameType;
use wav1c::video::{
    BitDepth, ColorDescription, ColorRange, ContentLightLevel, MasteringDisplayMetadata,
    VideoSignal,
};
use wav1c::y4m::FramePixels;

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
    pub b_frames: i32,
    pub gop_size: usize,
}

#[repr(C)]
pub struct Wav1cConfigEx {
    pub base_q_idx: u8,
    pub keyint: usize,
    pub target_bitrate: u64,
    pub fps: f64,
    pub b_frames: i32,
    pub gop_size: usize,
    pub bit_depth: u8,
    pub color_range: i32,              // 0 limited, 1 full
    pub color_primaries: i32,          // -1 unset
    pub transfer_characteristics: i32, // -1 unset
    pub matrix_coefficients: i32,      // -1 unset
    pub has_cll: i32,
    pub max_cll: u16,
    pub max_fall: u16,
    pub has_mdcv: i32,
    pub red_x: u16,
    pub red_y: u16,
    pub green_x: u16,
    pub green_y: u16,
    pub blue_x: u16,
    pub blue_y: u16,
    pub white_x: u16,
    pub white_y: u16,
    pub max_luminance: u32,
    pub min_luminance: u32,
}

fn parse_color_range(v: i32) -> ColorRange {
    if v == 1 {
        ColorRange::Full
    } else {
        ColorRange::Limited
    }
}

fn build_encoder_config_from_legacy(cfg: &Wav1cConfig) -> EncoderConfig {
    EncoderConfig {
        base_q_idx: cfg.base_q_idx,
        keyint: cfg.keyint,
        target_bitrate: if cfg.target_bitrate == 0 {
            None
        } else {
            Some(cfg.target_bitrate)
        },
        fps: cfg.fps,
        b_frames: cfg.b_frames != 0,
        gop_size: if cfg.gop_size > 0 { cfg.gop_size } else { 3 },
        video_signal: VideoSignal::default(),
        content_light: None,
        mastering_display: None,
    }
}

fn build_encoder_config_from_ex(cfg: &Wav1cConfigEx) -> Option<EncoderConfig> {
    let bit_depth = BitDepth::from_u8(cfg.bit_depth)?;
    let color_description = if cfg.color_primaries >= 0
        && cfg.transfer_characteristics >= 0
        && cfg.matrix_coefficients >= 0
    {
        Some(ColorDescription {
            color_primaries: cfg.color_primaries as u8,
            transfer_characteristics: cfg.transfer_characteristics as u8,
            matrix_coefficients: cfg.matrix_coefficients as u8,
        })
    } else {
        None
    };
    let content_light = if cfg.has_cll != 0 {
        Some(ContentLightLevel {
            max_content_light_level: cfg.max_cll,
            max_frame_average_light_level: cfg.max_fall,
        })
    } else {
        None
    };
    let mastering_display = if cfg.has_mdcv != 0 {
        Some(MasteringDisplayMetadata {
            primaries: [
                [cfg.red_x, cfg.red_y],
                [cfg.green_x, cfg.green_y],
                [cfg.blue_x, cfg.blue_y],
            ],
            white_point: [cfg.white_x, cfg.white_y],
            max_luminance: cfg.max_luminance,
            min_luminance: cfg.min_luminance,
        })
    } else {
        None
    };

    Some(EncoderConfig {
        base_q_idx: cfg.base_q_idx,
        keyint: cfg.keyint,
        target_bitrate: if cfg.target_bitrate == 0 {
            None
        } else {
            Some(cfg.target_bitrate)
        },
        fps: cfg.fps,
        b_frames: cfg.b_frames != 0,
        gop_size: if cfg.gop_size > 0 { cfg.gop_size } else { 3 },
        video_signal: VideoSignal {
            bit_depth,
            color_range: parse_color_range(cfg.color_range),
            color_description,
        },
        content_light,
        mastering_display,
    })
}

fn pack_u8_plane(
    src: *const u8,
    width: usize,
    height: usize,
    stride: usize,
    len: usize,
) -> Vec<u16> {
    if stride == width {
        unsafe { std::slice::from_raw_parts(src, len) }
            .iter()
            .map(|&x| x as u16)
            .collect()
    } else {
        let mut packed = Vec::with_capacity(width * height);
        for row in 0..height {
            let row_ptr = unsafe { src.add(row * stride) };
            let row_slice = unsafe { std::slice::from_raw_parts(row_ptr, width) };
            packed.extend(row_slice.iter().map(|&x| x as u16));
        }
        packed
    }
}

fn pack_u16_plane(
    src: *const u16,
    width: usize,
    height: usize,
    stride: usize,
    len: usize,
) -> Vec<u16> {
    if stride == width {
        unsafe { std::slice::from_raw_parts(src, len) }.to_vec()
    } else {
        let mut packed = Vec::with_capacity(width * height);
        for row in 0..height {
            let row_ptr = unsafe { src.add(row * stride) };
            let row_slice = unsafe { std::slice::from_raw_parts(row_ptr, width) };
            packed.extend_from_slice(row_slice);
        }
        packed
    }
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
    let config = build_encoder_config_from_legacy(cfg);

    match wav1c::Encoder::new(width, height, config) {
        Ok(inner) => Box::into_raw(Box::new(Wav1cEncoder {
            inner,
            headers_cache: Vec::new(),
        })),
        Err(_) => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_new_ex(
    width: u32,
    height: u32,
    cfg: *const Wav1cConfigEx,
) -> *mut Wav1cEncoder {
    if cfg.is_null() {
        return ptr::null_mut();
    }
    let cfg = unsafe { &*cfg };
    let Some(config) = build_encoder_config_from_ex(cfg) else {
        return ptr::null_mut();
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
    y_stride: i32,
    uv_stride: i32,
) -> i32 {
    if enc.is_null() || y.is_null() || u.is_null() || v.is_null() {
        return -1;
    }

    let enc = unsafe { &mut *enc };
    let width = enc.inner.width() as usize;
    let height = enc.inner.height() as usize;
    let uv_w = width.div_ceil(2);
    let uv_h = height.div_ceil(2);
    let y_stride = if y_stride > 0 {
        y_stride as usize
    } else {
        width
    };
    let uv_stride = if uv_stride > 0 {
        uv_stride as usize
    } else {
        uv_w
    };

    let y_plane = pack_u8_plane(y, width, height, y_stride, y_len);
    let u_plane = pack_u8_plane(u, uv_w, uv_h, uv_stride, u_len);
    let v_plane = pack_u8_plane(v, uv_w, uv_h, uv_stride, v_len);

    let frame = FramePixels {
        y: y_plane,
        u: u_plane,
        v: v_plane,
        width: width as u32,
        height: height as u32,
        bit_depth: BitDepth::Eight,
        color_range: ColorRange::Limited,
    };

    match enc.inner.send_frame(&frame) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_send_frame_u16(
    enc: *mut Wav1cEncoder,
    y: *const u16,
    y_len: usize,
    u: *const u16,
    u_len: usize,
    v: *const u16,
    v_len: usize,
    y_stride: i32,
    uv_stride: i32,
) -> i32 {
    if enc.is_null() || y.is_null() || u.is_null() || v.is_null() {
        return -1;
    }

    let enc = unsafe { &mut *enc };
    let width = enc.inner.width() as usize;
    let height = enc.inner.height() as usize;
    let uv_w = width.div_ceil(2);
    let uv_h = height.div_ceil(2);
    let y_stride = if y_stride > 0 {
        y_stride as usize
    } else {
        width
    };
    let uv_stride = if uv_stride > 0 {
        uv_stride as usize
    } else {
        uv_w
    };

    let y_plane = pack_u16_plane(y, width, height, y_stride, y_len);
    let u_plane = pack_u16_plane(u, uv_w, uv_h, uv_stride, u_len);
    let v_plane = pack_u16_plane(v, uv_w, uv_h, uv_stride, v_len);

    let frame = FramePixels {
        y: y_plane,
        u: u_plane,
        v: v_plane,
        width: width as u32,
        height: height as u32,
        bit_depth: BitDepth::Ten,
        color_range: ColorRange::Limited,
    };

    match enc.inner.send_frame(&frame) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_receive_packet(enc: *mut Wav1cEncoder) -> *mut Wav1cPacket {
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
