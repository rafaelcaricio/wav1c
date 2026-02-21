#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitDepth {
    Eight = 8,
    Ten = 10,
}

impl BitDepth {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            8 => Some(Self::Eight),
            10 => Some(Self::Ten),
            _ => None,
        }
    }

    pub fn bits(self) -> u8 {
        self as u8
    }

    pub fn max_value(self) -> u16 {
        (1u16 << self.bits()) - 1
    }

    pub fn mid_value(self) -> u16 {
        1u16 << (self.bits() - 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRange {
    Limited,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorDescription {
    pub color_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coefficients: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentLightLevel {
    pub max_content_light_level: u16,
    pub max_frame_average_light_level: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasteringDisplayMetadata {
    pub primaries: [[u16; 2]; 3],
    pub white_point: [u16; 2],
    pub max_luminance: u32,
    pub min_luminance: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoSignal {
    pub bit_depth: BitDepth,
    pub color_range: ColorRange,
    pub color_description: Option<ColorDescription>,
}

impl Default for VideoSignal {
    fn default() -> Self {
        Self {
            bit_depth: BitDepth::Eight,
            color_range: ColorRange::Limited,
            color_description: None,
        }
    }
}

impl VideoSignal {
    pub fn hdr10(color_range: ColorRange) -> Self {
        Self {
            bit_depth: BitDepth::Ten,
            color_range,
            color_description: Some(ColorDescription {
                color_primaries: 9,
                transfer_characteristics: 16,
                matrix_coefficients: 9,
            }),
        }
    }
}
