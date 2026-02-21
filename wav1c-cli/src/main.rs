#![forbid(unsafe_code)]

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process;

use wav1c::{
    BitDepth, ColorDescription, ColorRange, ContentLightLevel, EncodeConfig, EncoderConfig,
    MasteringDisplayMetadata, VideoSignal,
};

struct CliArgs {
    input: InputMode,
    output_path: String,
    config: EncodeConfig,
    bit_depth_explicit: bool,
    color_range_explicit: bool,
}

enum InputMode {
    Y4m(String),
    Solid {
        width: u32,
        height: u32,
        y: u16,
        u: u16,
        v: u16,
    },
}

fn parse_bitrate(s: &str) -> Result<u64, String> {
    let (num, mult) = if let Some(n) = s.strip_suffix('k').or_else(|| s.strip_suffix('K')) {
        (n, 1_000u64)
    } else if let Some(n) = s.strip_suffix('m').or_else(|| s.strip_suffix('M')) {
        (n, 1_000_000u64)
    } else {
        (s, 1u64)
    };
    num.parse::<u64>()
        .map(|v| v * mult)
        .map_err(|_| format!("invalid bitrate: {s}"))
}

fn parse_color_range(s: &str) -> Result<ColorRange, String> {
    match s {
        "limited" | "tv" => Ok(ColorRange::Limited),
        "full" | "pc" => Ok(ColorRange::Full),
        _ => Err(format!("invalid color range: {s}")),
    }
}

fn parse_bit_depth(s: &str) -> Result<BitDepth, String> {
    let v: u8 = s.parse().map_err(|_| format!("invalid bit depth: {s}"))?;
    BitDepth::from_u8(v).ok_or_else(|| format!("unsupported bit depth: {v}"))
}

fn parse_mdcv(s: &str) -> Result<MasteringDisplayMetadata, String> {
    let values: Vec<&str> = s.split(',').collect();
    if values.len() != 10 {
        return Err("invalid --mdcv value: expected 10 comma-separated integers".to_owned());
    }
    let parse_u16 = |x: &str| -> Result<u16, String> {
        x.parse::<u16>()
            .map_err(|_| format!("invalid u16 value in --mdcv: {x}"))
    };
    let parse_u32 = |x: &str| -> Result<u32, String> {
        x.parse::<u32>()
            .map_err(|_| format!("invalid u32 value in --mdcv: {x}"))
    };
    Ok(MasteringDisplayMetadata {
        primaries: [
            [parse_u16(values[0])?, parse_u16(values[1])?],
            [parse_u16(values[2])?, parse_u16(values[3])?],
            [parse_u16(values[4])?, parse_u16(values[5])?],
        ],
        white_point: [parse_u16(values[6])?, parse_u16(values[7])?],
        max_luminance: parse_u32(values[8])?,
        min_luminance: parse_u32(values[9])?,
    })
}

fn parse_cli() -> CliArgs {
    let mut positional = Vec::new();
    let mut output_path: Option<String> = None;

    let mut config = EncodeConfig::default();
    let mut bit_depth_explicit = false;
    let mut color_range_explicit = false;
    let mut hdr10 = false;

    let mut cp: Option<u8> = None;
    let mut tc: Option<u8> = None;
    let mut mc: Option<u8> = None;
    let mut max_cll: Option<u16> = None;
    let mut max_fall: Option<u16> = None;
    let mut mdcv: Option<MasteringDisplayMetadata> = None;

    let mut args = env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" => output_path = Some(args.next().unwrap_or_default()),
            "-q" => {
                let value = args.next().unwrap_or_default();
                config.base_q_idx = value.parse().unwrap_or_else(|_| {
                    eprintln!("Error: invalid -q value: {value}");
                    process::exit(1);
                });
            }
            "--keyint" => {
                let value = args.next().unwrap_or_default();
                config.keyint = value.parse().unwrap_or_else(|_| {
                    eprintln!("Error: invalid --keyint value: {value}");
                    process::exit(1);
                });
            }
            "--bitrate" => {
                let value = args.next().unwrap_or_default();
                config.target_bitrate = Some(parse_bitrate(&value).unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }));
            }
            "--bit-depth" => {
                let value = args.next().unwrap_or_default();
                config.video_signal.bit_depth = parse_bit_depth(&value).unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    process::exit(1);
                });
                bit_depth_explicit = true;
            }
            "--hdr10" => {
                hdr10 = true;
            }
            "--color-range" => {
                let value = args.next().unwrap_or_default();
                config.video_signal.color_range = parse_color_range(&value).unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    process::exit(1);
                });
                color_range_explicit = true;
            }
            "--color-primaries" => {
                let value = args.next().unwrap_or_default();
                cp = Some(value.parse::<u8>().unwrap_or_else(|_| {
                    eprintln!("Error: invalid --color-primaries value: {value}");
                    process::exit(1);
                }));
            }
            "--transfer" => {
                let value = args.next().unwrap_or_default();
                tc = Some(value.parse::<u8>().unwrap_or_else(|_| {
                    eprintln!("Error: invalid --transfer value: {value}");
                    process::exit(1);
                }));
            }
            "--matrix" => {
                let value = args.next().unwrap_or_default();
                mc = Some(value.parse::<u8>().unwrap_or_else(|_| {
                    eprintln!("Error: invalid --matrix value: {value}");
                    process::exit(1);
                }));
            }
            "--max-cll" => {
                let value = args.next().unwrap_or_default();
                max_cll = Some(value.parse::<u16>().unwrap_or_else(|_| {
                    eprintln!("Error: invalid --max-cll value: {value}");
                    process::exit(1);
                }));
            }
            "--max-fall" => {
                let value = args.next().unwrap_or_default();
                max_fall = Some(value.parse::<u16>().unwrap_or_else(|_| {
                    eprintln!("Error: invalid --max-fall value: {value}");
                    process::exit(1);
                }));
            }
            "--mdcv" => {
                let value = args.next().unwrap_or_default();
                mdcv = Some(parse_mdcv(&value).unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }));
            }
            _ => positional.push(arg),
        }
    }

    if hdr10 {
        config.video_signal = VideoSignal::hdr10(config.video_signal.color_range);
        if !bit_depth_explicit {
            config.video_signal.bit_depth = BitDepth::Ten;
        }
    }

    match (cp, tc, mc) {
        (Some(color_primaries), Some(transfer_characteristics), Some(matrix_coefficients)) => {
            config.video_signal.color_description = Some(ColorDescription {
                color_primaries,
                transfer_characteristics,
                matrix_coefficients,
            });
        }
        (None, None, None) => {}
        _ => {
            eprintln!(
                "Error: --color-primaries, --transfer, and --matrix must be provided together"
            );
            process::exit(1);
        }
    }

    match (max_cll, max_fall) {
        (Some(max_content_light_level), Some(max_frame_average_light_level)) => {
            config.content_light = Some(ContentLightLevel {
                max_content_light_level,
                max_frame_average_light_level,
            });
        }
        (None, None) => {}
        _ => {
            eprintln!("Error: --max-cll and --max-fall must be provided together");
            process::exit(1);
        }
    }

    config.mastering_display = mdcv;

    let output_path = match output_path {
        Some(p) if !p.is_empty() => p,
        _ => {
            print_usage();
            process::exit(1);
        }
    };

    let input = if positional.len() == 1 && positional[0].ends_with(".y4m") {
        InputMode::Y4m(positional[0].clone())
    } else if positional.len() == 5 {
        let width = positional[0].parse::<u32>().unwrap_or_else(|_| {
            eprintln!("Error: width must be a positive integer");
            process::exit(1);
        });
        let height = positional[1].parse::<u32>().unwrap_or_else(|_| {
            eprintln!("Error: height must be a positive integer");
            process::exit(1);
        });
        let y = positional[2].parse::<u16>().unwrap_or_else(|_| {
            eprintln!("Error: Y must be an integer");
            process::exit(1);
        });
        let u = positional[3].parse::<u16>().unwrap_or_else(|_| {
            eprintln!("Error: U must be an integer");
            process::exit(1);
        });
        let v = positional[4].parse::<u16>().unwrap_or_else(|_| {
            eprintln!("Error: V must be an integer");
            process::exit(1);
        });
        InputMode::Solid {
            width,
            height,
            y,
            u,
            v,
        }
    } else {
        print_usage();
        process::exit(1);
    };

    CliArgs {
        input,
        output_path,
        config,
        bit_depth_explicit,
        color_range_explicit,
    }
}

fn print_usage() {
    eprintln!("Usage: wav1c <input.y4m> -o <output.ivf> [options]");
    eprintln!("       wav1c <width> <height> <Y> <U> <V> -o <output.ivf> [options]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -q <0-255>              Quantizer index (default=128)");
    eprintln!("  --keyint <N>            Keyframe interval (default=25)");
    eprintln!("  --bitrate <N>           Target bitrate (e.g. 500k, 2M)");
    eprintln!("  --bit-depth <8|10>      Signal bit depth");
    eprintln!("  --hdr10                 Apply HDR10 defaults (BT.2020/PQ/BT.2020NC)");
    eprintln!("  --color-range <limited|full>");
    eprintln!("  --color-primaries <u8>");
    eprintln!("  --transfer <u8>");
    eprintln!("  --matrix <u8>");
    eprintln!("  --max-cll <u16>         Content light level metadata");
    eprintln!("  --max-fall <u16>        Content light level metadata");
    eprintln!("  --mdcv <rx,ry,gx,gy,bx,by,wx,wy,max_lum,min_lum>");
}

fn main() {
    let mut cli = parse_cli();

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
        } => {
            let max = cli.config.video_signal.bit_depth.max_value();
            if *y > max || *u > max || *v > max {
                eprintln!(
                    "Error: solid values exceed {}-bit range (max {})",
                    cli.config.video_signal.bit_depth.bits(),
                    max
                );
                process::exit(1);
            }
            vec![wav1c::y4m::FramePixels::solid_with_bit_depth(
                *width,
                *height,
                *y,
                *u,
                *v,
                cli.config.video_signal.bit_depth,
                cli.config.video_signal.color_range,
            )]
        }
    };

    if frames.is_empty() {
        eprintln!("Error: no input frames");
        process::exit(1);
    }

    if matches!(cli.input, InputMode::Y4m(_)) {
        if !cli.bit_depth_explicit {
            cli.config.video_signal.bit_depth = frames[0].bit_depth;
        }
        if !cli.color_range_explicit {
            cli.config.video_signal.color_range = frames[0].color_range;
        }
    }

    let width = frames[0].width;
    let height = frames[0].height;
    let encoder_config = EncoderConfig::from(&cli.config);
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
        let dq = wav1c::dequant::lookup_dequant(
            cli.config.base_q_idx,
            cli.config.video_signal.bit_depth,
        );
        eprintln!(
            "Wrote {} bytes to {} ({} frames, q={}, keyint={}, bit_depth={}, dc_dq={}, ac_dq={})",
            output.len(),
            cli.output_path,
            frames.len(),
            cli.config.base_q_idx,
            cli.config.keyint,
            cli.config.video_signal.bit_depth.bits(),
            dq.dc,
            dq.ac
        );
    }
}
