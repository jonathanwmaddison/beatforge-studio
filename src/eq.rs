//! 3-band parametric EQ for mixer channels.
//! Low shelf, mid peak, high shelf — standard channel strip EQ.

use std::f64::consts::PI;

/// Parameters for a 3-band EQ
#[derive(Clone)]
pub struct EqParams {
    pub low_freq: f32,    // Hz (20-500)
    pub low_gain: f32,    // dB (-12 to +12)
    pub mid_freq: f32,    // Hz (200-8000)
    pub mid_gain: f32,    // dB (-12 to +12)
    pub mid_q: f32,       // 0.1 to 10
    pub high_freq: f32,   // Hz (2000-20000)
    pub high_gain: f32,   // dB (-12 to +12)
    pub enabled: bool,
}

impl Default for EqParams {
    fn default() -> Self {
        EqParams {
            low_freq: 100.0,
            low_gain: 0.0,
            mid_freq: 1000.0,
            mid_gain: 0.0,
            mid_q: 1.0,
            high_freq: 8000.0,
            high_gain: 0.0,
            enabled: true,
        }
    }
}

/// A biquad filter section
struct BiquadSection {
    b0: f64, b1: f64, b2: f64,
    a1: f64, a2: f64,
    x1: f64, x2: f64,
    y1: f64, y2: f64,
}

impl BiquadSection {
    fn new() -> Self {
        BiquadSection {
            b0: 1.0, b1: 0.0, b2: 0.0,
            a1: 0.0, a2: 0.0,
            x1: 0.0, x2: 0.0,
            y1: 0.0, y2: 0.0,
        }
    }

    fn set_low_shelf(&mut self, freq: f64, gain_db: f64, sr: f64) {
        let a = 10.0f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / 2.0 * (2.0f64).sqrt();
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
        self.b0 = (a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha)) / a0;
        self.b1 = (2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0)) / a0;
        self.b2 = (a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha)) / a0;
        self.a1 = (-2.0 * ((a - 1.0) + (a + 1.0) * cos_w0)) / a0;
        self.a2 = ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha) / a0;
    }

    fn set_high_shelf(&mut self, freq: f64, gain_db: f64, sr: f64) {
        let a = 10.0f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / 2.0 * (2.0f64).sqrt();
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
        self.b0 = (a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha)) / a0;
        self.b1 = (-2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0)) / a0;
        self.b2 = (a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha)) / a0;
        self.a1 = (2.0 * ((a - 1.0) - (a + 1.0) * cos_w0)) / a0;
        self.a2 = ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha) / a0;
    }

    fn set_peaking(&mut self, freq: f64, gain_db: f64, q: f64, sr: f64) {
        let a = 10.0f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sr;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);

        let a0 = 1.0 + alpha / a;
        self.b0 = (1.0 + alpha * a) / a0;
        self.b1 = (-2.0 * cos_w0) / a0;
        self.b2 = (1.0 - alpha * a) / a0;
        self.a1 = (-2.0 * cos_w0) / a0;
        self.a2 = (1.0 - alpha / a) / a0;
    }

    fn process(&mut self, input: f32) -> f32 {
        let x = input as f64;
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y as f32
    }
}

/// 3-band parametric EQ processor
pub struct ChannelEq {
    low: BiquadSection,
    mid: BiquadSection,
    high: BiquadSection,
    params: EqParams,
    sr: f64,
    needs_update: bool,
}

impl ChannelEq {
    pub fn new(sr: f64) -> Self {
        ChannelEq {
            low: BiquadSection::new(),
            mid: BiquadSection::new(),
            high: BiquadSection::new(),
            params: EqParams::default(),
            sr,
            needs_update: true,
        }
    }

    pub fn set_params(&mut self, params: &EqParams) {
        self.params = params.clone();
        self.needs_update = true;
    }

    fn update_coefficients(&mut self) {
        self.low.set_low_shelf(self.params.low_freq as f64, self.params.low_gain as f64, self.sr);
        self.mid.set_peaking(self.params.mid_freq as f64, self.params.mid_gain as f64, self.params.mid_q as f64, self.sr);
        self.high.set_high_shelf(self.params.high_freq as f64, self.params.high_gain as f64, self.sr);
        self.needs_update = false;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        if !self.params.enabled {
            return input;
        }
        if self.needs_update {
            self.update_coefficients();
        }
        let x = self.low.process(input);
        let x = self.mid.process(x);
        self.high.process(x)
    }
}

/// Compute frequency response magnitude for display (returns dB values)
pub fn freq_response(params: &EqParams, sr: f64, num_points: usize) -> Vec<(f32, f32)> {
    let mut eq = ChannelEq::new(sr);
    eq.set_params(params);
    eq.update_coefficients();

    let min_freq = 20.0f64;
    let max_freq = 20000.0f64;

    (0..num_points).map(|i| {
        let t = i as f64 / (num_points - 1) as f64;
        let freq = min_freq * (max_freq / min_freq).powf(t);

        // Compute magnitude response at this frequency for each band
        let w = 2.0 * PI * freq / sr;
        let mag = band_magnitude(&eq.low, w) * band_magnitude(&eq.mid, w) * band_magnitude(&eq.high, w);
        let db = 20.0 * mag.log10();

        (freq as f32, db as f32)
    }).collect()
}

fn band_magnitude(section: &BiquadSection, w: f64) -> f64 {
    let cos_w = w.cos();
    let cos_2w = (2.0 * w).cos();
    let sin_w = w.sin();
    let sin_2w = (2.0 * w).sin();

    let num_real = section.b0 + section.b1 * cos_w + section.b2 * cos_2w;
    let num_imag = -(section.b1 * sin_w + section.b2 * sin_2w);
    let den_real = 1.0 + section.a1 * cos_w + section.a2 * cos_2w;
    let den_imag = -(section.a1 * sin_w + section.a2 * sin_2w);

    let num_mag = (num_real * num_real + num_imag * num_imag).sqrt();
    let den_mag = (den_real * den_real + den_imag * den_imag).sqrt();

    if den_mag > 1e-10 { num_mag / den_mag } else { 1.0 }
}
