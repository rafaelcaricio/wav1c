use std::io::Write;
use std::process::Command;

#[test]
fn dav1d_decodes_output() {
    let dav1d_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dav1d/build/tools/dav1d");

    if !dav1d_path.exists() {
        eprintln!("Skipping: dav1d not found at {:?}", dav1d_path);
        return;
    }

    let output = wav1c::encode_av1_ivf();
    let ivf_path = std::env::temp_dir().join("wav1c_integration_test.ivf");
    let mut file = std::fs::File::create(&ivf_path).unwrap();
    file.write_all(&output).unwrap();

    let result = Command::new(dav1d_path)
        .args(["-i", ivf_path.to_str().unwrap(), "-o", "/dev/null"])
        .output()
        .expect("Failed to run dav1d");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "dav1d failed: {}", stderr);
    assert!(
        stderr.contains("Decoded 1/1 frames"),
        "Unexpected dav1d output: {}",
        stderr
    );
}
