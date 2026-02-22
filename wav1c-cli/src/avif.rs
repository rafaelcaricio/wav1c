use std::io::{self, Write};

use crate::mp4::{box_wrap, build_av1c, build_colr, full_box, strip_temporal_delimiters};
use wav1c::{BitDepth, VideoSignal};

pub struct AvifConfig {
    pub width: u32,
    pub height: u32,
    pub config_obus: Vec<u8>,
    pub video_signal: VideoSignal,
}

pub fn write_avif<W: Write>(w: &mut W, config: &AvifConfig, obu_data: &[u8]) -> io::Result<()> {
    let data = strip_temporal_delimiters(obu_data);

    let ftyp = build_ftyp();

    let hdlr = build_hdlr();
    let pitm = build_pitm();
    let iinf = build_iinf();
    let iprp = build_iprp(config);

    let children_before_iloc = [&hdlr[..], &pitm[..], &iinf[..], &iprp[..]].concat();

    let iloc_size = 30u32;
    let meta_content_size = 4 + children_before_iloc.len() as u32 + iloc_size;
    let meta_size = 8 + meta_content_size;
    let data_offset = ftyp.len() as u32 + meta_size + 8;

    let iloc = build_iloc(data_offset, data.len() as u32);

    let mut meta_payload = Vec::new();
    meta_payload.push(0);
    meta_payload.extend_from_slice(&0u32.to_be_bytes()[1..4]);
    meta_payload.extend_from_slice(&children_before_iloc);
    meta_payload.extend_from_slice(&iloc);

    let meta = box_wrap(b"meta", &meta_payload);

    let mdat = box_wrap(b"mdat", &data);

    w.write_all(&ftyp)?;
    w.write_all(&meta)?;
    w.write_all(&mdat)?;
    Ok(())
}

fn build_ftyp() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"avif");
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(b"avif");
    p.extend_from_slice(b"mif1");
    box_wrap(b"ftyp", &p)
}

fn build_hdlr() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(b"pict");
    p.extend_from_slice(&[0u8; 12]);
    p.push(0);
    full_box(b"hdlr", 0, 0, &p)
}

fn build_pitm() -> Vec<u8> {
    full_box(b"pitm", 0, 0, &1u16.to_be_bytes())
}

fn build_iloc(data_offset: u32, data_length: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.push(0x44);
    p.push(0x00);
    p.extend_from_slice(&1u16.to_be_bytes());
    p.extend_from_slice(&1u16.to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes());
    p.extend_from_slice(&1u16.to_be_bytes());
    p.extend_from_slice(&data_offset.to_be_bytes());
    p.extend_from_slice(&data_length.to_be_bytes());
    full_box(b"iloc", 0, 0, &p)
}

fn build_iinf() -> Vec<u8> {
    let mut infe_payload = Vec::new();
    infe_payload.extend_from_slice(&1u16.to_be_bytes());
    infe_payload.extend_from_slice(&0u16.to_be_bytes());
    infe_payload.extend_from_slice(b"av01");
    infe_payload.push(0);
    let infe = full_box(b"infe", 2, 0, &infe_payload);

    let mut p = Vec::new();
    p.extend_from_slice(&1u16.to_be_bytes());
    p.extend_from_slice(&infe);
    full_box(b"iinf", 0, 0, &p)
}

fn build_iprp(config: &AvifConfig) -> Vec<u8> {
    let av1c = build_av1c(config.video_signal.bit_depth, &config.config_obus);
    let ispe = build_ispe(config.width, config.height);
    let colr = build_colr(&config.video_signal);
    let pixi = build_pixi(config.video_signal.bit_depth);

    let mut ipco_payload = Vec::new();
    ipco_payload.extend_from_slice(&av1c);
    ipco_payload.extend_from_slice(&ispe);
    ipco_payload.extend_from_slice(&colr);
    ipco_payload.extend_from_slice(&pixi);
    let ipco = box_wrap(b"ipco", &ipco_payload);

    let ipma = build_ipma();

    let mut p = Vec::new();
    p.extend_from_slice(&ipco);
    p.extend_from_slice(&ipma);
    box_wrap(b"iprp", &p)
}

fn build_ispe(width: u32, height: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&width.to_be_bytes());
    p.extend_from_slice(&height.to_be_bytes());
    full_box(b"ispe", 0, 0, &p)
}

fn build_pixi(bit_depth: BitDepth) -> Vec<u8> {
    let bits = bit_depth.bits();
    let p = vec![3, bits, bits, bits];
    full_box(b"pixi", 0, 0, &p)
}

fn build_ipma() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&1u16.to_be_bytes());
    p.push(4);
    p.push(0x81);
    p.push(0x82);
    p.push(0x83);
    p.push(0x84);
    full_box(b"ipma", 0, 0, &p)
}
