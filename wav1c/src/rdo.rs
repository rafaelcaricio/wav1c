use crate::tile::dct::TxType;

/// Converts AV1 base_q_idx to a lambda multiplier for RDO cost calculation
#[inline]
pub fn calculate_lambda(base_q_idx: u8) -> u32 {
    let q = base_q_idx as u32;
    // A heuristic lambda mapping approximation.
    // In actual AV1 encoders, lambda is derived directly from the
    // quantizer scale tables mapping q_idx to AC scale.
    // We approximate it simply:
    let q2 = q * q;
    // Lower lambda encourages more bits/splits, which improves VMAF quality.
    // SATD is L1 norm, while standard RDO is L2 norm. So lambda must be scaled down.
    let lambda = 1.max(q2 >> 8);
    lambda
}

/// Computes the full RDO cost metric J = D + lambda * R
#[inline]
pub fn calculate_rd_cost(distortion: u32, bits: u32, lambda: u32) -> u64 {
    (distortion as u64) + (lambda as u64) * (bits as u64)
}

/// A very rough heuristic of how many bits signaling an intra mode takes
/// In reality, this depends on the context and MSAC probabilities.
pub fn estimate_intra_mode_bits(mode: u8) -> u32 {
    match mode {
        0 => 8,  // DC_PRED (often most common)
        1 => 12, // V_PRED
        2 => 12, // H_PRED
        _ => 20, // complex directional/smooth/paeth modes
    }
}

/// Estimates the bit cost of signaling a specific TxType
pub fn estimate_tx_type_bits(tx_type: TxType) -> u32 {
    match tx_type {
        TxType::DctDct => 4, // Most common, cheapest
        TxType::Idtx => 12,  // Identity transform
        _ => 16,             // Other 1D/2D transforms
    }
}

/// Estimates the bit cost of signaling a partition split vs none
pub fn estimate_partition_bits(is_split: bool) -> u32 {
    if is_split { 12 } else { 4 }
}
