use crate::bitwriter::BitWriter;
use crate::obu;
use crate::video::{ContentLightLevel, MasteringDisplayMetadata};

const OBU_META_HDR_CLL: u64 = 1;
const OBU_META_HDR_MDCV: u64 = 2;

pub fn encode_hdr_cll(cll: &ContentLightLevel) -> Vec<u8> {
    let mut payload = obu::leb128_encode(OBU_META_HDR_CLL);
    let mut w = BitWriter::new();
    w.write_bits(cll.max_content_light_level as u64, 16);
    w.write_bits(cll.max_frame_average_light_level as u64, 16);
    payload.extend_from_slice(&w.trailing_bits());
    payload
}

pub fn encode_hdr_mdcv(mdcv: &MasteringDisplayMetadata) -> Vec<u8> {
    let mut payload = obu::leb128_encode(OBU_META_HDR_MDCV);
    let mut w = BitWriter::new();

    for p in mdcv.primaries {
        w.write_bits(p[0] as u64, 16);
        w.write_bits(p[1] as u64, 16);
    }
    w.write_bits(mdcv.white_point[0] as u64, 16);
    w.write_bits(mdcv.white_point[1] as u64, 16);
    w.write_bits(mdcv.max_luminance as u64, 32);
    w.write_bits(mdcv.min_luminance as u64, 32);

    payload.extend_from_slice(&w.trailing_bits());
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cll_payload_shape() {
        let payload = encode_hdr_cll(&ContentLightLevel {
            max_content_light_level: 1000,
            max_frame_average_light_level: 400,
        });
        assert_eq!(payload[0], 1);
        assert_eq!(payload.len(), 1 + 4 + 1);
    }

    #[test]
    fn mdcv_payload_shape() {
        let payload = encode_hdr_mdcv(&MasteringDisplayMetadata {
            primaries: [[34000, 16000], [13250, 34500], [7500, 3000]],
            white_point: [15635, 16450],
            max_luminance: 10000000,
            min_luminance: 1,
        });
        assert_eq!(payload[0], 2);
        assert_eq!(payload.len(), 1 + 24 + 1);
    }
}
