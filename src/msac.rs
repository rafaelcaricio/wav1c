const EC_PROB_SHIFT: u32 = 6;
const EC_MIN_PROB: u32 = 4;

type EcWindow = u32;

pub struct MsacEncoder {
    low: EcWindow,
    rng: u16,
    cnt: i16,
    precarry: Vec<u16>,
    pub allow_update_cdf: bool,
}

impl MsacEncoder {
    pub fn new() -> Self {
        Self {
            low: 0,
            rng: 0x8000,
            cnt: -9,
            precarry: Vec::new(),
            allow_update_cdf: true,
        }
    }

    fn compute_bounds(&self, fl: u16, fh: u16, nms: u16) -> (EcWindow, u16) {
        let r = self.rng as u32;
        let mut u = (((r >> 8) * ((fl as u32) >> EC_PROB_SHIFT)) >> (7 - EC_PROB_SHIFT))
            + EC_MIN_PROB * nms as u32;
        if fl >= 32768 {
            u = r;
        }
        let v = (((r >> 8) * ((fh as u32) >> EC_PROB_SHIFT)) >> (7 - EC_PROB_SHIFT))
            + EC_MIN_PROB * (nms as u32 - 1);
        ((r - u) as EcWindow, (u - v) as u16)
    }

    fn store(&mut self, fl: u16, fh: u16, nms: u16) {
        let (l, r) = self.compute_bounds(fl, fh, nms);
        let mut low = l + self.low;
        let mut c = self.cnt;
        let d = r.leading_zeros() as i16;
        let mut s = c + d;

        if s >= 0 {
            c += 16;
            let mut m = ((1u32 << c) - 1) as EcWindow;
            if s >= 8 {
                self.precarry.push((low >> c) as u16);
                low &= m;
                c -= 8;
                m >>= 8;
            }
            self.precarry.push((low >> c) as u16);
            s = c + d - 24;
            low &= m;
        }
        self.low = low << d;
        self.rng = r << d;
        self.cnt = s;
    }

    pub fn encode_symbol(&mut self, symbol: u32, cdf: &mut [u16], n_symbols: u32) {
        let ns = n_symbols as usize;
        let s = symbol as usize;
        let nms = (ns + 1 - s) as u16;
        let fl = if s > 0 { cdf[s - 1] } else { 32768 };
        let fh = if s < ns { cdf[s] } else { 0 };
        self.store(fl, fh, nms);

        if self.allow_update_cdf {
            Self::update_cdf(cdf, symbol, n_symbols);
        }
    }

    pub fn encode_bool(&mut self, val: bool, cdf: &mut [u16]) {
        let f = cdf[0];
        let nms = if val { 1u16 } else { 2u16 };
        let fl = if val { f } else { 32768 };
        let fh = if val { 0 } else { f };
        self.store(fl, fh, nms);

        if self.allow_update_cdf {
            let count = cdf[1];
            let rate = 4 + (count >> 4);
            if val {
                cdf[0] += (32768 - cdf[0]) >> rate;
            } else {
                cdf[0] -= cdf[0] >> rate;
            }
            cdf[1] = count + if count < 32 { 1 } else { 0 };
        }
    }

    pub fn encode_bool_prob(&mut self, val: bool, prob: u16) {
        let nms = if val { 1u16 } else { 2u16 };
        let fl = if val { prob } else { 32768 };
        let fh = if val { 0 } else { prob };
        self.store(fl, fh, nms);
    }

    pub fn encode_bool_equi(&mut self, val: bool) {
        let r = self.rng as u32;
        let v = (((r >> 8) << 7) + EC_MIN_PROB) as u16;

        let (l, new_rng): (EcWindow, u16) = if val {
            ((r - v as u32) as EcWindow, v)
        } else {
            (0, r as u16 - v)
        };

        let mut low = l + self.low;
        let mut c = self.cnt;
        let d = new_rng.leading_zeros() as i16;
        let mut s = c + d;

        if s >= 0 {
            c += 16;
            let mut m = ((1u32 << c) - 1) as EcWindow;
            if s >= 8 {
                self.precarry.push((low >> c) as u16);
                low &= m;
                c -= 8;
                m >>= 8;
            }
            self.precarry.push((low >> c) as u16);
            s = c + d - 24;
            low &= m;
        }
        self.low = low << d;
        self.rng = new_rng << d;
        self.cnt = s;
    }

    pub fn encode_golomb(&mut self, val: u32) {
        let x = val + 1;
        let num_bits = 31 - x.leading_zeros();

        for _ in 0..num_bits {
            self.encode_bool_equi(false);
        }
        self.encode_bool_equi(true);

        for i in (0..num_bits).rev() {
            self.encode_bool_equi((x >> i) & 1 == 1);
        }
    }

    pub fn update_cdf(cdf: &mut [u16], symbol: u32, n_symbols: u32) {
        let count = cdf[n_symbols as usize];
        let rate = 4 + (count >> 4) + if n_symbols > 2 { 1 } else { 0 };
        for i in 0..n_symbols {
            if i < symbol {
                cdf[i as usize] += (32768 - cdf[i as usize]) >> rate;
            } else {
                cdf[i as usize] -= cdf[i as usize] >> rate;
            }
        }
        cdf[n_symbols as usize] = count + if count < 32 { 1 } else { 0 };
    }

    pub fn finalize(mut self) -> Vec<u8> {
        let l = self.low;
        let mut c = self.cnt;
        let mut s: i16 = 10;
        let m: EcWindow = 0x3FFF;
        let mut e = ((l + m) & !m) | (m + 1);

        s += c;

        if s > 0 {
            let mut n = ((1u32 << (c + 16)) - 1) as EcWindow;

            loop {
                self.precarry.push((e >> (c + 16)) as u16);
                e &= n;
                s -= 8;
                c -= 8;
                n >>= 8;

                if s <= 0 {
                    break;
                }
            }
        }

        let mut carry: u32 = 0;
        let mut offs = self.precarry.len();
        let mut out = vec![0u8; offs];
        while offs > 0 {
            offs -= 1;
            carry += self.precarry[offs] as u32;
            out[offs] = carry as u8;
            carry >>= 8;
        }

        out
    }
}

impl Default for MsacEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_single_symbol_produces_bytes() {
        let mut enc = MsacEncoder::new();
        let mut cdf = [24576u16, 16384, 0];
        enc.encode_symbol(0, &mut cdf, 2);
        let bytes = enc.finalize();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_multiple_symbols_produces_bytes() {
        let mut enc = MsacEncoder::new();
        let mut cdf = [24576u16, 16384, 8192, 0];
        enc.encode_symbol(0, &mut cdf, 3);
        enc.encode_symbol(1, &mut cdf, 3);
        enc.encode_symbol(2, &mut cdf, 3);
        let bytes = enc.finalize();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn cdf_update_shifts_probability_toward_observed_symbol() {
        let mut cdf = [16384u16, 0];
        MsacEncoder::update_cdf(&mut cdf, 0, 1);
        assert!(cdf[0] < 16384);
    }

    #[test]
    fn cdf_update_counter_increments() {
        let mut cdf = [16384u16, 8192, 0];
        assert_eq!(cdf[2], 0);
        MsacEncoder::update_cdf(&mut cdf, 0, 2);
        assert_eq!(cdf[2], 1);
    }

    #[test]
    fn encode_bool_equi_produces_bytes() {
        let mut enc = MsacEncoder::new();
        for _ in 0..32 {
            enc.encode_bool_equi(true);
        }
        let bytes = enc.finalize();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_bool_equi_different_values_produce_different_output() {
        let mut enc_true = MsacEncoder::new();
        let mut enc_false = MsacEncoder::new();
        enc_true.encode_bool_equi(true);
        enc_false.encode_bool_equi(false);
        let bytes_true = enc_true.finalize();
        let bytes_false = enc_false.finalize();
        assert_ne!(bytes_true, bytes_false);
    }

    #[test]
    fn encode_golomb_zero() {
        let mut enc = MsacEncoder::new();
        enc.encode_golomb(0);
        let bytes = enc.finalize();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_golomb_nonzero() {
        let mut enc = MsacEncoder::new();
        enc.encode_golomb(5);
        let bytes = enc.finalize();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_bool_with_cdf_update() {
        let mut enc = MsacEncoder::new();
        let mut cdf = [16384u16, 0];
        enc.encode_bool(true, &mut cdf);
        assert!(cdf[0] > 16384);
        assert_eq!(cdf[1], 1);
        let bytes = enc.finalize();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_bool_false_with_cdf_update() {
        let mut enc = MsacEncoder::new();
        let mut cdf = [16384u16, 0];
        enc.encode_bool(false, &mut cdf);
        assert!(cdf[0] < 16384);
        assert_eq!(cdf[1], 1);
    }
}
