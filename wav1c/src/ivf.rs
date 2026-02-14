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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ivf_header_64x64() {
        let mut buf = Vec::new();
        write_ivf_header(&mut buf, 64, 64, 1).unwrap();
        assert_eq!(buf.len(), 32);
        assert_eq!(&buf[0..4], b"DKIF");
        assert_eq!(&buf[4..6], &0u16.to_le_bytes());
        assert_eq!(&buf[6..8], &32u16.to_le_bytes());
        assert_eq!(&buf[8..12], b"AV01");
        assert_eq!(&buf[12..14], &64u16.to_le_bytes());
        assert_eq!(&buf[14..16], &64u16.to_le_bytes());
        assert_eq!(&buf[16..20], &25u32.to_le_bytes());
        assert_eq!(&buf[20..24], &1u32.to_le_bytes());
        assert_eq!(&buf[24..28], &1u32.to_le_bytes());
        assert_eq!(&buf[28..32], &0u32.to_le_bytes());
    }

    #[test]
    fn ivf_frame_wrapper() {
        let mut buf = Vec::new();
        let data = vec![0xAA, 0xBB, 0xCC];
        write_ivf_frame(&mut buf, 0, &data).unwrap();
        assert_eq!(buf.len(), 12 + 3);
        assert_eq!(&buf[0..4], &3u32.to_le_bytes());
        assert_eq!(&buf[4..12], &0u64.to_le_bytes());
        assert_eq!(&buf[12..], &[0xAA, 0xBB, 0xCC]);
    }
}
