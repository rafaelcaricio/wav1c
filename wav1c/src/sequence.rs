use crate::bitwriter::BitWriter;
use crate::fps::Fps;
use crate::video::{BitDepth, ColorRange, VideoSignal};

pub const SEQ_LEVEL_IDX_5_1: u8 = 13;
pub const SEQ_LEVEL_IDX_MAX_PARAMETERS: u8 = 31;

#[derive(Clone, Copy)]
struct LevelConstraint {
    seq_level_idx: u8,
    max_pic_size: u64,
    max_h_size: u32,
    max_v_size: u32,
    max_display_rate: u64,
    max_decode_rate: u64,
}

const LEVEL_CONSTRAINTS: [LevelConstraint; 7] = [
    LevelConstraint {
        seq_level_idx: 13, // 5.1
        max_pic_size: 8_912_896,
        max_h_size: 8_192,
        max_v_size: 4_352,
        max_display_rate: 534_773_760,
        max_decode_rate: 547_430_400,
    },
    LevelConstraint {
        seq_level_idx: 14, // 5.2
        max_pic_size: 8_912_896,
        max_h_size: 8_192,
        max_v_size: 4_352,
        max_display_rate: 1_069_547_520,
        max_decode_rate: 1_094_860_800,
    },
    LevelConstraint {
        seq_level_idx: 15, // 5.3
        max_pic_size: 8_912_896,
        max_h_size: 8_192,
        max_v_size: 4_352,
        max_display_rate: 1_069_547_520,
        max_decode_rate: 1_176_502_272,
    },
    LevelConstraint {
        seq_level_idx: 16, // 6.0
        max_pic_size: 35_651_584,
        max_h_size: 16_384,
        max_v_size: 8_704,
        max_display_rate: 1_069_547_520,
        max_decode_rate: 1_176_502_272,
    },
    LevelConstraint {
        seq_level_idx: 17, // 6.1
        max_pic_size: 35_651_584,
        max_h_size: 16_384,
        max_v_size: 8_704,
        max_display_rate: 2_139_095_040,
        max_decode_rate: 2_189_721_600,
    },
    LevelConstraint {
        seq_level_idx: 18, // 6.2
        max_pic_size: 35_651_584,
        max_h_size: 16_384,
        max_v_size: 8_704,
        max_display_rate: 4_278_190_080,
        max_decode_rate: 4_379_443_200,
    },
    LevelConstraint {
        seq_level_idx: 19, // 6.3
        max_pic_size: 35_651_584,
        max_h_size: 16_384,
        max_v_size: 8_704,
        max_display_rate: 4_278_190_080,
        max_decode_rate: 4_706_009_088,
    },
];

fn bits_needed(v: u32) -> u8 {
    if v == 0 {
        1
    } else {
        32 - v.leading_zeros() as u8
    }
}

pub fn derive_sequence_level_idx(width: u32, height: u32, fps: Fps) -> u8 {
    let pic_size = width as u64 * height as u64;
    let display_rate_num = pic_size as u128 * fps.num as u128;
    let display_rate_den = fps.den as u128;

    for level in LEVEL_CONSTRAINTS {
        let max_display_rate = level.max_display_rate as u128;
        let max_decode_rate = level.max_decode_rate as u128;
        if width <= level.max_h_size
            && height <= level.max_v_size
            && pic_size <= level.max_pic_size
            && display_rate_num <= max_display_rate * display_rate_den
            && display_rate_num <= max_decode_rate * display_rate_den
        {
            return level.seq_level_idx.max(SEQ_LEVEL_IDX_5_1);
        }
    }

    SEQ_LEVEL_IDX_MAX_PARAMETERS
}

pub fn encode_sequence_header(width: u32, height: u32, signal: &VideoSignal) -> Vec<u8> {
    let seq_level_idx = derive_sequence_level_idx(width, height, Fps::default());
    encode_sequence_header_with_level(width, height, signal, seq_level_idx)
}

pub fn encode_still_picture_sequence_header(
    width: u32,
    height: u32,
    signal: &VideoSignal,
) -> Vec<u8> {
    let seq_level_idx = derive_sequence_level_idx(width, height, Fps::default());
    encode_still_picture_sequence_header_with_level(width, height, signal, seq_level_idx)
}

pub fn encode_sequence_header_with_level(
    width: u32,
    height: u32,
    signal: &VideoSignal,
    seq_level_idx: u8,
) -> Vec<u8> {
    encode_sequence_header_with_level_impl(width, height, signal, seq_level_idx, false)
}

pub fn encode_still_picture_sequence_header_with_level(
    width: u32,
    height: u32,
    signal: &VideoSignal,
    seq_level_idx: u8,
) -> Vec<u8> {
    encode_sequence_header_with_level_impl(width, height, signal, seq_level_idx, true)
}

fn encode_sequence_header_with_level_impl(
    width: u32,
    height: u32,
    signal: &VideoSignal,
    seq_level_idx: u8,
    still_picture_mode: bool,
) -> Vec<u8> {
    let mut w = BitWriter::new();

    let seq_profile = 0u64;
    let still_picture = still_picture_mode;
    let reduced_still_picture_header = false;
    let timing_info_present = false;
    let initial_display_delay_present = false;
    let operating_points_cnt_minus_1 = 0u64;
    let operating_point_idc = 0u64;

    w.write_bits(seq_profile, 3);
    w.write_bit(still_picture);
    w.write_bit(reduced_still_picture_header);
    if reduced_still_picture_header {
        w.write_bits(seq_level_idx as u64, 5);
    } else {
        w.write_bit(timing_info_present);
        w.write_bit(initial_display_delay_present);
        w.write_bits(operating_points_cnt_minus_1, 5);
        w.write_bits(operating_point_idc, 12);
        w.write_bits(seq_level_idx as u64, 5);
        if seq_level_idx > 7 {
            w.write_bit(false);
        }
    }

    let frame_width_bits_minus_1 = bits_needed(width - 1) - 1;
    let frame_height_bits_minus_1 = bits_needed(height - 1) - 1;
    w.write_bits(frame_width_bits_minus_1 as u64, 4);
    w.write_bits(frame_height_bits_minus_1 as u64, 4);
    w.write_bits((width - 1) as u64, frame_width_bits_minus_1 + 1);
    w.write_bits((height - 1) as u64, frame_height_bits_minus_1 + 1);

    let use_128x128_superblock = false;
    let enable_filter_intra = false;
    let enable_intra_edge_filter = false;
    let enable_superres = false;
    let enable_cdef = true;
    let enable_restoration = false;
    if reduced_still_picture_header {
        w.write_bit(use_128x128_superblock);
        w.write_bit(enable_filter_intra);
        w.write_bit(enable_intra_edge_filter);
        w.write_bit(enable_superres);
        w.write_bit(enable_cdef);
        w.write_bit(enable_restoration);
    } else {
        let frame_id_numbers_present = false;
        let enable_interintra_compound = false;
        let enable_masked_compound = false;
        let enable_warped_motion = false;
        let enable_dual_filter = false;
        let enable_order_hint = false;
        let seq_choose_screen_content_tools = false;
        let seq_force_screen_content_tools = false;

        w.write_bit(frame_id_numbers_present);
        w.write_bit(use_128x128_superblock);
        w.write_bit(enable_filter_intra);
        w.write_bit(enable_intra_edge_filter);
        w.write_bit(enable_interintra_compound);
        w.write_bit(enable_masked_compound);
        w.write_bit(enable_warped_motion);
        w.write_bit(enable_dual_filter);
        w.write_bit(enable_order_hint);
        w.write_bit(seq_choose_screen_content_tools);
        w.write_bit(seq_force_screen_content_tools);
        w.write_bit(enable_superres);
        w.write_bit(enable_cdef);
        w.write_bit(enable_restoration);
    }

    let high_bitdepth = signal.bit_depth == BitDepth::Ten;
    let mono_chrome = false;
    let color_description_present = signal.color_description.is_some();
    let color_range = signal.color_range == ColorRange::Full;
    let chroma_sample_position = 0u64;
    let separate_uv_delta_q = false;
    let film_grain_params_present = false;

    w.write_bit(high_bitdepth);
    w.write_bit(mono_chrome);
    w.write_bit(color_description_present);
    if let Some(desc) = signal.color_description {
        w.write_bits(desc.color_primaries as u64, 8);
        w.write_bits(desc.transfer_characteristics as u64, 8);
        w.write_bits(desc.matrix_coefficients as u64, 8);
    }
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
        let bytes = encode_sequence_header(64, 64, &VideoSignal::default());

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
        expected.write_bit(true);
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
        let bytes = encode_sequence_header(100, 100, &VideoSignal::default());
        assert!(bytes.len() >= 8 && bytes.len() <= 12);
    }

    #[test]
    fn sequence_header_320x240() {
        let bytes = encode_sequence_header(320, 240, &VideoSignal::default());
        assert!(bytes.len() >= 8 && bytes.len() <= 12);
    }

    #[test]
    fn sequence_header_1920x1080() {
        let bytes = encode_sequence_header(1920, 1080, &VideoSignal::default());
        assert!(bytes.len() >= 8 && bytes.len() <= 12);
    }

    #[test]
    fn sequence_header_1x1() {
        let bytes = encode_sequence_header(1, 1, &VideoSignal::default());
        assert!(bytes.len() >= 8 && bytes.len() <= 12);
    }

    #[test]
    fn different_dimensions_produce_different_output() {
        let small = encode_sequence_header(64, 64, &VideoSignal::default());
        let large = encode_sequence_header(1920, 1080, &VideoSignal::default());
        assert_ne!(small, large);
    }

    #[test]
    fn width_bits_vary_with_dimension() {
        let bytes_64 = encode_sequence_header(64, 64, &VideoSignal::default());
        let bytes_1920 = encode_sequence_header(1920, 1080, &VideoSignal::default());
        assert!(bytes_1920.len() > bytes_64.len());
    }

    #[test]
    fn hdr10_signal_changes_payload() {
        let sdr = encode_sequence_header(320, 240, &VideoSignal::default());
        let hdr = encode_sequence_header(320, 240, &VideoSignal::hdr10(ColorRange::Limited));
        assert_ne!(sdr, hdr);
        assert!(hdr.len() > sdr.len());
    }

    #[test]
    fn still_picture_header_sets_still_flag_without_reduced_header() {
        let seq_level_idx = derive_sequence_level_idx(64, 64, Fps::default());
        let still = encode_still_picture_sequence_header_with_level(
            64,
            64,
            &VideoSignal::default(),
            seq_level_idx,
        );
        assert_eq!(still[0] & 0b0001_0000, 0b0001_0000);
        assert_eq!(still[0] & 0b0000_1000, 0);
    }

    #[test]
    fn still_picture_header_differs_from_regular_header() {
        let regular = encode_sequence_header(64, 64, &VideoSignal::default());
        let still = encode_still_picture_sequence_header(64, 64, &VideoSignal::default());
        assert_ne!(regular, still);
    }

    #[test]
    fn derive_level_small_frames_floor_to_5_1() {
        let level = derive_sequence_level_idx(320, 240, Fps::default());
        assert_eq!(level, SEQ_LEVEL_IDX_5_1);
    }

    #[test]
    fn derive_level_large_frame_selects_higher_level() {
        let level = derive_sequence_level_idx(4284, 5712, Fps::default());
        assert!(level > SEQ_LEVEL_IDX_5_1);
    }

    #[test]
    fn derive_level_out_of_table_falls_back_to_max_parameters() {
        let level = derive_sequence_level_idx(20_000, 20_000, Fps::from_int(30).unwrap());
        assert_eq!(level, SEQ_LEVEL_IDX_MAX_PARAMETERS);
    }
}
