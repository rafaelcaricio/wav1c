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

    struct Dav1dMsacDecoder {
        dif: u64,
        rng: u32,
        cnt: i32,
        buf: Vec<u8>,
        pos: usize,
        allow_update_cdf: bool,
    }

    impl Dav1dMsacDecoder {
        fn new(data: &[u8], allow_update_cdf: bool) -> Self {
            let mut dec = Self {
                dif: 0,
                rng: 0x8000,
                cnt: -15,
                buf: data.to_vec(),
                pos: 0,
                allow_update_cdf,
            };
            dec.refill();
            dec
        }

        fn refill(&mut self) {
            let mut c = 48 - self.cnt - 24;
            let mut dif = self.dif;
            while c >= 0 {
                let byte = if self.pos < self.buf.len() {
                    let b = self.buf[self.pos];
                    self.pos += 1;
                    b ^ 0xFF
                } else {
                    0xFF
                };
                dif |= (byte as u64) << c;
                c -= 8;
            }
            self.dif = dif;
            self.cnt = 48 - c - 24;
        }

        fn norm(&mut self, dif: u64, rng: u32) {
            let d = rng.leading_zeros() as i32 - 16;
            let cnt = self.cnt;
            self.dif = dif << d;
            self.rng = rng << d;
            self.cnt = cnt - d;
            if (cnt as u32) < (d as u32) {
                self.refill();
            }
        }

        fn decode_symbol_adapt(&mut self, cdf: &mut [u16], n_symbols: usize) -> u32 {
            let c = (self.dif >> 32) as u32;
            let r = self.rng >> 8;
            let mut u;
            let mut v = self.rng;
            let mut val: u32 = u32::MAX;

            loop {
                val = val.wrapping_add(1);
                u = v;
                v = r * ((cdf[val as usize] >> EC_PROB_SHIFT) as u32);
                v >>= 7 - EC_PROB_SHIFT;
                v += EC_MIN_PROB * (n_symbols as u32 - val);
                if c >= v {
                    break;
                }
            }

            self.norm(self.dif - ((v as u64) << 32), u - v);

            if self.allow_update_cdf {
                MsacEncoder::update_cdf(cdf, val, n_symbols as u32);
            }

            val
        }

        fn decode_bool_adapt(&mut self, cdf: &mut [u16]) -> bool {
            let bit = self.decode_bool(cdf[0] as u32);
            if self.allow_update_cdf {
                let count = cdf[1];
                let rate = 4 + (count >> 4);
                if bit {
                    cdf[0] += (32768 - cdf[0]) >> rate;
                } else {
                    cdf[0] -= cdf[0] >> rate;
                }
                cdf[1] = count + if count < 32 { 1 } else { 0 };
            }
            bit
        }

        fn decode_bool(&mut self, f: u32) -> bool {
            let r = self.rng;
            let dif = self.dif;
            let mut v = (((r >> 8) * (f >> EC_PROB_SHIFT)) >> (7 - EC_PROB_SHIFT)) + EC_MIN_PROB;
            let vw = (v as u64) << 32;
            let ret = dif >= vw;
            let new_dif = if ret { dif - vw } else { dif };
            if ret {
                v = v.wrapping_add(r.wrapping_sub(2u32.wrapping_mul(v)));
            }
            self.norm(new_dif, v);
            !ret
        }

        fn decode_bool_equi(&mut self) -> bool {
            let r = self.rng;
            let dif = self.dif;
            let mut v = ((r >> 8) << 7) + EC_MIN_PROB;
            let vw = (v as u64) << 32;
            let ret = dif >= vw;
            let new_dif = if ret { dif - vw } else { dif };
            if ret {
                v = v.wrapping_add(r.wrapping_sub(2u32.wrapping_mul(v)));
            }
            self.norm(new_dif, v);
            !ret
        }

        fn decode_golomb(&mut self) -> u32 {
            let mut len = 0u32;
            while len < 32 && !self.decode_bool_equi() {
                len += 1;
            }
            let mut val = 1u32 << len;
            for i in (0..len).rev() {
                if self.decode_bool_equi() {
                    val += 1 << i;
                }
            }
            val - 1
        }
    }

    #[test]
    fn msac_roundtrip_single_symbol() {
        for symbol in 0..3u32 {
            let mut enc = MsacEncoder::new();
            let mut cdf_enc = [24576u16, 16384, 8192, 0];
            enc.encode_symbol(symbol, &mut cdf_enc, 3);
            let bytes = enc.finalize();

            let mut dec = Dav1dMsacDecoder::new(&bytes, true);
            let mut cdf_dec = [24576u16, 16384, 8192, 0];
            let decoded = dec.decode_symbol_adapt(&mut cdf_dec, 3);
            assert_eq!(decoded, symbol, "Symbol mismatch for symbol={symbol}");
            assert_eq!(cdf_enc, cdf_dec, "CDF mismatch after symbol={symbol}");
        }
    }

    #[test]
    fn msac_roundtrip_many_symbols() {
        let symbols = [0u32, 1, 2, 0, 0, 1, 2, 1, 0, 2, 2, 1, 0, 0, 0, 1, 2, 2, 1, 0];
        let mut enc = MsacEncoder::new();
        let mut cdf_enc = [24576u16, 16384, 8192, 0];
        for &s in &symbols {
            enc.encode_symbol(s, &mut cdf_enc, 3);
        }
        let bytes = enc.finalize();

        let mut dec = Dav1dMsacDecoder::new(&bytes, true);
        let mut cdf_dec = [24576u16, 16384, 8192, 0];
        for (i, &expected) in symbols.iter().enumerate() {
            let decoded = dec.decode_symbol_adapt(&mut cdf_dec, 3);
            assert_eq!(decoded, expected, "Symbol mismatch at index {i}: expected={expected} got={decoded}");
        }
        assert_eq!(cdf_enc, cdf_dec, "CDF mismatch after all symbols");
    }

    #[test]
    fn msac_roundtrip_bool_adapt() {
        let values = [true, false, true, true, false, false, true, false];
        let mut enc = MsacEncoder::new();
        let mut cdf_enc = [16384u16, 0];
        for &v in &values {
            enc.encode_bool(v, &mut cdf_enc);
        }
        let bytes = enc.finalize();

        let mut dec = Dav1dMsacDecoder::new(&bytes, true);
        let mut cdf_dec = [16384u16, 0];
        for (i, &expected) in values.iter().enumerate() {
            let decoded = dec.decode_bool_adapt(&mut cdf_dec);
            assert_eq!(decoded, expected, "Bool mismatch at index {i}");
        }
        assert_eq!(cdf_enc, cdf_dec, "CDF mismatch after all bools");
    }

    #[test]
    fn msac_roundtrip_bool_equi() {
        let values = [true, false, true, true, false, false, true, false, true, true];
        let mut enc = MsacEncoder::new();
        for &v in &values {
            enc.encode_bool_equi(v);
        }
        let bytes = enc.finalize();

        let mut dec = Dav1dMsacDecoder::new(&bytes, true);
        for (i, &expected) in values.iter().enumerate() {
            let decoded = dec.decode_bool_equi();
            assert_eq!(decoded, expected, "Bool equi mismatch at index {i}");
        }
    }

    #[test]
    fn msac_roundtrip_golomb() {
        let values = [0u32, 1, 5, 15, 100, 0, 3, 7];
        let mut enc = MsacEncoder::new();
        for &v in &values {
            enc.encode_golomb(v);
        }
        let bytes = enc.finalize();

        let mut dec = Dav1dMsacDecoder::new(&bytes, true);
        for (i, &expected) in values.iter().enumerate() {
            let decoded = dec.decode_golomb();
            assert_eq!(decoded, expected, "Golomb mismatch at index {i}: expected={expected} got={decoded}");
        }
    }

    #[test]
    fn msac_roundtrip_mixed_operations() {
        let mut enc = MsacEncoder::new();
        let mut cdf3_enc = [24576u16, 16384, 8192, 0];
        let mut cdf_bool_enc = [16384u16, 0];

        enc.encode_bool(false, &mut cdf_bool_enc);
        enc.encode_symbol(1, &mut cdf3_enc, 3);
        enc.encode_bool_equi(true);
        enc.encode_symbol(0, &mut cdf3_enc, 3);
        enc.encode_bool(true, &mut cdf_bool_enc);
        enc.encode_golomb(7);
        enc.encode_symbol(2, &mut cdf3_enc, 3);
        enc.encode_bool_equi(false);
        enc.encode_golomb(0);
        enc.encode_bool(false, &mut cdf_bool_enc);
        let bytes = enc.finalize();

        let mut dec = Dav1dMsacDecoder::new(&bytes, true);
        let mut cdf3_dec = [24576u16, 16384, 8192, 0];
        let mut cdf_bool_dec = [16384u16, 0];

        assert!(!dec.decode_bool_adapt(&mut cdf_bool_dec));
        assert_eq!(dec.decode_symbol_adapt(&mut cdf3_dec, 3), 1);
        assert!(dec.decode_bool_equi());
        assert_eq!(dec.decode_symbol_adapt(&mut cdf3_dec, 3), 0);
        assert!(dec.decode_bool_adapt(&mut cdf_bool_dec));
        assert_eq!(dec.decode_golomb(), 7);
        assert_eq!(dec.decode_symbol_adapt(&mut cdf3_dec, 3), 2);
        assert!(!dec.decode_bool_equi());
        assert_eq!(dec.decode_golomb(), 0);
        assert!(!dec.decode_bool_adapt(&mut cdf_bool_dec));

        assert_eq!(cdf3_enc, cdf3_dec, "CDF3 mismatch");
        assert_eq!(cdf_bool_enc, cdf_bool_dec, "CDF bool mismatch");
    }
}
