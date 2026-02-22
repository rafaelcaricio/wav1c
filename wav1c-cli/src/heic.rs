use libheif_rs::{Chroma, ColorSpace, HeifContext, LibHeif};
use wav1c::y4m::FramePixels;
use wav1c::{BitDepth, ColorRange};

pub fn decode_heic(path: &str) -> Result<FramePixels, String> {
    let ctx = HeifContext::read_from_file(path)
        .map_err(|e| format!("failed to open HEIC file: {e}"))?;

    let handle = ctx
        .primary_image_handle()
        .map_err(|e| format!("failed to get primary image: {e}"))?;

    let width = handle.width();
    let height = handle.height();
    let luma_bits = handle.luma_bits_per_pixel();

    let bit_depth = if luma_bits > 8 {
        BitDepth::Ten
    } else {
        BitDepth::Eight
    };

    let lib_heif = LibHeif::new();
    let image = lib_heif
        .decode(&handle, ColorSpace::YCbCr(Chroma::C420), None)
        .map_err(|e| format!("failed to decode HEIC: {e}"))?;

    let planes = image.planes();
    let y_plane = planes.y.ok_or("no Y plane in decoded HEIC")?;
    let u_plane = planes.cb.ok_or("no Cb plane in decoded HEIC")?;
    let v_plane = planes.cr.ok_or("no Cr plane in decoded HEIC")?;

    let uv_width = (width as usize + 1) / 2;
    let uv_height = (height as usize + 1) / 2;

    let y = extract_plane(y_plane.data, y_plane.stride, width as usize, height as usize, bit_depth);
    let u = extract_plane(u_plane.data, u_plane.stride, uv_width, uv_height, bit_depth);
    let v = extract_plane(v_plane.data, v_plane.stride, uv_width, uv_height, bit_depth);

    Ok(FramePixels {
        y,
        u,
        v,
        width,
        height,
        bit_depth,
        color_range: ColorRange::Full,
    })
}

fn extract_plane(data: &[u8], stride: usize, width: usize, height: usize, bit_depth: BitDepth) -> Vec<u16> {
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
