use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process;

struct CliArgs {
    input: InputMode,
    output_path: String,
    config: wav1c::EncodeConfig,
}

enum InputMode {
    Y4m(String),
    Solid {
        width: u32,
        height: u32,
        y: u8,
        u: u8,
        v: u8,
    },
}

fn parse_option<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    for i in 0..args.len().saturating_sub(1) {
        if args[i] == flag {
            return args[i + 1].parse().ok();
        }
    }
    None
}

fn strip_options(args: &[String]) -> Vec<String> {
    let option_flags = ["-q", "--keyint"];
    let mut result = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if option_flags.contains(&args[i].as_str()) {
            i += 2;
        } else {
            result.push(args[i].clone());
            i += 1;
        }
    }
    result
}

fn parse_cli() -> CliArgs {
    let raw_args: Vec<String> = env::args().collect();

    let quality: u8 = parse_option(&raw_args, "-q").unwrap_or(wav1c::DEFAULT_BASE_Q_IDX);
    let keyint: usize = parse_option(&raw_args, "--keyint").unwrap_or(wav1c::DEFAULT_KEYINT);

    let args = strip_options(&raw_args);

    if args.len() < 4 {
        print_usage();
        process::exit(1);
    }

    let config = wav1c::EncodeConfig {
        base_q_idx: quality,
        keyint,
    };

    if args[1].ends_with(".y4m") {
        if args.len() < 4 || args[2] != "-o" {
            print_usage();
            process::exit(1);
        }
        CliArgs {
            input: InputMode::Y4m(args[1].clone()),
            output_path: args[3].clone(),
            config,
        }
    } else {
        if args.len() < 8 || args[6] != "-o" {
            print_usage();
            process::exit(1);
        }
        CliArgs {
            input: InputMode::Solid {
                width: args[1].parse().unwrap_or_else(|_| {
                    eprintln!("Error: width must be a positive integer");
                    process::exit(1);
                }),
                height: args[2].parse().unwrap_or_else(|_| {
                    eprintln!("Error: height must be a positive integer");
                    process::exit(1);
                }),
                y: args[3].parse().unwrap_or_else(|_| {
                    eprintln!("Error: Y must be 0-255");
                    process::exit(1);
                }),
                u: args[4].parse().unwrap_or_else(|_| {
                    eprintln!("Error: U must be 0-255");
                    process::exit(1);
                }),
                v: args[5].parse().unwrap_or_else(|_| {
                    eprintln!("Error: V must be 0-255");
                    process::exit(1);
                }),
            },
            output_path: args[7].clone(),
            config,
        }
    }
}

fn print_usage() {
    eprintln!("Usage: wav1c <input.y4m> -o <output.ivf> [options]");
    eprintln!("       wav1c <width> <height> <Y> <U> <V> -o <output.ivf> [options]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -q <0-255>      Quantizer index (0=best, 255=smallest, default=128)");
    eprintln!("  --keyint <N>    Keyframe interval in frames (default=25)");
}

fn main() {
    let cli = parse_cli();

    let frames = match &cli.input {
        InputMode::Y4m(path) => {
            wav1c::y4m::FramePixels::all_from_y4m_file(Path::new(path)).unwrap_or_else(|e| {
                eprintln!("Error reading {}: {}", path, e);
                process::exit(1);
            })
        }
        InputMode::Solid {
            width,
            height,
            y,
            u,
            v,
        } => vec![wav1c::y4m::FramePixels::solid(*width, *height, *y, *u, *v)],
    };

    let data = wav1c::encode(&frames, &cli.config);

    let mut file = File::create(&cli.output_path).unwrap_or_else(|e| {
        eprintln!("Error creating {}: {}", cli.output_path, e);
        process::exit(1);
    });
    file.write_all(&data).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {}", cli.output_path, e);
        process::exit(1);
    });

    let dq = wav1c::dequant::lookup_dequant(cli.config.base_q_idx);
    eprintln!(
        "Wrote {} bytes to {} (q={}, keyint={}, dc_dq={}, ac_dq={})",
        data.len(),
        cli.output_path,
        cli.config.base_q_idx,
        cli.config.keyint,
        dq.dc,
        dq.ac
    );
}
