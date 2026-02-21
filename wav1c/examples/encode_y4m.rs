use std::env;
use std::fs;
use std::path::Path;
use wav1c::y4m::FramePixels;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: cargo run --release --example encode_y4m <input.y4m> <output.ivf>");
        return;
    }

    let input_path = &args[1];
    let output_path = &args[2];

    println!("Loading frames from {}...", input_path);
    let frames = FramePixels::all_from_y4m_file(Path::new(input_path))
        .expect("Failed to load y4m");
    println!("Loaded {} frames", frames.len());

    println!("Encoding...");
    let ivf_data = wav1c::encode_av1_ivf_multi(&frames);
    
    println!("Writing to {}...", output_path);
    fs::write(output_path, &ivf_data).expect("Failed to write IVF");
    println!("Done!");
}
