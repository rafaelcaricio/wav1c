#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FpsError {
    ZeroNum,
    ZeroDen,
}

impl std::fmt::Display for FpsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FpsError::ZeroNum => write!(f, "fps num must be > 0"),
            FpsError::ZeroDen => write!(f, "fps den must be > 0"),
        }
    }
}

impl std::error::Error for FpsError {}

impl Fps {
    pub fn new(num: u32, den: u32) -> Result<Self, FpsError> {
        if num == 0 {
            return Err(FpsError::ZeroNum);
        }
        if den == 0 {
            return Err(FpsError::ZeroDen);
        }
        let g = gcd(num, den);
        Ok(Self {
            num: num / g,
            den: den / g,
        })
    }

    pub fn from_int(fps: u32) -> Result<Self, FpsError> {
        Self::new(fps, 1)
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

impl Default for Fps {
    fn default() -> Self {
        Self { num: 25, den: 1 }
    }
}

const fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    if a == 0 { 1 } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_valid_num_den() {
        let fps = Fps::new(30, 1).unwrap();
        assert_eq!(fps.num, 30);
        assert_eq!(fps.den, 1);
    }

    #[test]
    fn new_rejects_zero_num() {
        let err = Fps::new(0, 1).unwrap_err();
        assert_eq!(err, FpsError::ZeroNum);
    }

    #[test]
    fn new_rejects_zero_den() {
        let err = Fps::new(1, 0).unwrap_err();
        assert_eq!(err, FpsError::ZeroDen);
    }

    #[test]
    fn new_normalizes_ratio() {
        let fps = Fps::new(60, 2).unwrap();
        assert_eq!(fps, Fps { num: 30, den: 1 });
    }

    #[test]
    fn keeps_non_reducible_ratio() {
        let fps = Fps::new(30_000, 1_001).unwrap();
        assert_eq!(
            fps,
            Fps {
                num: 30_000,
                den: 1_001
            }
        );
    }
}
