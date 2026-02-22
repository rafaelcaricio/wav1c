use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use wav1c::y4m::FramePixels;
use wav1c::{EncodeConfig, Fps};

fn print_usage() {
    eprintln!("Usage: encode_y4m <input.y4m> <output.ivf> [options]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --quality N      Base quantizer index (0-255, default: 128)");
    eprintln!("  --keyint N       Keyframe interval (default: 25)");
    eprintln!("  --b-frames       Enable B-frame encoding (experimental)");
    eprintln!("  --gop-size N     Mini-GOP size for B-frames (default: 3)");
    eprintln!("  --bitrate N      Target bitrate in kbps (enables rate control)");
    eprintln!("  --fps N[/D]      Frame rate (default: 25/1)");
}

fn parse_fps(value: &str) -> Fps {
    if let Some((num_s, den_s)) = value.split_once('/') {
        let num = num_s.parse().expect("invalid fps numerator");
        let den = den_s.parse().expect("invalid fps denominator");
        Fps::new(num, den).expect("fps num/den must be > 0")
    } else {
        let fps = value.parse().expect("invalid fps value");
        Fps::from_int(fps).expect("fps must be > 0")
    }
}

fn write_ivf(frames: &[FramePixels], config: &EncodeConfig) -> Vec<u8> {
    let packets = wav1c::encode_packets(frames, config);
    let width = frames[0].width as u16;
    let height = frames[0].height as u16;
    let mut out = Vec::new();
    out.write_all(b"DKIF").unwrap();
    out.write_all(&0u16.to_le_bytes()).unwrap();
    out.write_all(&32u16.to_le_bytes()).unwrap();
    out.write_all(b"AV01").unwrap();
    out.write_all(&width.to_le_bytes()).unwrap();
    out.write_all(&height.to_le_bytes()).unwrap();
    out.write_all(&config.fps.num.to_le_bytes()).unwrap();
    out.write_all(&config.fps.den.to_le_bytes()).unwrap();
    out.write_all(&(packets.len() as u32).to_le_bytes())
        .unwrap();
    out.write_all(&0u32.to_le_bytes()).unwrap();
    for p in &packets {
        out.write_all(&(p.data.len() as u32).to_le_bytes()).unwrap();
        out.write_all(&p.frame_number.to_le_bytes()).unwrap();
        out.write_all(&p.data).unwrap();
    }
    out
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        print_usage();
        return;
    }

    let input_path = &args[1];
    let output_path = &args[2];

    let mut config = EncodeConfig::default();
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--quality" => {
                i += 1;
                config.base_q_idx = args[i].parse().expect("invalid quality value");
            }
            "--keyint" => {
                i += 1;
                config.keyint = args[i].parse().expect("invalid keyint value");
            }
            "--b-frames" => {
                config.b_frames = true;
            }
            "--gop-size" => {
                i += 1;
                config.gop_size = args[i].parse().expect("invalid gop-size value");
            }
            "--bitrate" => {
                i += 1;
                let kbps: u64 = args[i].parse().expect("invalid bitrate value");
                config.target_bitrate = Some(kbps * 1000);
            }
            "--fps" => {
                i += 1;
                config.fps = parse_fps(&args[i]);
            }
            other => {
                eprintln!("Unknown option: {}", other);
                print_usage();
                return;
            }
        }
        i += 1;
    }

    println!("Loading frames from {}...", input_path);
    let frames = FramePixels::all_from_y4m_file(Path::new(input_path)).expect("Failed to load y4m");
    println!("Loaded {} frames", frames.len());

    println!(
        "Encoding (q={}, keyint={}, b_frames={}, gop_size={})...",
        config.base_q_idx, config.keyint, config.b_frames, config.gop_size
    );
    let ivf_data = write_ivf(&frames, &config);

    println!("Writing to {}...", output_path);
    fs::write(output_path, &ivf_data).expect("Failed to write IVF");
    println!("Done! ({} bytes)", ivf_data.len());
}
