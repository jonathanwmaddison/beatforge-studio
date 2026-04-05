//! Insert effects: distortion, bitcrusher, chorus, phaser.
//! These run as per-channel insert effects in the mixer.

use std::f64::consts::PI;

// ═══════════════════════════════════════════════════════════
//  DISTORTION / SATURATION
// ═══════════════════════════════════════════════════════════

pub struct Distortion {
    pub drive: f32,     // 0-1 (maps to 1-20× gain)
    pub mix: f32,       // 0-1 wet/dry
    pub tone: f32,      // post-distortion tone (LP filter freq)
    filter: OnePole,
}

impl Distortion {
    pub fn new(sr: f64) -> Self {
        Distortion {
            drive: 0.0,
            mix: 0.0,
            tone: 8000.0,
            filter: OnePole::new(8000.0, sr),
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        if self.mix < 0.01 { return input; }

        let gain = 1.0 + self.drive * 19.0;
        let driven = (input * gain).tanh(); // soft saturation
        let filtered = self.filter.process(driven);
        input * (1.0 - self.mix) + filtered * self.mix
    }

    pub fn set_tone(&mut self, freq: f32, sr: f64) {
        self.tone = freq;
        self.filter = OnePole::new(freq as f64, sr);
    }
}

// ═══════════════════════════════════════════════════════════
//  BITCRUSHER / LO-FI
// ═══════════════════════════════════════════════════════════

pub struct Bitcrusher {
    pub bits: f32,       // 1-16 (bit depth)
    pub rate: f32,       // sample rate reduction factor (1 = none, 32 = max crush)
    pub mix: f32,
    hold: f32,
    counter: f32,
}

impl Bitcrusher {
    pub fn new() -> Self {
        Bitcrusher {
            bits: 16.0,
            rate: 1.0,
            mix: 0.0,
            hold: 0.0,
            counter: 0.0,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        if self.mix < 0.01 { return input; }

        // Sample rate reduction
        self.counter += 1.0;
        if self.counter >= self.rate {
            self.counter -= self.rate;
            self.hold = input;
        }

        // Bit depth reduction
        let levels = 2.0f32.powf(self.bits);
        let crushed = (self.hold * levels).round() / levels;

        input * (1.0 - self.mix) + crushed * self.mix
    }
}

// ═══════════════════════════════════════════════════════════
//  CHORUS
// ═══════════════════════════════════════════════════════════

pub struct Chorus {
    pub rate: f32,       // LFO rate Hz
    pub depth: f32,      // modulation depth in ms (0-10)
    pub mix: f32,
    buffer: Vec<f32>,
    write_pos: usize,
    lfo_phase: f64,
    sr: f64,
}

impl Chorus {
    pub fn new(sr: f64) -> Self {
        let buf_size = (sr * 0.05) as usize; // 50ms max delay
        Chorus {
            rate: 0.5,
            depth: 3.0,
            mix: 0.0,
            buffer: vec![0.0; buf_size],
            write_pos: 0,
            lfo_phase: 0.0,
            sr,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        if self.mix < 0.01 { return input; }

        let len = self.buffer.len();
        self.buffer[self.write_pos] = input;
        self.write_pos = (self.write_pos + 1) % len;

        // LFO modulates delay time
        self.lfo_phase += self.rate as f64 / self.sr;
        if self.lfo_phase >= 1.0 { self.lfo_phase -= 1.0; }
        let lfo = (self.lfo_phase * 2.0 * PI).sin();

        // Delay time in samples (center + modulation)
        let center_delay = self.sr * 0.007; // 7ms center
        let mod_depth = self.depth as f64 * self.sr / 1000.0; // depth in samples
        let delay = center_delay + lfo * mod_depth * 0.5;
        let delay = delay.max(1.0).min((len - 1) as f64);

        // Read with linear interpolation
        let read_pos = (self.write_pos as f64 + len as f64 - delay) % len as f64;
        let idx = read_pos as usize;
        let frac = read_pos - idx as f64;
        let a = self.buffer[idx % len];
        let b = self.buffer[(idx + 1) % len];
        let delayed = (a as f64 * (1.0 - frac) + b as f64 * frac) as f32;

        input * (1.0 - self.mix * 0.5) + delayed * self.mix
    }
}

// ═══════════════════════════════════════════════════════════
//  PHASER
// ═══════════════════════════════════════════════════════════

pub struct Phaser {
    pub rate: f32,       // LFO rate Hz
    pub depth: f32,      // 0-1
    pub feedback: f32,   // 0-0.9
    pub mix: f32,
    allpass: [AllpassStage; 4],
    lfo_phase: f64,
    last_out: f32,
    sr: f64,
}

impl Phaser {
    pub fn new(sr: f64) -> Self {
        Phaser {
            rate: 0.3,
            depth: 0.5,
            feedback: 0.5,
            mix: 0.0,
            allpass: [AllpassStage::new(); 4],
            lfo_phase: 0.0,
            last_out: 0.0,
            sr,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        if self.mix < 0.01 { return input; }

        // LFO
        self.lfo_phase += self.rate as f64 / self.sr;
        if self.lfo_phase >= 1.0 { self.lfo_phase -= 1.0; }
        let lfo = (self.lfo_phase * 2.0 * PI).sin() as f32;

        // Modulate allpass frequencies
        let min_freq = 200.0f64;
        let max_freq = 4000.0f64;
        let mod_freq = min_freq + (max_freq - min_freq) * (0.5 + 0.5 * lfo as f64 * self.depth as f64);

        // Process through allpass chain
        let mut sample = input + self.last_out * self.feedback;

        for ap in &mut self.allpass {
            let coeff = (PI * mod_freq / self.sr).tan();
            let coeff = (coeff - 1.0) / (coeff + 1.0);
            sample = ap.process(sample, coeff as f32);
        }

        self.last_out = sample;
        input * (1.0 - self.mix) + sample * self.mix
    }
}

#[derive(Clone, Copy)]
struct AllpassStage {
    x1: f32,
    y1: f32,
}

impl AllpassStage {
    fn new() -> Self {
        AllpassStage { x1: 0.0, y1: 0.0 }
    }

    fn process(&mut self, input: f32, coeff: f32) -> f32 {
        let y = coeff * input + self.x1 - coeff * self.y1;
        self.x1 = input;
        self.y1 = y;
        y
    }
}

// ═══════════════════════════════════════════════════════════
//  PER-CHANNEL FX CHAIN
// ═══════════════════════════════════════════════════════════

/// Complete insert effect chain for one mixer channel
pub struct InsertFxChain {
    pub distortion: Distortion,
    pub bitcrusher: Bitcrusher,
    pub chorus: Chorus,
    pub phaser: Phaser,
}

impl InsertFxChain {
    pub fn new(sr: f64) -> Self {
        InsertFxChain {
            distortion: Distortion::new(sr),
            bitcrusher: Bitcrusher::new(),
            chorus: Chorus::new(sr),
            phaser: Phaser::new(sr),
        }
    }

    /// Process a sample through the entire chain
    pub fn process(&mut self, input: f32) -> f32 {
        let mut s = input;
        s = self.distortion.process(s);
        s = self.bitcrusher.process(s);
        s = self.chorus.process(s);
        s = self.phaser.process(s);
        s
    }
}

// ═══════════════════════════════════════════════════════════
//  ONE-POLE FILTER (for distortion tone)
// ═══════════════════════════════════════════════════════════

struct OnePole {
    coeff: f64,
    prev: f64,
}

impl OnePole {
    fn new(freq: f64, sr: f64) -> Self {
        let coeff = (-2.0 * PI * freq / sr).exp();
        OnePole { coeff, prev: 0.0 }
    }

    fn process(&mut self, input: f32) -> f32 {
        let x = input as f64;
        self.prev = x * (1.0 - self.coeff) + self.prev * self.coeff;
        self.prev as f32
    }
}
