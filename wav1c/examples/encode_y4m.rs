use std::env;
use std::fs;
use std::path::Path;
use wav1c::y4m::FramePixels;
use wav1c::EncodeConfig;

fn print_usage() {
    eprintln!("Usage: encode_y4m <input.y4m> <output.ivf> [options]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --quality N      Base quantizer index (0-255, default: 128)");
    eprintln!("  --keyint N       Keyframe interval (default: 25)");
    eprintln!("  --b-frames       Enable B-frame encoding (experimental)");
    eprintln!("  --gop-size N     Mini-GOP size for B-frames (default: 3)");
    eprintln!("  --bitrate N      Target bitrate in kbps (enables rate control)");
    eprintln!("  --fps N          Frames per second (default: 25)");
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
                config.fps = args[i].parse().expect("invalid fps value");
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
    let frames = FramePixels::all_from_y4m_file(Path::new(input_path))
        .expect("Failed to load y4m");
    println!("Loaded {} frames", frames.len());

    println!("Encoding (q={}, keyint={}, b_frames={}, gop_size={})...",
        config.base_q_idx, config.keyint, config.b_frames, config.gop_size);
    let ivf_data = wav1c::encode(&frames, &config);

    println!("Writing to {}...", output_path);
    fs::write(output_path, &ivf_data).expect("Failed to write IVF");
    println!("Done! ({} bytes)", ivf_data.len());
}
