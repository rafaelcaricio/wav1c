use wav1c::y4m::FramePixels;

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

fn main() {
    let dav1d = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dav1d/build/tools/dav1d");
    if !dav1d.exists() {
        eprintln!("dav1d not found");
        return;
    }

    test_pattern(&dav1d, "only_u_gradient", 320, 240, |col, _row| {
        let u = ((col * 256 / 320) as u8).min(255);
        (128, u, 128)
    });

    test_pattern(&dav1d, "v_col_gradient", 320, 240, |col, _row| {
        let v = ((col * 256 / 320) as u8).min(255);
        (128, 128, v)
    });

    test_pattern(&dav1d, "failing_pattern", 320, 240, |col, row| {
        let y = ((row % 256) as u8).wrapping_add((col % 64) as u8).wrapping_mul(3);
        (y, 128, 128)
    });

    test_pattern(&dav1d, "varying_chroma", 320, 240, |col, row| {
        let y = ((row * 256 / 240) as u8).min(255);
        let u = ((col * 256 / 320) as u8).min(255);
        (y, u, 128)
    });

    test_pattern(&dav1d, "all_varying", 320, 240, |col, row| {
        let y = ((row * 256 / 240) as u8).min(255);
        let u = ((col * 256 / 320) as u8).min(255);
        let v = (((row + col) * 128 / 320) as u8).min(255);
        (y, u, v)
    });

    test_pattern(&dav1d, "complex_640x480", 640, 480, |col, row| {
        let y = ((row % 256) as u8).wrapping_add((col % 64) as u8).wrapping_mul(3);
        let u = ((col * 256 / 640) as u8).min(255);
        let v = ((row * 256 / 480) as u8).min(255);
        (y, u, v)
    });
}

fn test_pattern(dav1d: &std::path::Path, name: &str, w: u32, h: u32, f: impl Fn(u32, u32) -> (u8, u8, u8)) {
    let y4m_data = create_test_y4m(w, h, f);
    let pixels = FramePixels::from_y4m(&y4m_data);
    let output = wav1c::encode_av1_ivf_y4m(&pixels);

    let ivf_path = std::env::temp_dir().join(format!("wav1c_{}.ivf", name));
    let y4m_path = std::env::temp_dir().join(format!("wav1c_{}.y4m", name));
    std::fs::write(&ivf_path, &output).unwrap();

    let result = std::process::Command::new(dav1d)
        .args(["-i", ivf_path.to_str().unwrap(), "-o", y4m_path.to_str().unwrap()])
        .output()
        .expect("Failed to run dav1d");

    let stderr = String::from_utf8_lossy(&result.stderr);
    let status = if result.status.success() { "PASS" } else { "FAIL" };
    eprintln!("[{}] {} ({}x{}): {}", status, name, w, h,
              if result.status.success() { "decoded OK".to_string() } else { stderr.to_string() });
}
