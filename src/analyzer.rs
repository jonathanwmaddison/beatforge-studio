//! Real-time spectrum analyzer using a simple DFT on the output buffer.
//! Provides frequency-domain data for the spectrum display.

use std::f64::consts::PI;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Number of frequency bins for the display
pub const NUM_BINS: usize = 64;
const FFT_SIZE: usize = 2048;

/// Shared spectrum data between audio and UI threads
pub struct SpectrumData {
    /// Magnitude per bin (linear scale, 0.0-1.0), stored as u64 bits
    bins: [AtomicU64; NUM_BINS],
}

impl SpectrumData {
    pub fn new() -> Arc<Self> {
        Arc::new(SpectrumData {
            bins: std::array::from_fn(|_| AtomicU64::new(0)),
        })
    }

    pub fn set_bin(&self, idx: usize, value: f32) {
        if idx < NUM_BINS {
            self.bins[idx].store(f64::to_bits(value as f64), Ordering::Relaxed);
        }
    }

    pub fn get_bin(&self, idx: usize) -> f32 {
        if idx < NUM_BINS {
            f64::from_bits(self.bins[idx].load(Ordering::Relaxed)) as f32
        } else {
            0.0
        }
    }

    pub fn get_all(&self) -> [f32; NUM_BINS] {
        std::array::from_fn(|i| self.get_bin(i))
    }
}

/// Spectrum analyzer that runs on the audio thread
pub struct SpectrumAnalyzer {
    buffer: Vec<f32>,
    write_pos: usize,
    window: Vec<f32>,
    output: Arc<SpectrumData>,
    counter: usize,
    update_interval: usize, // samples between updates
}

impl SpectrumAnalyzer {
    pub fn new(sr: f64, output: Arc<SpectrumData>) -> Self {
        // Hann window
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                let t = i as f64 / (FFT_SIZE - 1) as f64;
                (0.5 * (1.0 - (2.0 * PI * t).cos())) as f32
            })
            .collect();

        SpectrumAnalyzer {
            buffer: vec![0.0; FFT_SIZE],
            write_pos: 0,
            window,
            output,
            counter: 0,
            update_interval: (sr / 30.0) as usize, // ~30 FPS updates
        }
    }

    /// Push a mono sample into the buffer
    pub fn push(&mut self, sample: f32) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % FFT_SIZE;
        self.counter += 1;

        if self.counter >= self.update_interval {
            self.counter = 0;
            self.analyze();
        }
    }

    /// Perform DFT and update bins
    fn analyze(&self) {
        // Apply window and compute DFT at logarithmically-spaced frequencies
        // (We use a simplified Goertzel-like approach for efficiency)

        let min_freq: f64 = 30.0;
        let max_freq: f64 = 16000.0;

        for bin in 0..NUM_BINS {
            let t = bin as f64 / (NUM_BINS - 1) as f64;
            let freq = min_freq * (max_freq / min_freq).powf(t);

            // Goertzel algorithm for this frequency
            let k = (freq * FFT_SIZE as f64 / 44100.0) as usize;
            let w = 2.0 * PI * k as f64 / FFT_SIZE as f64;
            let coeff = 2.0 * w.cos();

            let mut s0: f64;
            let mut s1 = 0.0f64;
            let mut s2 = 0.0f64;

            for i in 0..FFT_SIZE {
                let idx = (self.write_pos + i) % FFT_SIZE;
                let windowed = self.buffer[idx] as f64 * self.window[i] as f64;
                s0 = windowed + coeff * s1 - s2;
                s2 = s1;
                s1 = s0;
            }

            let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
            let magnitude = power.abs().sqrt() / (FFT_SIZE as f64 * 0.5);

            // Convert to display range (0-1) with some scaling
            let db = 20.0 * (magnitude + 1e-10).log10();
            let normalized = ((db + 60.0) / 60.0).clamp(0.0, 1.0);

            // Smooth with previous value (decay)
            let prev = self.output.get_bin(bin) as f64;
            let smoothed = if normalized > prev {
                normalized * 0.7 + prev * 0.3 // fast attack
            } else {
                normalized * 0.1 + prev * 0.9 // slow decay
            };

            self.output.set_bin(bin, smoothed as f32);
        }
    }
}
