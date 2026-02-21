use std::path::Path;
use std::process::Command;
use std::ptr;

use wav1c_ffi::{
    Wav1cConfig, wav1c_encoder_flush, wav1c_encoder_free, wav1c_encoder_headers, wav1c_encoder_new,
    wav1c_encoder_receive_packet, wav1c_encoder_send_frame, wav1c_packet_free,
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

#[test]
fn encode_solid_frame() {
    let cfg = Wav1cConfig {
        base_q_idx: 128,
        keyint: 25,
        target_bitrate: 0,
        fps: 25.0,
    };

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
fn null_config_returns_null() {
    let enc = unsafe { wav1c_encoder_new(64, 64, ptr::null()) };
    assert!(enc.is_null());
}

#[test]
fn invalid_dimensions_returns_null() {
    let cfg = Wav1cConfig {
        base_q_idx: 128,
        keyint: 25,
        target_bitrate: 0,
        fps: 25.0,
    };

    let enc = unsafe { wav1c_encoder_new(0, 64, &cfg) };
    assert!(enc.is_null());
}

#[test]
fn headers_returns_sequence_header() {
    let cfg = Wav1cConfig {
        base_q_idx: 128,
        keyint: 25,
        target_bitrate: 0,
        fps: 25.0,
    };

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

    let cfg = Wav1cConfig {
        base_q_idx: 128,
        keyint: 25,
        target_bitrate: 0,
        fps: 25.0,
    };

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
    let cfg = Wav1cConfig {
        base_q_idx: 128,
        keyint: 25,
        target_bitrate: 0,
        fps: 25.0,
    };

    let enc = unsafe { wav1c_encoder_new(64, 64, &cfg) };
    assert!(!enc.is_null());

    unsafe { wav1c_encoder_flush(enc) };

    let pkt = unsafe { wav1c_encoder_receive_packet(enc) };
    assert!(pkt.is_null());

    unsafe { wav1c_encoder_free(enc) };
}
