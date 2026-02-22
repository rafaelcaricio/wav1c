#![allow(clippy::missing_safety_doc)]

use std::cell::RefCell;
use std::ffi::c_char;
use std::ptr;

use wav1c::EncoderConfig;
use wav1c::Fps;
use wav1c::packet::FrameType;
use wav1c::rc::RateControlStats;
use wav1c::video::{
    BitDepth, ColorDescription, ColorRange, ContentLightLevel, MasteringDisplayMetadata,
    VideoSignal,
};
use wav1c::y4m::FramePixels;

const WAV1C_STATUS_OK: i32 = 0;
const WAV1C_STATUS_INVALID_ARGUMENT: i32 = -1;
const WAV1C_STATUS_ENCODE_FAILED: i32 = -3;

thread_local! {
    static LAST_ERROR: RefCell<Vec<u8>> = RefCell::new(vec![0]);
}

fn set_last_error(message: impl AsRef<str>) {
    let message = message.as_ref();
    LAST_ERROR.with(|slot| {
        let mut buf = slot.borrow_mut();
        buf.clear();
        for b in message.bytes() {
            if b != 0 {
                buf.push(b);
            }
        }
        buf.push(0);
    });
}

fn clear_last_error() {
    set_last_error("");
}

pub struct Wav1cEncoder {
    inner: wav1c::Encoder,
    headers_cache: Vec<u8>,
    color_range: ColorRange,
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
    pub fps_num: u32,
    pub fps_den: u32,
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

#[repr(C)]
pub struct Wav1cRateControlStats {
    pub target_bitrate: u64,
    pub frames_encoded: u64,
    pub buffer_fullness_pct: u32,
    pub avg_qp: u8,
}

fn parse_flag(name: &str, value: i32) -> Result<bool, String> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(format!("{name} must be 0 or 1")),
    }
}

fn parse_color_range(v: i32) -> Result<ColorRange, String> {
    match v {
        0 => Ok(ColorRange::Limited),
        1 => Ok(ColorRange::Full),
        _ => Err("color_range must be 0 (limited) or 1 (full)".to_owned()),
    }
}

fn parse_code_point(name: &str, value: i32) -> Result<u8, String> {
    if (0..=u8::MAX as i32).contains(&value) {
        Ok(value as u8)
    } else {
        Err(format!("{name} must be in 0..=255"))
    }
}

fn parse_color_description(cfg: &Wav1cConfig) -> Result<Option<ColorDescription>, String> {
    let values = [
        cfg.color_primaries,
        cfg.transfer_characteristics,
        cfg.matrix_coefficients,
    ];

    if values.iter().any(|&v| v < -1) {
        return Err(
            "color_primaries, transfer_characteristics, and matrix_coefficients must be -1 or in 0..=255"
                .to_owned(),
        );
    }

    let provided = values.iter().filter(|&&v| v >= 0).count();

    if provided == 0 {
        return Ok(None);
    }
    if provided != 3 {
        return Err(
            "color_primaries, transfer_characteristics, and matrix_coefficients must be provided together"
                .to_owned(),
        );
    }

    Ok(Some(ColorDescription {
        color_primaries: parse_code_point("color_primaries", cfg.color_primaries)?,
        transfer_characteristics: parse_code_point(
            "transfer_characteristics",
            cfg.transfer_characteristics,
        )?,
        matrix_coefficients: parse_code_point("matrix_coefficients", cfg.matrix_coefficients)?,
    }))
}

fn parse_content_light(cfg: &Wav1cConfig) -> Result<Option<ContentLightLevel>, String> {
    let has_cll = parse_flag("has_cll", cfg.has_cll)?;
    if !has_cll {
        if cfg.max_cll != 0 || cfg.max_fall != 0 {
            return Err("max_cll/max_fall require has_cll=1".to_owned());
        }
        return Ok(None);
    }

    Ok(Some(ContentLightLevel {
        max_content_light_level: cfg.max_cll,
        max_frame_average_light_level: cfg.max_fall,
    }))
}

fn parse_mastering_display(cfg: &Wav1cConfig) -> Result<Option<MasteringDisplayMetadata>, String> {
    let has_mdcv = parse_flag("has_mdcv", cfg.has_mdcv)?;
    if !has_mdcv {
        if cfg.red_x != 0
            || cfg.red_y != 0
            || cfg.green_x != 0
            || cfg.green_y != 0
            || cfg.blue_x != 0
            || cfg.blue_y != 0
            || cfg.white_x != 0
            || cfg.white_y != 0
            || cfg.max_luminance != 0
            || cfg.min_luminance != 0
        {
            return Err("MDCV fields require has_mdcv=1".to_owned());
        }
        return Ok(None);
    }

    Ok(Some(MasteringDisplayMetadata {
        primaries: [
            [cfg.red_x, cfg.red_y],
            [cfg.green_x, cfg.green_y],
            [cfg.blue_x, cfg.blue_y],
        ],
        white_point: [cfg.white_x, cfg.white_y],
        max_luminance: cfg.max_luminance,
        min_luminance: cfg.min_luminance,
    }))
}

fn build_encoder_config(cfg: &Wav1cConfig) -> Result<EncoderConfig, String> {
    let bit_depth = BitDepth::from_u8(cfg.bit_depth)
        .ok_or_else(|| format!("bit_depth must be 8 or 10 (got {})", cfg.bit_depth))?;
    let color_range = parse_color_range(cfg.color_range)?;
    let color_description = parse_color_description(cfg)?;
    let content_light = parse_content_light(cfg)?;
    let mastering_display = parse_mastering_display(cfg)?;
    let fps = Fps::new(cfg.fps_num, cfg.fps_den).map_err(|e| e.to_string())?;

    Ok(EncoderConfig {
        base_q_idx: cfg.base_q_idx,
        keyint: cfg.keyint,
        target_bitrate: if cfg.target_bitrate == 0 {
            None
        } else {
            Some(cfg.target_bitrate)
        },
        fps,
        b_frames: cfg.b_frames != 0,
        gop_size: if cfg.gop_size > 0 { cfg.gop_size } else { 3 },
        video_signal: VideoSignal {
            bit_depth,
            color_range,
            color_description,
        },
        content_light,
        mastering_display,
    })
}

fn parse_stride(stride: i32, default_width: usize, label: &str) -> Result<usize, String> {
    if stride <= 0 {
        return Ok(default_width);
    }
    let stride = stride as usize;
    if stride < default_width {
        return Err(format!(
            "{label} stride ({stride}) must be >= plane width ({default_width})"
        ));
    }
    Ok(stride)
}

fn required_plane_len(
    width: usize,
    height: usize,
    stride: usize,
    label: &str,
) -> Result<usize, String> {
    if width == 0 || height == 0 {
        return Err(format!("{label} plane dimensions must be non-zero"));
    }
    let row_offsets = (height - 1)
        .checked_mul(stride)
        .ok_or_else(|| format!("{} plane dimensions overflowed", label))?;
    row_offsets
        .checked_add(width)
        .ok_or_else(|| format!("{} plane dimensions overflowed", label))
}

fn validate_plane_layout(
    width: usize,
    height: usize,
    stride: usize,
    len: usize,
    label: &str,
) -> Result<(), String> {
    let required = required_plane_len(width, height, stride, label)?;
    if len < required {
        return Err(format!(
            "{label} plane length too small: got {len}, need at least {required} samples for width={width}, height={height}, stride={stride}"
        ));
    }
    Ok(())
}

fn packed_samples(width: usize, height: usize, label: &str) -> Result<usize, String> {
    width
        .checked_mul(height)
        .ok_or_else(|| format!("{} plane dimensions overflowed", label))
}

fn pack_u8_plane(
    src: *const u8,
    width: usize,
    height: usize,
    stride: usize,
    len: usize,
) -> Result<Vec<u16>, String> {
    validate_plane_layout(width, height, stride, len, "u8")?;
    if stride == width {
        let samples = packed_samples(width, height, "u8")?;
        Ok(unsafe { std::slice::from_raw_parts(src, samples) }
            .iter()
            .map(|&x| x as u16)
            .collect())
    } else {
        let mut packed = Vec::with_capacity(packed_samples(width, height, "u8")?);
        for row in 0..height {
            let row_ptr = unsafe { src.add(row * stride) };
            let row_slice = unsafe { std::slice::from_raw_parts(row_ptr, width) };
            packed.extend(row_slice.iter().map(|&x| x as u16));
        }
        Ok(packed)
    }
}

fn pack_u16_plane(
    src: *const u16,
    width: usize,
    height: usize,
    stride: usize,
    len: usize,
) -> Result<Vec<u16>, String> {
    validate_plane_layout(width, height, stride, len, "u16")?;
    if stride == width {
        let samples = packed_samples(width, height, "u16")?;
        Ok(unsafe { std::slice::from_raw_parts(src, samples) }.to_vec())
    } else {
        let mut packed = Vec::with_capacity(packed_samples(width, height, "u16")?);
        for row in 0..height {
            let row_ptr = unsafe { src.add(row * stride) };
            let row_slice = unsafe { std::slice::from_raw_parts(row_ptr, width) };
            packed.extend_from_slice(row_slice);
        }
        Ok(packed)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn wav1c_default_config() -> Wav1cConfig {
    Wav1cConfig {
        base_q_idx: 128,
        keyint: 25,
        target_bitrate: 0,
        fps_num: 25,
        fps_den: 1,
        b_frames: 0,
        gop_size: 3,
        bit_depth: 8,
        color_range: 0,
        color_primaries: -1,
        transfer_characteristics: -1,
        matrix_coefficients: -1,
        has_cll: 0,
        max_cll: 0,
        max_fall: 0,
        has_mdcv: 0,
        red_x: 0,
        red_y: 0,
        green_x: 0,
        green_y: 0,
        blue_x: 0,
        blue_y: 0,
        white_x: 0,
        white_y: 0,
        max_luminance: 0,
        min_luminance: 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn wav1c_last_error_message() -> *const c_char {
    LAST_ERROR.with(|slot| slot.borrow().as_ptr() as *const c_char)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_new(
    width: u32,
    height: u32,
    cfg: *const Wav1cConfig,
) -> *mut Wav1cEncoder {
    if cfg.is_null() {
        set_last_error("cfg must not be null");
        return ptr::null_mut();
    }

    let cfg = unsafe { &*cfg };
    let config = match build_encoder_config(cfg) {
        Ok(config) => config,
        Err(reason) => {
            set_last_error(reason);
            return ptr::null_mut();
        }
    };
    let color_range = config.video_signal.color_range;

    match wav1c::Encoder::new(width, height, config) {
        Ok(inner) => {
            clear_last_error();
            Box::into_raw(Box::new(Wav1cEncoder {
                inner,
                headers_cache: Vec::new(),
                color_range,
            }))
        }
        Err(e) => {
            set_last_error(e.to_string());
            ptr::null_mut()
        }
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
        set_last_error("enc and out_data must not be null");
        return 0;
    }

    let enc = unsafe { &mut *enc };
    enc.headers_cache = enc.inner.headers();
    unsafe { *out_data = enc.headers_cache.as_ptr() };
    clear_last_error();
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
        set_last_error("enc, y, u, and v must not be null");
        return WAV1C_STATUS_INVALID_ARGUMENT;
    }

    let enc = unsafe { &mut *enc };
    let width = enc.inner.width() as usize;
    let height = enc.inner.height() as usize;
    let uv_w = width.div_ceil(2);
    let uv_h = height.div_ceil(2);

    let y_stride = match parse_stride(y_stride, width, "y") {
        Ok(v) => v,
        Err(reason) => {
            set_last_error(reason);
            return WAV1C_STATUS_INVALID_ARGUMENT;
        }
    };
    let uv_stride = match parse_stride(uv_stride, uv_w, "uv") {
        Ok(v) => v,
        Err(reason) => {
            set_last_error(reason);
            return WAV1C_STATUS_INVALID_ARGUMENT;
        }
    };

    let y_plane = match pack_u8_plane(y, width, height, y_stride, y_len) {
        Ok(p) => p,
        Err(reason) => {
            set_last_error(reason);
            return WAV1C_STATUS_INVALID_ARGUMENT;
        }
    };
    let u_plane = match pack_u8_plane(u, uv_w, uv_h, uv_stride, u_len) {
        Ok(p) => p,
        Err(reason) => {
            set_last_error(reason);
            return WAV1C_STATUS_INVALID_ARGUMENT;
        }
    };
    let v_plane = match pack_u8_plane(v, uv_w, uv_h, uv_stride, v_len) {
        Ok(p) => p,
        Err(reason) => {
            set_last_error(reason);
            return WAV1C_STATUS_INVALID_ARGUMENT;
        }
    };

    let frame = FramePixels {
        y: y_plane,
        u: u_plane,
        v: v_plane,
        width: width as u32,
        height: height as u32,
        bit_depth: BitDepth::Eight,
        color_range: enc.color_range,
    };

    match enc.inner.send_frame(&frame) {
        Ok(()) => {
            clear_last_error();
            WAV1C_STATUS_OK
        }
        Err(e) => {
            set_last_error(e.to_string());
            WAV1C_STATUS_ENCODE_FAILED
        }
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
        set_last_error("enc, y, u, and v must not be null");
        return WAV1C_STATUS_INVALID_ARGUMENT;
    }

    let enc = unsafe { &mut *enc };
    let width = enc.inner.width() as usize;
    let height = enc.inner.height() as usize;
    let uv_w = width.div_ceil(2);
    let uv_h = height.div_ceil(2);

    let y_stride = match parse_stride(y_stride, width, "y") {
        Ok(v) => v,
        Err(reason) => {
            set_last_error(reason);
            return WAV1C_STATUS_INVALID_ARGUMENT;
        }
    };
    let uv_stride = match parse_stride(uv_stride, uv_w, "uv") {
        Ok(v) => v,
        Err(reason) => {
            set_last_error(reason);
            return WAV1C_STATUS_INVALID_ARGUMENT;
        }
    };

    let y_plane = match pack_u16_plane(y, width, height, y_stride, y_len) {
        Ok(p) => p,
        Err(reason) => {
            set_last_error(reason);
            return WAV1C_STATUS_INVALID_ARGUMENT;
        }
    };
    let u_plane = match pack_u16_plane(u, uv_w, uv_h, uv_stride, u_len) {
        Ok(p) => p,
        Err(reason) => {
            set_last_error(reason);
            return WAV1C_STATUS_INVALID_ARGUMENT;
        }
    };
    let v_plane = match pack_u16_plane(v, uv_w, uv_h, uv_stride, v_len) {
        Ok(p) => p,
        Err(reason) => {
            set_last_error(reason);
            return WAV1C_STATUS_INVALID_ARGUMENT;
        }
    };

    let frame = FramePixels {
        y: y_plane,
        u: u_plane,
        v: v_plane,
        width: width as u32,
        height: height as u32,
        bit_depth: BitDepth::Ten,
        color_range: enc.color_range,
    };

    match enc.inner.send_frame(&frame) {
        Ok(()) => {
            clear_last_error();
            WAV1C_STATUS_OK
        }
        Err(e) => {
            set_last_error(e.to_string());
            WAV1C_STATUS_ENCODE_FAILED
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_receive_packet(enc: *mut Wav1cEncoder) -> *mut Wav1cPacket {
    if enc.is_null() {
        set_last_error("enc must not be null");
        return ptr::null_mut();
    }

    let enc = unsafe { &mut *enc };

    match enc.inner.receive_packet() {
        Some(packet) => {
            clear_last_error();
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
        None => {
            clear_last_error();
            ptr::null_mut()
        }
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
        set_last_error("enc must not be null");
        return;
    }

    let enc = unsafe { &mut *enc };
    enc.inner.flush();
    clear_last_error();
}

fn to_ffi_rate_control_stats(stats: RateControlStats) -> Wav1cRateControlStats {
    Wav1cRateControlStats {
        target_bitrate: stats.target_bitrate,
        frames_encoded: stats.frames_encoded,
        buffer_fullness_pct: stats.buffer_fullness_pct,
        avg_qp: stats.avg_qp,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wav1c_encoder_rate_control_stats(
    enc: *const Wav1cEncoder,
    out_stats: *mut Wav1cRateControlStats,
) -> i32 {
    if enc.is_null() || out_stats.is_null() {
        set_last_error("enc and out_stats must not be null");
        return WAV1C_STATUS_INVALID_ARGUMENT;
    }

    let enc = unsafe { &*enc };
    match enc.inner.rate_control_stats() {
        Some(stats) => {
            unsafe {
                *out_stats = to_ffi_rate_control_stats(stats);
            }
            clear_last_error();
            1
        }
        None => {
            clear_last_error();
            0
        }
    }
}
