use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 4 {
        eprintln!("Usage: wav1c <input.y4m> -o <output.ivf>");
        eprintln!("       wav1c <width> <height> <Y> <U> <V> -o <output.ivf>");
        process::exit(1);
    }

    let output = if args[1].ends_with(".y4m") {
        if args.len() < 4 || args[2] != "-o" {
            eprintln!("Usage: wav1c <input.y4m> -o <output.ivf>");
            process::exit(1);
        }
        let input_path = Path::new(&args[1]);
        let pixels = wav1c::y4m::FramePixels::from_y4m_file(input_path).unwrap_or_else(|e| {
            eprintln!("Error reading {}: {}", args[1], e);
            process::exit(1);
        });
        (wav1c::encode_av1_ivf_y4m(&pixels), args[3].clone())
    } else {
        if args.len() < 8 || args[6] != "-o" {
            eprintln!("Usage: wav1c <width> <height> <Y> <U> <V> -o <output.ivf>");
            process::exit(1);
        }

        let width: u32 = args[1].parse().unwrap_or_else(|_| {
            eprintln!("Error: width must be a positive integer");
            process::exit(1);
        });
        let height: u32 = args[2].parse().unwrap_or_else(|_| {
            eprintln!("Error: height must be a positive integer");
            process::exit(1);
        });
        let y: u8 = args[3].parse().unwrap_or_else(|_| {
            eprintln!("Error: Y must be 0-255");
            process::exit(1);
        });
        let u: u8 = args[4].parse().unwrap_or_else(|_| {
            eprintln!("Error: U must be 0-255");
            process::exit(1);
        });
        let v: u8 = args[5].parse().unwrap_or_else(|_| {
            eprintln!("Error: V must be 0-255");
            process::exit(1);
        });

        (wav1c::encode_av1_ivf(width, height, y, u, v), args[7].clone())
    };

    let (data, output_path) = output;

    let mut file = File::create(&output_path).unwrap_or_else(|e| {
        eprintln!("Error creating {}: {}", output_path, e);
        process::exit(1);
    });
    file.write_all(&data).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {}", output_path, e);
        process::exit(1);
    });

    eprintln!("Wrote {} bytes to {}", data.len(), output_path);
}
