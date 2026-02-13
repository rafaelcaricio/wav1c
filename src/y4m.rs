pub struct FramePixels {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl FramePixels {
    pub fn from_y4m(data: &[u8]) -> Self {
        let header_end = data
            .iter()
            .position(|&b| b == b'\n')
            .expect("No header line in Y4M data");
        let header_line = std::str::from_utf8(&data[..header_end]).expect("Invalid Y4M header");

        assert!(
            header_line.starts_with("YUV4MPEG2"),
            "Not a YUV4MPEG2 file"
        );

        let mut width = 0u32;
        let mut height = 0u32;

        for token in header_line.split_whitespace().skip(1) {
            let (key, val) = token.split_at(1);
            match key {
                "W" => width = val.parse().expect("Invalid width"),
                "H" => height = val.parse().expect("Invalid height"),
                "C" => {
                    assert!(
                        val.starts_with("420"),
                        "Only 4:2:0 colorspace is supported"
                    );
                }
                _ => {}
            }
        }

        assert!(width > 0 && height > 0, "Missing W/H in Y4M header");

        let frame_marker = b"FRAME\n";
        let frame_start = data
            .windows(frame_marker.len())
            .position(|w| w == frame_marker)
            .expect("No FRAME marker in Y4M data")
            + frame_marker.len();

        let y_size = (width * height) as usize;
        let uv_w = width.div_ceil(2) as usize;
        let uv_h = height.div_ceil(2) as usize;
        let uv_size = uv_w * uv_h;

        let y_plane = data[frame_start..frame_start + y_size].to_vec();
        let u_plane = data[frame_start + y_size..frame_start + y_size + uv_size].to_vec();
        let v_plane =
            data[frame_start + y_size + uv_size..frame_start + y_size + 2 * uv_size].to_vec();

        Self {
            y: y_plane,
            u: u_plane,
            v: v_plane,
            width,
            height,
        }
    }

    pub fn from_y4m_file(path: &std::path::Path) -> std::io::Result<Self> {
        let data = std::fs::read(path)?;
        Ok(Self::from_y4m(&data))
    }

    pub fn solid(width: u32, height: u32, y: u8, u: u8, v: u8) -> Self {
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
    }

    #[test]
    fn parse_y4m_no_colorspace() {
        let header = b"YUV4MPEG2 W16 H16 F25:1\n";
        let mut data = header.to_vec();
        data.extend_from_slice(b"FRAME\n");
        data.extend(vec![200u8; 16 * 16]);
        data.extend(vec![100u8; 8 * 8]);
        data.extend(vec![50u8; 8 * 8]);

        let pixels = FramePixels::from_y4m(&data);
        assert_eq!(pixels.width, 16);
        assert_eq!(pixels.height, 16);
        assert!(pixels.y.iter().all(|&p| p == 200));
        assert!(pixels.u.iter().all(|&p| p == 100));
        assert!(pixels.v.iter().all(|&p| p == 50));
    }

    #[test]
    fn solid_constructor_matches_y4m() {
        let y4m = create_test_y4m(64, 64, 81, 91, 81);
        let from_y4m = FramePixels::from_y4m(&y4m);
        let from_solid = FramePixels::solid(64, 64, 81, 91, 81);

        assert_eq!(from_y4m.y, from_solid.y);
        assert_eq!(from_y4m.u, from_solid.u);
        assert_eq!(from_y4m.v, from_solid.v);
    }

    #[test]
    fn solid_odd_dimensions() {
        let pixels = FramePixels::solid(17, 33, 128, 128, 128);
        assert_eq!(pixels.y.len(), 17 * 33);
        assert_eq!(pixels.u.len(), 9 * 17);
        assert_eq!(pixels.v.len(), 9 * 17);
    }
}
