use std::io::{self, Write};

use crate::video::{BitDepth, ColorRange, VideoSignal};

pub struct Mp4Config {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub config_obus: Vec<u8>,
    pub video_signal: VideoSignal,
}

pub struct Mp4Sample {
    pub data: Vec<u8>,
    pub is_sync: bool,
}

pub fn strip_temporal_delimiters(data: &[u8]) -> Vec<u8> {
    if data.len() >= 2 && data[0] == 0x12 && data[1] == 0x00 {
        data[2..].to_vec()
    } else {
        data.to_vec()
    }
}

pub fn fps_to_rational(fps: f64) -> (u32, u32) {
    let drop_frame = [
        (24000u32, 1001u32, 23.976),
        (30000, 1001, 29.97),
        (60000, 1001, 59.94),
    ];
    for (num, den, approx) in &drop_frame {
        if (fps - approx).abs() < 0.01 {
            return (*num, *den);
        }
    }
    let rounded = fps.round() as u32;
    if (fps - rounded as f64).abs() < 0.001 {
        return (rounded, 1);
    }
    ((fps * 1000.0).round() as u32, 1000)
}

pub fn write_mp4<W: Write>(
    w: &mut W,
    config: &Mp4Config,
    samples: &[Mp4Sample],
) -> io::Result<()> {
    let ftyp = build_ftyp();

    let mut mdat_payload = Vec::new();
    for s in samples {
        mdat_payload.extend_from_slice(&s.data);
    }
    let mdat = build_mdat(&mdat_payload);

    let data_offset = ftyp.len() as u32 + 8;
    let moov = build_moov(config, samples, data_offset);

    w.write_all(&ftyp)?;
    w.write_all(&mdat)?;
    w.write_all(&moov)?;
    Ok(())
}

fn box_wrap(box_type: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = (8 + payload.len()) as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(box_type);
    out.extend_from_slice(payload);
    out
}

fn full_box(box_type: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut inner = Vec::with_capacity(4 + payload.len());
    inner.push(version);
    inner.extend_from_slice(&flags.to_be_bytes()[1..4]);
    inner.extend_from_slice(payload);
    box_wrap(box_type, &inner)
}

fn build_ftyp() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"isom");
    p.extend_from_slice(&512u32.to_be_bytes());
    p.extend_from_slice(b"isom");
    p.extend_from_slice(b"av01");
    p.extend_from_slice(b"iso2");
    p.extend_from_slice(b"mp41");
    box_wrap(b"ftyp", &p)
}

fn build_mdat(data: &[u8]) -> Vec<u8> {
    box_wrap(b"mdat", data)
}

fn build_moov(config: &Mp4Config, samples: &[Mp4Sample], data_offset: u32) -> Vec<u8> {
    let num_samples = samples.len() as u64;
    let media_duration = num_samples * config.fps_den as u64;
    let total_ms = if config.fps_num > 0 {
        (media_duration * 1000) / config.fps_num as u64
    } else {
        0
    };

    let mvhd = build_mvhd(total_ms as u32);
    let trak = build_trak(config, samples, data_offset, media_duration as u32, total_ms as u32);

    let mut payload = Vec::new();
    payload.extend_from_slice(&mvhd);
    payload.extend_from_slice(&trak);
    box_wrap(b"moov", &payload)
}

fn build_mvhd(duration_ms: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&1000u32.to_be_bytes());
    p.extend_from_slice(&duration_ms.to_be_bytes());
    p.extend_from_slice(&0x00010000u32.to_be_bytes());
    p.extend_from_slice(&0x0100u16.to_be_bytes());
    p.extend_from_slice(&[0u8; 10]);
    let matrix: [u32; 9] = [
        0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000,
    ];
    for m in &matrix {
        p.extend_from_slice(&m.to_be_bytes());
    }
    p.extend_from_slice(&[0u8; 24]);
    p.extend_from_slice(&2u32.to_be_bytes());
    full_box(b"mvhd", 0, 0, &p)
}

fn build_trak(
    config: &Mp4Config,
    samples: &[Mp4Sample],
    data_offset: u32,
    media_duration: u32,
    duration_ms: u32,
) -> Vec<u8> {
    let tkhd = build_tkhd(config, duration_ms);
    let edts = build_edts(duration_ms);
    let mdia = build_mdia(config, samples, data_offset, media_duration);

    let mut payload = Vec::new();
    payload.extend_from_slice(&tkhd);
    payload.extend_from_slice(&edts);
    payload.extend_from_slice(&mdia);
    box_wrap(b"trak", &payload)
}

fn build_tkhd(config: &Mp4Config, duration_ms: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&duration_ms.to_be_bytes());
    p.extend_from_slice(&[0u8; 8]);
    p.extend_from_slice(&0u16.to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes());
    let matrix: [u32; 9] = [
        0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000,
    ];
    for m in &matrix {
        p.extend_from_slice(&m.to_be_bytes());
    }
    p.extend_from_slice(&(config.width << 16).to_be_bytes());
    p.extend_from_slice(&(config.height << 16).to_be_bytes());
    full_box(b"tkhd", 0, 3, &p)
}

fn build_edts(duration_ms: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&duration_ms.to_be_bytes());
    p.extend_from_slice(&0i32.to_be_bytes());
    p.extend_from_slice(&0x00010000u32.to_be_bytes());
    let elst = full_box(b"elst", 0, 0, &p);
    box_wrap(b"edts", &elst)
}

fn build_mdia(
    config: &Mp4Config,
    samples: &[Mp4Sample],
    data_offset: u32,
    media_duration: u32,
) -> Vec<u8> {
    let mdhd = build_mdhd(config.fps_num, media_duration);
    let hdlr = build_hdlr();
    let minf = build_minf(config, samples, data_offset);

    let mut payload = Vec::new();
    payload.extend_from_slice(&mdhd);
    payload.extend_from_slice(&hdlr);
    payload.extend_from_slice(&minf);
    box_wrap(b"mdia", &payload)
}

fn build_mdhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&timescale.to_be_bytes());
    p.extend_from_slice(&duration.to_be_bytes());
    p.extend_from_slice(&0x55C4u16.to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes());
    full_box(b"mdhd", 0, 0, &p)
}

fn build_hdlr() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(b"vide");
    p.extend_from_slice(&[0u8; 12]);
    p.extend_from_slice(b"VideoHandler\0");
    full_box(b"hdlr", 0, 0, &p)
}

fn build_minf(config: &Mp4Config, samples: &[Mp4Sample], data_offset: u32) -> Vec<u8> {
    let vmhd = full_box(b"vmhd", 0, 1, &[0u8; 8]);
    let dinf = build_dinf();
    let stbl = build_stbl(config, samples, data_offset);

    let mut payload = Vec::new();
    payload.extend_from_slice(&vmhd);
    payload.extend_from_slice(&dinf);
    payload.extend_from_slice(&stbl);
    box_wrap(b"minf", &payload)
}

fn build_dinf() -> Vec<u8> {
    let url = full_box(b"url ", 0, 1, &[]);

    let mut dref_payload = Vec::new();
    dref_payload.extend_from_slice(&1u32.to_be_bytes());
    dref_payload.extend_from_slice(&url);
    let dref = full_box(b"dref", 0, 0, &dref_payload);

    box_wrap(b"dinf", &dref)
}

fn build_stbl(config: &Mp4Config, samples: &[Mp4Sample], data_offset: u32) -> Vec<u8> {
    let stsd = build_stsd(config);
    let stts = build_stts(samples.len() as u32, config.fps_den);
    let stsc = build_stsc(samples.len() as u32);
    let stsz = build_stsz(samples);
    let stco = build_stco(data_offset);

    let mut payload = Vec::new();
    payload.extend_from_slice(&stsd);
    payload.extend_from_slice(&stts);
    payload.extend_from_slice(&stsc);
    payload.extend_from_slice(&stsz);
    payload.extend_from_slice(&stco);

    let has_inter = samples.iter().any(|s| !s.is_sync);
    if has_inter {
        let stss = build_stss(samples);
        payload.extend_from_slice(&stss);
    }

    box_wrap(b"stbl", &payload)
}

fn build_stsd(config: &Mp4Config) -> Vec<u8> {
    let av01 = build_av01(config);

    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&av01);
    full_box(b"stsd", 0, 0, &p)
}

fn build_av01(config: &Mp4Config) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 6]);
    p.extend_from_slice(&1u16.to_be_bytes());
    p.extend_from_slice(&[0u8; 16]);
    p.extend_from_slice(&(config.width as u16).to_be_bytes());
    p.extend_from_slice(&(config.height as u16).to_be_bytes());
    p.extend_from_slice(&0x00480000u32.to_be_bytes());
    p.extend_from_slice(&0x00480000u32.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&1u16.to_be_bytes());
    p.extend_from_slice(&[0u8; 32]);
    p.extend_from_slice(&0x0018u16.to_be_bytes());
    p.extend_from_slice(&0xFFFFu16.to_be_bytes());

    p.extend_from_slice(&build_av1c(config));
    p.extend_from_slice(&build_colr(config));
    p.extend_from_slice(&build_pasp());

    box_wrap(b"av01", &p)
}

fn build_av1c(config: &Mp4Config) -> Vec<u8> {
    let high_bitdepth = config.video_signal.bit_depth == BitDepth::Ten;

    let mut p = Vec::new();
    p.push(0x81);
    p.push(0x0D);
    p.push(if high_bitdepth { 0x4C } else { 0x0C });
    p.push(0x00);
    p.extend_from_slice(&config.config_obus);

    box_wrap(b"av1C", &p)
}

fn build_colr(config: &Mp4Config) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"nclx");

    if let Some(cd) = config.video_signal.color_description {
        p.extend_from_slice(&(cd.color_primaries as u16).to_be_bytes());
        p.extend_from_slice(&(cd.transfer_characteristics as u16).to_be_bytes());
        p.extend_from_slice(&(cd.matrix_coefficients as u16).to_be_bytes());
    } else {
        p.extend_from_slice(&2u16.to_be_bytes());
        p.extend_from_slice(&2u16.to_be_bytes());
        p.extend_from_slice(&2u16.to_be_bytes());
    }

    let full_range = match config.video_signal.color_range {
        ColorRange::Full => 0x80u8,
        ColorRange::Limited => 0x00u8,
    };
    p.push(full_range);

    box_wrap(b"colr", &p)
}

fn build_pasp() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&1u32.to_be_bytes());
    box_wrap(b"pasp", &p)
}

fn build_stts(num_samples: u32, sample_delta: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&num_samples.to_be_bytes());
    p.extend_from_slice(&sample_delta.to_be_bytes());
    full_box(b"stts", 0, 0, &p)
}

fn build_stsc(num_samples: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&num_samples.to_be_bytes());
    p.extend_from_slice(&1u32.to_be_bytes());
    full_box(b"stsc", 0, 0, &p)
}

fn build_stsz(samples: &[Mp4Sample]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for s in samples {
        p.extend_from_slice(&(s.data.len() as u32).to_be_bytes());
    }
    full_box(b"stsz", 0, 0, &p)
}

fn build_stco(chunk_offset: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&chunk_offset.to_be_bytes());
    full_box(b"stco", 0, 0, &p)
}

fn build_stss(samples: &[Mp4Sample]) -> Vec<u8> {
    let sync_indices: Vec<u32> = samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_sync)
        .map(|(i, _)| (i + 1) as u32)
        .collect();

    let mut p = Vec::new();
    p.extend_from_slice(&(sync_indices.len() as u32).to_be_bytes());
    for idx in &sync_indices {
        p.extend_from_slice(&idx.to_be_bytes());
    }
    full_box(b"stss", 0, 0, &p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_td_removes_prefix() {
        let data = vec![0x12, 0x00, 0x0A, 0x05, 0xFF];
        assert_eq!(strip_temporal_delimiters(&data), vec![0x0A, 0x05, 0xFF]);
    }

    #[test]
    fn strip_td_preserves_non_td() {
        let data = vec![0x0A, 0x05, 0xFF];
        assert_eq!(strip_temporal_delimiters(&data), data);
    }

    #[test]
    fn ftyp_is_32_bytes() {
        let ftyp = build_ftyp();
        assert_eq!(ftyp.len(), 32);
        assert_eq!(&ftyp[0..4], &32u32.to_be_bytes());
        assert_eq!(&ftyp[4..8], b"ftyp");
        assert_eq!(&ftyp[8..12], b"isom");
    }

    #[test]
    fn fps_rational_common_rates() {
        assert_eq!(fps_to_rational(25.0), (25, 1));
        assert_eq!(fps_to_rational(30.0), (30, 1));
        assert_eq!(fps_to_rational(24.0), (24, 1));
        assert_eq!(fps_to_rational(29.97), (30000, 1001));
        assert_eq!(fps_to_rational(23.976), (24000, 1001));
    }

    #[test]
    fn write_mp4_produces_valid_boxes() {
        let config = Mp4Config {
            width: 64,
            height: 64,
            fps_num: 25,
            fps_den: 1,
            config_obus: vec![0x0A, 0x05, 0x00, 0x00, 0x00, 0x01],
            video_signal: VideoSignal::default(),
        };
        let samples = vec![Mp4Sample {
            data: vec![0x0A, 0x05, 0xAA, 0xBB],
            is_sync: true,
        }];

        let mut buf = Vec::new();
        write_mp4(&mut buf, &config, &samples).unwrap();

        assert_eq!(&buf[4..8], b"ftyp");

        let mdat_offset = 32;
        assert_eq!(&buf[mdat_offset + 4..mdat_offset + 8], b"mdat");

        let mdat_size =
            u32::from_be_bytes(buf[mdat_offset..mdat_offset + 4].try_into().unwrap()) as usize;
        let moov_offset = mdat_offset + mdat_size;
        assert_eq!(&buf[moov_offset + 4..moov_offset + 8], b"moov");
    }

    #[test]
    fn stco_points_to_mdat_data() {
        let ftyp = build_ftyp();
        let data_offset = ftyp.len() as u32 + 8;
        assert_eq!(data_offset, 40);
    }
}
