//! Reference tube-amplifier circuit model used ONLY to generate training
//! targets for the bundled NAM models (an offline "virtual capture").
//!
//! It is never part of the plugin signal path: the plugin's amplifier stage
//! is NAM inference. This exists so the bundled `.nam` files are 100% our own
//! work with no third-party capture licensing questions. Users can load any
//! NAM capture of a real amplifier instead.
//!
//! Topology per voice: coupling high-pass -> cascaded asymmetric triode
//! stages with interstage filtering (bright caps, Miller roll-off, cathode
//! bypass emphasis) -> passive-style tone shaping -> push-pull power stage
//! with supply sag -> presence / resonance (negative-feedback emulation).
//! Everything nonlinear runs at 4x oversampling.

use tonetinkerer_dsp::biquad::{Biquad, Coeffs};
use tonetinkerer_dsp::oversample::{FACTOR, LATENCY, Oversampler4x};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice {
    Clean,
    Crunch,
    Lead,
    Metal,
}

impl Voice {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "clean" => Some(Voice::Clean),
            "crunch" => Some(Voice::Crunch),
            "lead" => Some(Voice::Lead),
            "metal" => Some(Voice::Metal),
            _ => None,
        }
    }
}

struct Stage {
    pre: Vec<Biquad>,
    gain: f32,
    bias: f32,
    post: Vec<Biquad>,
}

impl Stage {
    #[inline]
    fn process(&mut self, mut x: f32) -> f32 {
        for f in &mut self.pre {
            x = f.process(x);
        }
        // Phase-inverting common-cathode stage.
        x = -triode(x * self.gain, self.bias);
        for f in &mut self.post {
            x = f.process(x);
        }
        x
    }
}

/// Asymmetric soft saturation: soft grid-conduction limiting on the positive
/// swing, earlier and firmer cutoff on the negative swing. C1-continuous.
#[inline]
fn shaper(v: f32) -> f32 {
    if v >= 0.0 {
        v.tanh()
    } else {
        -(1.0 - (1.4 * v).exp()) / 1.4
    }
}

#[inline]
fn triode(x: f32, bias: f32) -> f32 {
    shaper(x + bias) - shaper(bias)
}

pub struct RefAmp {
    os: Oversampler4x,
    stages: Vec<Stage>,
    tone: Vec<Biquad>,
    power_gain: f32,
    sag_amount: f32,
    sag_env: f32,
    sag_att: f32,
    sag_rel: f32,
    post: Vec<Biquad>,
    out_gain: f32,
}

impl RefAmp {
    pub fn new(voice: Voice, sample_rate: f32) -> Self {
        let sr = sample_rate * FACTOR as f32;
        let q = std::f32::consts::FRAC_1_SQRT_2;
        let hp = |f: f32| Biquad::new(Coeffs::highpass(sr, f, q));
        let lp = |f: f32| Biquad::new(Coeffs::lowpass(sr, f, q));
        let pk = |f: f32, qq: f32, g: f32| Biquad::new(Coeffs::peaking(sr, f, qq, g));
        let ls = |f: f32, g: f32| Biquad::new(Coeffs::low_shelf(sr, f, q, g));
        let hs = |f: f32, g: f32| Biquad::new(Coeffs::high_shelf(sr, f, q, g));

        let (stages, tone, power_gain, sag_amount, post, out_gain) = match voice {
            Voice::Clean => (
                vec![
                    Stage {
                        pre: vec![hp(30.0), hs(2_500.0, 3.0)],
                        gain: 3.0,
                        bias: 0.15,
                        post: vec![lp(12_000.0)],
                    },
                    Stage {
                        pre: vec![
                            ls(120.0, 1.0),
                            pk(500.0, 0.7, -5.0),
                            hs(2_500.0, 2.0),
                            hp(40.0),
                        ],
                        gain: 2.2,
                        bias: 0.1,
                        post: vec![lp(11_000.0)],
                    },
                ],
                vec![],
                1.1,
                0.25,
                vec![hs(4_000.0, 2.0), pk(100.0, 1.0, 1.5)],
                0.9,
            ),
            Voice::Crunch => (
                vec![
                    Stage {
                        pre: vec![hp(60.0), hs(2_000.0, 4.0)],
                        gain: 6.0,
                        bias: 0.2,
                        post: vec![lp(10_000.0)],
                    },
                    Stage {
                        pre: vec![hp(90.0)],
                        gain: 4.5,
                        bias: 0.25,
                        post: vec![lp(8_500.0)],
                    },
                ],
                vec![pk(700.0, 0.8, 2.5), ls(120.0, -1.0), hs(3_000.0, 1.0)],
                1.8,
                0.45,
                vec![hs(3_500.0, 1.5), pk(110.0, 1.0, 2.0)],
                0.7,
            ),
            Voice::Lead => (
                vec![
                    Stage {
                        pre: vec![hp(70.0), hs(1_500.0, 3.0)],
                        gain: 10.0,
                        bias: 0.2,
                        post: vec![lp(11_000.0)],
                    },
                    Stage {
                        pre: vec![hp(140.0)],
                        gain: 10.0,
                        bias: 0.3,
                        post: vec![lp(8_000.0)],
                    },
                    Stage {
                        pre: vec![pk(800.0, 0.8, 4.0)],
                        gain: 6.0,
                        bias: 0.25,
                        post: vec![lp(6_500.0)],
                    },
                ],
                vec![pk(650.0, 0.9, 2.0), hs(3_000.0, -2.0), ls(100.0, -1.5)],
                2.4,
                0.6,
                vec![hs(3_500.0, 1.5), pk(110.0, 1.1, 2.0)],
                0.6,
            ),
            // Tight, scooped modern / thrash high gain: high-passed interstage
            // coupling keeps palm mutes tight, the post-distortion scoop and
            // resonance give the chug, low sag keeps the attack fast.
            Voice::Metal => (
                vec![
                    Stage {
                        pre: vec![hp(90.0), hs(2_000.0, 2.0)],
                        gain: 12.0,
                        bias: 0.2,
                        post: vec![lp(10_000.0)],
                    },
                    Stage {
                        pre: vec![hp(180.0)],
                        gain: 12.0,
                        bias: 0.3,
                        post: vec![lp(8_000.0)],
                    },
                    Stage {
                        pre: vec![hp(150.0)],
                        gain: 8.0,
                        bias: 0.25,
                        post: vec![lp(7_000.0)],
                    },
                    Stage {
                        pre: vec![],
                        gain: 4.0,
                        bias: 0.15,
                        post: vec![lp(6_500.0)],
                    },
                ],
                vec![pk(600.0, 0.7, -6.0), ls(100.0, 2.0), hs(3_000.0, 2.0)],
                2.2,
                0.2,
                vec![hs(3_500.0, 3.0), pk(90.0, 1.2, 3.0)],
                0.6,
            ),
        };

        let coeff = |ms: f32| (-1.0 / (sr * ms * 0.001)).exp();
        Self {
            os: Oversampler4x::new(),
            stages,
            tone,
            power_gain,
            sag_amount,
            sag_env: 0.0,
            sag_att: coeff(12.0),
            sag_rel: coeff(180.0),
            post,
            out_gain,
        }
    }

    /// Latency introduced by the oversampler, in base-rate samples.
    pub fn latency() -> usize {
        LATENCY
    }

    #[inline]
    fn process_hi(&mut self, mut x: f32) -> f32 {
        for s in &mut self.stages {
            x = s.process(x);
        }
        for f in &mut self.tone {
            x = f.process(x);
        }
        // Power stage: sag lowers the drive under sustained load (compression
        // and "bloom"), then a symmetric push-pull saturation.
        let a = x.abs();
        let c = if a > self.sag_env {
            self.sag_att
        } else {
            self.sag_rel
        };
        self.sag_env = a + (self.sag_env - a) * c;
        let g = self.power_gain / (1.0 + self.sag_amount * self.sag_env);
        x = (x * g).tanh();
        for f in &mut self.post {
            x = f.process(x);
        }
        x * self.out_gain
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        let mut hi = [0.0; FACTOR];
        for x in buf.iter_mut() {
            self.os.upsample(*x, &mut hi);
            for h in hi.iter_mut() {
                *h = self.process_hi(*h);
            }
            *x = self.os.downsample(&hi);
        }
    }
}

/// Process a whole signal and remove the oversampler latency so the
/// target is sample-aligned with the input (NAM training wants delay = 0).
pub fn render_aligned(voice: Voice, sample_rate: f32, input: &[f32]) -> Vec<f32> {
    let mut amp = RefAmp::new(voice, sample_rate);
    let lat = RefAmp::latency();
    let mut buf = input.to_vec();
    buf.extend(std::iter::repeat_n(0.0, lat));
    amp.process(&mut buf);
    buf.drain(..lat);
    buf
}
