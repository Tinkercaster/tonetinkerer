//! Small helpers shared by every DSP module.

/// Decibels to linear gain.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db * 0.05)
}

/// Linear gain to decibels, clamped at -200 dB for zero input.
#[inline]
pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.abs().max(1e-10).log10()
}

/// Replace NaN/inf with silence. Used at module boundaries so a single bad
/// sample can never poison recursive filter or feedback state.
#[inline]
pub fn sanitize(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// Sanitize a whole buffer in place. Returns `true` if anything was replaced.
pub fn sanitize_buffer(buf: &mut [f32]) -> bool {
    let mut bad = false;
    for x in buf.iter_mut() {
        if !x.is_finite() {
            *x = 0.0;
            bad = true;
        }
    }
    bad
}

/// Flush values that are close to denormal range to zero. Cheap insurance
/// for recursive state on platforms without flush-to-zero.
#[inline]
pub fn undenormal(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// One-pole exponential smoother operating per sample. Used for every
/// continuously variable parameter so that host automation never zipper-steps.
///
/// The remaining distance to the target is kept as its own state so the
/// decay never stalls on f32 rounding near the target value.
#[derive(Clone, Copy, Debug)]
pub struct Smoothed {
    target: f32,
    diff: f32,
    coeff: f32,
}

impl Smoothed {
    pub fn new(value: f32) -> Self {
        Self {
            target: value,
            diff: 0.0,
            coeff: 0.0,
        }
    }

    /// Set the time constant (time to reach ~63% of a step).
    pub fn set_time(&mut self, sample_rate: f32, millis: f32) {
        let samples = (sample_rate * millis * 0.001).max(1.0);
        self.coeff = (-1.0 / samples).exp();
    }

    #[inline]
    pub fn set_target(&mut self, target: f32) {
        if target != self.target {
            self.diff += self.target - target;
            self.target = target;
        }
    }

    /// Jump straight to `value` (used on reset / state restore).
    pub fn reset(&mut self, value: f32) {
        self.target = value;
        self.diff = 0.0;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        if self.diff != 0.0 {
            self.diff *= self.coeff;
            if self.diff.abs() <= 1e-7 * self.target.abs().max(1.0) {
                self.diff = 0.0;
            }
        }
        self.target + self.diff
    }

    #[inline]
    pub fn value(&self) -> f32 {
        self.target + self.diff
    }

    #[inline]
    pub fn target(&self) -> f32 {
        self.target
    }

    #[inline]
    pub fn is_settled(&self) -> bool {
        self.diff == 0.0
    }
}

/// Linear crossfade used for click-free bypass switching and model swaps.
/// `amount` ramps from 0 (bypassed / old) to 1 (engaged / new).
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    amount: f32,
    target: f32,
    step: f32,
}

impl Fade {
    pub fn new(engaged: bool) -> Self {
        let v = if engaged { 1.0 } else { 0.0 };
        Self {
            amount: v,
            target: v,
            step: 0.001,
        }
    }

    pub fn set_time(&mut self, sample_rate: f32, millis: f32) {
        self.step = 1.0 / (sample_rate * millis * 0.001).max(1.0);
    }

    #[inline]
    pub fn set_engaged(&mut self, engaged: bool) {
        self.target = if engaged { 1.0 } else { 0.0 };
    }

    pub fn reset(&mut self, engaged: bool) {
        self.target = if engaged { 1.0 } else { 0.0 };
        self.amount = self.target;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        if self.amount < self.target {
            self.amount = (self.amount + self.step).min(self.target);
        } else if self.amount > self.target {
            self.amount = (self.amount - self.step).max(self.target);
        }
        self.amount
    }

    /// Fully bypassed and not fading: the module can be skipped entirely.
    #[inline]
    pub fn is_off(&self) -> bool {
        self.amount == 0.0 && self.target == 0.0
    }

    /// Fully engaged and not fading.
    #[inline]
    pub fn is_on(&self) -> bool {
        self.amount == 1.0 && self.target == 1.0
    }

    #[inline]
    pub fn amount(&self) -> f32 {
        self.amount
    }
}

/// First-order DC blocking high-pass filter.
#[derive(Clone, Copy, Debug, Default)]
pub struct DcBlocker {
    r: f32,
    x1: f32,
    y1: f32,
}

impl DcBlocker {
    pub fn new(sample_rate: f32, cutoff_hz: f32) -> Self {
        let r = (-2.0 * std::f32::consts::PI * cutoff_hz / sample_rate).exp();
        Self {
            r,
            x1: 0.0,
            y1: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = undenormal(y);
        self.y1
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_roundtrip() {
        for db in [-60.0, -6.0, 0.0, 6.0, 24.0] {
            assert!((gain_to_db(db_to_gain(db)) - db).abs() < 1e-3);
        }
    }

    #[test]
    fn smoother_converges_exactly() {
        let mut s = Smoothed::new(0.0);
        s.set_time(48_000.0, 10.0);
        s.set_target(1.0);
        for _ in 0..48_000 {
            s.next();
        }
        assert_eq!(s.value(), 1.0);
        assert!(s.is_settled());
    }

    #[test]
    fn fade_reaches_targets() {
        let mut f = Fade::new(false);
        f.set_time(48_000.0, 10.0);
        f.set_engaged(true);
        for _ in 0..480 {
            f.next();
        }
        assert!(f.is_on());
        f.set_engaged(false);
        for _ in 0..480 {
            f.next();
        }
        assert!(f.is_off());
    }

    #[test]
    fn sanitize_replaces_non_finite() {
        let mut b = [1.0, f32::NAN, f32::INFINITY, -f32::INFINITY, 0.5];
        assert!(sanitize_buffer(&mut b));
        assert_eq!(b, [1.0, 0.0, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn dc_blocker_removes_dc() {
        let mut d = DcBlocker::new(48_000.0, 10.0);
        let mut y = 0.0;
        for _ in 0..48_000 {
            y = d.process(1.0);
        }
        assert!(y.abs() < 1e-3);
    }
}
