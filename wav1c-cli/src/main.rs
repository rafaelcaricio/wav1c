#![forbid(unsafe_code)]

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

fn parse_bitrate(args: &[String]) -> Option<u64> {
    for i in 0..args.len().saturating_sub(1) {
        if args[i] == "--bitrate" {
            let s = &args[i + 1];
            let (num, mult) = if let Some(n) = s.strip_suffix('k').or_else(|| s.strip_suffix('K')) {
                (n, 1_000u64)
            } else if let Some(n) = s.strip_suffix('m').or_else(|| s.strip_suffix('M')) {
                (n, 1_000_000u64)
            } else {
                (s.as_str(), 1u64)
            };
            return num.parse::<u64>().ok().map(|v| v * mult);
        }
    }
    None
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
    let option_flags = ["-q", "--keyint", "--bitrate"];
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
    let bitrate: Option<u64> = parse_bitrate(&raw_args);

    let args = strip_options(&raw_args);

    if args.len() < 4 {
        print_usage();
        process::exit(1);
    }

    let config = wav1c::EncodeConfig {
        base_q_idx: quality,
        keyint,
        target_bitrate: bitrate,
        ..Default::default()
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
    eprintln!("  --bitrate <N>   Target bitrate (e.g., 500k, 2M). Overrides -q");
}

fn main() {
    let cli = parse_cli();

    let frames = match &cli.input {
        InputMode::Y4m(path) => wav1c::y4m::FramePixels::all_from_y4m_file(Path::new(path))
            .unwrap_or_else(|e| {
                eprintln!("Error reading {}: {}", path, e);
                process::exit(1);
            }),
        InputMode::Solid {
            width,
            height,
            y,
            u,
            v,
        } => vec![wav1c::y4m::FramePixels::solid(*width, *height, *y, *u, *v)],
    };

    let width = frames[0].width;
    let height = frames[0].height;
    let encoder_config = wav1c::EncoderConfig::from(&cli.config);
    let mut encoder = wav1c::Encoder::new(width, height, encoder_config).unwrap_or_else(|e| {
        eprintln!("Error creating encoder: {:?}", e);
        process::exit(1);
    });

    let mut output = Vec::new();
    wav1c::ivf::write_ivf_header(
        &mut output,
        width as u16,
        height as u16,
        frames.len() as u32,
    )
    .unwrap();

    for frame in &frames {
        encoder.send_frame(frame).unwrap_or_else(|e| {
            eprintln!("Error encoding frame: {:?}", e);
            process::exit(1);
        });

        if let Some(packet) = encoder.receive_packet() {
            let frame_type_str = match packet.frame_type {
                wav1c::FrameType::Key => "KEY",
                wav1c::FrameType::Inter => "INTER",
            };
            eprintln!(
                "frame {:>4}  {:>5}  {} bytes",
                packet.frame_number,
                frame_type_str,
                packet.data.len()
            );

            wav1c::ivf::write_ivf_frame(&mut output, packet.frame_number, &packet.data).unwrap();
        }
    }

    encoder.flush();

    while let Some(packet) = encoder.receive_packet() {
        let frame_type_str = match packet.frame_type {
            wav1c::FrameType::Key => "KEY",
            wav1c::FrameType::Inter => "INTER",
        };
        eprintln!(
            "frame {:>4}  {:>5}  {} bytes",
            packet.frame_number,
            frame_type_str,
            packet.data.len()
        );
        wav1c::ivf::write_ivf_frame(&mut output, packet.frame_number, &packet.data).unwrap();
    }

    let mut file = File::create(&cli.output_path).unwrap_or_else(|e| {
        eprintln!("Error creating {}: {}", cli.output_path, e);
        process::exit(1);
    });
    file.write_all(&output).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {}", cli.output_path, e);
        process::exit(1);
    });

    eprintln!();
    if let Some(stats) = encoder.rate_control_stats() {
        eprintln!(
            "Wrote {} bytes to {} ({} frames, target={}kbps, avg_qp={}, buffer={}%, keyint={})",
            output.len(),
            cli.output_path,
            frames.len(),
            stats.target_bitrate / 1000,
            stats.avg_qp,
            stats.buffer_fullness_pct,
            cli.config.keyint
        );
    } else {
        let dq = wav1c::dequant::lookup_dequant(cli.config.base_q_idx);
        eprintln!(
            "Wrote {} bytes to {} ({} frames, q={}, keyint={}, dc_dq={}, ac_dq={})",
            output.len(),
            cli.output_path,
            frames.len(),
            cli.config.base_q_idx,
            cli.config.keyint,
            dq.dc,
            dq.ac
        );
    }
}
