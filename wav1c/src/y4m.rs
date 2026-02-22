use crate::video::{BitDepth, ColorRange};

#[derive(Debug, Clone)]
pub struct FramePixels {
    pub y: Vec<u16>,
    pub u: Vec<u16>,
    pub v: Vec<u16>,
    pub width: u32,
    pub height: u32,
    pub bit_depth: BitDepth,
    pub color_range: ColorRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Y4mError {
    MissingHeader,
    InvalidHeaderUtf8,
    InvalidHeader(&'static str),
    UnsupportedColorspace(String),
    InvalidDimensions,
    NoFrameMarker,
    TruncatedFrameData,
}

impl std::fmt::Display for Y4mError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Y4mError::MissingHeader => write!(f, "No header line in Y4M data"),
            Y4mError::InvalidHeaderUtf8 => write!(f, "Invalid Y4M header"),
            Y4mError::InvalidHeader(msg) => write!(f, "Invalid Y4M header: {msg}"),
            Y4mError::UnsupportedColorspace(cs) => {
                write!(f, "Only 4:2:0 Y4M is supported, got {cs}")
            }
            Y4mError::InvalidDimensions => write!(f, "Missing or invalid W/H in Y4M header"),
            Y4mError::NoFrameMarker => write!(f, "No FRAME marker in Y4M data"),
            Y4mError::TruncatedFrameData => write!(f, "Truncated frame data"),
        }
    }
}

impl std::error::Error for Y4mError {}

fn parse_color_range_token(token: &str) -> Option<ColorRange> {
    if let Some(v) = token.strip_prefix("XCOLORRANGE=") {
        match v {
            "FULL" => Some(ColorRange::Full),
            "LIMITED" => Some(ColorRange::Limited),
            _ => None,
        }
    } else {
        None
    }
}

fn parse_bit_depth_from_colorspace(colorspace: &str) -> Result<BitDepth, Y4mError> {
    if !colorspace.starts_with("420") {
        return Err(Y4mError::UnsupportedColorspace(colorspace.to_owned()));
    }
    if colorspace.contains("p10") || colorspace.contains("P10") {
        Ok(BitDepth::Ten)
    } else {
        Ok(BitDepth::Eight)
    }
}

fn parse_frame_header_line(
    line: &[u8],
    default_color_range: ColorRange,
) -> Result<ColorRange, Y4mError> {
    let s = std::str::from_utf8(line).map_err(|_| Y4mError::InvalidHeaderUtf8)?;
    if !s.starts_with("FRAME") {
        return Err(Y4mError::NoFrameMarker);
    }
    let mut color_range = default_color_range;
    for token in s.split_whitespace().skip(1) {
        if let Some(r) = parse_color_range_token(token) {
            color_range = r;
        }
    }
    Ok(color_range)
}

impl FramePixels {
    pub fn try_all_from_y4m(data: &[u8]) -> Result<Vec<Self>, Y4mError> {
        let header_end = data
            .iter()
            .position(|&b| b == b'\n')
            .ok_or(Y4mError::MissingHeader)?;
        let header_line =
            std::str::from_utf8(&data[..header_end]).map_err(|_| Y4mError::InvalidHeaderUtf8)?;

        if !header_line.starts_with("YUV4MPEG2") {
            return Err(Y4mError::InvalidHeader("Not a YUV4MPEG2 file"));
        }

        let mut width = 0u32;
        let mut height = 0u32;
        let mut bit_depth = BitDepth::Eight;
        let mut default_color_range = ColorRange::Limited;

        for token in header_line.split_whitespace().skip(1) {
            let (key, val) = token.split_at(1);
            match key {
                "W" => {
                    width = val
                        .parse()
                        .map_err(|_| Y4mError::InvalidHeader("Invalid width"))?;
                }
                "H" => {
                    height = val
                        .parse()
                        .map_err(|_| Y4mError::InvalidHeader("Invalid height"))?;
                }
                "C" => {
                    bit_depth = parse_bit_depth_from_colorspace(val)?;
                }
                _ => {
                    if let Some(r) = parse_color_range_token(token) {
                        default_color_range = r;
                    }
                }
            }
        }

        if width == 0 || height == 0 {
            return Err(Y4mError::InvalidDimensions);
        }

        let y_size = (width * height) as usize;
        let uv_w = width.div_ceil(2) as usize;
        let uv_h = height.div_ceil(2) as usize;
        let uv_size = uv_w * uv_h;
        let bytes_per_sample = if bit_depth == BitDepth::Ten { 2 } else { 1 };
        let frame_data_size = (y_size + 2 * uv_size) * bytes_per_sample;

        let mut frames = Vec::new();
        let mut pos = header_end + 1;

        while pos < data.len() {
            let line_end_rel = data[pos..]
                .iter()
                .position(|&b| b == b'\n')
                .ok_or(Y4mError::TruncatedFrameData)?;
            let line_end = pos + line_end_rel;
            let line = &data[pos..line_end];
            let mut color_range = default_color_range;
            if !line.is_empty() {
                color_range = parse_frame_header_line(line, default_color_range)?;
            }

            let pixel_start = line_end + 1;
            if pixel_start + frame_data_size > data.len() {
                return Err(Y4mError::TruncatedFrameData);
            }

            let frame_data = &data[pixel_start..pixel_start + frame_data_size];

            let (y_plane, u_plane, v_plane) = if bytes_per_sample == 1 {
                let y_plane = frame_data[..y_size].iter().map(|&b| b as u16).collect();
                let u_plane = frame_data[y_size..y_size + uv_size]
                    .iter()
                    .map(|&b| b as u16)
                    .collect();
                let v_plane = frame_data[y_size + uv_size..y_size + 2 * uv_size]
                    .iter()
                    .map(|&b| b as u16)
                    .collect();
                (y_plane, u_plane, v_plane)
            } else {
                let parse_16le = |slice: &[u8]| -> Vec<u16> {
                    slice
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect()
                };
                let y_bytes = y_size * 2;
                let uv_bytes = uv_size * 2;
                let y_plane = parse_16le(&frame_data[..y_bytes]);
                let u_plane = parse_16le(&frame_data[y_bytes..y_bytes + uv_bytes]);
                let v_plane = parse_16le(&frame_data[y_bytes + uv_bytes..y_bytes + 2 * uv_bytes]);
                (y_plane, u_plane, v_plane)
            };

            frames.push(Self {
                y: y_plane,
                u: u_plane,
                v: v_plane,
                width,
                height,
                bit_depth,
                color_range,
            });

            pos = pixel_start + frame_data_size;
        }

        if frames.is_empty() {
            return Err(Y4mError::NoFrameMarker);
        }

        Ok(frames)
    }

    pub fn all_from_y4m(data: &[u8]) -> Vec<Self> {
        Self::try_all_from_y4m(data).expect("Failed to parse Y4M")
    }

    pub fn all_from_y4m_file(path: &std::path::Path) -> std::io::Result<Vec<Self>> {
        let data = std::fs::read(path)?;
        Self::try_all_from_y4m(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn try_from_y4m(data: &[u8]) -> Result<Self, Y4mError> {
        let mut frames = Self::try_all_from_y4m(data)?;
        Ok(frames.swap_remove(0))
    }

    pub fn from_y4m(data: &[u8]) -> Self {
        Self::try_from_y4m(data).expect("Failed to parse Y4M")
    }

    pub fn solid(width: u32, height: u32, y: u8, u: u8, v: u8) -> Self {
        Self::solid_with_bit_depth(
            width,
            height,
            y as u16,
            u as u16,
            v as u16,
            BitDepth::Eight,
            ColorRange::Limited,
        )
    }

    pub fn solid_with_bit_depth(
        width: u32,
        height: u32,
        y: u16,
        u: u16,
        v: u16,
        bit_depth: BitDepth,
        color_range: ColorRange,
    ) -> Self {
        let y_size = (width * height) as usize;
        let uv_w = width.div_ceil(2) as usize;
        let uv_h = height.div_ceil(2) as usize;
        let uv_size = uv_w * uv_h;

        Self {
            y: vec![y; y_size],
            u: vec![u; uv_size],
            v: vec![v; uv_size],
            width,
            height,
            bit_depth,
            color_range,
        }
    }

    pub fn grid(
        width: u32,
        height: u32,
        cell_size: u32,
        bright: [u16; 3],
        dark: [u16; 3],
        bit_depth: BitDepth,
        color_range: ColorRange,
    ) -> Self {
        let y_size = (width * height) as usize;
        let uv_w = width.div_ceil(2) as usize;
        let uv_h = height.div_ceil(2) as usize;
        let uv_size = uv_w * uv_h;

        let mut y_plane = vec![0u16; y_size];
        let mut u_plane = vec![0u16; uv_size];
        let mut v_plane = vec![0u16; uv_size];

        for py in 0..height {
            for px in 0..width {
                let cell_x = px / cell_size;
                let cell_y = py / cell_size;
                let is_bright = (cell_x + cell_y).is_multiple_of(2);
                let yuv = if is_bright { bright } else { dark };
                y_plane[(py * width + px) as usize] = yuv[0];
            }
        }

        for cy in 0..uv_h as u32 {
            for cx in 0..uv_w as u32 {
                let px = cx * 2;
                let py = cy * 2;
                let cell_x = px / cell_size;
                let cell_y = py / cell_size;
                let is_bright = (cell_x + cell_y).is_multiple_of(2);
                let yuv = if is_bright { bright } else { dark };
                let idx = (cy * uv_w as u32 + cx) as usize;
                u_plane[idx] = yuv[1];
                v_plane[idx] = yuv[2];
            }
        }

        Self {
            y: y_plane,
            u: u_plane,
            v: v_plane,
            width,
            height,
            bit_depth,
            color_range,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_y4m(width: u32, height: u32, y_val: u8, u_val: u8, v_val: u8) -> Vec<u8> {
        let header = format!("YUV4MPEG2 W{} H{} F30:1 Ip C420jpeg\n", width, height);
        let mut data = header.into_bytes();
        data.extend_from_slice(b"FRAME\n");
        let y_size = (width * height) as usize;
        let uv_w = width.div_ceil(2) as usize;
        let uv_h = height.div_ceil(2) as usize;
        let uv_size = uv_w * uv_h;
        data.extend(vec![y_val; y_size]);
        data.extend(vec![u_val; uv_size]);
        data.extend(vec![v_val; uv_size]);
        data
    }

    #[test]
    fn parse_solid_y4m() {
        let y4m = create_test_y4m(64, 64, 128, 128, 128);
        let pixels = FramePixels::from_y4m(&y4m);
        assert_eq!(pixels.width, 64);
        assert_eq!(pixels.height, 64);
        assert_eq!(pixels.y.len(), 64 * 64);
        assert_eq!(pixels.u.len(), 32 * 32);
        assert_eq!(pixels.v.len(), 32 * 32);
        assert!(pixels.y.iter().all(|&p| p == 128));
        assert_eq!(pixels.bit_depth, BitDepth::Eight);
    }

    #[test]
    fn parse_10bit_y4m() {
        let mut data =
            b"YUV4MPEG2 W2 H2 F1:1 Ip C420p10 XYSCSS=420P10 XCOLORRANGE=FULL\nFRAME\n".to_vec();
        let samples = [
            1023u16, 900, 512, 0,   // Y
            512, // U
            600, // V
        ];
        for s in samples {
            data.extend_from_slice(&s.to_le_bytes());
        }
        let pixels = FramePixels::from_y4m(&data);
        assert_eq!(pixels.bit_depth, BitDepth::Ten);
        assert_eq!(pixels.color_range, ColorRange::Full);
        assert_eq!(pixels.y, vec![1023, 900, 512, 0]);
        assert_eq!(pixels.u, vec![512]);
        assert_eq!(pixels.v, vec![600]);
    }

    #[test]
    fn parse_frame_header_with_params() {
        let header = b"YUV4MPEG2 W2 H2 F1:1 Ip C420p10\n";
        let mut data = header.to_vec();
        data.extend_from_slice(b"FRAME XCOLORRANGE=FULL\n");
        for s in [100u16, 200, 300, 400, 500, 600] {
            data.extend_from_slice(&s.to_le_bytes());
        }

        let frame = FramePixels::from_y4m(&data);
        assert_eq!(frame.color_range, ColorRange::Full);
    }

    #[test]
    fn parse_errors_are_typed() {
        let err = FramePixels::try_all_from_y4m(b"bad data").unwrap_err();
        assert!(matches!(
            err,
            Y4mError::MissingHeader | Y4mError::InvalidHeader(_)
        ));
    }
}
