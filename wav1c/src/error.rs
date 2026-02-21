use std::fmt;

#[derive(Debug)]
pub enum EncoderError {
    InvalidDimensions {
        width: u32,
        height: u32,
    },
    DimensionMismatch {
        expected_w: u32,
        expected_h: u32,
        got_w: u32,
        got_h: u32,
    },
    UnsupportedBitDepth {
        bit_depth: u8,
    },
    FrameBitDepthMismatch {
        expected: u8,
        got: u8,
    },
    SampleOutOfRange {
        bit_depth: u8,
        sample: u16,
    },
    InvalidHdrMetadata {
        reason: &'static str,
    },
}

impl fmt::Display for EncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncoderError::InvalidDimensions { width, height } => {
                write!(
                    f,
                    "invalid dimensions {}x{}: width must be 1..=4096, height must be 1..=2304",
                    width, height
                )
            }
            EncoderError::DimensionMismatch {
                expected_w,
                expected_h,
                got_w,
                got_h,
            } => {
                write!(
                    f,
                    "frame dimension mismatch: expected {}x{}, got {}x{}",
                    expected_w, expected_h, got_w, got_h
                )
            }
            EncoderError::UnsupportedBitDepth { bit_depth } => {
                write!(f, "unsupported bit depth: {}", bit_depth)
            }
            EncoderError::FrameBitDepthMismatch { expected, got } => {
                write!(
                    f,
                    "frame bit-depth mismatch: expected {}-bit, got {}-bit",
                    expected, got
                )
            }
            EncoderError::SampleOutOfRange { bit_depth, sample } => {
                write!(
                    f,
                    "sample value {} is out of range for {}-bit content",
                    sample, bit_depth
                )
            }
            EncoderError::InvalidHdrMetadata { reason } => {
                write!(f, "invalid HDR metadata: {}", reason)
            }
        }
    }
}

impl std::error::Error for EncoderError {}
