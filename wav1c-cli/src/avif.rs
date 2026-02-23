use std::io::{self, Write};

use crate::mp4::{box_wrap, build_av1c, build_colr, full_box, strip_temporal_delimiters};
use wav1c::{BitDepth, ContentLightLevel, MasteringDisplayMetadata, VideoSignal};

#[cfg(feature = "heic")]
const TMAP_GAIN_MAX_FLOOR: f64 = 2.5;
#[cfg(feature = "heic")]
const FRACTION_SCALE: u32 = 1_000_000;

pub struct AvifConfig {
    pub width: u32,
    pub height: u32,
    pub config_obus: Vec<u8>,
    pub video_signal: VideoSignal,
    pub content_light: Option<ContentLightLevel>,
    pub mastering_display: Option<MasteringDisplayMetadata>,
}

#[cfg(feature = "heic")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedFraction {
    pub n: i32,
    pub d: u32,
}

#[cfg(feature = "heic")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsignedFraction {
    pub n: u32,
    pub d: u32,
}

#[cfg(feature = "heic")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToneMapMetadata {
    pub base_hdr_headroom: UnsignedFraction,
    pub alternate_hdr_headroom: UnsignedFraction,
    pub gain_map_min: [SignedFraction; 3],
    pub gain_map_max: [SignedFraction; 3],
    pub gain_map_gamma: [UnsignedFraction; 3],
    pub base_offset: [SignedFraction; 3],
    pub alternate_offset: [SignedFraction; 3],
    pub use_base_color_space: bool,
}

#[cfg(feature = "heic")]
pub fn derive_tmap_metadata_from_apple(
    hdr_headroom_num: i32,
    hdr_headroom_den: i32,
    hdr_gain_num: i32,
    hdr_gain_den: i32,
) -> Result<ToneMapMetadata, String> {
    if hdr_headroom_den == 0 || hdr_gain_den == 0 {
        return Err("Apple HDR scalar denominator must not be zero".to_owned());
    }

    let hdr_headroom = normalize_positive_unsigned(hdr_headroom_num, hdr_headroom_den)?;
    let gain_value = {
        let num = hdr_gain_num as f64;
        let den = hdr_gain_den as f64;
        num / den
    };
    if !gain_value.is_finite() {
        return Err("Apple HDRGain value is not finite".to_owned());
    }
    if gain_value < 0.0 {
        return Err("Apple HDRGain value must be >= 0".to_owned());
    }

    let alternate_headroom_f = hdr_headroom.n as f64 / hdr_headroom.d as f64;
    let gain_map_min = SignedFraction { n: 0, d: 1 };
    let gain_map_max = f64_to_signed_fraction(alternate_headroom_f.max(TMAP_GAIN_MAX_FLOOR))?;
    let gain_map_gamma = UnsignedFraction { n: 1, d: 1 };
    let offset = SignedFraction { n: 1, d: 64 };

    Ok(ToneMapMetadata {
        base_hdr_headroom: UnsignedFraction { n: 0, d: 1 },
        alternate_hdr_headroom: hdr_headroom,
        gain_map_min: [gain_map_min; 3],
        gain_map_max: [gain_map_max; 3],
        gain_map_gamma: [gain_map_gamma; 3],
        base_offset: [offset; 3],
        alternate_offset: [offset; 3],
        use_base_color_space: true,
    })
}

#[cfg(feature = "heic")]
fn normalize_positive_unsigned(num: i32, den: i32) -> Result<UnsignedFraction, String> {
    let mut n = i64::from(num);
    let mut d = i64::from(den);
    if d < 0 {
        d = -d;
        n = -n;
    }
    if d == 0 {
        return Err("fraction denominator is zero".to_owned());
    }
    if n < 0 {
        return Err("fraction numerator must be non-negative".to_owned());
    }
    if n > i64::from(u32::MAX) || d > i64::from(u32::MAX) {
        return Err("fraction component exceeds u32 range".to_owned());
    }
    Ok(UnsignedFraction {
        n: n as u32,
        d: d as u32,
    })
}

#[cfg(feature = "heic")]
fn f64_to_signed_fraction(value: f64) -> Result<SignedFraction, String> {
    if !value.is_finite() {
        return Err("cannot convert non-finite value to fraction".to_owned());
    }
    let scaled = (value * FRACTION_SCALE as f64).round();
    if scaled < i32::MIN as f64 || scaled > i32::MAX as f64 {
        return Err("fraction numerator out of i32 range".to_owned());
    }
    Ok(SignedFraction {
        n: scaled as i32,
        d: FRACTION_SCALE,
    })
}

#[cfg(feature = "heic")]
pub fn build_tmap_payload(metadata: &ToneMapMetadata) -> Result<Vec<u8>, String> {
    validate_tmap_metadata(metadata)?;

    let is_multichannel = !has_identical_channels(metadata);
    let channel_count = if is_multichannel { 3usize } else { 1usize };

    let mut payload = Vec::with_capacity(1 + 21 + channel_count * 40);
    payload.push(0); // ToneMapImage version
    payload.extend_from_slice(&0u16.to_be_bytes()); // minimum_version
    payload.extend_from_slice(&0u16.to_be_bytes()); // writer_version
    let flags = ((is_multichannel as u8) << 7) | ((metadata.use_base_color_space as u8) << 6);
    payload.push(flags);

    write_u32(&mut payload, metadata.base_hdr_headroom.n);
    write_u32(&mut payload, metadata.base_hdr_headroom.d);
    write_u32(&mut payload, metadata.alternate_hdr_headroom.n);
    write_u32(&mut payload, metadata.alternate_hdr_headroom.d);

    for c in 0..channel_count {
        write_i32_twos_complement(&mut payload, metadata.gain_map_min[c].n);
        write_u32(&mut payload, metadata.gain_map_min[c].d);
        write_i32_twos_complement(&mut payload, metadata.gain_map_max[c].n);
        write_u32(&mut payload, metadata.gain_map_max[c].d);
        write_u32(&mut payload, metadata.gain_map_gamma[c].n);
        write_u32(&mut payload, metadata.gain_map_gamma[c].d);
        write_i32_twos_complement(&mut payload, metadata.base_offset[c].n);
        write_u32(&mut payload, metadata.base_offset[c].d);
        write_i32_twos_complement(&mut payload, metadata.alternate_offset[c].n);
        write_u32(&mut payload, metadata.alternate_offset[c].d);
    }

    Ok(payload)
}

#[cfg(feature = "heic")]
fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

#[cfg(feature = "heic")]
fn write_i32_twos_complement(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&(value as u32).to_be_bytes());
}

#[cfg(feature = "heic")]
fn validate_tmap_metadata(metadata: &ToneMapMetadata) -> Result<(), String> {
    if metadata.base_hdr_headroom.d == 0 || metadata.alternate_hdr_headroom.d == 0 {
        return Err("tmap headroom denominators must be non-zero".to_owned());
    }
    for c in 0..3 {
        if metadata.gain_map_min[c].d == 0
            || metadata.gain_map_max[c].d == 0
            || metadata.gain_map_gamma[c].d == 0
            || metadata.base_offset[c].d == 0
            || metadata.alternate_offset[c].d == 0
        {
            return Err(format!("tmap channel {c} contains a zero denominator"));
        }
        if metadata.gain_map_gamma[c].n == 0 {
            return Err(format!("tmap channel {c} gamma numerator must be non-zero"));
        }
        let left = i64::from(metadata.gain_map_max[c].n) * i64::from(metadata.gain_map_min[c].d);
        let right = i64::from(metadata.gain_map_min[c].n) * i64::from(metadata.gain_map_max[c].d);
        if left < right {
            return Err(format!("tmap channel {c} has gain_map_max < gain_map_min"));
        }
    }
    Ok(())
}

#[cfg(feature = "heic")]
fn has_identical_channels(metadata: &ToneMapMetadata) -> bool {
    metadata.gain_map_min[0] == metadata.gain_map_min[1]
        && metadata.gain_map_min[0] == metadata.gain_map_min[2]
        && metadata.gain_map_max[0] == metadata.gain_map_max[1]
        && metadata.gain_map_max[0] == metadata.gain_map_max[2]
        && metadata.gain_map_gamma[0] == metadata.gain_map_gamma[1]
        && metadata.gain_map_gamma[0] == metadata.gain_map_gamma[2]
        && metadata.base_offset[0] == metadata.base_offset[1]
        && metadata.base_offset[0] == metadata.base_offset[2]
        && metadata.alternate_offset[0] == metadata.alternate_offset[1]
        && metadata.alternate_offset[0] == metadata.alternate_offset[2]
}

fn build_item_obu_data(config_obus: &[u8], packet_obu_data: &[u8]) -> Vec<u8> {
    let packet_data = strip_temporal_delimiters(packet_obu_data);
    let frame_offset = strip_leading_seq_and_metadata_offset(&packet_data);
    let frame_data = &packet_data[frame_offset..];

    let mut out = Vec::with_capacity(config_obus.len() + frame_data.len());
    out.extend_from_slice(config_obus);
    out.extend_from_slice(frame_data);
    out
}

fn strip_leading_seq_and_metadata_offset(data: &[u8]) -> usize {
    let mut pos = 0usize;
    while pos < data.len() {
        let Some((obu_type, obu_len)) = parse_obu_type_and_len(data, pos) else {
            break;
        };
        if obu_type == 1 || obu_type == 2 || obu_type == 5 {
            pos += obu_len;
            continue;
        }
        break;
    }
    pos
}

fn parse_obu_type_and_len(data: &[u8], start: usize) -> Option<(u8, usize)> {
    if start >= data.len() {
        return None;
    }
    let header = data[start];
    let obu_type = (header >> 3) & 0x0F;
    let extension_flag = ((header >> 2) & 1) != 0;
    let has_size_field = ((header >> 1) & 1) != 0;
    let mut pos = start + 1;
    if extension_flag {
        if pos >= data.len() {
            return None;
        }
        pos += 1;
    }
    if !has_size_field {
        return None;
    }

    let mut size = 0usize;
    let mut shift = 0usize;
    let mut leb_len = 0usize;
    loop {
        if pos >= data.len() || shift > 63 || leb_len > 8 {
            return None;
        }
        let byte = data[pos];
        pos += 1;
        leb_len += 1;
        size |= ((byte & 0x7F) as usize) << shift;
        if (byte & 0x80) == 0 {
            break;
        }
        shift += 7;
    }

    let header_and_size_len = pos - start;
    let total_len = header_and_size_len.checked_add(size)?;
    if start.checked_add(total_len)? > data.len() {
        return None;
    }
    Some((obu_type, total_len))
}

pub fn write_avif<W: Write>(w: &mut W, config: &AvifConfig, obu_data: &[u8]) -> io::Result<()> {
    let data = build_item_obu_data(&config.config_obus, obu_data);

    let ftyp = build_ftyp();
    let hdlr = build_hdlr();
    let pitm = build_pitm();
    let iinf = build_iinf_single();
    let iprp = build_iprp_single(config);

    let children_before_iloc = [&hdlr[..], &pitm[..], &iinf[..], &iprp[..]].concat();
    let iloc = build_iloc(&[IlocEntry {
        item_id: 1,
        offset: 0,
        length: data.len() as u32,
    }]);

    let meta_content_size = 4 + children_before_iloc.len() as u32 + iloc.len() as u32;
    let meta_size = 8 + meta_content_size;
    let data_offset = ftyp.len() as u32 + meta_size + 8;

    let iloc = build_iloc(&[IlocEntry {
        item_id: 1,
        offset: data_offset,
        length: data.len() as u32,
    }]);

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

#[cfg(feature = "heic")]
pub fn write_avif_with_tmap_gain_map<W: Write>(
    w: &mut W,
    base_config: &AvifConfig,
    base_obu_data: &[u8],
    gain_map_config: &AvifConfig,
    gain_map_obu_data: &[u8],
    tmap_payload: &[u8],
) -> io::Result<()> {
    let base_data = build_item_obu_data(&base_config.config_obus, base_obu_data);
    let gain_map_data = build_item_obu_data(&gain_map_config.config_obus, gain_map_obu_data);
    let mut mdat_payload = Vec::new();
    mdat_payload.extend_from_slice(&base_data);
    mdat_payload.extend_from_slice(tmap_payload);
    mdat_payload.extend_from_slice(&gain_map_data);

    let ftyp = build_ftyp_tmap();
    let hdlr = build_hdlr();
    let pitm = build_pitm();
    let iinf = build_iinf_tmap();
    let iref = build_iref_tmap();
    let iprp = build_iprp_tmap(base_config, gain_map_config);
    let grpl = build_grpl_altr_tmap();
    let children_before_iloc = [
        &hdlr[..],
        &pitm[..],
        &iinf[..],
        &iref[..],
        &iprp[..],
        &grpl[..],
    ]
    .concat();

    let temp_iloc = build_iloc(&[
        IlocEntry {
            item_id: 1,
            offset: 0,
            length: base_data.len() as u32,
        },
        IlocEntry {
            item_id: 2,
            offset: 0,
            length: tmap_payload.len() as u32,
        },
        IlocEntry {
            item_id: 3,
            offset: 0,
            length: gain_map_data.len() as u32,
        },
    ]);
    let meta_content_size = 4 + children_before_iloc.len() as u32 + temp_iloc.len() as u32;
    let meta_size = 8 + meta_content_size;
    let data_offset = ftyp.len() as u32 + meta_size + 8;

    let base_offset = data_offset;
    let tmap_offset = base_offset + base_data.len() as u32;
    let gain_offset = tmap_offset + tmap_payload.len() as u32;
    let iloc = build_iloc(&[
        IlocEntry {
            item_id: 1,
            offset: base_offset,
            length: base_data.len() as u32,
        },
        IlocEntry {
            item_id: 2,
            offset: tmap_offset,
            length: tmap_payload.len() as u32,
        },
        IlocEntry {
            item_id: 3,
            offset: gain_offset,
            length: gain_map_data.len() as u32,
        },
    ]);

    let mut meta_payload = Vec::new();
    meta_payload.push(0);
    meta_payload.extend_from_slice(&0u32.to_be_bytes()[1..4]);
    meta_payload.extend_from_slice(&children_before_iloc);
    meta_payload.extend_from_slice(&iloc);
    let meta = box_wrap(b"meta", &meta_payload);

    let mdat = box_wrap(b"mdat", &mdat_payload);
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

#[cfg(feature = "heic")]
fn build_ftyp_tmap() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"avif");
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(b"avif");
    p.extend_from_slice(b"mif1");
    p.extend_from_slice(b"miaf");
    p.extend_from_slice(b"MA1A");
    p.extend_from_slice(b"tmap");
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

#[derive(Clone, Copy)]
struct IlocEntry {
    item_id: u16,
    offset: u32,
    length: u32,
}

fn build_iloc(entries: &[IlocEntry]) -> Vec<u8> {
    let mut p = Vec::new();
    p.push(0x44);
    p.push(0x00);
    p.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    for entry in entries {
        p.extend_from_slice(&entry.item_id.to_be_bytes());
        p.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        p.extend_from_slice(&1u16.to_be_bytes()); // extent_count
        p.extend_from_slice(&entry.offset.to_be_bytes());
        p.extend_from_slice(&entry.length.to_be_bytes());
    }
    full_box(b"iloc", 0, 0, &p)
}

struct InfeEntry<'a> {
    item_id: u16,
    item_type: [u8; 4],
    hidden: bool,
    name: &'a str,
}

fn build_infe(entry: &InfeEntry<'_>) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&entry.item_id.to_be_bytes());
    payload.extend_from_slice(&0u16.to_be_bytes()); // item_protection_index
    payload.extend_from_slice(&entry.item_type);
    payload.extend_from_slice(entry.name.as_bytes());
    payload.push(0);
    full_box(b"infe", 2, if entry.hidden { 1 } else { 0 }, &payload)
}

fn build_iinf_single() -> Vec<u8> {
    let entries = [InfeEntry {
        item_id: 1,
        item_type: *b"av01",
        hidden: false,
        name: "Color",
    }];
    build_iinf(&entries)
}

#[cfg(feature = "heic")]
fn build_iinf_tmap() -> Vec<u8> {
    let entries = [
        InfeEntry {
            item_id: 1,
            item_type: *b"av01",
            hidden: false,
            name: "Color",
        },
        InfeEntry {
            item_id: 2,
            item_type: *b"tmap",
            hidden: false,
            name: "TMap",
        },
        InfeEntry {
            item_id: 3,
            item_type: *b"av01",
            hidden: true,
            name: "GMap",
        },
    ];
    build_iinf(&entries)
}

fn build_iinf(entries: &[InfeEntry<'_>]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    for entry in entries {
        p.extend_from_slice(&build_infe(entry));
    }
    full_box(b"iinf", 0, 0, &p)
}

#[cfg(feature = "heic")]
fn build_iref_tmap() -> Vec<u8> {
    let mut dimg_payload = Vec::new();
    dimg_payload.extend_from_slice(&2u16.to_be_bytes()); // from_item_id (tmap)
    dimg_payload.extend_from_slice(&2u16.to_be_bytes()); // reference_count
    dimg_payload.extend_from_slice(&1u16.to_be_bytes()); // base
    dimg_payload.extend_from_slice(&3u16.to_be_bytes()); // gain map
    let dimg = box_wrap(b"dimg", &dimg_payload);
    full_box(b"iref", 0, 0, &dimg)
}

#[cfg(feature = "heic")]
fn build_grpl_altr_tmap() -> Vec<u8> {
    let mut altr_payload = Vec::new();
    altr_payload.extend_from_slice(&1u32.to_be_bytes()); // group_id
    altr_payload.extend_from_slice(&2u32.to_be_bytes()); // num_entities_in_group
    altr_payload.extend_from_slice(&2u32.to_be_bytes()); // tmap
    altr_payload.extend_from_slice(&1u32.to_be_bytes()); // base
    let altr = full_box(b"altr", 0, 0, &altr_payload);
    box_wrap(b"grpl", &altr)
}

fn build_iprp_single(config: &AvifConfig) -> Vec<u8> {
    let mut ipco_payload = Vec::new();
    let mut next_property_index = 1u8;
    let mut base_associations = vec![
        append_property(
            &mut ipco_payload,
            &mut next_property_index,
            build_av1c(config.video_signal.bit_depth, &config.config_obus),
        ),
        append_property(
            &mut ipco_payload,
            &mut next_property_index,
            build_ispe(config.width, config.height),
        ),
        append_property(
            &mut ipco_payload,
            &mut next_property_index,
            build_colr(&config.video_signal),
        ),
        append_property(
            &mut ipco_payload,
            &mut next_property_index,
            build_pixi(config.video_signal.bit_depth),
        ),
    ];
    if let Some(cll) = config.content_light {
        base_associations.push(append_property(
            &mut ipco_payload,
            &mut next_property_index,
            build_clli(&cll),
        ));
    }
    if let Some(mdcv) = config.mastering_display {
        base_associations.push(append_property(
            &mut ipco_payload,
            &mut next_property_index,
            build_mdcv(&mdcv),
        ));
    }
    let ipco = box_wrap(b"ipco", &ipco_payload);
    let ipma_entries = [(1u16, base_associations.as_slice())];
    let ipma = build_ipma(&ipma_entries);

    let mut p = Vec::new();
    p.extend_from_slice(&ipco);
    p.extend_from_slice(&ipma);
    box_wrap(b"iprp", &p)
}

#[cfg(feature = "heic")]
fn build_iprp_tmap(base: &AvifConfig, gain: &AvifConfig) -> Vec<u8> {
    let mut ipco_payload = Vec::new();
    let mut next_property_index = 1u8;

    let mut base_associations = Vec::new();
    base_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_av1c(base.video_signal.bit_depth, &base.config_obus),
    ));
    base_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_ispe(base.width, base.height),
    ));
    base_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_colr(&base.video_signal),
    ));
    base_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_pixi(base.video_signal.bit_depth),
    ));
    if let Some(cll) = base.content_light {
        base_associations.push(append_property(
            &mut ipco_payload,
            &mut next_property_index,
            build_clli(&cll),
        ));
    }
    if let Some(mdcv) = base.mastering_display {
        base_associations.push(append_property(
            &mut ipco_payload,
            &mut next_property_index,
            build_mdcv(&mdcv),
        ));
    }

    let mut tmap_associations = Vec::new();
    tmap_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_ispe(base.width, base.height),
    ));
    tmap_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_colr(&base.video_signal),
    ));
    tmap_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_pixi(base.video_signal.bit_depth),
    ));

    let mut gain_associations = Vec::new();
    gain_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_av1c(gain.video_signal.bit_depth, &gain.config_obus),
    ));
    gain_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_ispe(gain.width, gain.height),
    ));
    gain_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_colr(&gain.video_signal),
    ));
    gain_associations.push(append_property(
        &mut ipco_payload,
        &mut next_property_index,
        build_pixi(gain.video_signal.bit_depth),
    ));
    let ipco = box_wrap(b"ipco", &ipco_payload);
    let ipma_entries = [
        (1u16, base_associations.as_slice()),
        (2u16, tmap_associations.as_slice()),
        (3u16, gain_associations.as_slice()),
    ];
    let ipma = build_ipma(&ipma_entries);

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

fn build_clli(cll: &ContentLightLevel) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&cll.max_content_light_level.to_be_bytes());
    p.extend_from_slice(&cll.max_frame_average_light_level.to_be_bytes());
    box_wrap(b"clli", &p)
}

fn build_mdcv(mdcv: &MasteringDisplayMetadata) -> Vec<u8> {
    let mut p = Vec::new();
    for primary in mdcv.primaries {
        p.extend_from_slice(&primary[0].to_be_bytes());
        p.extend_from_slice(&primary[1].to_be_bytes());
    }
    p.extend_from_slice(&mdcv.white_point[0].to_be_bytes());
    p.extend_from_slice(&mdcv.white_point[1].to_be_bytes());
    p.extend_from_slice(&mdcv.max_luminance.to_be_bytes());
    p.extend_from_slice(&mdcv.min_luminance.to_be_bytes());
    box_wrap(b"mdcv", &p)
}

fn append_property(
    ipco_payload: &mut Vec<u8>,
    next_property_index: &mut u8,
    property: Vec<u8>,
) -> u8 {
    let property_index = *next_property_index;
    ipco_payload.extend_from_slice(&property);
    *next_property_index = next_property_index
        .checked_add(1)
        .expect("AVIF property index overflow");
    property_index
}

fn build_ipma(entries: &[(u16, &[u8])]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&(entries.len() as u32).to_be_bytes());

    for (item_id, associations) in entries {
        p.extend_from_slice(&item_id.to_be_bytes());
        p.push(associations.len() as u8);
        for property_index in *associations {
            p.push(0x80 | *property_index);
        }
    }

    full_box(b"ipma", 0, 0, &p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wav1c::{ColorDescription, ColorRange, VideoSignal};

    fn sample_signal(bit_depth: BitDepth) -> VideoSignal {
        VideoSignal {
            bit_depth,
            color_range: ColorRange::Full,
            color_description: Some(ColorDescription {
                color_primaries: 9,
                transfer_characteristics: 16,
                matrix_coefficients: 9,
            }),
        }
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn single_item_avif_includes_clli_and_mdcv_properties() {
        let cll = ContentLightLevel {
            max_content_light_level: 480,
            max_frame_average_light_level: 21,
        };
        let mdcv = MasteringDisplayMetadata {
            primaries: [[34000, 16000], [13250, 34500], [7500, 3000]],
            white_point: [15635, 16450],
            max_luminance: 10_000_000,
            min_luminance: 1,
        };
        let config = AvifConfig {
            width: 64,
            height: 64,
            config_obus: vec![0x0A, 0x01, 0x80],
            video_signal: sample_signal(BitDepth::Ten),
            content_light: Some(cll),
            mastering_display: Some(mdcv),
        };

        let mut out = Vec::new();
        write_avif(&mut out, &config, &[0x12, 0x00, 0x11, 0x22]).expect("write");

        assert!(contains(&out, &build_clli(&cll)));
        assert!(contains(&out, &build_mdcv(&mdcv)));
    }

    #[test]
    fn single_item_avif_omits_hdr_item_properties_when_unset() {
        let config = AvifConfig {
            width: 64,
            height: 64,
            config_obus: vec![0x0A, 0x01, 0x80],
            video_signal: sample_signal(BitDepth::Ten),
            content_light: None,
            mastering_display: None,
        };

        let mut out = Vec::new();
        write_avif(&mut out, &config, &[0x12, 0x00, 0x11, 0x22]).expect("write");

        assert!(!contains(&out, b"clli"));
        assert!(!contains(&out, b"mdcv"));
    }

    #[test]
    fn item_data_uses_config_obus_and_drops_packet_seq_prefix() {
        let config_obus = vec![0x0A, 0x01, 0x1C];
        let packet_data = vec![0x12, 0x00, 0x0A, 0x01, 0x00, 0x32, 0x01, 0xAA];

        let out = build_item_obu_data(&config_obus, &packet_data);

        assert!(out.starts_with(&config_obus));
        assert_eq!(&out[config_obus.len()..], &[0x32, 0x01, 0xAA]);
    }
}

#[cfg(all(test, feature = "heic"))]
mod heic_tests {
    use super::*;
    use wav1c::{ColorDescription, ColorRange, VideoSignal};

    fn sample_signal(bit_depth: BitDepth) -> VideoSignal {
        VideoSignal {
            bit_depth,
            color_range: ColorRange::Full,
            color_description: Some(ColorDescription {
                color_primaries: 1,
                transfer_characteristics: 13,
                matrix_coefficients: 6,
            }),
        }
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn derive_tmap_uses_min_zero_and_floor_max() {
        let metadata = derive_tmap_metadata_from_apple(48400, 65123, 1804, 556975)
            .expect("derive should succeed");
        assert_eq!(metadata.gain_map_min[0], SignedFraction { n: 0, d: 1 });
        assert_eq!(metadata.gain_map_min[1], SignedFraction { n: 0, d: 1 });
        assert_eq!(metadata.gain_map_min[2], SignedFraction { n: 0, d: 1 });
        assert_eq!(
            metadata.gain_map_max[0],
            SignedFraction {
                n: 2_500_000,
                d: 1_000_000
            }
        );
        assert_eq!(
            metadata.gain_map_max[1],
            SignedFraction {
                n: 2_500_000,
                d: 1_000_000
            }
        );
        assert_eq!(
            metadata.gain_map_max[2],
            SignedFraction {
                n: 2_500_000,
                d: 1_000_000
            }
        );
    }

    #[test]
    fn derive_tmap_preserves_larger_headroom() {
        let metadata =
            derive_tmap_metadata_from_apple(7, 2, 1, 100).expect("derive should succeed");
        assert_eq!(
            metadata.gain_map_max[0],
            SignedFraction {
                n: 3_500_000,
                d: 1_000_000
            }
        );
        assert_eq!(
            metadata.gain_map_max[1],
            SignedFraction {
                n: 3_500_000,
                d: 1_000_000
            }
        );
        assert_eq!(
            metadata.gain_map_max[2],
            SignedFraction {
                n: 3_500_000,
                d: 1_000_000
            }
        );
    }

    #[test]
    fn derive_tmap_accepts_zero_hdr_gain() {
        let metadata = derive_tmap_metadata_from_apple(1, 2, 0, 1).expect("derive should succeed");
        assert_eq!(
            metadata.alternate_hdr_headroom,
            UnsignedFraction { n: 1, d: 2 }
        );
        assert_eq!(
            metadata.gain_map_max[0],
            SignedFraction {
                n: 2_500_000,
                d: 1_000_000
            }
        );
        assert_eq!(
            metadata.gain_map_max[1],
            SignedFraction {
                n: 2_500_000,
                d: 1_000_000
            }
        );
        assert_eq!(
            metadata.gain_map_max[2],
            SignedFraction {
                n: 2_500_000,
                d: 1_000_000
            }
        );
    }

    #[test]
    fn tmap_payload_size_identical_channels_is_62_bytes() {
        let metadata = ToneMapMetadata {
            base_hdr_headroom: UnsignedFraction { n: 0, d: 1 },
            alternate_hdr_headroom: UnsignedFraction { n: 1, d: 1 },
            gain_map_min: [SignedFraction { n: -1000, d: 1000 }; 3],
            gain_map_max: [SignedFraction { n: 2000, d: 1000 }; 3],
            gain_map_gamma: [UnsignedFraction { n: 1, d: 1 }; 3],
            base_offset: [SignedFraction { n: 1, d: 64 }; 3],
            alternate_offset: [SignedFraction { n: 1, d: 64 }; 3],
            use_base_color_space: true,
        };

        let payload = build_tmap_payload(&metadata).expect("payload");
        assert_eq!(payload.len(), 62);
        assert_eq!(payload[0], 0);
        assert_eq!(payload[5], 0x40);
    }

    #[test]
    fn tmap_payload_size_multichannel_is_142_bytes() {
        let mut metadata = ToneMapMetadata {
            base_hdr_headroom: UnsignedFraction { n: 0, d: 1 },
            alternate_hdr_headroom: UnsignedFraction { n: 1, d: 1 },
            gain_map_min: [SignedFraction { n: -1000, d: 1000 }; 3],
            gain_map_max: [SignedFraction { n: 2000, d: 1000 }; 3],
            gain_map_gamma: [UnsignedFraction { n: 1, d: 1 }; 3],
            base_offset: [SignedFraction { n: 1, d: 64 }; 3],
            alternate_offset: [SignedFraction { n: 1, d: 64 }; 3],
            use_base_color_space: true,
        };
        metadata.gain_map_max[1].n = 2100;

        let payload = build_tmap_payload(&metadata).expect("payload");
        assert_eq!(payload.len(), 142);
        assert_eq!(payload[5], 0xC0);
    }

    #[test]
    fn gain_map_avif_container_has_expected_graph() {
        let base_cfg = AvifConfig {
            width: 640,
            height: 480,
            config_obus: vec![0x01, 0x02, 0x03],
            video_signal: sample_signal(BitDepth::Eight),
            content_light: None,
            mastering_display: None,
        };
        let gain_cfg = AvifConfig {
            width: 320,
            height: 240,
            config_obus: vec![0x04, 0x05, 0x06],
            video_signal: VideoSignal {
                bit_depth: BitDepth::Eight,
                color_range: ColorRange::Full,
                color_description: Some(ColorDescription {
                    color_primaries: 2,
                    transfer_characteristics: 2,
                    matrix_coefficients: 2,
                }),
            },
            content_light: None,
            mastering_display: None,
        };
        let tmap = vec![0u8; 62];
        let mut out = Vec::new();
        write_avif_with_tmap_gain_map(
            &mut out,
            &base_cfg,
            &[0x12, 0x00, 0x11, 0x22],
            &gain_cfg,
            &[0x12, 0x00, 0x33, 0x44],
            &tmap,
        )
        .expect("write");

        assert!(contains(&out[..64], b"tmap"));
        assert!(contains(&out, b"miaf"));
        assert!(contains(&out, b"MA1A"));
        assert!(contains(&out, b"\x00\x01\x00\x00av01"));
        assert!(contains(&out, b"\x00\x02\x00\x00tmap"));
        assert!(contains(&out, b"\x00\x03\x00\x00av01"));
        assert!(contains(&out, b"dimg\x00\x02\x00\x02\x00\x01\x00\x03"));
        assert!(contains(
            &out,
            b"altr\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x02\x00\x00\x00\x02\x00\x00\x00\x01"
        ));
    }
}
