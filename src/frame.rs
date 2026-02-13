use crate::bitwriter::BitWriter;
use crate::y4m::FramePixels;

const MAX_TILE_COLS: u32 = 64;
const MAX_TILE_ROWS: u32 = 64;
const MAX_TILE_WIDTH_SB: u32 = 4096 / 64;
const MAX_TILE_AREA_SB: u32 = 4096 * 2304 / (64 * 64);

fn tile_log2(blk_size: u32, target: u32) -> u32 {
    let mut k = 0;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}

pub fn encode_frame(pixels: &FramePixels) -> Vec<u8> {
    encode_frame_with_recon(pixels).0
}

pub fn encode_frame_with_recon(pixels: &FramePixels) -> (Vec<u8>, FramePixels) {
    let base_q_idx: u8 = 128;
    let mut w = BitWriter::new();

    let sbw = pixels.width.div_ceil(64);
    let sbh = pixels.height.div_ceil(64);

    w.write_bit(false);
    w.write_bits(0, 2);
    w.write_bit(true);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);

    w.write_bit(false);

    write_tile_info(&mut w, sbw, sbh);
    write_quant_params(&mut w, base_q_idx);

    w.write_bit(false);

    w.write_bit(false);

    write_loopfilter_params(&mut w);

    w.write_bit(false);

    let mut header_bytes = w.finalize();
    let (tile_data, recon) = crate::tile::encode_tile_with_recon(pixels);
    header_bytes.extend_from_slice(&tile_data);
    (header_bytes, recon)
}

fn write_tile_info(w: &mut BitWriter, sbw: u32, sbh: u32) {
    w.write_bit(true);

    let min_log2_cols = tile_log2(MAX_TILE_WIDTH_SB, sbw);
    let max_log2_cols = tile_log2(1, sbw.min(MAX_TILE_COLS));
    let log2_cols = min_log2_cols;

    if min_log2_cols < max_log2_cols {
        w.write_bit(false);
    }

    let min_log2_tiles = tile_log2(MAX_TILE_AREA_SB, sbw * sbh).max(min_log2_cols);
    let min_log2_rows = min_log2_tiles.saturating_sub(log2_cols);
    let max_log2_rows = tile_log2(1, sbh.min(MAX_TILE_ROWS));

    if min_log2_rows < max_log2_rows {
        w.write_bit(false);
    }
}

fn write_quant_params(w: &mut BitWriter, base_q_idx: u8) {
    w.write_bits(base_q_idx as u64, 8);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);
}

fn write_loopfilter_params(w: &mut BitWriter) {
    w.write_bits(0, 6);
    w.write_bits(0, 6);
    w.write_bits(0, 3);
    w.write_bit(true);
    w.write_bit(false);
}

pub fn encode_inter_frame(
    pixels: &FramePixels,
    reference: &FramePixels,
    refresh_frame_flags: u8,
    ref_slot: u8,
) -> Vec<u8> {
    encode_inter_frame_with_recon(pixels, reference, refresh_frame_flags, ref_slot).0
}

pub fn encode_inter_frame_with_recon(
    pixels: &FramePixels,
    reference: &FramePixels,
    refresh_frame_flags: u8,
    ref_slot: u8,
) -> (Vec<u8>, FramePixels) {
    let base_q_idx: u8 = 128;
    let mut w = BitWriter::new();

    let sbw = pixels.width.div_ceil(64);
    let sbh = pixels.height.div_ceil(64);

    w.write_bit(false);
    w.write_bits(1, 2);
    w.write_bit(true);
    w.write_bit(true);
    w.write_bit(true);
    w.write_bit(false);

    w.write_bits(refresh_frame_flags as u64, 8);

    for _ in 0..7 {
        w.write_bits(ref_slot as u64, 3);
    }

    w.write_bit(false);

    w.write_bit(false);
    w.write_bit(false);
    w.write_bits(0, 2);
    w.write_bit(false);

    write_tile_info(&mut w, sbw, sbh);

    write_quant_params(&mut w, base_q_idx);

    w.write_bit(false);

    w.write_bit(false);

    write_loopfilter_params(&mut w);

    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(false);

    for _ in 0..7 {
        w.write_bit(false);
    }

    let mut header_bytes = w.finalize();
    let (tile_data, recon) = crate::tile::encode_inter_tile_with_recon(pixels, reference);
    header_bytes.extend_from_slice(&tile_data);
    (header_bytes, recon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_log2_basic() {
        assert_eq!(tile_log2(64, 1), 0);
        assert_eq!(tile_log2(64, 64), 0);
        assert_eq!(tile_log2(64, 65), 1);
        assert_eq!(tile_log2(1, 1), 0);
        assert_eq!(tile_log2(1, 2), 1);
        assert_eq!(tile_log2(1, 64), 6);
    }

    #[test]
    fn frame_header_64x64_bit_layout() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes = encode_frame(&pixels);

        let mut expected = BitWriter::new();

        expected.write_bit(false);
        expected.write_bits(0, 2);
        expected.write_bit(true);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bit(true);

        expected.write_bits(128, 8);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bits(0, 6);
        expected.write_bits(0, 6);
        expected.write_bits(0, 3);
        expected.write_bit(true);
        expected.write_bit(false);

        expected.write_bit(false);

        let expected_header = expected.finalize();
        assert_eq!(&bytes[..expected_header.len()], &expected_header[..]);
    }

    #[test]
    fn frame_header_128x128_differs_from_64x64() {
        let pixels_64 = FramePixels::solid(64, 64, 128, 128, 128);
        let pixels_128 = FramePixels::solid(128, 128, 128, 128, 128);
        let bytes_64 = encode_frame(&pixels_64);
        let bytes_128 = encode_frame(&pixels_128);

        assert_ne!(bytes_64, bytes_128);
    }

    #[test]
    fn frame_header_starts_with_show_existing_frame_false() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes = encode_frame(&pixels);
        assert_eq!(bytes[0] & 0x80, 0);
    }

    #[test]
    fn frame_for_nonzero_residual_is_larger() {
        let skip_pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let color_pixels = FramePixels::solid(64, 64, 0, 0, 0);
        let skip_bytes = encode_frame(&skip_pixels);
        let color_bytes = encode_frame(&color_pixels);
        assert!(color_bytes.len() > skip_bytes.len());
    }

    #[test]
    fn frame_header_320x240_has_tile_bits() {
        let mut expected = BitWriter::new();

        expected.write_bit(false);
        expected.write_bits(0, 2);
        expected.write_bit(true);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bit(true);

        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bits(128, 8);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bits(0, 6);
        expected.write_bits(0, 6);
        expected.write_bits(0, 3);
        expected.write_bit(true);
        expected.write_bit(false);

        expected.write_bit(false);

        let expected_header = expected.finalize();
        let pixels = FramePixels::solid(320, 240, 128, 128, 128);
        let bytes = encode_frame(&pixels);
        assert_eq!(&bytes[..expected_header.len()], &expected_header[..]);
    }

    #[test]
    fn inter_frame_header_64x64_bit_layout() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes = encode_inter_frame(&pixels, &reference, 0x01, 0);

        let mut expected = BitWriter::new();

        expected.write_bit(false);
        expected.write_bits(1, 2);
        expected.write_bit(true);
        expected.write_bit(true);
        expected.write_bit(true);
        expected.write_bit(false);

        expected.write_bits(0x01, 8);

        for _ in 0..7 {
            expected.write_bits(0, 3);
        }

        expected.write_bit(false);

        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bits(0, 2);
        expected.write_bit(false);

        expected.write_bit(true);

        expected.write_bits(128, 8);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bit(false);

        expected.write_bits(0, 6);
        expected.write_bits(0, 6);
        expected.write_bits(0, 3);
        expected.write_bit(true);
        expected.write_bit(false);

        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);

        for _ in 0..7 {
            expected.write_bit(false);
        }

        let expected_header = expected.finalize();
        assert_eq!(&bytes[..expected_header.len()], &expected_header[..]);
    }

    #[test]
    fn inter_frame_header_differs_from_keyframe() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let key_bytes = encode_frame(&pixels);
        let inter_bytes = encode_inter_frame(&pixels, &reference, 0x01, 0);
        assert_ne!(key_bytes, inter_bytes);
    }

    #[test]
    fn inter_frame_header_ref_slot_encoded() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes_slot0 = encode_inter_frame(&pixels, &reference, 0x01, 0);
        let bytes_slot3 = encode_inter_frame(&pixels, &reference, 0x01, 3);
        assert_ne!(bytes_slot0, bytes_slot3);
    }

    #[test]
    fn inter_frame_header_refresh_flags_encoded() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes_01 = encode_inter_frame(&pixels, &reference, 0x01, 0);
        let bytes_ff = encode_inter_frame(&pixels, &reference, 0xFF, 0);
        assert_ne!(bytes_01, bytes_ff);
    }

    #[test]
    fn inter_frame_starts_with_show_existing_frame_false() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes = encode_inter_frame(&pixels, &reference, 0x01, 0);
        assert_eq!(bytes[0] & 0x80, 0);
    }

    #[test]
    fn inter_frame_has_frame_type_1() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes = encode_inter_frame(&pixels, &reference, 0x01, 0);
        let frame_type = (bytes[0] >> 5) & 0x03;
        assert_eq!(frame_type, 1);
    }
}
