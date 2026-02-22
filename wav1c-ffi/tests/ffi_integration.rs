use std::ffi::CStr;
use std::path::Path;
use std::process::Command;
use std::ptr;

use wav1c_ffi::{
    Wav1cConfig, Wav1cRateControlStats, wav1c_default_config, wav1c_encoder_flush,
    wav1c_encoder_free, wav1c_encoder_headers, wav1c_encoder_new, wav1c_encoder_rate_control_stats,
    wav1c_encoder_receive_packet, wav1c_encoder_send_frame, wav1c_encoder_send_frame_u16,
    wav1c_last_error_message, wav1c_packet_free,
};

fn dav1d_path() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("DAV1D") {
        let path = std::path::PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    if let Ok(output) = Command::new("which").arg("dav1d").output()
        && output.status.success()
    {
        let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(std::path::PathBuf::from(p));
        }
    }

    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dav1d/build/tools/dav1d");
    if local.exists() {
        return Some(local);
    }

    eprintln!("Skipping: dav1d not found (set DAV1D env var or install dav1d in PATH)");
    None
}

fn write_test_ivf(width: u16, height: u16, frame_data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"DKIF");
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&32u16.to_le_bytes());
    buf.extend_from_slice(b"AV01");
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(&25u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(frame_data.len() as u32).to_le_bytes());
    buf.extend_from_slice(&0u64.to_le_bytes());
    buf.extend_from_slice(frame_data);
    buf
}

fn default_config() -> Wav1cConfig {
    wav1c_default_config()
}

fn last_error_message() -> String {
    let ptr = wav1c_last_error_message();
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string()
}

#[test]
fn encode_solid_frame() {
    let cfg = default_config();

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(!enc.is_null());

    let y_plane = vec![128u8; 64 * 64];
    let u_plane = vec![128u8; 32 * 32];
    let v_plane = vec![128u8; 32 * 32];

    let ret = unsafe {
        wav1c_encoder_send_frame(
            enc,
            y_plane.as_ptr(),
            y_plane.len(),
            u_plane.as_ptr(),
            u_plane.len(),
            v_plane.as_ptr(),
            v_plane.len(),
            0,
            0,
        )
    };
    assert_eq!(ret, 0);

    let pkt = unsafe { wav1c_encoder_receive_packet(enc) };
    assert!(!pkt.is_null());

    let packet = unsafe { &*pkt };
    assert_eq!(packet.is_keyframe, 1);
    assert_eq!(packet.frame_number, 0);
    assert!(packet.size > 0);

    let data = unsafe { std::slice::from_raw_parts(packet.data, packet.size) };
    assert_eq!(data[0], 0x12);
    assert_eq!(data[1], 0x00);

    unsafe { wav1c_packet_free(pkt) };
    unsafe { wav1c_encoder_free(enc) };
}

#[test]
fn encode_solid_frame_10bit() {
    let mut cfg = default_config();
    cfg.bit_depth = 10;
    cfg.color_range = 1;
    cfg.color_primaries = 9;
    cfg.transfer_characteristics = 16;
    cfg.matrix_coefficients = 9;

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(!enc.is_null());

    let y_plane = vec![512u16; 64 * 64];
    let u_plane = vec![512u16; 32 * 32];
    let v_plane = vec![512u16; 32 * 32];

    let ret = unsafe {
        wav1c_encoder_send_frame_u16(
            enc,
            y_plane.as_ptr(),
            y_plane.len(),
            u_plane.as_ptr(),
            u_plane.len(),
            v_plane.as_ptr(),
            v_plane.len(),
            0,
            0,
        )
    };
    assert_eq!(ret, 0);

    let pkt = unsafe { wav1c_encoder_receive_packet(enc) };
    assert!(!pkt.is_null());
    unsafe { wav1c_packet_free(pkt) };
    unsafe { wav1c_encoder_free(enc) };
}

#[test]
fn null_config_returns_null() {
    let enc = unsafe { wav1c_encoder_new(64, 64, ptr::null()) };
    assert!(enc.is_null());
}

#[test]
fn invalid_dimensions_returns_null() {
    let cfg = default_config();

    let enc = unsafe { wav1c_encoder_new(0, 64, &cfg) };
    assert!(enc.is_null());
}

#[test]
fn headers_returns_sequence_header() {
    let cfg = default_config();

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(!enc.is_null());

    let mut out_data: *const u8 = ptr::null();
    let size = unsafe { wav1c_encoder_headers(enc, &mut out_data) };
    assert!(size > 0);
    assert!(!out_data.is_null());

    let header_bytes = unsafe { std::slice::from_raw_parts(out_data, size) };
    assert_eq!(header_bytes[0], 0x0A);

    unsafe { wav1c_encoder_free(enc) };
}

#[test]
fn encode_and_decode_with_dav1d() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let cfg = default_config();

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(!enc.is_null());

    let y_plane = vec![128u8; 64 * 64];
    let u_plane = vec![128u8; 32 * 32];
    let v_plane = vec![128u8; 32 * 32];

    let ret = unsafe {
        wav1c_encoder_send_frame(
            enc,
            y_plane.as_ptr(),
            y_plane.len(),
            u_plane.as_ptr(),
            u_plane.len(),
            v_plane.as_ptr(),
            v_plane.len(),
            0,
            0,
        )
    };
    assert_eq!(ret, 0);

    let pkt = unsafe { wav1c_encoder_receive_packet(enc) };
    assert!(!pkt.is_null());

    let packet = unsafe { &*pkt };
    let frame_data = unsafe { std::slice::from_raw_parts(packet.data, packet.size) };
    let ivf = write_test_ivf(64, 64, frame_data);

    let ivf_path = std::env::temp_dir().join("wav1c_ffi_roundtrip.ivf");
    std::fs::write(&ivf_path, &ivf).expect("failed to write IVF file");

    let result = Command::new(dav1d.as_os_str())
        .args(["-i", ivf_path.to_str().unwrap(), "-o", "/dev/null"])
        .output()
        .expect("failed to run dav1d");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "dav1d failed: {}", stderr);
    assert!(
        stderr.contains("Decoded 1/1 frames"),
        "Unexpected dav1d output: {}",
        stderr
    );

    unsafe { wav1c_packet_free(pkt) };
    unsafe { wav1c_encoder_free(enc) };
}

#[test]
fn flush_is_safe_to_call() {
    let cfg = default_config();

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(!enc.is_null());

    unsafe { wav1c_encoder_flush(enc) };

    let pkt = unsafe { wav1c_encoder_receive_packet(enc) };
    assert!(pkt.is_null());

    unsafe { wav1c_encoder_free(enc) };
}

#[test]
fn invalid_partial_color_description_returns_null() {
    let mut cfg = default_config();
    cfg.color_primaries = 9;

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(enc.is_null());
    assert!(last_error_message().contains("must be provided together"));
}

#[test]
fn invalid_negative_color_description_returns_null() {
    let mut cfg = default_config();
    cfg.color_primaries = -2;
    cfg.transfer_characteristics = -1;
    cfg.matrix_coefficients = -1;

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(enc.is_null());
    assert!(last_error_message().contains("must be -1 or in 0..=255"));
}

#[test]
fn invalid_color_range_returns_null() {
    let mut cfg = default_config();
    cfg.color_range = 7;

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(enc.is_null());
    assert!(last_error_message().contains("color_range must be 0"));
}

#[test]
fn zero_fps_denominator_returns_null() {
    let mut cfg = default_config();
    cfg.fps_den = 0;

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(enc.is_null());
    assert!(last_error_message().contains("fps den must be > 0"));
}

#[test]
fn short_plane_lengths_are_rejected() {
    let cfg = default_config();
    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(!enc.is_null());

    let y_plane = vec![128u8; 64 * 64 - 1];
    let u_plane = vec![128u8; 32 * 32];
    let v_plane = vec![128u8; 32 * 32];
    let ret = unsafe {
        wav1c_encoder_send_frame(
            enc,
            y_plane.as_ptr(),
            y_plane.len(),
            u_plane.as_ptr(),
            u_plane.len(),
            v_plane.as_ptr(),
            v_plane.len(),
            0,
            0,
        )
    };
    assert_eq!(ret, -1);
    assert!(last_error_message().contains("plane length too small"));

    unsafe { wav1c_encoder_free(enc) };
}

#[test]
fn rate_control_stats_available_when_bitrate_enabled() {
    let mut cfg = default_config();
    cfg.target_bitrate = 500_000;

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(!enc.is_null());

    let y_plane = vec![128u8; 64 * 64];
    let u_plane = vec![128u8; 32 * 32];
    let v_plane = vec![128u8; 32 * 32];
    let ret = unsafe {
        wav1c_encoder_send_frame(
            enc,
            y_plane.as_ptr(),
            y_plane.len(),
            u_plane.as_ptr(),
            u_plane.len(),
            v_plane.as_ptr(),
            v_plane.len(),
            0,
            0,
        )
    };
    assert_eq!(ret, 0);

    let mut stats = Wav1cRateControlStats {
        target_bitrate: 0,
        frames_encoded: 0,
        buffer_fullness_pct: 0,
        avg_qp: 0,
    };
    let has_stats = unsafe { wav1c_encoder_rate_control_stats(enc, &mut stats) };
    assert_eq!(has_stats, 1);
    assert_eq!(stats.target_bitrate, 500_000);
    assert!(stats.avg_qp > 0);

    unsafe { wav1c_encoder_free(enc) };
}
