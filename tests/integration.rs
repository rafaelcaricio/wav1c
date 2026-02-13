use std::io::Write;
use std::path::Path;
use std::process::Command;
use wav1c::y4m::FramePixels;

fn dav1d_path() -> Option<std::path::PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dav1d/build/tools/dav1d");
    if path.exists() {
        Some(path)
    } else {
        eprintln!("Skipping: dav1d not found at {:?}", path);
        None
    }
}

fn decode_to_y4m(dav1d: &Path, ivf_data: &[u8], name: &str) -> (bool, String, Vec<u8>) {
    let ivf_path = std::env::temp_dir().join(format!("wav1c_{}.ivf", name));
    let y4m_path = std::env::temp_dir().join(format!("wav1c_{}.y4m", name));
    std::fs::File::create(&ivf_path)
        .unwrap()
        .write_all(ivf_data)
        .unwrap();

    let result = Command::new(dav1d)
        .args([
            "-i",
            ivf_path.to_str().unwrap(),
            "-o",
            y4m_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to run dav1d");

    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    let y4m_data = std::fs::read(&y4m_path).unwrap_or_default();
    (result.status.success(), stderr, y4m_data)
}

fn extract_y4m_planes(y4m_data: &[u8], width: u32, height: u32) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let frame_marker = b"FRAME\n";
    let frame_start = y4m_data
        .windows(frame_marker.len())
        .position(|w| w == frame_marker)
        .expect("No FRAME marker in Y4M")
        + frame_marker.len();

    let y_size = (width * height) as usize;
    let uv_size = ((width / 2) * (height / 2)) as usize;
    let y_plane = y4m_data[frame_start..frame_start + y_size].to_vec();
    let u_plane = y4m_data[frame_start + y_size..frame_start + y_size + uv_size].to_vec();
    let v_plane = y4m_data[frame_start + y_size + uv_size..frame_start + y_size + 2 * uv_size].to_vec();
    (y_plane, u_plane, v_plane)
}

#[test]
fn dav1d_decodes_default_gray() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let output = wav1c::encode_av1_ivf(64, 64, 128, 128, 128);
    let ivf_path = std::env::temp_dir().join("wav1c_gray.ivf");
    std::fs::File::create(&ivf_path)
        .unwrap()
        .write_all(&output)
        .unwrap();

    let result = Command::new(&dav1d)
        .args(["-i", ivf_path.to_str().unwrap(), "-o", "/dev/null"])
        .output()
        .expect("Failed to run dav1d");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "dav1d failed: {}", stderr);
    assert!(
        stderr.contains("Decoded 1/1 frames"),
        "Unexpected: {}",
        stderr
    );
}

#[test]
fn dav1d_decodes_all_colors() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let test_cases: &[(u8, u8, u8)] = &[
        (128, 128, 128),
        (81, 91, 81),
        (0, 128, 128),
        (255, 128, 128),
        (0, 0, 0),
        (255, 255, 255),
        (16, 128, 128),
        (235, 128, 128),
    ];

    for &(y, u, v) in test_cases {
        let output = wav1c::encode_av1_ivf(64, 64, y, u, v);
        let (success, stderr, _) =
            decode_to_y4m(&dav1d, &output, &format!("color_{}_{}_{}", y, u, v));
        assert!(
            success,
            "dav1d failed for ({},{},{}): {}",
            y, u, v, stderr
        );
        assert!(
            stderr.contains("Decoded 1/1 frames"),
            "Unexpected for ({},{},{}): {}",
            y, u, v, stderr
        );
    }
}

#[test]
fn decoded_pixels_match_input() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let test_cases: &[(u8, u8, u8, i16)] = &[
        (128, 128, 128, 0),
        (0, 128, 128, 1),
        (255, 128, 128, 1),
        (81, 91, 81, 1),
        (0, 0, 0, 1),
        (255, 255, 255, 1),
    ];

    for &(y, u, v, max_error) in test_cases {
        let output = wav1c::encode_av1_ivf(64, 64, y, u, v);
        let (success, stderr, y4m_data) =
            decode_to_y4m(&dav1d, &output, &format!("pixel_{}_{}_{}", y, u, v));
        assert!(
            success,
            "dav1d failed for ({},{},{}): {}",
            y, u, v, stderr
        );

        if y4m_data.is_empty() {
            panic!("No Y4M output for ({},{},{})", y, u, v);
        }

        let (y_plane, u_plane, v_plane) = extract_y4m_planes(&y4m_data, 64, 64);

        for &py in y_plane.iter() {
            assert!(
                (py as i16 - y as i16).abs() <= max_error,
                "Y mismatch for input ({},{},{}): got {} expected {} (±{})",
                y,
                u,
                v,
                py,
                y,
                max_error
            );
        }

        for &pu in u_plane.iter() {
            assert!(
                (pu as i16 - u as i16).abs() <= max_error,
                "U mismatch for input ({},{},{}): got {} expected {} (±{})",
                y,
                u,
                v,
                pu,
                u,
                max_error
            );
        }

        for &pv in v_plane.iter() {
            assert!(
                (pv as i16 - v as i16).abs() <= max_error,
                "V mismatch for input ({},{},{}): got {} expected {} (±{})",
                y,
                u,
                v,
                pv,
                v,
                max_error
            );
        }
    }
}

#[test]
fn dav1d_decodes_various_dimensions() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let dimensions: &[(u32, u32)] = &[
        (8, 8),
        (16, 16),
        (64, 64),
        (100, 100),
        (128, 128),
        (320, 240),
        (640, 480),
        (1280, 720),
        (1920, 1080),
        (17, 33),
        (33, 17),
        (65, 65),
        (127, 127),
        (129, 129),
        (255, 255),
        (257, 257),
    ];

    for &(w, h) in dimensions {
        let output = wav1c::encode_av1_ivf(w, h, 128, 128, 128);
        let (success, stderr, _) =
            decode_to_y4m(&dav1d, &output, &format!("dim_{}x{}", w, h));
        assert!(
            success,
            "dav1d failed for {}x{}: {}",
            w, h, stderr
        );
        assert!(
            stderr.contains("Decoded 1/1 frames"),
            "Unexpected for {}x{}: {}",
            w, h, stderr
        );
    }
}

#[test]
fn dav1d_decodes_colors_at_various_dimensions() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let cases: &[(u32, u32, u8, u8, u8)] = &[
        (320, 240, 0, 128, 128),
        (320, 240, 255, 255, 255),
        (640, 480, 81, 91, 81),
        (1280, 720, 0, 0, 0),
        (100, 100, 128, 128, 128),
    ];

    for &(w, h, y, u, v) in cases {
        let output = wav1c::encode_av1_ivf(w, h, y, u, v);
        let (success, stderr, _) = decode_to_y4m(
            &dav1d,
            &output,
            &format!("dimcolor_{}x{}_{}_{}", w, h, y, u),
        );
        assert!(
            success,
            "dav1d failed for {}x{} color ({},{},{}): {}",
            w, h, y, u, v, stderr
        );
    }
}

fn create_test_y4m(
    width: u32,
    height: u32,
    pixel_fn: impl Fn(u32, u32) -> (u8, u8, u8),
) -> Vec<u8> {
    let header = format!("YUV4MPEG2 W{} H{} F30:1 Ip C420jpeg\n", width, height);
    let mut data = header.into_bytes();
    data.extend_from_slice(b"FRAME\n");
    for row in 0..height {
        for col in 0..width {
            let (y, _, _) = pixel_fn(col, row);
            data.push(y);
        }
    }
    for row in 0..height / 2 {
        for col in 0..width / 2 {
            let (_, u, _) = pixel_fn(col * 2, row * 2);
            data.push(u);
        }
    }
    for row in 0..height / 2 {
        for col in 0..width / 2 {
            let (_, _, v) = pixel_fn(col * 2, row * 2);
            data.push(v);
        }
    }
    data
}

#[test]
fn dav1d_decodes_gradient_y4m() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let y4m_data = create_test_y4m(64, 64, |col, row| {
        let y = ((row * 4) as u8).min(252);
        let u = ((col * 4) as u8).min(252);
        let v = 128;
        (y, u, v)
    });
    let pixels = FramePixels::from_y4m(&y4m_data);
    let output = wav1c::encode_av1_ivf_y4m(&pixels);
    let (success, stderr, _) = decode_to_y4m(&dav1d, &output, "gradient_y4m");
    assert!(success, "dav1d failed for gradient Y4M: {}", stderr);
    assert!(
        stderr.contains("Decoded 1/1 frames"),
        "Unexpected: {}",
        stderr
    );
}

#[test]
fn dav1d_decodes_solid_y4m() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let test_cases: &[(u8, u8, u8)] = &[
        (128, 128, 128),
        (0, 128, 128),
        (255, 128, 128),
        (81, 91, 81),
    ];

    for &(y, u, v) in test_cases {
        let pixels = FramePixels::solid(64, 64, y, u, v);
        let y4m_output = wav1c::encode_av1_ivf_y4m(&pixels);
        let solid_output = wav1c::encode_av1_ivf(64, 64, y, u, v);
        assert_eq!(
            y4m_output, solid_output,
            "Y4M and solid API differ for ({},{},{})",
            y, u, v
        );

        let (success, stderr, _) =
            decode_to_y4m(&dav1d, &y4m_output, &format!("solid_y4m_{}_{}_{}", y, u, v));
        assert!(
            success,
            "dav1d failed for solid Y4M ({},{},{}): {}",
            y, u, v, stderr
        );
    }
}

#[test]
fn y4m_various_dimensions() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let dimensions: &[(u32, u32)] = &[
        (64, 64),
        (100, 100),
        (128, 128),
        (320, 240),
        (640, 480),
    ];

    for &(w, h) in dimensions {
        let y4m_data = create_test_y4m(w, h, |col, row| {
            let y = ((row % 256) as u8).wrapping_add((col % 64) as u8).wrapping_mul(3);
            let u = ((col * 256 / w) as u8).min(255);
            let v = ((row * 256 / h) as u8).min(255);
            (y, u, v)
        });
        let pixels = FramePixels::from_y4m(&y4m_data);
        let output = wav1c::encode_av1_ivf_y4m(&pixels);
        let (success, stderr, _) =
            decode_to_y4m(&dav1d, &output, &format!("y4m_dim_{}x{}", w, h));
        assert!(
            success,
            "dav1d failed for Y4M {}x{}: {}",
            w, h, stderr
        );
        assert!(
            stderr.contains("Decoded 1/1 frames"),
            "Unexpected for Y4M {}x{}: {}",
            w, h, stderr
        );
    }
}

#[test]
fn dav1d_decodes_gradient_multi_sb() {
    let Some(dav1d) = dav1d_path() else {
        return;
    };

    let dimensions: &[(u32, u32)] = &[
        (128, 128),
        (320, 240),
        (640, 480),
    ];

    for &(w, h) in dimensions {
        let y4m_data = create_test_y4m(w, h, |_col, row| {
            let y = if row < 64 { 50 } else { 200 };
            (y, 128, 128)
        });
        let pixels = FramePixels::from_y4m(&y4m_data);
        let output = wav1c::encode_av1_ivf_y4m(&pixels);
        let (success, stderr, _) =
            decode_to_y4m(&dav1d, &output, &format!("gradient_multi_{}x{}", w, h));
        assert!(
            success,
            "dav1d failed for gradient {}x{}: {}",
            w, h, stderr
        );
    }
}
