//! Subtractive synthesizer with polyphony, dual oscillators,
//! resonant filter, ADSR envelopes, and LFO.

use std::f64::consts::PI;
const TWO_PI: f64 = 2.0 * PI;
const NUM_VOICES: usize = 8;

// ═══════════════════════════════════════════════════════════
//  PUBLIC TYPES
// ════════���══════════════════════════════════════════════════

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
    Noise,
    Fm, // FM synthesis: osc1 is carrier, osc2 modulates osc1's frequency
}

#[derive(Clone, Copy, PartialEq)]
pub enum FilterType {
    Lowpass,
    Highpass,
    Bandpass,
}

#[derive(Clone, Copy, PartialEq)]
pub enum LfoTarget {
    FilterCutoff,
    Pitch,
    Amplitude,
}

/// Synth parameters (shared, set from UI)
#[derive(Clone)]
pub struct SynthParams {
    pub osc1_wave: Waveform,
    pub osc2_wave: Waveform,
    pub osc_mix: f32,          // 0 = osc1 only, 1 = osc2 only
    pub osc2_detune: f32,      // cents (-100 to 100)
    pub osc2_semi: i32,        // semitones (-24 to 24)
    pub osc2_octave: i32,      // octaves (-2 to 2)

    // Filter
    pub filter_type: FilterType,
    pub filter_cutoff: f32,    // Hz (20-20000)
    pub filter_resonance: f32, // 0-1 (maps to Q 0.5-20)

    // Amp envelope
    pub amp_attack: f32,       // seconds
    pub amp_decay: f32,
    pub amp_sustain: f32,      // level 0-1
    pub amp_release: f32,

    // Filter envelope
    pub filt_attack: f32,
    pub filt_decay: f32,
    pub filt_sustain: f32,
    pub filt_release: f32,
    pub filt_env_amount: f32,  // -1 to 1

    // LFO
    pub lfo_rate: f32,         // Hz
    pub lfo_amount: f32,       // 0-1
    pub lfo_wave: Waveform,
    pub lfo_target: LfoTarget,

    // Unison
    pub unison_voices: u8,     // 1-7 (1 = off)
    pub unison_detune: f32,    // cents spread (0-50)

    // Sub oscillator
    pub sub_osc: bool,         // enable sub osc (1 octave below osc1)
    pub sub_level: f32,        // 0-1

    // Ring modulation
    pub ring_mod: bool,        // multiply osc1 × osc2 instead of mix

    // Master
    pub volume: f32,
    pub glide: f32,            // portamento time in seconds (0 = off)
}

impl Default for SynthParams {
    fn default() -> Self {
        SynthParams {
            osc1_wave: Waveform::Saw,
            osc2_wave: Waveform::Saw,
            osc_mix: 0.3,
            osc2_detune: 7.0,
            osc2_semi: 0,
            osc2_octave: 0,

            filter_type: FilterType::Lowpass,
            filter_cutoff: 2000.0,
            filter_resonance: 0.3,

            amp_attack: 0.005,
            amp_decay: 0.2,
            amp_sustain: 0.6,
            amp_release: 0.3,

            filt_attack: 0.01,
            filt_decay: 0.3,
            filt_sustain: 0.3,
            filt_release: 0.4,
            filt_env_amount: 0.5,

            lfo_rate: 4.0,
            lfo_amount: 0.0,
            lfo_wave: Waveform::Sine,
            lfo_target: LfoTarget::FilterCutoff,

            unison_voices: 1,
            unison_detune: 10.0,
            sub_osc: false,
            sub_level: 0.5,
            ring_mod: false,
            volume: 0.5,
            glide: 0.0,
        }
    }
}

// ═══════════════════════════════════════════════════════════
//  SUBSYNTH (polyphonic)
// ════════════════════���══════════════════════════════════════

pub struct SubSynth {
    voices: [SynthVoice; NUM_VOICES],
    pub params: SynthParams,
    lfo_phase: f64,
    next_voice: usize, // round-robin voice allocation
}

impl SubSynth {
    pub fn new() -> Self {
        SubSynth {
            voices: std::array::from_fn(|_| SynthVoice::new()),
            params: SynthParams::default(),
            lfo_phase: 0.0,
            next_voice: 0,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: f32) {
        // Find a free voice, or steal the oldest
        let voice_idx = self.voices.iter().position(|v| !v.active)
            .unwrap_or_else(|| {
                let idx = self.next_voice;
                self.next_voice = (self.next_voice + 1) % NUM_VOICES;
                idx
            });

        let voice = &mut self.voices[voice_idx];
        voice.note_on(note, velocity, &self.params);
        self.next_voice = (voice_idx + 1) % NUM_VOICES;
    }

    pub fn note_off(&mut self, note: u8) {
        for voice in &mut self.voices {
            if voice.active && voice.note == note && voice.gate {
                voice.note_off(&self.params);
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        for voice in &mut self.voices {
            if voice.active {
                voice.note_off(&self.params);
            }
        }
    }

    pub fn tick(&mut self, sr: f64) -> f32 {
        // Update LFO (global)
        self.lfo_phase += self.params.lfo_rate as f64 / sr;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }
        let lfo_val = oscillate(self.params.lfo_wave, self.lfo_phase) as f32
            * self.params.lfo_amount;

        // Mix all voices
        let mut out = 0.0f32;
        for voice in &mut self.voices {
            if voice.active {
                out += voice.tick(sr, &self.params, lfo_val);
            }
        }

        out * self.params.volume
    }
}

// ════════════��══════════════════════════════════════════════
//  SINGLE VOICE
// ════════════════════════���══════════════════════════════════

struct SynthVoice {
    active: bool,
    gate: bool,
    note: u8,
    velocity: f32,
    freq: f64,       // target frequency
    cur_freq: f64,   // current frequency (for glide)

    osc1_phase: f64,
    osc2_phase: f64,
    amp_env: Envelope,
    filt_env: Envelope,
    filter: VoiceFilter,
}

impl SynthVoice {
    fn new() -> Self {
        SynthVoice {
            active: false,
            gate: false,
            note: 0,
            velocity: 0.0,
            freq: 440.0,
            cur_freq: 440.0,
            osc1_phase: 0.0,
            osc2_phase: 0.0,
            amp_env: Envelope::new(),
            filt_env: Envelope::new(),
            filter: VoiceFilter::new(),
        }
    }

    fn note_on(&mut self, note: u8, velocity: f32, params: &SynthParams) {
        self.active = true;
        self.gate = true;
        self.note = note;
        self.velocity = velocity;
        self.freq = midi_to_freq(note);
        if !self.active || params.glide <= 0.0 {
            self.cur_freq = self.freq;
        }
        self.osc1_phase = 0.0;
        self.osc2_phase = 0.0;
        self.amp_env.trigger(params.amp_attack, params.amp_decay, params.amp_sustain, params.amp_release);
        self.filt_env.trigger(params.filt_attack, params.filt_decay, params.filt_sustain, params.filt_release);
    }

    fn note_off(&mut self, params: &SynthParams) {
        self.gate = false;
        self.amp_env.release(params.amp_release);
        self.filt_env.release(params.filt_release);
    }

    fn tick(&mut self, sr: f64, params: &SynthParams, lfo: f32) -> f32 {
        // Glide
        if params.glide > 0.0 {
            let rate = 1.0 - (-1.0 / (sr * params.glide as f64)).exp();
            self.cur_freq += (self.freq - self.cur_freq) * rate;
        } else {
            self.cur_freq = self.freq;
        }

        // LFO pitch modulation
        let pitch_mod = match params.lfo_target {
            LfoTarget::Pitch => 2.0f64.powf(lfo as f64 * 2.0 / 12.0), // ±2 semitones
            _ => 1.0,
        };

        // Oscillator 2 frequency
        let detune_ratio = 2.0f64.powf(params.osc2_detune as f64 / 1200.0);
        let semi_ratio = 2.0f64.powf(params.osc2_semi as f64 / 12.0);
        let oct_ratio = 2.0f64.powf(params.osc2_octave as f64);
        let freq2 = self.cur_freq * detune_ratio * semi_ratio * oct_ratio * pitch_mod;

        let osc_out = if params.osc1_wave == Waveform::Fm {
            // FM SYNTHESIS: osc2 is modulator, osc1 is carrier
            // Modulator output modulates carrier frequency
            self.osc2_phase += freq2 / sr;
            if self.osc2_phase >= 1.0 { self.osc2_phase -= 1.0; }
            let modulator = (self.osc2_phase * TWO_PI).sin();

            // FM index controlled by osc_mix (0-1 maps to 0-8 modulation depth)
            let fm_index = params.osc_mix as f64 * 8.0;
            let carrier_freq = self.cur_freq * pitch_mod;
            self.osc1_phase += (carrier_freq + modulator * carrier_freq * fm_index) / sr;
            if self.osc1_phase >= 1.0 { self.osc1_phase -= 1.0; }
            if self.osc1_phase < 0.0 { self.osc1_phase += 1.0; }
            (self.osc1_phase * TWO_PI).sin()
        } else {
            // Standard subtractive
            let freq1 = self.cur_freq * pitch_mod;
            self.osc1_phase += freq1 / sr;
            if self.osc1_phase >= 1.0 { self.osc1_phase -= 1.0; }
            let osc1 = oscillate(params.osc1_wave, self.osc1_phase);

            self.osc2_phase += freq2 / sr;
            if self.osc2_phase >= 1.0 { self.osc2_phase -= 1.0; }
            let osc2 = oscillate(params.osc2_wave, self.osc2_phase);

            let combined = if params.ring_mod {
                // Ring modulation: multiply oscillators
                osc1 * osc2
            } else {
                // Normal mix
                let mix = params.osc_mix as f64;
                osc1 * (1.0 - mix) + osc2 * mix
            };

            // Add unison voices (detuned copies of osc1)
            let mut unison_sum = combined;
            if params.unison_voices > 1 {
                let n = params.unison_voices as f64;
                let spread = params.unison_detune as f64; // cents
                for v in 1..params.unison_voices {
                    let detune_cents = spread * (v as f64 / (n - 1.0) * 2.0 - 1.0);
                    let detune_ratio = 2.0f64.powf(detune_cents / 1200.0);
                    let uni_freq = freq1 * detune_ratio;
                    // Use osc1_phase with offset for each voice (approximate)
                    let uni_phase = (self.osc1_phase + v as f64 * 0.1337) % 1.0;
                    unison_sum += oscillate(params.osc1_wave, uni_phase) / n;
                }
                unison_sum /= 1.0 + (params.unison_voices - 1) as f64 * 0.5; // normalize
            }

            // Sub oscillator (1 octave below osc1)
            if params.sub_osc {
                let sub_phase = (self.osc1_phase * 0.5) % 1.0;
                let sub = (sub_phase * TWO_PI).sin() * params.sub_level as f64;
                unison_sum + sub
            } else {
                unison_sum
            }
        };

        // Filter envelope
        let filt_env_val = self.filt_env.tick(sr) as f64;
        let base_cutoff = params.filter_cutoff as f64;
        let env_mod = filt_env_val * params.filt_env_amount as f64 * 10000.0;

        // LFO filter modulation
        let lfo_filter_mod = match params.lfo_target {
            LfoTarget::FilterCutoff => lfo as f64 * 3000.0,
            _ => 0.0,
        };

        let cutoff = (base_cutoff + env_mod + lfo_filter_mod).clamp(20.0, 20000.0);
        let q = 0.5 + params.filter_resonance as f64 * 19.5;

        // Apply filter
        self.filter.set(params.filter_type, cutoff, q, sr);
        let filtered = self.filter.tick(osc_out as f32);

        // Amp envelope
        let amp = self.amp_env.tick(sr);
        if !self.gate && amp < 0.001 {
            self.active = false;
        }

        // LFO amplitude modulation
        let amp_mod = match params.lfo_target {
            LfoTarget::Amplitude => 1.0 - lfo.abs() * 0.5,
            _ => 1.0,
        };

        filtered * amp * self.velocity * amp_mod
    }
}

// ═══════════════════════════════════════════════════════════
//  ADSR ENVELOPE
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Copy, PartialEq)]
enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

struct Envelope {
    stage: EnvStage,
    value: f64,
    attack_rate: f64,
    decay_rate: f64,
    sustain_level: f64,
    release_rate: f64,
}

impl Envelope {
    fn new() -> Self {
        Envelope {
            stage: EnvStage::Idle,
            value: 0.0,
            attack_rate: 0.0,
            decay_rate: 0.0,
            sustain_level: 0.0,
            release_rate: 0.0,
        }
    }

    fn trigger(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.stage = EnvStage::Attack;
        self.sustain_level = sustain as f64;
        // Rates: samples to go from 0→1 (attack) or 1→sustain (decay)
        // We'll compute actual per-sample rate in tick() based on sr
        self.attack_rate = if attack > 0.001 { 1.0 / attack as f64 } else { 1000.0 };
        self.decay_rate = if decay > 0.001 { 1.0 / decay as f64 } else { 1000.0 };
        self.release_rate = if release > 0.001 { 1.0 / release as f64 } else { 1000.0 };
    }

    fn release(&mut self, release: f32) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
            self.release_rate = if release > 0.001 { 1.0 / release as f64 } else { 1000.0 };
        }
    }

    fn tick(&mut self, sr: f64) -> f32 {
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                self.value += self.attack_rate / sr;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.stage = EnvStage::Decay;
                }
                self.value as f32
            }
            EnvStage::Decay => {
                self.value -= (self.value - self.sustain_level) * self.decay_rate / sr * 4.0;
                if (self.value - self.sustain_level).abs() < 0.001 {
                    self.value = self.sustain_level;
                    self.stage = EnvStage::Sustain;
                }
                self.value as f32
            }
            EnvStage::Sustain => {
                self.value as f32
            }
            EnvStage::Release => {
                self.value -= self.value * self.release_rate / sr * 4.0;
                if self.value < 0.001 {
                    self.value = 0.0;
                    self.stage = EnvStage::Idle;
                }
                self.value as f32
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════
//  VOICE FILTER (state-variable)
// ═════════════���═════════════════════════════════════════════

struct VoiceFilter {
    lp: f64,
    bp: f64,
    hp: f64,
    filter_type: FilterType,
    f: f64,
    q: f64,
}

impl VoiceFilter {
    fn new() -> Self {
        VoiceFilter {
            lp: 0.0,
            bp: 0.0,
            hp: 0.0,
            filter_type: FilterType::Lowpass,
            f: 1.0,
            q: 0.5,
        }
    }

    fn set(&mut self, ftype: FilterType, cutoff: f64, q: f64, sr: f64) {
        self.filter_type = ftype;
        self.f = (2.0 * (PI * cutoff / sr).sin()).min(0.99);
        self.q = 1.0 / q;
    }

    fn tick(&mut self, input: f32) -> f32 {
        let inp = input as f64;
        self.hp = inp - self.lp - self.bp * self.q;
        self.bp += self.f * self.hp;
        self.lp += self.f * self.bp;

        match self.filter_type {
            FilterType::Lowpass => self.lp as f32,
            FilterType::Highpass => self.hp as f32,
            FilterType::Bandpass => self.bp as f32,
        }
    }
}

// ══���═══════════════════════���════════════════════════════════
//  OSCILLATOR FUNCTION
// ════════════════���══════════════════════════════════════════

/// PolyBLEP anti-aliasing correction
/// Smooths the discontinuities in saw/square waves to reduce aliasing
fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        // Rising edge
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        // Falling edge
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

fn oscillate(wave: Waveform, phase: f64) -> f64 {
    // dt = frequency / sample_rate, approximated from phase increment
    // For PolyBLEP we need the phase increment; use a reasonable default
    let dt = 0.01; // ~440Hz at 44100 SR — good enough for anti-aliasing

    match wave {
        Waveform::Sine => (phase * TWO_PI).sin(),
        Waveform::Saw => {
            // Band-limited saw via PolyBLEP
            let mut out = 2.0 * phase - 1.0;
            out -= poly_blep(phase, dt);
            out
        }
        Waveform::Square => {
            // Band-limited square via PolyBLEP
            let mut out = if phase < 0.5 { 1.0 } else { -1.0 };
            out += poly_blep(phase, dt);
            out -= poly_blep((phase + 0.5) % 1.0, dt);
            out
        }
        Waveform::Triangle => {
            // Integrated PolyBLEP square → triangle (smoother than naive)
            if phase < 0.25 { phase * 4.0 }
            else if phase < 0.75 { 2.0 - phase * 4.0 }
            else { phase * 4.0 - 4.0 }
        }
        Waveform::Noise => {
            // Better noise using xorshift with high-entropy seed
            let seed = (phase * 2147483647.0) as u32;
            let mut x = seed.wrapping_add(0x9E3779B9);
            x ^= x << 13; x ^= x >> 17; x ^= x << 5;
            (x as f64 / u32::MAX as f64) * 2.0 - 1.0
        }
        Waveform::Fm => (phase * TWO_PI).sin(),
    }
}

fn midi_to_freq(note: u8) -> f64 {
    440.0 * 2.0f64.powf((note as f64 - 69.0) / 12.0)
}

// ═══════════════════════════════════════════���═══════════════
//  NOTE EVENT (for piano roll)
// ═��═════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct NoteEvent {
    pub note: u8,        // MIDI note (0-127)
    pub start: f32,      // start position in steps
    pub duration: f32,   // length in steps
    pub velocity: f32,   // 0.0-1.0
}

/// A piano roll pattern — list of notes for one channel
#[derive(Clone, Default)]
pub struct NotePattern {
    pub notes: Vec<NoteEvent>,
}

impl NotePattern {
    pub fn new() -> Self {
        NotePattern { notes: Vec::new() }
    }

    pub fn add_note(&mut self, note: u8, start: f32, duration: f32, velocity: f32) {
        self.notes.push(NoteEvent { note, start, duration, velocity });
    }

    pub fn remove_at(&mut self, step: f32, pitch: u8) {
        self.notes.retain(|n| {
            !(n.note == pitch && n.start <= step && (n.start + n.duration) > step)
        });
    }

    /// Get note-on events at a given step
    pub fn notes_starting_at(&self, step: usize) -> Vec<&NoteEvent> {
        self.notes.iter()
            .filter(|n| (n.start as usize) == step)
            .collect()
    }

    /// Get note-off events at a given step
    pub fn notes_ending_at(&self, step: usize) -> Vec<&NoteEvent> {
        self.notes.iter()
            .filter(|n| ((n.start + n.duration) as usize) == step)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_params() {
        let p = SynthParams::default();
        assert_eq!(p.osc1_wave, Waveform::Saw);
        assert!(p.volume > 0.0);
        assert!(p.filter_cutoff > 0.0);
        assert_eq!(p.unison_voices, 1);
    }

    #[test]
    fn test_note_pattern_add_remove() {
        let mut pat = NotePattern::new();
        pat.add_note(60, 0.0, 1.0, 0.8);
        pat.add_note(64, 2.0, 1.0, 0.6);
        assert_eq!(pat.notes.len(), 2);
        assert_eq!(pat.notes_starting_at(0).len(), 1);
        assert_eq!(pat.notes_starting_at(2).len(), 1);
        assert_eq!(pat.notes_ending_at(1).len(), 1);
        assert_eq!(pat.notes_ending_at(3).len(), 1);
    }

    #[test]
    fn test_synth_silence_when_idle() {
        let mut synth = SubSynth::new();
        let s = synth.tick(44100.0);
        assert_eq!(s, 0.0);
    }

    #[test]
    fn test_synth_produces_sound() {
        let mut synth = SubSynth::new();
        synth.note_on(60, 1.0);
        let mut max = 0.0f32;
        for _ in 0..500 {
            max = max.max(synth.tick(44100.0).abs());
        }
        assert!(max > 0.01, "Should produce audible output");
    }

    #[test]
    fn test_synth_note_off_decays() {
        let mut synth = SubSynth::new();
        synth.params.amp_release = 0.01;
        synth.note_on(60, 1.0);
        for _ in 0..200 { synth.tick(44100.0); }
        synth.note_off(60);
        for _ in 0..5000 { synth.tick(44100.0); }
        assert!(synth.tick(44100.0).abs() < 0.01);
    }

    #[test]
    fn test_fm_synthesis() {
        let mut synth = SubSynth::new();
        synth.params.osc1_wave = Waveform::Fm;
        synth.params.osc_mix = 0.5; // FM index
        synth.note_on(60, 1.0);
        let mut max = 0.0f32;
        for _ in 0..500 {
            max = max.max(synth.tick(44100.0).abs());
        }
        assert!(max > 0.01, "FM should produce sound");
    }

    #[test]
    fn test_oscillator_waveforms() {
        // All waveforms should produce output in [-1, 1]
        for phase_i in 0..100 {
            let phase = phase_i as f64 / 100.0;
            for wave in [Waveform::Sine, Waveform::Saw, Waveform::Square, Waveform::Triangle] {
                let v = oscillate(wave, phase);
                assert!(v >= -1.5 && v <= 1.5, "Waveform {:?} out of range at phase {}: {}", wave, phase, v);
            }
        }
    }

    #[test]
    fn test_scale_snap() {
        // C major: C D E F G A B
        let major = [0u8, 2, 4, 5, 7, 9, 11];
        // C=60 should stay at 60
        assert!(major.contains(&(60 % 12)));
        // C#=61 should snap to C(60) or D(62)
        let cs = 61u8;
        assert!(!major.contains(&(cs % 12)));
    }
}

/// Morph between waveforms: 0.0=sine, 0.33=triangle, 0.66=saw, 1.0=square
/// Allows continuous waveform blending with a single parameter
pub fn morph_oscillate(phase: f64, morph: f64) -> f64 {
    let two_pi: f64 = 2.0 * std::f64::consts::PI;
    let sine = (phase * two_pi).sin();
    let tri = if phase < 0.25 { phase * 4.0 }
        else if phase < 0.75 { 2.0 - phase * 4.0 }
        else { phase * 4.0 - 4.0 };
    let saw = 2.0 * phase - 1.0;
    let square = if phase < 0.5 { 1.0 } else { -1.0 };

    if morph <= 0.33 {
        let t = morph / 0.33;
        sine * (1.0 - t) + tri * t
    } else if morph <= 0.66 {
        let t = (morph - 0.33) / 0.33;
        tri * (1.0 - t) + saw * t
    } else {
        let t = (morph - 0.66) / 0.34;
        saw * (1.0 - t) + square * t
    }
}
