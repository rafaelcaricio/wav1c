use std::env;
use std::fs::File;
use std::io::Write;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 6 || args[4] != "-o" {
        eprintln!("Usage: wav1c <Y> <U> <V> -o <output.ivf>");
        eprintln!("Example: wav1c 81 91 81 -o green.ivf");
        process::exit(1);
    }

    let _y: u8 = args[1].parse().unwrap_or_else(|_| {
        eprintln!("Error: Y must be 0-255");
        process::exit(1);
    });
    let _u: u8 = args[2].parse().unwrap_or_else(|_| {
        eprintln!("Error: U must be 0-255");
        process::exit(1);
    });
    let _v: u8 = args[3].parse().unwrap_or_else(|_| {
        eprintln!("Error: V must be 0-255");
        process::exit(1);
    });
    let output_path = &args[5];

    eprintln!("Warning: Color input is ignored in this iteration. Output is always solid green (Y=81, U=91, V=81).");

    let output = wav1c::encode_av1_ivf();

    let mut file = File::create(output_path).unwrap_or_else(|e| {
        eprintln!("Error creating {}: {}", output_path, e);
        process::exit(1);
    });
    file.write_all(&output).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {}", output_path, e);
        process::exit(1);
    });

    eprintln!("Wrote {} bytes to {}", output.len(), output_path);
}
