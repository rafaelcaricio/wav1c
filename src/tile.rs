use crate::cdf::CdfContext;
use crate::msac::MsacEncoder;

pub fn encode_tile(y: u8, u: u8, v: u8, base_q_idx: u8) -> Vec<u8> {
    let mut cdf = CdfContext::new(base_q_idx);
    let mut enc = MsacEncoder::new();

    let y_residual = y as i16 - 128;
    let u_residual = u as i16 - 128;
    let v_residual = v as i16 - 128;
    let is_skip = y_residual == 0 && u_residual == 0 && v_residual == 0;

    enc.encode_symbol(0, &mut cdf.partition[1][0], 10);

    enc.encode_bool(is_skip, &mut cdf.skip[0]);

    enc.encode_symbol(0, &mut cdf.kf_y_mode[0][0], 12);

    enc.encode_symbol(0, &mut cdf.uv_mode[0][0], 12);

    if !is_skip {
        todo!("Non-zero residuals not yet supported");
    }

    enc.finalize()
}
