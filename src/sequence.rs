use crate::bitwriter::BitWriter;

fn bits_needed(v: u32) -> u8 {
    if v == 0 {
        1
    } else {
        32 - v.leading_zeros() as u8
    }
}

pub fn encode_sequence_header(width: u32, height: u32) -> Vec<u8> {
    let mut w = BitWriter::new();

    let seq_profile = 0u64;
    let still_picture = false;
    let reduced_still_picture_header = false;
    let timing_info_present = false;
    let initial_display_delay_present = false;
    let operating_points_cnt_minus_1 = 0u64;
    let operating_point_idc = 0u64;
    let seq_level_idx = 13u64;

    w.write_bits(seq_profile, 3);
    w.write_bit(still_picture);
    w.write_bit(reduced_still_picture_header);
    w.write_bit(timing_info_present);
    w.write_bit(initial_display_delay_present);
    w.write_bits(operating_points_cnt_minus_1, 5);
    w.write_bits(operating_point_idc, 12);
    w.write_bits(seq_level_idx, 5);
    w.write_bit(false);

    let frame_width_bits_minus_1 = bits_needed(width - 1) - 1;
    let frame_height_bits_minus_1 = bits_needed(height - 1) - 1;
    w.write_bits(frame_width_bits_minus_1 as u64, 4);
    w.write_bits(frame_height_bits_minus_1 as u64, 4);
    w.write_bits((width - 1) as u64, frame_width_bits_minus_1 + 1);
    w.write_bits((height - 1) as u64, frame_height_bits_minus_1 + 1);

    let frame_id_numbers_present = false;
    let use_128x128_superblock = false;
    let enable_filter_intra = false;
    let enable_intra_edge_filter = false;
    let enable_interintra_compound = false;
    let enable_masked_compound = false;
    let enable_warped_motion = false;
    let enable_dual_filter = false;
    let enable_order_hint = false;

    w.write_bit(frame_id_numbers_present);
    w.write_bit(use_128x128_superblock);
    w.write_bit(enable_filter_intra);
    w.write_bit(enable_intra_edge_filter);
    w.write_bit(enable_interintra_compound);
    w.write_bit(enable_masked_compound);
    w.write_bit(enable_warped_motion);
    w.write_bit(enable_dual_filter);
    w.write_bit(enable_order_hint);

    let seq_choose_screen_content_tools = false;
    let seq_force_screen_content_tools = false;
    w.write_bit(seq_choose_screen_content_tools);
    w.write_bit(seq_force_screen_content_tools);

    let enable_superres = false;
    let enable_cdef = false;
    let enable_restoration = false;
    w.write_bit(enable_superres);
    w.write_bit(enable_cdef);
    w.write_bit(enable_restoration);

    let high_bitdepth = false;
    let mono_chrome = false;
    let color_description_present = false;
    let color_range = false;
    let chroma_sample_position = 0u64;
    let separate_uv_delta_q = false;
    let film_grain_params_present = false;

    w.write_bit(high_bitdepth);
    w.write_bit(mono_chrome);
    w.write_bit(color_description_present);
    w.write_bit(color_range);
    w.write_bits(chroma_sample_position, 2);
    w.write_bit(separate_uv_delta_q);
    w.write_bit(film_grain_params_present);

    w.write_bit(true);

    w.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn bits_needed_zero() {
        assert_eq!(bits_needed(0), 1);
    }

    #[test]
    fn bits_needed_one() {
        assert_eq!(bits_needed(1), 1);
    }

    #[test]
    fn bits_needed_63() {
        assert_eq!(bits_needed(63), 6);
    }

    #[test]
    fn bits_needed_99() {
        assert_eq!(bits_needed(99), 7);
    }

    #[test]
    fn bits_needed_1919() {
        assert_eq!(bits_needed(1919), 11);
    }

    #[test]
    fn sequence_header_64x64() {
        let bytes = encode_sequence_header(64, 64);

        let mut expected = BitWriter::new();
        expected.write_bits(0, 3);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bits(0, 5);
        expected.write_bits(0, 12);
        expected.write_bits(13, 5);
        expected.write_bit(false);
        expected.write_bits(5, 4);
        expected.write_bits(5, 4);
        expected.write_bits(63, 6);
        expected.write_bits(63, 6);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bits(0, 2);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bit(false);
        expected.write_bits(0, 2);
        expected.write_bit(false);
        expected.write_bit(false);

        expected.write_bit(true);

        assert_eq!(bytes, expected.finalize());
    }

    #[test]
    fn sequence_header_100x100() {
        let bytes = encode_sequence_header(100, 100);
        assert!(bytes.len() >= 8 && bytes.len() <= 12);
    }

    #[test]
    fn sequence_header_320x240() {
        let bytes = encode_sequence_header(320, 240);
        assert!(bytes.len() >= 8 && bytes.len() <= 12);
    }

    #[test]
    fn sequence_header_1920x1080() {
        let bytes = encode_sequence_header(1920, 1080);
        assert!(bytes.len() >= 8 && bytes.len() <= 12);
    }

    #[test]
    fn sequence_header_1x1() {
        let bytes = encode_sequence_header(1, 1);
        assert!(bytes.len() >= 8 && bytes.len() <= 12);
    }

    #[test]
    fn different_dimensions_produce_different_output() {
        let small = encode_sequence_header(64, 64);
        let large = encode_sequence_header(1920, 1080);
        assert_ne!(small, large);
    }

    #[test]
    fn width_bits_vary_with_dimension() {
        let bytes_64 = encode_sequence_header(64, 64);
        let bytes_1920 = encode_sequence_header(1920, 1080);
        assert!(bytes_1920.len() > bytes_64.len());
    }
}
