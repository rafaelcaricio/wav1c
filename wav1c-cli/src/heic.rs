use libheif_rs::{
    Chroma, ColorPrimaries, ColorSpace, ImageHandle, MatrixCoefficients, TransferCharacteristics,
};
use libheif_rs::{HeifContext, LibHeif};
use wav1c::y4m::FramePixels;
use wav1c::{BitDepth, ColorDescription, ColorRange};

pub const APPLE_HDR_GAINMAP_AUX_TYPE: &str = "urn:com:apple:photo:2020:aux:hdrgainmap";
const HDR_GAINMAP_VERSION_KEY: &[u8] = b"HDRGainMapVersion";
const HDR_HEADROOM_TAG: u16 = 0x0021;
const HDR_GAIN_TAG: u16 = 0x0030;
const EXIF_IFD_POINTER_TAG: u16 = 0x8769;
const MAKER_NOTE_TAG: u16 = 0x927c;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedRational {
    pub numerator: i32,
    pub denominator: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppleHdrScalars {
    pub hdr_headroom: SignedRational,
    pub hdr_gain: SignedRational,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceNclx {
    pub color_description: Option<ColorDescription>,
    pub color_range: ColorRange,
}

#[derive(Debug)]
pub struct HeicDecodeResult {
    pub base: FramePixels,
    pub gain_map: Option<FramePixels>,
    pub apple_hdr_scalars: Option<AppleHdrScalars>,
    pub apple_hdr_scalars_error: Option<String>,
    pub gain_map_has_xmp_version: bool,
    pub source_nclx: Option<SourceNclx>,
}

pub fn decode_heic(path: &str) -> Result<HeicDecodeResult, String> {
    let ctx =
        HeifContext::read_from_file(path).map_err(|e| format!("failed to open HEIC file: {e}"))?;

    let handle = ctx
        .primary_image_handle()
        .map_err(|e| format!("failed to get primary image: {e}"))?;

    let source_nclx = extract_source_nclx(&handle);
    let base_color_range = source_nclx
        .map(|n| n.color_range)
        .unwrap_or(ColorRange::Full);

    let lib_heif = LibHeif::new();
    let base = decode_handle_to_frame(&lib_heif, &handle, base_color_range)?;

    let mut gain_map = None;
    let mut gain_map_has_xmp_version = false;

    for aux in handle.auxiliary_images(None) {
        let aux_type = aux.auxiliary_type().unwrap_or_default();
        if aux_type == APPLE_HDR_GAINMAP_AUX_TYPE {
            gain_map_has_xmp_version = metadata_contains_key(&aux, HDR_GAINMAP_VERSION_KEY);
            gain_map = Some(decode_handle_to_frame(&lib_heif, &aux, ColorRange::Full)?);
            break;
        }
    }

    let (apple_hdr_scalars, apple_hdr_scalars_error) = match extract_apple_hdr_scalars(&handle) {
        Ok(v) => (v, None),
        Err(e) => (None, Some(e)),
    };

    Ok(HeicDecodeResult {
        base,
        gain_map,
        apple_hdr_scalars,
        apple_hdr_scalars_error,
        gain_map_has_xmp_version,
        source_nclx,
    })
}

fn decode_handle_to_frame(
    lib_heif: &LibHeif,
    handle: &ImageHandle,
    color_range: ColorRange,
) -> Result<FramePixels, String> {
    let width = handle.width();
    let height = handle.height();
    let bit_depth = if handle.luma_bits_per_pixel() > 8 {
        BitDepth::Ten
    } else {
        BitDepth::Eight
    };

    let image = lib_heif
        .decode(handle, ColorSpace::YCbCr(Chroma::C420), None)
        .map_err(|e| format!("failed to decode HEIC item: {e}"))?;

    let planes = image.planes();
    let y_plane = planes.y.ok_or("no Y plane in decoded HEIC item")?;
    let uv_width = (width as usize).div_ceil(2);
    let uv_height = (height as usize).div_ceil(2);
    let neutral = bit_depth.mid_value();

    let y = extract_plane(
        y_plane.data,
        y_plane.stride,
        width as usize,
        height as usize,
        bit_depth,
    );
    let u = match planes.cb {
        Some(cb) => extract_plane(cb.data, cb.stride, uv_width, uv_height, bit_depth),
        None => vec![neutral; uv_width * uv_height],
    };
    let v = match planes.cr {
        Some(cr) => extract_plane(cr.data, cr.stride, uv_width, uv_height, bit_depth),
        None => vec![neutral; uv_width * uv_height],
    };

    Ok(FramePixels {
        y,
        u,
        v,
        width,
        height,
        bit_depth,
        color_range,
    })
}

fn metadata_contains_key(handle: &ImageHandle, needle: &[u8]) -> bool {
    handle
        .all_metadata()
        .iter()
        .any(|m| m.raw_data.windows(needle.len()).any(|w| w == needle))
}

fn extract_source_nclx(handle: &ImageHandle) -> Option<SourceNclx> {
    let nclx = handle.color_profile_nclx()?;
    let color_range = if nclx.full_range_flag() != 0 {
        ColorRange::Full
    } else {
        ColorRange::Limited
    };
    let color_description = match (
        map_color_primaries(nclx.color_primaries()),
        map_transfer_characteristics(nclx.transfer_characteristics()),
        map_matrix_coefficients(nclx.matrix_coefficients()),
    ) {
        (Some(color_primaries), Some(transfer_characteristics), Some(matrix_coefficients)) => {
            Some(ColorDescription {
                color_primaries,
                transfer_characteristics,
                matrix_coefficients,
            })
        }
        _ => None,
    };
    Some(SourceNclx {
        color_description,
        color_range,
    })
}

fn map_color_primaries(v: ColorPrimaries) -> Option<u8> {
    match v {
        ColorPrimaries::Unspecified | ColorPrimaries::Unknown => None,
        _ => u8::try_from(v as i32).ok(),
    }
}

fn map_transfer_characteristics(v: TransferCharacteristics) -> Option<u8> {
    match v {
        TransferCharacteristics::Unspecified | TransferCharacteristics::Unknown => None,
        _ => u8::try_from(v as i32).ok(),
    }
}

fn map_matrix_coefficients(v: MatrixCoefficients) -> Option<u8> {
    match v {
        MatrixCoefficients::Unspecified | MatrixCoefficients::Unknown => None,
        _ => u8::try_from(v as i32).ok(),
    }
}

fn extract_apple_hdr_scalars(handle: &ImageHandle) -> Result<Option<AppleHdrScalars>, String> {
    let exif_raw = handle
        .all_metadata()
        .into_iter()
        .find(|m| m.item_type.to_string() == "Exif")
        .map(|m| m.raw_data);

    let Some(exif_raw) = exif_raw else {
        return Ok(None);
    };

    let tiff_data = extract_tiff_payload(&exif_raw).ok_or_else(|| {
        "invalid Exif payload: failed to locate TIFF header while parsing Apple HDR tags".to_owned()
    })?;
    let maker_note = extract_maker_note_blob(tiff_data)?;
    let parsed = parse_apple_maker_note_scalars(&maker_note)?;
    Ok(Some(parsed))
}

fn extract_tiff_payload(exif_raw: &[u8]) -> Option<&[u8]> {
    if has_tiff_header(exif_raw) {
        return Some(exif_raw);
    }
    if exif_raw.len() >= 4 {
        let offset = u32::from_be_bytes(exif_raw[0..4].try_into().ok()?) as usize;
        if offset < exif_raw.len() && has_tiff_header(&exif_raw[offset..]) {
            return Some(&exif_raw[offset..]);
        }
        let shifted = 4usize.checked_add(offset)?;
        if shifted < exif_raw.len() && has_tiff_header(&exif_raw[shifted..]) {
            return Some(&exif_raw[shifted..]);
        }
        if exif_raw.len() >= 8 && has_tiff_header(&exif_raw[4..]) {
            return Some(&exif_raw[4..]);
        }
    }
    None
}

fn has_tiff_header(data: &[u8]) -> bool {
    data.len() >= 8
        && ((data[0] == b'I' && data[1] == b'I' && data[2] == 0x2a && data[3] == 0x00)
            || (data[0] == b'M' && data[1] == b'M' && data[2] == 0x00 && data[3] == 0x2a))
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

fn read_u16(data: &[u8], offset: usize, endian: Endian) -> Result<u16, String> {
    let bytes: [u8; 2] = data
        .get(offset..offset + 2)
        .ok_or_else(|| "unexpected EOF while reading u16".to_owned())?
        .try_into()
        .map_err(|_| "failed to read u16".to_owned())?;
    Ok(match endian {
        Endian::Little => u16::from_le_bytes(bytes),
        Endian::Big => u16::from_be_bytes(bytes),
    })
}

fn read_u32(data: &[u8], offset: usize, endian: Endian) -> Result<u32, String> {
    let bytes: [u8; 4] = data
        .get(offset..offset + 4)
        .ok_or_else(|| "unexpected EOF while reading u32".to_owned())?
        .try_into()
        .map_err(|_| "failed to read u32".to_owned())?;
    Ok(match endian {
        Endian::Little => u32::from_le_bytes(bytes),
        Endian::Big => u32::from_be_bytes(bytes),
    })
}

#[derive(Clone, Copy)]
struct IfdEntry {
    tag: u16,
    field_type: u16,
    count: u32,
    value_or_offset: u32,
    entry_offset: usize,
}

fn extract_maker_note_blob(tiff: &[u8]) -> Result<Vec<u8>, String> {
    let endian = match tiff.get(0..2) {
        Some(b"II") => Endian::Little,
        Some(b"MM") => Endian::Big,
        _ => return Err("invalid TIFF byte order".to_owned()),
    };

    let first_ifd_offset = read_u32(tiff, 4, endian)? as usize;
    let ifd0 = parse_ifd_entries(tiff, first_ifd_offset, endian)?;
    let exif_ifd_offset = ifd0
        .iter()
        .find(|e| e.tag == EXIF_IFD_POINTER_TAG)
        .map(|e| e.value_or_offset as usize)
        .ok_or_else(|| "missing Exif IFD pointer tag (0x8769)".to_owned())?;

    let exif_ifd = parse_ifd_entries(tiff, exif_ifd_offset, endian)?;
    let maker_note_entry = exif_ifd
        .iter()
        .find(|e| e.tag == MAKER_NOTE_TAG)
        .ok_or_else(|| "missing MakerNote tag (0x927c)".to_owned())?;

    extract_ifd_value_bytes(tiff, *maker_note_entry, endian)
}

fn parse_ifd_entries(
    tiff: &[u8],
    ifd_offset: usize,
    endian: Endian,
) -> Result<Vec<IfdEntry>, String> {
    let count = read_u16(tiff, ifd_offset, endian)? as usize;
    let mut entries = Vec::with_capacity(count);
    let mut entry_offset = ifd_offset
        .checked_add(2)
        .ok_or_else(|| "IFD offset overflow".to_owned())?;
    for _ in 0..count {
        let tag = read_u16(tiff, entry_offset, endian)?;
        let field_type = read_u16(tiff, entry_offset + 2, endian)?;
        let count = read_u32(tiff, entry_offset + 4, endian)?;
        let value_or_offset = read_u32(tiff, entry_offset + 8, endian)?;
        entries.push(IfdEntry {
            tag,
            field_type,
            count,
            value_or_offset,
            entry_offset,
        });
        entry_offset = entry_offset
            .checked_add(12)
            .ok_or_else(|| "IFD entry offset overflow".to_owned())?;
    }
    Ok(entries)
}

fn extract_ifd_value_bytes(
    tiff: &[u8],
    entry: IfdEntry,
    _endian: Endian,
) -> Result<Vec<u8>, String> {
    let type_size = tiff_type_size(entry.field_type).ok_or_else(|| {
        format!(
            "unsupported TIFF type {} in MakerNote extraction",
            entry.field_type
        )
    })?;
    let byte_len = usize::try_from(entry.count)
        .map_err(|_| "TIFF count too large".to_owned())?
        .checked_mul(type_size)
        .ok_or_else(|| "TIFF value length overflow".to_owned())?;

    if byte_len <= 4 {
        let raw = tiff
            .get(entry.entry_offset + 8..entry.entry_offset + 12)
            .ok_or_else(|| "unexpected EOF in inline TIFF value".to_owned())?;
        return Ok(raw[..byte_len].to_vec());
    }

    let offset = entry.value_or_offset as usize;
    let end = offset
        .checked_add(byte_len)
        .ok_or_else(|| "TIFF value offset overflow".to_owned())?;
    let value = tiff
        .get(offset..end)
        .ok_or_else(|| "TIFF value points outside payload".to_owned())?;
    Ok(value.to_vec())
}

fn tiff_type_size(field_type: u16) -> Option<usize> {
    match field_type {
        1 | 2 | 6 | 7 => Some(1),
        3 | 8 => Some(2),
        4 | 9 | 11 => Some(4),
        5 | 10 | 12 => Some(8),
        _ => None,
    }
}

fn parse_apple_maker_note_scalars(maker_note: &[u8]) -> Result<AppleHdrScalars, String> {
    if maker_note.len() < 16 || !maker_note.starts_with(b"Apple iOS") {
        return Err("MakerNote is not in Apple iOS format".to_owned());
    }
    if maker_note.get(12..14) != Some(b"MM") {
        return Err("Apple MakerNote missing expected MM marker".to_owned());
    }

    let entry_count = u16::from_be_bytes(
        maker_note
            .get(14..16)
            .ok_or_else(|| "truncated Apple MakerNote header".to_owned())?
            .try_into()
            .map_err(|_| "failed to parse Apple MakerNote entry count".to_owned())?,
    ) as usize;

    let mut hdr_headroom = None;
    let mut hdr_gain = None;

    for idx in 0..entry_count {
        let off = 16usize
            .checked_add(
                idx.checked_mul(12)
                    .ok_or_else(|| "Apple MakerNote entry offset overflow".to_owned())?,
            )
            .ok_or_else(|| "Apple MakerNote entry offset overflow".to_owned())?;
        let entry = maker_note
            .get(off..off + 12)
            .ok_or_else(|| "truncated Apple MakerNote entry table".to_owned())?;
        let tag = u16::from_be_bytes([entry[0], entry[1]]);
        let field_type = u16::from_be_bytes([entry[2], entry[3]]);
        let count = u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]);
        let value_offset = u32::from_be_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;

        if tag != HDR_HEADROOM_TAG && tag != HDR_GAIN_TAG {
            continue;
        }
        if field_type != 10 || count != 1 {
            return Err(format!(
                "Apple MakerNote tag 0x{tag:04x} has unexpected type/count ({field_type}/{count})"
            ));
        }

        let value = maker_note
            .get(value_offset..value_offset + 8)
            .ok_or_else(|| format!("Apple MakerNote tag 0x{tag:04x} points outside payload"))?;
        let mut numerator = i32::from_be_bytes([value[0], value[1], value[2], value[3]]);
        let mut denominator = i32::from_be_bytes([value[4], value[5], value[6], value[7]]);
        if denominator == 0 {
            return Err(format!(
                "Apple MakerNote tag 0x{tag:04x} has zero denominator"
            ));
        }
        if denominator < 0 {
            denominator = -denominator;
            numerator = -numerator;
        }

        let parsed = SignedRational {
            numerator,
            denominator,
        };
        if tag == HDR_HEADROOM_TAG {
            hdr_headroom = Some(parsed);
        } else if tag == HDR_GAIN_TAG {
            hdr_gain = Some(parsed);
        }
    }

    let hdr_headroom = hdr_headroom
        .ok_or_else(|| "missing Apple MakerNote tag 0x0021 (HDRHeadroom)".to_owned())?;
    let hdr_gain =
        hdr_gain.ok_or_else(|| "missing Apple MakerNote tag 0x0030 (HDRGain)".to_owned())?;

    Ok(AppleHdrScalars {
        hdr_headroom,
        hdr_gain,
    })
}

fn extract_plane(
    data: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    bit_depth: BitDepth,
) -> Vec<u16> {
    let mut out = Vec::with_capacity(width * height);
    if bit_depth == BitDepth::Eight {
        for row in 0..height {
            let row_start = row * stride;
            for col in 0..width {
                out.push(data[row_start + col] as u16);
            }
        }
    } else {
        for row in 0..height {
            let row_start = row * stride;
            for col in 0..width {
                let offset = row_start + col * 2;
                out.push(u16::from_le_bytes([data[offset], data[offset + 1]]));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_maker_note() -> Vec<u8> {
        let mut data = vec![0u8; 128];
        data[0..9].copy_from_slice(b"Apple iOS");
        data[12..14].copy_from_slice(b"MM");
        data[14..16].copy_from_slice(&2u16.to_be_bytes());

        let first = 16usize;
        data[first..first + 2].copy_from_slice(&HDR_HEADROOM_TAG.to_be_bytes());
        data[first + 2..first + 4].copy_from_slice(&10u16.to_be_bytes());
        data[first + 4..first + 8].copy_from_slice(&1u32.to_be_bytes());
        data[first + 8..first + 12].copy_from_slice(&64u32.to_be_bytes());

        let second = 28usize;
        data[second..second + 2].copy_from_slice(&HDR_GAIN_TAG.to_be_bytes());
        data[second + 2..second + 4].copy_from_slice(&10u16.to_be_bytes());
        data[second + 4..second + 8].copy_from_slice(&1u32.to_be_bytes());
        data[second + 8..second + 12].copy_from_slice(&72u32.to_be_bytes());

        data[64..68].copy_from_slice(&48400i32.to_be_bytes());
        data[68..72].copy_from_slice(&65123i32.to_be_bytes());
        data[72..76].copy_from_slice(&1804i32.to_be_bytes());
        data[76..80].copy_from_slice(&556975i32.to_be_bytes());

        data
    }

    #[test]
    fn parse_apple_maker_note_extracts_expected_tags() {
        let note = sample_maker_note();
        let parsed = parse_apple_maker_note_scalars(&note).expect("parse should succeed");
        assert_eq!(parsed.hdr_headroom.numerator, 48400);
        assert_eq!(parsed.hdr_headroom.denominator, 65123);
        assert_eq!(parsed.hdr_gain.numerator, 1804);
        assert_eq!(parsed.hdr_gain.denominator, 556975);
    }

    #[test]
    fn parse_apple_maker_note_fails_on_missing_tag() {
        let mut note = sample_maker_note();
        note[28..30].copy_from_slice(&0x0040u16.to_be_bytes());
        let err = parse_apple_maker_note_scalars(&note).unwrap_err();
        assert!(err.contains("0x0030"));
    }

    #[test]
    fn parse_apple_maker_note_fails_on_zero_denominator() {
        let mut note = sample_maker_note();
        note[68..72].copy_from_slice(&0i32.to_be_bytes());
        let err = parse_apple_maker_note_scalars(&note).unwrap_err();
        assert!(err.contains("zero denominator"));
    }
}
