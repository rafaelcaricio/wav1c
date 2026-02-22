#![deny(unsafe_code)]

mod avif;
mod ivf;
mod mp4;

#[cfg(feature = "heic")]
mod heic;

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process;

use wav1c::{
    BitDepth, ColorDescription, ColorRange, ContentLightLevel, EncodeConfig, EncoderConfig, Fps,
    MasteringDisplayMetadata, VideoSignal,
};

struct CliArgs {
    input: InputMode,
    output_path: String,
    config: EncodeConfig,
    fps_explicit: bool,
    bit_depth_explicit: bool,
    color_range_explicit: bool,
    #[cfg(feature = "heic")]
    color_description_explicit: bool,
    hdr10_requested: bool,
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
    Grid {
        width: u32,
        height: u32,
    },
    #[cfg(feature = "heic")]
    Heic(String),
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

fn parse_fps(s: &str) -> Result<Fps, String> {
    if s.contains('.') {
        return Err(format!(
            "invalid --fps value: {s} (use INT or NUM/DEN, e.g. 30 or 30000/1001)"
        ));
    }

    if let Some((num_s, den_s)) = s.split_once('/') {
        let num = num_s
            .parse::<u32>()
            .map_err(|_| format!("invalid --fps numerator: {num_s}"))?;
        let den = den_s
            .parse::<u32>()
            .map_err(|_| format!("invalid --fps denominator: {den_s}"))?;
        return Fps::new(num, den).map_err(|e| format!("invalid --fps value: {e}"));
    }

    let fps = s
        .parse::<u32>()
        .map_err(|_| format!("invalid --fps value: {s} (use INT or NUM/DEN)"))?;
    Fps::from_int(fps).map_err(|e| format!("invalid --fps value: {e}"))
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
    let mut fps_explicit = false;
    let mut bit_depth_explicit = false;
    let mut color_range_explicit = false;
    let mut hdr10 = false;

    let mut cp: Option<u8> = None;
    let mut tc: Option<u8> = None;
    let mut mc: Option<u8> = None;
    #[cfg(feature = "heic")]
    let mut color_description_explicit = false;
    let mut max_cll: Option<u16> = None;
    let mut max_fall: Option<u16> = None;
    let mut mdcv: Option<MasteringDisplayMetadata> = None;
    let mut pattern: Option<String> = None;

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
            "--fps" => {
                let value = args.next().unwrap_or_default();
                config.fps = parse_fps(&value).unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    process::exit(1);
                });
                fps_explicit = true;
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
            "--pattern" => {
                pattern = Some(args.next().unwrap_or_default());
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
            #[cfg(feature = "heic")]
            {
                color_description_explicit = true;
            }
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
    } else if positional.len() == 1
        && (positional[0].ends_with(".heic") || positional[0].ends_with(".heif"))
    {
        #[cfg(feature = "heic")]
        {
            InputMode::Heic(positional[0].clone())
        }
        #[cfg(not(feature = "heic"))]
        {
            eprintln!("Error: HEIC input requires building with --features heic (needs libheif)");
            process::exit(1);
        }
    } else if positional.len() == 2 && pattern.is_some() {
        let width = positional[0].parse::<u32>().unwrap_or_else(|_| {
            eprintln!("Error: width must be a positive integer");
            process::exit(1);
        });
        let height = positional[1].parse::<u32>().unwrap_or_else(|_| {
            eprintln!("Error: height must be a positive integer");
            process::exit(1);
        });
        match pattern.as_deref() {
            Some("grid") => InputMode::Grid { width, height },
            Some(p) => {
                eprintln!("Error: unknown pattern: {p}");
                eprintln!("Available patterns: grid");
                process::exit(1);
            }
            None => unreachable!(),
        }
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
        fps_explicit,
        bit_depth_explicit,
        color_range_explicit,
        #[cfg(feature = "heic")]
        color_description_explicit,
        hdr10_requested: hdr10,
    }
}

fn print_usage() {
    eprintln!("Usage: wav1c <input.y4m|heic> -o <output.ivf|mp4|avif> [options]");
    eprintln!("       wav1c <width> <height> <Y> <U> <V> -o <output.ivf|mp4|avif> [options]");
    eprintln!("       wav1c <width> <height> --pattern <name> -o <output.ivf|mp4|avif> [options]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -q <0-255>              Quantizer index (default=128)");
    eprintln!("  --keyint <N>            Keyframe interval (default=25)");
    eprintln!("  --bitrate <N>           Target bitrate (e.g. 500k, 2M)");
    eprintln!("  --fps <INT|NUM/DEN>     Frame rate (e.g. 30 or 30000/1001)");
    eprintln!("  --bit-depth <8|10>      Signal bit depth");
    eprintln!("  --hdr10                 Apply HDR10 defaults (BT.2020/PQ/BT.2020NC)");
    eprintln!("  --color-range <limited|full>");
    eprintln!("  --color-primaries <u8>");
    eprintln!("  --transfer <u8>");
    eprintln!("  --matrix <u8>");
    eprintln!("  --max-cll <u16>         Content light level metadata");
    eprintln!("  --max-fall <u16>        Content light level metadata");
    eprintln!("  --mdcv <rx,ry,gx,gy,bx,by,wx,wy,max_lum,min_lum>");
    eprintln!("  --pattern <name>        Test pattern (grid)");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Ivf,
    Mp4,
    Avif,
}

fn validate_bit_depth_constraints(
    hdr10_requested: bool,
    input_bit_depth: BitDepth,
    signal_bit_depth: BitDepth,
) -> Result<(), String> {
    if hdr10_requested && input_bit_depth == BitDepth::Eight {
        return Err(
            "--hdr10 requires 10-bit source frames. Hint: for 8-bit Apple HEIC gain-map input \
             targeting AVIF, omit --hdr10 to use the auto tmap path; for HDR10, provide a true \
             10-bit source."
                .to_owned(),
        );
    }
    if input_bit_depth != signal_bit_depth {
        return Err(format!(
            "input bit depth {} does not match configured signal bit depth {}. \
             Automatic bit-depth scaling was removed.",
            input_bit_depth.bits(),
            signal_bit_depth.bits(),
        ));
    }
    Ok(())
}

#[cfg(feature = "heic")]
fn should_use_auto_heic_gain_map(
    is_heic_input: bool,
    output_format: OutputFormat,
    source_bit_depth: BitDepth,
    has_apple_gain_map_aux: bool,
    hdr10_requested: bool,
) -> bool {
    is_heic_input
        && output_format == OutputFormat::Avif
        && source_bit_depth == BitDepth::Eight
        && has_apple_gain_map_aux
        && !hdr10_requested
}

fn detect_format(path: &str) -> OutputFormat {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp4") | Some("m4v") => OutputFormat::Mp4,
        Some("avif") => OutputFormat::Avif,
        _ => OutputFormat::Ivf,
    }
}

fn validate_output_dimensions(format: OutputFormat, width: u32, height: u32) -> Result<(), String> {
    let max = u16::MAX as u32;
    if (format == OutputFormat::Ivf || format == OutputFormat::Mp4) && (width > max || height > max)
    {
        let label = if format == OutputFormat::Ivf {
            "IVF"
        } else {
            "MP4"
        };
        return Err(format!(
            "{label} output does not support dimensions above 65535x65535 (got {}x{}). \
             Hint: choose AVIF output for large dimensions.",
            width, height
        ));
    }
    Ok(())
}

fn main() {
    let mut cli = parse_cli();
    let format = detect_format(&cli.output_path);

    #[cfg(feature = "heic")]
    let mut heic_gain_map: Option<wav1c::y4m::FramePixels> = None;
    #[cfg(feature = "heic")]
    let mut heic_apple_hdr_scalars: Option<heic::AppleHdrScalars> = None;
    #[cfg(feature = "heic")]
    let mut heic_apple_hdr_error: Option<String> = None;
    #[cfg(feature = "heic")]
    let mut heic_gain_map_has_xmp_version = false;
    #[cfg(feature = "heic")]
    let mut heic_source_nclx: Option<heic::SourceNclx> = None;

    let mut source_fps: Option<Fps> = None;
    let frames = match &cli.input {
        InputMode::Y4m(path) => {
            let (frames, fps) =
                wav1c::y4m::FramePixels::all_from_y4m_file_with_fps(Path::new(path))
                    .unwrap_or_else(|e| {
                        eprintln!("Error reading {}: {}", path, e);
                        process::exit(1);
                    });
            source_fps = fps;
            frames
        }
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
        InputMode::Grid { width, height } => {
            let bd = cli.config.video_signal.bit_depth;
            let cr = cli.config.video_signal.color_range;
            let neutral_chroma = if bd == BitDepth::Ten { 512 } else { 128 };
            let (bright_y, dark_y) = match (bd, cr) {
                (BitDepth::Ten, ColorRange::Limited) => (940, 504),
                (BitDepth::Ten, ColorRange::Full) => (1023, 520),
                (BitDepth::Eight, ColorRange::Limited) => (235, 130),
                (BitDepth::Eight, ColorRange::Full) => (255, 134),
            };
            vec![wav1c::y4m::FramePixels::grid(
                *width,
                *height,
                64,
                [bright_y, neutral_chroma, neutral_chroma],
                [dark_y, neutral_chroma, neutral_chroma],
                bd,
                cr,
            )]
        }
        #[cfg(feature = "heic")]
        InputMode::Heic(path) => {
            let decoded = heic::decode_heic(path).unwrap_or_else(|e| {
                eprintln!("Error reading HEIC {}: {}", path, e);
                process::exit(1);
            });
            heic_gain_map = decoded.gain_map;
            heic_apple_hdr_scalars = decoded.apple_hdr_scalars;
            heic_apple_hdr_error = decoded.apple_hdr_scalars_error;
            heic_gain_map_has_xmp_version = decoded.gain_map_has_xmp_version;
            heic_source_nclx = decoded.source_nclx;
            vec![decoded.base]
        }
    };

    if frames.is_empty() {
        eprintln!("Error: no input frames");
        process::exit(1);
    }

    let is_file_input = match &cli.input {
        InputMode::Y4m(_) => true,
        #[cfg(feature = "heic")]
        InputMode::Heic(_) => true,
        _ => false,
    };

    if is_file_input {
        if !cli.bit_depth_explicit {
            cli.config.video_signal.bit_depth = frames[0].bit_depth;
        }
        if !cli.color_range_explicit {
            cli.config.video_signal.color_range = frames[0].color_range;
        }
        if !cli.fps_explicit
            && matches!(&cli.input, InputMode::Y4m(_))
            && let Some(fps) = source_fps
        {
            cli.config.fps = fps;
        }
    }

    if let Err(message) = validate_bit_depth_constraints(
        cli.hdr10_requested,
        frames[0].bit_depth,
        cli.config.video_signal.bit_depth,
    ) {
        eprintln!("Error: {message}");
        process::exit(1);
    }

    #[cfg(feature = "heic")]
    let use_heic_gain_map_path = should_use_auto_heic_gain_map(
        matches!(cli.input, InputMode::Heic(_)),
        format,
        frames[0].bit_depth,
        heic_gain_map.is_some(),
        cli.hdr10_requested,
    );
    #[cfg(not(feature = "heic"))]
    let use_heic_gain_map_path = false;

    #[cfg(feature = "heic")]
    if use_heic_gain_map_path {
        if !heic_gain_map_has_xmp_version {
            eprintln!(
                "Error: Apple HDR gain-map auxiliary image is present but required XMP \
                 field HDRGainMapVersion is missing."
            );
            process::exit(1);
        }

        if !cli.color_description_explicit {
            let source_color_description = heic_source_nclx.and_then(|n| n.color_description);
            if let Some(color_description) = source_color_description {
                cli.config.video_signal.color_description = Some(color_description);
                let source_color_range = heic_source_nclx.map(|source| source.color_range);
                if let (false, Some(color_range)) = (cli.color_range_explicit, source_color_range) {
                    cli.config.video_signal.color_range = color_range;
                }
            } else {
                cli.config.video_signal.color_description = Some(ColorDescription {
                    color_primaries: 1,
                    transfer_characteristics: 13,
                    matrix_coefficients: 6,
                });
                if !cli.color_range_explicit {
                    cli.config.video_signal.color_range = ColorRange::Full;
                }
            }
        }
    }

    let width = frames[0].width;
    let height = frames[0].height;
    if let Err(message) = validate_output_dimensions(format, width, height) {
        eprintln!("Error: {message}");
        process::exit(1);
    }

    let encoder_config = EncoderConfig::from(&cli.config);
    let mut encoder = wav1c::Encoder::new(width, height, encoder_config).unwrap_or_else(|e| {
        eprintln!("Error creating encoder: {:?}", e);
        process::exit(1);
    });

    let mut packets: Vec<wav1c::Packet> = Vec::new();

    for frame in &frames {
        encoder.send_frame(frame).unwrap_or_else(|e| {
            eprintln!("Error encoding frame: {:?}", e);
            process::exit(1);
        });

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
            packets.push(packet);
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
        packets.push(packet);
    }

    let mut file = File::create(&cli.output_path).unwrap_or_else(|e| {
        eprintln!("Error creating {}: {}", cli.output_path, e);
        process::exit(1);
    });

    let output_size = match format {
        OutputFormat::Ivf => {
            let mut output = Vec::new();
            ivf::write_ivf_header(
                &mut output,
                width,
                height,
                packets.len() as u32,
                cli.config.fps.num,
                cli.config.fps.den,
            )
            .unwrap();
            for p in &packets {
                ivf::write_ivf_frame(&mut output, p.frame_number, &p.data).unwrap();
            }
            file.write_all(&output).unwrap_or_else(|e| {
                eprintln!("Error writing {}: {}", cli.output_path, e);
                process::exit(1);
            });
            output.len()
        }
        OutputFormat::Mp4 => {
            let config_obus = encoder.headers();
            let samples: Vec<mp4::Mp4Sample> = packets
                .iter()
                .map(|p| mp4::Mp4Sample {
                    data: mp4::strip_temporal_delimiters(&p.data),
                    is_sync: p.frame_type == wav1c::FrameType::Key,
                })
                .collect();
            let mp4_config = mp4::Mp4Config {
                width,
                height,
                fps_num: cli.config.fps.num,
                fps_den: cli.config.fps.den,
                config_obus,
                video_signal: cli.config.video_signal,
            };
            let mut output = Vec::new();
            mp4::write_mp4(&mut output, &mp4_config, &samples).unwrap();
            file.write_all(&output).unwrap_or_else(|e| {
                eprintln!("Error writing {}: {}", cli.output_path, e);
                process::exit(1);
            });
            output.len()
        }
        OutputFormat::Avif => {
            if packets.is_empty() {
                eprintln!("Error: no frames to encode");
                process::exit(1);
            }
            let mut output = Vec::new();
            if use_heic_gain_map_path {
                #[cfg(feature = "heic")]
                {
                    let gain_map_frame = heic_gain_map.as_ref().unwrap_or_else(|| {
                        eprintln!(
                            "Error: HEIC gain-map path selected but no Apple HDR gain-map \
                             auxiliary image was decoded."
                        );
                        process::exit(1);
                    });
                    if gain_map_frame.bit_depth != BitDepth::Eight {
                        eprintln!(
                            "Error: Apple HDR gain-map auxiliary image must be 8-bit, got {}-bit.",
                            gain_map_frame.bit_depth.bits()
                        );
                        process::exit(1);
                    }

                    let hdr_scalars = heic_apple_hdr_scalars.unwrap_or_else(|| {
                        if let Some(err) = heic_apple_hdr_error.as_deref() {
                            eprintln!(
                                "Error: could not parse Apple HDR MakerNote tags (0x0021/0x0030): \
                                 {err}"
                            );
                        } else {
                            eprintln!(
                                "Error: missing Apple HDR MakerNote tags (0x0021 HDRHeadroom \
                                 and 0x0030 HDRGain)."
                            );
                        }
                        process::exit(1);
                    });

                    let tmap_metadata = avif::derive_tmap_metadata_from_apple(
                        hdr_scalars.hdr_headroom.numerator,
                        hdr_scalars.hdr_headroom.denominator,
                        hdr_scalars.hdr_gain.numerator,
                        hdr_scalars.hdr_gain.denominator,
                    )
                    .unwrap_or_else(|e| {
                        eprintln!("Error: failed to derive tmap metadata from Apple HDR tags: {e}");
                        process::exit(1);
                    });
                    let tmap_payload =
                        avif::build_tmap_payload(&tmap_metadata).unwrap_or_else(|e| {
                            eprintln!("Error: failed to serialize tmap metadata payload: {e}");
                            process::exit(1);
                        });

                    let mut gain_map_encode_config = cli.config.clone();
                    gain_map_encode_config.target_bitrate = None;
                    gain_map_encode_config.video_signal = VideoSignal {
                        bit_depth: BitDepth::Eight,
                        color_range: ColorRange::Full,
                        color_description: Some(ColorDescription {
                            color_primaries: 2,
                            transfer_characteristics: 2,
                            matrix_coefficients: 2,
                        }),
                    };
                    let gain_encoder_config = EncoderConfig::from(&gain_map_encode_config);
                    let mut gain_encoder = wav1c::Encoder::new(
                        gain_map_frame.width,
                        gain_map_frame.height,
                        gain_encoder_config,
                    )
                    .unwrap_or_else(|e| {
                        eprintln!("Error creating gain-map encoder: {:?}", e);
                        process::exit(1);
                    });

                    let mut gain_packets = Vec::new();
                    gain_encoder.send_frame(gain_map_frame).unwrap_or_else(|e| {
                        eprintln!("Error encoding gain-map frame: {:?}", e);
                        process::exit(1);
                    });
                    while let Some(packet) = gain_encoder.receive_packet() {
                        gain_packets.push(packet);
                    }
                    gain_encoder.flush();
                    while let Some(packet) = gain_encoder.receive_packet() {
                        gain_packets.push(packet);
                    }
                    if gain_packets.is_empty() {
                        eprintln!("Error: gain-map encoder produced no frames");
                        process::exit(1);
                    }

                    let base_avif_config = avif::AvifConfig {
                        width,
                        height,
                        config_obus: encoder.headers(),
                        video_signal: cli.config.video_signal,
                    };
                    let gain_map_avif_config = avif::AvifConfig {
                        width: gain_map_frame.width,
                        height: gain_map_frame.height,
                        config_obus: gain_encoder.headers(),
                        video_signal: gain_map_encode_config.video_signal,
                    };
                    avif::write_avif_with_tmap_gain_map(
                        &mut output,
                        &base_avif_config,
                        &packets[0].data,
                        &gain_map_avif_config,
                        &gain_packets[0].data,
                        &tmap_payload,
                    )
                    .unwrap_or_else(|e| {
                        eprintln!("Error writing gain-map AVIF: {e}");
                        process::exit(1);
                    });
                }
                #[cfg(not(feature = "heic"))]
                {
                    unreachable!("HEIC gain-map path is unavailable without heic feature");
                }
            } else {
                let avif_config = avif::AvifConfig {
                    width,
                    height,
                    config_obus: encoder.headers(),
                    video_signal: cli.config.video_signal,
                };
                avif::write_avif(&mut output, &avif_config, &packets[0].data).unwrap();
            }
            file.write_all(&output).unwrap_or_else(|e| {
                eprintln!("Error writing {}: {}", cli.output_path, e);
                process::exit(1);
            });
            output.len()
        }
    };

    eprintln!();
    if let Some(stats) = encoder.rate_control_stats() {
        eprintln!(
            "Wrote {} bytes to {} ({} frames, target={}kbps, avg_qp={}, buffer={}%, keyint={})",
            output_size,
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
            output_size,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fps_accepts_integer() {
        let fps = parse_fps("30").expect("expected integer fps to parse");
        assert_eq!(fps, Fps { num: 30, den: 1 });
    }

    #[test]
    fn parse_fps_accepts_num_den() {
        let fps = parse_fps("30000/1001").expect("expected fraction fps to parse");
        assert_eq!(
            fps,
            Fps {
                num: 30_000,
                den: 1_001
            }
        );
    }

    #[test]
    fn parse_fps_rejects_decimal() {
        let err = parse_fps("29.97").expect_err("expected decimal fps to fail");
        assert!(err.contains("use INT or NUM/DEN"));
    }

    #[test]
    fn hdr10_on_8bit_input_is_rejected() {
        let err = validate_bit_depth_constraints(true, BitDepth::Eight, BitDepth::Ten)
            .expect_err("expected rejection");
        assert!(err.contains("--hdr10 requires 10-bit source frames"));
    }

    #[test]
    fn bit_depth_mismatch_is_rejected_without_scaling() {
        let err = validate_bit_depth_constraints(false, BitDepth::Eight, BitDepth::Ten)
            .expect_err("expected mismatch rejection");
        assert!(err.contains("Automatic bit-depth scaling was removed"));
    }

    #[test]
    #[cfg(feature = "heic")]
    fn heic_avif_auto_gain_map_path_trigger_conditions() {
        assert!(should_use_auto_heic_gain_map(
            true,
            OutputFormat::Avif,
            BitDepth::Eight,
            true,
            false
        ));
        assert!(!should_use_auto_heic_gain_map(
            true,
            OutputFormat::Avif,
            BitDepth::Eight,
            true,
            true
        ));
        assert!(!should_use_auto_heic_gain_map(
            true,
            OutputFormat::Ivf,
            BitDepth::Eight,
            true,
            false
        ));
        assert!(!should_use_auto_heic_gain_map(
            true,
            OutputFormat::Avif,
            BitDepth::Ten,
            true,
            false
        ));
        assert!(!should_use_auto_heic_gain_map(
            false,
            OutputFormat::Avif,
            BitDepth::Eight,
            true,
            false
        ));
        assert!(!should_use_auto_heic_gain_map(
            true,
            OutputFormat::Avif,
            BitDepth::Eight,
            false,
            false
        ));
    }

    #[test]
    fn oversized_ivf_output_is_rejected() {
        let err = validate_output_dimensions(OutputFormat::Ivf, 70_000, 1_000)
            .expect_err("expected IVF rejection");
        assert!(err.contains("IVF output does not support dimensions above 65535x65535"));
    }

    #[test]
    fn oversized_mp4_output_is_rejected() {
        let err = validate_output_dimensions(OutputFormat::Mp4, 1_000, 70_000)
            .expect_err("expected MP4 rejection");
        assert!(err.contains("MP4 output does not support dimensions above 65535x65535"));
    }

    #[test]
    fn oversized_avif_output_is_allowed() {
        validate_output_dimensions(OutputFormat::Avif, 70_000, 70_000).expect("expected AVIF ok");
    }
}
