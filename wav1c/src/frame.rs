use crate::bitwriter::BitWriter;
use crate::dequant::DequantValues;
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
    let dq = crate::dequant::lookup_dequant(crate::DEFAULT_BASE_Q_IDX, pixels.bit_depth);
    encode_frame_with_recon(pixels, crate::DEFAULT_BASE_Q_IDX, dq).0
}

pub fn encode_frame_with_recon(
    pixels: &FramePixels,
    base_q_idx: u8,
    dq: DequantValues,
) -> (Vec<u8>, FramePixels) {
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

    write_loopfilter_params(&mut w, base_q_idx);
    write_cdef_params(&mut w, base_q_idx);

    w.write_bit(false);
    w.write_bit(true);

    let mut header_bytes = w.finalize();
    let (tile_data, mut recon) = crate::tile::encode_tile_with_recon(pixels, dq, base_q_idx);

    let (damping_minus_3, y_strength, _uv_strength) = cdef_strength_for_qidx(base_q_idx);
    crate::cdef::apply_cdef_frame(
        &mut recon,
        (y_strength >> 2) as i32,
        (y_strength & 3) as i32,
        (damping_minus_3 + 3) as i32,
    );

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

fn cdef_strength_for_qidx(base_q_idx: u8) -> (u8, u8, u8) {
    if base_q_idx < 64 {
        (0, 0, 0)
    } else {
        let pri = (base_q_idx as u32 / 16).clamp(1, 15) as u8;
        let y_strength = pri << 2; // sec = 0
        let uv_strength = pri << 2; // sec = 0
        (2, y_strength, uv_strength) // damping = 5
    }
}

fn write_cdef_params(w: &mut BitWriter, base_q_idx: u8) {
    let (damping_minus_3, y_strength, uv_strength) = cdef_strength_for_qidx(base_q_idx);
    w.write_bits(damping_minus_3 as u64, 2);
    w.write_bits(0, 2);
    w.write_bits(y_strength as u64, 6);
    w.write_bits(uv_strength as u64, 6);
}

fn loop_filter_level_for_qidx(_base_q_idx: u8) -> u8 {
    0
}

fn write_loopfilter_params(w: &mut BitWriter, base_q_idx: u8) {
    let level = loop_filter_level_for_qidx(base_q_idx);
    w.write_bits(level as u64, 6);
    w.write_bits(level as u64, 6);
    if level > 0 {
        w.write_bits(level as u64, 6);
        w.write_bits(level as u64, 6);
    }
    w.write_bits(0, 3);
    w.write_bit(true);
    w.write_bit(false);
}

pub fn encode_inter_frame(
    pixels: &FramePixels,
    reference: &FramePixels,
    refresh_frame_flags: u8,
    ref_slot: u8,
    show_frame: bool,
) -> Vec<u8> {
    let dq = crate::dequant::lookup_dequant(crate::DEFAULT_BASE_Q_IDX, pixels.bit_depth);
    encode_inter_frame_with_recon(
        pixels,
        reference,
        None,
        refresh_frame_flags,
        ref_slot,
        0, // bwd_ref_slot
        show_frame,
        crate::DEFAULT_BASE_Q_IDX,
        dq,
    )
    .0
}

pub fn encode_show_existing_frame(slot: u8) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.write_bit(true); // show_existing_frame
    w.write_bits(slot as u64, 3); // frame_to_show_map_idx
    w.trailing_bits()
}

#[allow(clippy::too_many_arguments)]
pub fn encode_inter_frame_with_recon(
    pixels: &FramePixels,
    reference: &FramePixels,
    forward_reference: Option<&FramePixels>,
    refresh_frame_flags: u8,
    ref_slot: u8,
    bwd_ref_slot: u8,
    show_frame: bool,
    base_q_idx: u8,
    dq: DequantValues,
) -> (Vec<u8>, FramePixels) {
    let mut w = BitWriter::new();

    let sbw = pixels.width.div_ceil(64);
    let sbh = pixels.height.div_ceil(64);

    w.write_bit(false); // show_existing_frame
    w.write_bits(1, 2); // frame_type
    w.write_bit(show_frame); // show_frame
    if !show_frame {
        w.write_bit(true); // showable_frame
    }
    w.write_bit(true); // error_resilient_mode
    w.write_bit(true); // disable_cdf_update
    w.write_bit(false); // allow_high_precision_mv

    w.write_bits(refresh_frame_flags as u64, 8);

    // Write the 7 reference frame indices. Ref 0 is LAST_FRAME, Ref 1 is LAST2_FRAME... Ref 6 is ALTREF_FRAME
    // AV1 Ref frames:
    // 0: LAST_FRAME
    // 1: LAST2_FRAME
    // 2: LAST3_FRAME
    // 3: GOLDEN_FRAME
    // 4: BWDREF_FRAME
    // 5: ALTREF2_FRAME
    // 6: ALTREF_FRAME
    for i in 0..7 {
        if i >= 4 {
            w.write_bits(bwd_ref_slot as u64, 3);
        } else {
            w.write_bits(ref_slot as u64, 3);
        }
    }

    w.write_bit(false); // frame_size_override_flag

    w.write_bit(false); // render_and_frame_size_different
    w.write_bit(false); // is_filter_switchable
    w.write_bits(0, 2); // interpolation_filter
    w.write_bit(false); // is_motion_mode_switchable

    write_tile_info(&mut w, sbw, sbh);

    write_quant_params(&mut w, base_q_idx);

    w.write_bit(false);

    w.write_bit(false);

    write_loopfilter_params(&mut w, base_q_idx);
    write_cdef_params(&mut w, base_q_idx);

    w.write_bit(false);
    w.write_bit(false);
    w.write_bit(true);

    for _ in 0..7 {
        w.write_bit(false);
    }

    let mut header_bytes = w.finalize();
    let (tile_data, mut recon) = crate::tile::encode_inter_tile_with_recon(
        pixels,
        reference,
        forward_reference,
        dq,
        base_q_idx,
    );

    let (damping_minus_3, y_strength, _uv_strength) = cdef_strength_for_qidx(base_q_idx);
    crate::cdef::apply_cdef_frame(
        &mut recon,
        (y_strength >> 2) as i32,
        (y_strength & 3) as i32,
        (damping_minus_3 + 3) as i32,
    );

    header_bytes.extend_from_slice(&tile_data);
    (header_bytes, recon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdef_strength_mapping() {
        assert_eq!(cdef_strength_for_qidx(0), (0, 0, 0));
        assert_ne!(cdef_strength_for_qidx(128), (0, 0, 0));
    }

    #[test]
    fn loop_filter_level_mapping() {
        for q in 0..=255u8 {
            assert_eq!(loop_filter_level_for_qidx(q), 0);
        }
    }

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

        expected.write_bits(2, 2);
        expected.write_bits(0, 2);
        expected.write_bits(32, 6);
        expected.write_bits(32, 6);

        expected.write_bit(false);
        expected.write_bit(true);

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

        expected.write_bits(2, 2);
        expected.write_bits(0, 2);
        expected.write_bits(32, 6);
        expected.write_bits(32, 6);

        expected.write_bit(false);
        expected.write_bit(true);

        let expected_header = expected.finalize();
        let pixels = FramePixels::solid(320, 240, 128, 128, 128);
        let bytes = encode_frame(&pixels);
        assert_eq!(&bytes[..expected_header.len()], &expected_header[..]);
    }

    #[test]
    fn inter_frame_header_64x64_bit_layout() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes = encode_inter_frame(&pixels, &reference, 0x01, 0, true);

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

        expected.write_bits(2, 2);
        expected.write_bits(0, 2);
        expected.write_bits(32, 6);
        expected.write_bits(32, 6);

        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(true);

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
        let inter_bytes = encode_inter_frame(&pixels, &reference, 0x01, 0, true);
        assert_ne!(key_bytes, inter_bytes);
    }

    #[test]
    fn inter_frame_header_ref_slot_encoded() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes_slot0 = encode_inter_frame(&pixels, &reference, 0x01, 0, true);
        let bytes_slot3 = encode_inter_frame(&pixels, &reference, 0x01, 3, true);
        assert_ne!(bytes_slot0, bytes_slot3);
    }

    #[test]
    fn inter_frame_header_refresh_flags_encoded() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes_01 = encode_inter_frame(&pixels, &reference, 0x01, 0, true);
        let bytes_ff = encode_inter_frame(&pixels, &reference, 0xFF, 0, true);
        assert_ne!(bytes_01, bytes_ff);
    }

    #[test]
    fn inter_frame_starts_with_show_existing_frame_false() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes = encode_inter_frame(&pixels, &reference, 0x01, 0, true);
        assert_eq!(bytes[0] & 0x80, 0);
    }

    #[test]
    fn inter_frame_has_frame_type_1() {
        let pixels = FramePixels::solid(64, 64, 128, 128, 128);
        let reference = FramePixels::solid(64, 64, 128, 128, 128);
        let bytes = encode_inter_frame(&pixels, &reference, 0x01, 0, true);
        let frame_type = (bytes[0] >> 5) & 0x03;
        assert_eq!(frame_type, 1);
    }
}
