use std::io::Write;
use wav1c::y4m::FramePixels;

fn find_dav1d() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("DAV1D") {
        let path = std::path::PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    if let Ok(output) = std::process::Command::new("which").arg("dav1d").output()
        && output.status.success()
    {
        let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(std::path::PathBuf::from(p));
        }
    }

    let local =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dav1d/build/tools/dav1d");
    if local.exists() {
        return Some(local);
    }

    None
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

fn encode_to_ivf(pixels: &FramePixels) -> Vec<u8> {
    let config = wav1c::EncodeConfig::default();
    let packets = wav1c::encode_packets(std::slice::from_ref(pixels), &config);
    let width = pixels.width as u16;
    let height = pixels.height as u16;
    let mut out = Vec::new();
    out.write_all(b"DKIF").unwrap();
    out.write_all(&0u16.to_le_bytes()).unwrap();
    out.write_all(&32u16.to_le_bytes()).unwrap();
    out.write_all(b"AV01").unwrap();
    out.write_all(&width.to_le_bytes()).unwrap();
    out.write_all(&height.to_le_bytes()).unwrap();
    out.write_all(&25u32.to_le_bytes()).unwrap();
    out.write_all(&1u32.to_le_bytes()).unwrap();
    out.write_all(&(packets.len() as u32).to_le_bytes()).unwrap();
    out.write_all(&0u32.to_le_bytes()).unwrap();
    for p in &packets {
        out.write_all(&(p.data.len() as u32).to_le_bytes()).unwrap();
        out.write_all(&p.frame_number.to_le_bytes()).unwrap();
        out.write_all(&p.data).unwrap();
    }
    out
}

fn main() {
    let dav1d = find_dav1d();
    let Some(dav1d) = dav1d else {
        eprintln!("dav1d not found (set DAV1D env var or install dav1d in PATH)");
        return;
    };

    test_pattern(&dav1d, "only_u_gradient", 320, 240, |col, _row| {
        let u = (col * 256 / 320) as u8;
        (128, u, 128)
    });

    test_pattern(&dav1d, "v_col_gradient", 320, 240, |col, _row| {
        let v = (col * 256 / 320) as u8;
        (128, 128, v)
    });

    test_pattern(&dav1d, "failing_pattern", 320, 240, |col, row| {
        let y = ((row % 256) as u8)
            .wrapping_add((col % 64) as u8)
            .wrapping_mul(3);
        (y, 128, 128)
    });

    test_pattern(&dav1d, "varying_chroma", 320, 240, |col, row| {
        let y = (row * 256 / 240) as u8;
        let u = (col * 256 / 320) as u8;
        (y, u, 128)
    });

    test_pattern(&dav1d, "all_varying", 320, 240, |col, row| {
        let y = (row * 256 / 240) as u8;
        let u = (col * 256 / 320) as u8;
        let v = ((row + col) * 128 / 320) as u8;
        (y, u, v)
    });

    test_pattern(&dav1d, "complex_640x480", 640, 480, |col, row| {
        let y = ((row % 256) as u8)
            .wrapping_add((col % 64) as u8)
            .wrapping_mul(3);
        let u = (col * 256 / 640) as u8;
        let v = (row * 256 / 480) as u8;
        (y, u, v)
    });
}

fn test_pattern(
    dav1d: &std::path::Path,
    name: &str,
    w: u32,
    h: u32,
    f: impl Fn(u32, u32) -> (u8, u8, u8),
) {
    let y4m_data = create_test_y4m(w, h, f);
    let pixels = FramePixels::from_y4m(&y4m_data);
    let output = encode_to_ivf(&pixels);

    let ivf_path = std::env::temp_dir().join(format!("wav1c_{}.ivf", name));
    let y4m_path = std::env::temp_dir().join(format!("wav1c_{}.y4m", name));
    std::fs::write(&ivf_path, &output).unwrap();

    let result = std::process::Command::new(dav1d)
        .args([
            "-i",
            ivf_path.to_str().unwrap(),
            "-o",
            y4m_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to run dav1d");

    let stderr = String::from_utf8_lossy(&result.stderr);
    let status = if result.status.success() {
        "PASS"
    } else {
        "FAIL"
    };
    eprintln!(
        "[{}] {} ({}x{}): {}",
        status,
        name,
        w,
        h,
        if result.status.success() {
            "decoded OK".to_string()
        } else {
            stderr.to_string()
        }
    );
}
