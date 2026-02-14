pub struct RateControl {
    target_bitrate: u64,
    buffer_size: f64,
    buffer_fullness: f64,
    target_bits_per_frame: f64,
    avg_frame_bits: f64,
    avg_qp: f64,
    frames_encoded: u64,
    keyint: usize,
    keyframe_boost: f64,
}

fn initial_qp_from_bitrate(target_bitrate: u64, fps: f64, width: u32, height: u32) -> u8 {
    let bpp = target_bitrate as f64 / (fps * width as f64 * height as f64);
    if bpp > 1.0 {
        40
    } else if bpp > 0.5 {
        80
    } else if bpp > 0.2 {
        120
    } else if bpp > 0.1 {
        160
    } else if bpp > 0.05 {
        200
    } else {
        230
    }
}

impl RateControl {
    pub fn new(
        target_bitrate: u64,
        fps: f64,
        width: u32,
        height: u32,
        keyint: usize,
    ) -> Self {
        let initial_qp = initial_qp_from_bitrate(target_bitrate, fps, width, height);
        let target_bits_per_frame = target_bitrate as f64 / fps;
        let buffer_size = target_bitrate as f64;

        Self {
            target_bitrate,
            buffer_size,
            buffer_fullness: buffer_size / 2.0,
            target_bits_per_frame,
            avg_frame_bits: target_bits_per_frame,
            avg_qp: initial_qp as f64,
            frames_encoded: 0,
            keyint,
            keyframe_boost: 4.0,
        }
    }

    fn target_bits_for_frame(&self, is_keyframe: bool) -> f64 {
        let base = self.target_bits_per_frame;
        if is_keyframe {
            let boosted = base * self.keyframe_boost;
            boosted.min(self.buffer_size * 0.5)
        } else {
            let overspend = base * (self.keyframe_boost - 1.0);
            let reduction = overspend / (self.keyint as f64 - 1.0).max(1.0);
            (base - reduction).max(base * 0.3)
        }
    }

    pub fn compute_qp(&mut self, is_keyframe: bool) -> u8 {
        if self.frames_encoded == 0 {
            let qp = self.avg_qp as u8;
            return if is_keyframe {
                (qp as i32 - 15).clamp(1, 255) as u8
            } else {
                qp
            };
        }

        let target_bits = self.target_bits_for_frame(is_keyframe);

        let buffer_target = self.buffer_size / 2.0;
        let buffer_error = ((self.buffer_fullness - buffer_target) / buffer_target).clamp(-1.0, 1.0);

        let rate_error = if self.avg_frame_bits > 0.0 {
            ((self.avg_frame_bits - target_bits) / target_bits).clamp(-1.0, 1.0)
        } else {
            0.0
        };

        let combined = 0.6 * buffer_error + 0.4 * rate_error;
        let qp_delta = combined * 30.0;

        let mut new_qp = self.avg_qp + qp_delta;
        new_qp = new_qp.clamp(self.avg_qp - 10.0, self.avg_qp + 10.0);

        if is_keyframe {
            new_qp -= 15.0;
        }

        (new_qp.round() as i32).clamp(1, 255) as u8
    }

    pub fn update(&mut self, actual_bits: u64, qp_used: u8) {
        self.buffer_fullness += actual_bits as f64;
        self.buffer_fullness -= self.target_bits_per_frame;
        self.buffer_fullness = self.buffer_fullness.clamp(0.0, self.buffer_size);

        let alpha = 0.2;
        self.avg_frame_bits = alpha * actual_bits as f64 + (1.0 - alpha) * self.avg_frame_bits;
        self.avg_qp = alpha * qp_used as f64 + (1.0 - alpha) * self.avg_qp;

        self.frames_encoded += 1;
    }

    pub fn stats(&self) -> RateControlStats {
        RateControlStats {
            target_bitrate: self.target_bitrate,
            frames_encoded: self.frames_encoded,
            buffer_fullness_pct: (self.buffer_fullness / self.buffer_size * 100.0) as u32,
            avg_qp: self.avg_qp.round() as u8,
        }
    }
}

pub struct RateControlStats {
    pub target_bitrate: u64,
    pub frames_encoded: u64,
    pub buffer_fullness_pct: u32,
    pub avg_qp: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_qp_high_bitrate() {
        assert!(initial_qp_from_bitrate(10_000_000, 25.0, 320, 240) <= 80);
    }

    #[test]
    fn initial_qp_low_bitrate() {
        assert!(initial_qp_from_bitrate(50_000, 25.0, 640, 480) >= 200);
    }

    #[test]
    fn first_frame_uses_initial_qp() {
        let mut rc = RateControl::new(500_000, 25.0, 320, 240, 25);
        let qp = rc.compute_qp(true);
        assert!(qp > 0 && qp < 255);
    }

    #[test]
    fn qp_increases_when_over_budget() {
        let mut rc = RateControl::new(500_000, 25.0, 320, 240, 25);
        let initial_qp = rc.compute_qp(true);
        rc.update(100_000, initial_qp);

        let target = rc.target_bits_per_frame as u64;
        for _ in 0..5 {
            let qp = rc.compute_qp(false);
            rc.update(target * 3, qp);
        }
        let qp_after = rc.compute_qp(false);
        assert!(qp_after > initial_qp);
    }

    #[test]
    fn qp_decreases_when_under_budget() {
        let mut rc = RateControl::new(500_000, 25.0, 320, 240, 25);
        let initial_qp = rc.compute_qp(true);
        rc.update(1000, initial_qp);

        for _ in 0..5 {
            let qp = rc.compute_qp(false);
            rc.update(100, qp);
        }
        let qp_after = rc.compute_qp(false);
        assert!(qp_after < initial_qp);
    }

    #[test]
    fn keyframe_gets_lower_qp() {
        let mut rc = RateControl::new(500_000, 25.0, 320, 240, 25);
        rc.compute_qp(true);
        rc.update(20_000, 120);

        let inter_qp = rc.compute_qp(false);
        rc.update(20_000, inter_qp);

        let key_qp = rc.compute_qp(true);
        assert!(key_qp < inter_qp);
    }

    #[test]
    fn buffer_stays_in_range() {
        let mut rc = RateControl::new(500_000, 25.0, 320, 240, 25);
        for i in 0..100 {
            let is_key = i % 25 == 0;
            let qp = rc.compute_qp(is_key);
            let bits = if is_key { 80_000 } else { 15_000 };
            rc.update(bits, qp);
            let stats = rc.stats();
            assert!(stats.buffer_fullness_pct <= 100);
        }
    }
}
