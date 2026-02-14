use std::fmt;

#[derive(Debug)]
pub enum EncoderError {
    InvalidDimensions { width: u32, height: u32 },
    DimensionMismatch { expected_w: u32, expected_h: u32, got_w: u32, got_h: u32 },
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
            EncoderError::DimensionMismatch { expected_w, expected_h, got_w, got_h } => {
                write!(
                    f,
                    "frame dimension mismatch: expected {}x{}, got {}x{}",
                    expected_w, expected_h, got_w, got_h
                )
            }
        }
    }
}

impl std::error::Error for EncoderError {}
