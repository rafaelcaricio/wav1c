use std::io::{self, Write};

pub fn write_ivf_header<W: Write>(
    writer: &mut W,
    width: u16,
    height: u16,
    num_frames: u32,
) -> io::Result<()> {
    writer.write_all(b"DKIF")?;
    writer.write_all(&0u16.to_le_bytes())?;
    writer.write_all(&32u16.to_le_bytes())?;
    writer.write_all(b"AV01")?;
    writer.write_all(&width.to_le_bytes())?;
    writer.write_all(&height.to_le_bytes())?;
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
