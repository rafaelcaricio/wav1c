use std::io::{self, Write};

pub fn write_ivf_header<W: Write>(
    writer: &mut W,
    width: u32,
    height: u32,
    num_frames: u32,
) -> io::Result<()> {
    let width_u16 = u16::try_from(width).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "IVF width {} exceeds 16-bit container limit (max {})",
                width,
                u16::MAX
            ),
        )
    })?;
    let height_u16 = u16::try_from(height).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "IVF height {} exceeds 16-bit container limit (max {})",
                height,
                u16::MAX
            ),
        )
    })?;

    writer.write_all(b"DKIF")?;
    writer.write_all(&0u16.to_le_bytes())?;
    writer.write_all(&32u16.to_le_bytes())?;
    writer.write_all(b"AV01")?;
    writer.write_all(&width_u16.to_le_bytes())?;
    writer.write_all(&height_u16.to_le_bytes())?;
    writer.write_all(&25u32.to_le_bytes())?;
    writer.write_all(&1u32.to_le_bytes())?;
    writer.write_all(&num_frames.to_le_bytes())?;
    writer.write_all(&0u32.to_le_bytes())?;
    Ok(())
}

pub fn write_ivf_frame<W: Write>(
    writer: &mut W,
    timestamp: u64,
    frame_data: &[u8],
) -> io::Result<()> {
    writer.write_all(&(frame_data.len() as u32).to_le_bytes())?;
    writer.write_all(&timestamp.to_le_bytes())?;
    writer.write_all(frame_data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_width_above_u16_limit() {
        let mut out = Vec::new();
        let err = write_ivf_header(&mut out, 70_000, 1_000, 1).expect_err("expected rejection");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_height_above_u16_limit() {
        let mut out = Vec::new();
        let err = write_ivf_header(&mut out, 1_000, 70_000, 1).expect_err("expected rejection");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
