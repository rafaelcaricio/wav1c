use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process;

fn parse_quality(args: &[String]) -> u8 {
    for i in 0..args.len().saturating_sub(1) {
        if args[i] == "-q" {
            return args[i + 1].parse().unwrap_or_else(|_| {
                eprintln!("Error: quality must be 0-255");
                process::exit(1);
            });
        }
    }
    wav1c::DEFAULT_BASE_Q_IDX
}

fn strip_quality_args(args: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-q" {
            i += 2;
        } else {
            result.push(args[i].clone());
            i += 1;
        }
    }
    result
}

fn main() {
    let raw_args: Vec<String> = env::args().collect();
    let quality = parse_quality(&raw_args);
    let args = strip_quality_args(&raw_args);

    if args.len() < 4 {
        eprintln!("Usage: wav1c <input.y4m> -o <output.ivf> [-q <0-255>]");
        eprintln!("       wav1c <width> <height> <Y> <U> <V> -o <output.ivf> [-q <0-255>]");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  -q <0-255>  Quantizer index (0=best quality, 255=smallest, default=128)");
        process::exit(1);
    }

    let (data, output_path) = if args[1].ends_with(".y4m") {
        if args[2] != "-o" {
            eprintln!("Usage: wav1c <input.y4m> -o <output.ivf> [-q <0-255>]");
            process::exit(1);
        }
        let input_path = Path::new(&args[1]);
        let frames = wav1c::y4m::FramePixels::all_from_y4m_file(input_path).unwrap_or_else(|e| {
            eprintln!("Error reading {}: {}", args[1], e);
            process::exit(1);
        });
        (
            wav1c::encode_av1_ivf_multi_with_quality(&frames, quality),
            args[3].clone(),
        )
    } else {
        if args.len() < 8 || args[6] != "-o" {
            eprintln!("Usage: wav1c <width> <height> <Y> <U> <V> -o <output.ivf> [-q <0-255>]");
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

        let pixels = wav1c::y4m::FramePixels::solid(width, height, y, u, v);
        (
            wav1c::encode_av1_ivf_multi_with_quality(&[pixels], quality),
            args[7].clone(),
        )
    };

    let mut file = File::create(&output_path).unwrap_or_else(|e| {
        eprintln!("Error creating {}: {}", output_path, e);
        process::exit(1);
    });
    file.write_all(&data).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {}", output_path, e);
        process::exit(1);
    });

    let dq = wav1c::dequant::lookup_dequant(quality);
    eprintln!(
        "Wrote {} bytes to {} (q={}, dc_dq={}, ac_dq={})",
        data.len(),
        output_path,
        quality,
        dq.dc,
        dq.ac
    );
}
