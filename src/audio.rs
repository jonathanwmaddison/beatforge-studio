use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::f64::consts::PI;
use std::sync::atomic::{AtomicI32, AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use crate::synth::{SubSynth, SynthParams, NotePattern};
use crate::recorder::AudioRecorder;
use crate::eq::{ChannelEq, EqParams};
use crate::fx::InsertFxChain;
use crate::automation::{AutomationData, AutoTarget};
use crate::analyzer::{SpectrumAnalyzer, SpectrumData};

pub const NUM_PADS: usize = 16;
pub const MAX_STEPS: usize = 64;
const TWO_PI: f64 = 2.0 * PI;

// ═══════════════════════════════════════════════════════════
//  GROSS BEAT MODES
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Copy, PartialEq)]
pub enum GrossBeatMode {
    Off,
    HalfSpeed,
    TapeStop,
    Gate,
    Stutter,
    Reverse,
}

// ═══════════════════════════════════════════════════════════
//  COMMANDS (UI → audio thread)
// ═══════════════════════════════════════════════════════════

pub enum Cmd {
    Play,
    Stop,
    SetBpm(f32),
    SetSwing(f32),
    SetSteps(usize),
    SetCell {
        pad: usize,
        step: usize,
        vel: u8,
    },
    SetFullPattern(Vec<Vec<u8>>),
    TriggerPad(usize, f32),
    SetPadVol(usize, f32),
    SetPadPan(usize, f32),
    SetPadPitch(usize, f32),
    SetPadFilter(usize, f32),
    SetPadReverse(usize, bool),
    SetPadTrim(usize, f32, f32),
    SetPadMute(usize, bool),
    SetPadSolo(usize, bool),
    SetMasterVol(f32),
    SetMasterFilter(f32),
    SetReverb(f32),
    SetDelay(f32),
    LoadSample {
        pad: usize,
        data: Arc<Vec<f32>>,
        original_sr: u32,
    },
    RemoveSample(usize),
    // Synth commands
    SetPadSynth(usize, SynthParams),
    NoteOn { pad: usize, note: u8, velocity: f32 },
    NoteOff { pad: usize, note: u8 },
    SetNotePattern { pad: usize, pattern: NotePattern },
    // Recording
    StartRecording,
    StopRecording,
    // Choke groups & metronome
    SetChokeGroup(usize, u8),      // pad, group (0 = none, 1-4 = groups)
    SetMetronome(bool),
    CopyBank { from: usize, to: usize },
    SetPadEq(usize, EqParams),
    SetAutomation(AutomationData),
    // Sidechain: source pad ducks target pad
    SetSidechain { source: usize, target: usize, amount: f32 },
    ClearSidechain(usize), // clear sidechain on target
    // Gross Beat style effects
    SetGrossBeat(GrossBeatMode),
    // Per-step probability
    SetStepProbability(Vec<Vec<u8>>), // [pad][step] = 0-100
    // Per-channel sends
    SetPadReverbSend(usize, f32),
    SetPadDelaySend(usize, f32),
    // Master stereo width
    SetStereoWidth(f32),
    // Master enhancer (Soundgoodizer-style one-knob)
    SetEnhancer(f32), // 0-1 amount
    // All notes off (kill stuck synth voices)
    AllNotesOff,
    // Per-channel insert FX
    SetPadDistortion { pad: usize, drive: f32, mix: f32 },
    SetPadBitcrush { pad: usize, bits: f32, rate: f32, mix: f32 },
    SetPadChorus { pad: usize, rate: f32, depth: f32, mix: f32 },
    SetPadPhaser { pad: usize, rate: f32, depth: f32, feedback: f32, mix: f32 },
    // Per-pad sample envelope and loop mode
    SetPadAttack(usize, f32),
    SetPadRelease(usize, f32),
    SetPadLoopMode(usize, bool),
    SetDrumParams(usize, f32, f32, f32), // pad, tune, decay, color
    // Loop region
    SetLoopRegion(Option<usize>, Option<usize>),
    // Song mode
    SetSongMode(Vec<Vec<Vec<u8>>>),
    ClearSongMode,
}

// ═══════════════════════════════════════════════════════════
//  ENGINE (UI-side handle)
// ═══════════════════════════════════════════════════════════

/// Shared real-time state readable by the UI thread
pub struct SharedState {
    pub step: AtomicI32,
    pub is_recording: AtomicBool,
    /// Bitmask of pads triggered this step (bit N = pad N)
    pub triggered: AtomicU32,
    /// Per-pad peak level (stored as u32 bits of f32)
    pub pad_levels: [AtomicU32; NUM_PADS],
    /// Master peak level
    pub master_level: AtomicU32,
}

impl SharedState {
    pub fn new() -> Arc<Self> {
        Arc::new(SharedState {
            step: AtomicI32::new(-1),
            is_recording: AtomicBool::new(false),
            triggered: AtomicU32::new(0),
            pad_levels: std::array::from_fn(|_| AtomicU32::new(0)),
            master_level: AtomicU32::new(0),
        })
    }

    pub fn get_pad_level(&self, idx: usize) -> f32 {
        f32::from_bits(self.pad_levels[idx].load(Ordering::Relaxed))
    }

    pub fn set_pad_level(&self, idx: usize, val: f32) {
        self.pad_levels[idx].store(val.to_bits(), Ordering::Relaxed);
    }

    pub fn get_master_level(&self) -> f32 {
        f32::from_bits(self.master_level.load(Ordering::Relaxed))
    }

    pub fn set_master_level(&self, val: f32) {
        self.master_level.store(val.to_bits(), Ordering::Relaxed);
    }

    pub fn get_triggered(&self) -> u32 {
        self.triggered.swap(0, Ordering::Relaxed)
    }
}

pub struct Engine {
    tx: Sender<Cmd>,
    pub shared: Arc<SharedState>,
    pub spectrum: Arc<SpectrumData>,
    _stream: Option<cpal::Stream>,
    pub sample_rate: u32,
}

impl Engine {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        let shared = SharedState::new();
        let spectrum = SpectrumData::new();

        let host = cpal::default_host();
        let device = host.default_output_device();

        let (stream, sr) = if let Some(dev) = device {
            if let Ok(supported) = dev.default_output_config() {
                let sr = supported.sample_rate().0;
                let config = cpal::StreamConfig {
                    channels: 2,
                    sample_rate: supported.sample_rate(),
                    buffer_size: cpal::BufferSize::Default,
                };

                let mut state = AudioState::new(rx, shared.clone(), spectrum.clone(), sr as f64);

                let stream = dev
                    .build_output_stream(
                        &config,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            state.process(data);
                        },
                        |err| eprintln!("Audio error: {err}"),
                        None,
                    )
                    .ok();

                if let Some(ref s) = stream {
                    let _ = s.play();
                }
                (stream, sr)
            } else {
                (None, 44100)
            }
        } else {
            (None, 44100)
        };

        Engine {
            tx,
            shared,
            spectrum,
            _stream: stream,
            sample_rate: sr,
        }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn current_step(&self) -> i32 {
        self.shared.step.load(Ordering::Relaxed)
    }
}

// ═══════════════════════════════════════════════════════════
//  AUDIO STATE (lives on audio thread)
// ═══════════════════════════════════════════════════════════

struct AudioState {
    pads: Vec<Pad>,
    synths: Vec<Option<SubSynth>>,          // per-pad subtractive synth
    note_patterns: Vec<NotePattern>,         // per-pad piano roll data
    active_notes: Vec<Vec<u8>>,              // currently sounding notes per pad
    seq: Sequencer,
    master_vol: f32,
    reverb: Reverb,
    delay: Delay,
    filter_l: Biquad,
    filter_r: Biquad,
    reverb_mix: f32,
    delay_mix: f32,
    stereo_width: f32,
    enhancer_amount: f32,
    // Enhancer state (3-band: low, mid, high)
    enhancer_low_env: f32,
    enhancer_mid_env: f32,
    enhancer_high_env: f32,
    enhancer_lp: Biquad,
    enhancer_hp: Biquad,
    recorder: AudioRecorder,
    shared: Arc<SharedState>,
    // Sidechain: [target_pad] = (source_pad, amount, envelope)
    sidechain: Vec<Option<(usize, f32, f32)>>,
    // Gross Beat effect
    gross_beat: GrossBeatState,
    step_probability: Vec<Vec<u8>>,
    prob_rng: u32,
    metronome_on: bool,
    metronome_phase: f64,
    metronome_env: f64,
    analyzer: SpectrumAnalyzer,
    automation: AutomationData,
    rx: Receiver<Cmd>,
    sr: f64,
    // Per-pad peak tracking (for meters)
    pad_peaks: [f32; NUM_PADS],
    master_peak: f32,
    peak_decay: f32,
}

impl AudioState {
    fn new(rx: Receiver<Cmd>, shared: Arc<SharedState>, spectrum: Arc<SpectrumData>, sr: f64) -> Self {
        let pads = (0..NUM_PADS).map(|i| Pad::new(i, sr)).collect();
        AudioState {
            pads,
            synths: (0..NUM_PADS).map(|_| None).collect(),
            note_patterns: (0..NUM_PADS).map(|_| NotePattern::new()).collect(),
            active_notes: (0..NUM_PADS).map(|_| Vec::new()).collect(),
            seq: Sequencer::new(),
            master_vol: 0.8,
            reverb: Reverb::new(sr),
            delay: Delay::new(sr, 0.25),
            filter_l: Biquad::lowpass(20000.0, sr),
            filter_r: Biquad::lowpass(20000.0, sr),
            reverb_mix: 0.0,
            delay_mix: 0.0,
            stereo_width: 1.0,
            enhancer_amount: 0.0,
            enhancer_low_env: 0.0,
            enhancer_mid_env: 0.0,
            enhancer_high_env: 0.0,
            enhancer_lp: Biquad::lowpass(200.0, sr),
            enhancer_hp: Biquad::lowpass(4000.0, sr),
            recorder: AudioRecorder::new(sr as u32, 300.0), // 5 min max
            shared,
            sidechain: (0..NUM_PADS).map(|_| None).collect(),
            gross_beat: GrossBeatState::new(sr),
            step_probability: (0..NUM_PADS).map(|_| vec![100u8; MAX_STEPS]).collect(),
            prob_rng: 42,
            metronome_on: false,
            metronome_phase: 0.0,
            metronome_env: 0.0,
            analyzer: SpectrumAnalyzer::new(sr, spectrum),
            automation: AutomationData::new(),
            rx,
            sr,
            pad_peaks: [0.0; NUM_PADS],
            master_peak: 0.0,
            peak_decay: (-1.0 / (sr * 0.15)).exp() as f32, // 150ms decay
        }
    }

    fn process(&mut self, output: &mut [f32]) {
        // Drain command queue
        while let Ok(cmd) = self.rx.try_recv() {
            self.handle_cmd(cmd);
        }

        let sr = self.sr;

        for frame in output.chunks_mut(2) {
            // Advance sequencer — may return triggers
            if let Some(triggers) = self.seq.tick(sr) {
                self.shared.step
                    .store(self.seq.current_step as i32, Ordering::Relaxed);

                let any_solo = self.pads.iter().any(|p| p.soloed);

                // Collect which pads to trigger (need indices for choke logic)
                let mut to_trigger: Vec<(usize, f32)> = Vec::new();
                for (i, &vel) in triggers.iter().enumerate() {
                    if vel == 0 || i >= NUM_PADS { continue; }
                    let pad = &self.pads[i];
                    if pad.muted || (any_solo && !pad.soloed) { continue; }

                    // Per-step probability check
                    let prob = self.step_probability.get(i)
                        .and_then(|row| row.get(self.seq.current_step))
                        .copied().unwrap_or(100);
                    if prob < 100 {
                        // xorshift rng
                        self.prob_rng ^= self.prob_rng << 13;
                        self.prob_rng ^= self.prob_rng >> 17;
                        self.prob_rng ^= self.prob_rng << 5;
                        let roll = (self.prob_rng % 100) as u8;
                        if roll >= prob { continue; } // skip this trigger
                    }

                    let v = [0.0, 0.35, 0.65, 1.0][vel as usize % 4];
                    to_trigger.push((i, v));
                }

                // Apply choke groups: when a pad triggers, silence other pads in same group
                for &(idx, _) in &to_trigger {
                    let group = self.pads[idx].choke_group;
                    if group > 0 {
                        for j in 0..NUM_PADS {
                            if j != idx && self.pads[j].choke_group == group {
                                // Kill the other pad's voice
                                self.pads[j].voice.silence();
                                if let Some(ref mut s) = self.pads[j].sample {
                                    s.playing = false;
                                }
                            }
                        }
                    }
                }

                let mut trig_mask: u32 = 0;
                for (idx, vel) in to_trigger {
                    self.pads[idx].trigger(vel);
                    trig_mask |= 1 << idx;
                }
                self.shared.triggered.fetch_or(trig_mask, Ordering::Relaxed);

                // Metronome click on beat starts (every 4 steps)
                if self.metronome_on && self.seq.current_step % 4 == 0 {
                    self.metronome_env = if self.seq.current_step == 0 { 0.8 } else { 0.5 };
                }

                // Apply automation values at this step
                for (target, pad_idx, value) in self.automation.values_at_step(self.seq.current_step) {
                    match target {
                        AutoTarget::FilterCutoff => {
                            if pad_idx < NUM_PADS {
                                self.pads[pad_idx].filter = Biquad::lowpass(value as f64, self.sr);
                            }
                        }
                        AutoTarget::Volume => {
                            if pad_idx < NUM_PADS { self.pads[pad_idx].vol = value; }
                        }
                        AutoTarget::Pan => {
                            if pad_idx < NUM_PADS { self.pads[pad_idx].pan = value; }
                        }
                        AutoTarget::MasterFilter => {
                            self.filter_l = Biquad::lowpass(value as f64, self.sr);
                            self.filter_r = Biquad::lowpass(value as f64, self.sr);
                        }
                        AutoTarget::MasterVolume => {
                            self.master_vol = value;
                        }
                        _ => {} // ReverbSend, DelaySend, Pitch handled similarly
                    }
                }
            }

            // Process note patterns (piano roll) for synth pads
            if let Some(ref _triggers) = self.seq.last_triggers.take() {
                let step = self.seq.current_step;
                for (i, synth) in self.synths.iter_mut().enumerate() {
                    if let Some(ref mut s) = synth {
                        // Note-offs for notes ending at this step
                        let offs: Vec<u8> = self.note_patterns[i].notes_ending_at(step)
                            .iter().map(|n| n.note).collect();
                        for note in offs {
                            s.note_off(note);
                            self.active_notes[i].retain(|&n| n != note);
                        }
                        // Note-ons for notes starting at this step
                        let ons: Vec<(u8, f32)> = self.note_patterns[i].notes_starting_at(step)
                            .iter().map(|n| (n.note, n.velocity)).collect();
                        for (note, vel) in ons {
                            s.note_on(note, vel);
                            self.active_notes[i].push(note);
                        }
                    }
                }
            }

            // Generate per-pad outputs
            let decay = self.peak_decay;
            let mut pad_outputs = [0.0f32; NUM_PADS];

            for (i, pad) in self.pads.iter_mut().enumerate() {
                let mut s = pad.tick(sr);
                // Add synth output if present
                if let Some(ref mut synth) = self.synths[i] {
                    s += synth.tick(sr);
                }
                pad_outputs[i] = s;
                // Track per-pad peak level
                let abs = s.abs();
                if abs > self.pad_peaks[i] {
                    self.pad_peaks[i] = abs;
                } else {
                    self.pad_peaks[i] *= decay;
                }
            }

            // Apply sidechain compression
            for target in 0..NUM_PADS {
                if let Some((source, amount, env)) = &mut self.sidechain[target] {
                    let source_level = pad_outputs[*source].abs();
                    // Envelope follower
                    if source_level > *env {
                        *env = source_level;
                    } else {
                        *env *= 1.0 - (8.0 / sr as f32);
                    }
                    let duck = 1.0 - (*env * *amount).min(1.0);
                    pad_outputs[target] *= duck;
                }
            }

            // Mix to stereo with per-channel sends
            let (mut left, mut right) = (0.0f32, 0.0f32);
            let mut reverb_bus = 0.0f32;
            let mut delay_bus = 0.0f32;
            for (i, &s) in pad_outputs.iter().enumerate() {
                if s.abs() < 1e-10 { continue; }
                let pad = &self.pads[i];
                let pan_l = ((1.0 - pad.pan) * 0.5).sqrt();
                let pan_r = ((1.0 + pad.pan) * 0.5).sqrt();
                left += s * pan_l;
                right += s * pan_r;
                // Per-channel sends
                reverb_bus += s * pad.reverb_send;
                delay_bus += s * pad.delay_send;
            }

            // Gross Beat effect on master
            let (gb_l, gb_r) = self.gross_beat.process(left, right, sr);
            left = gb_l;
            right = gb_r;

            // Master volume
            left *= self.master_vol;
            right *= self.master_vol;

            // Delay (stereo) — fed by per-channel sends + master send
            let delay_input_l = left * self.delay_mix + delay_bus;
            let delay_input_r = right * self.delay_mix + delay_bus;
            let (dl, dr) = self.delay.process(delay_input_l, delay_input_r);
            left += dl;
            right += dr;

            // Reverb — fed by per-channel sends + master send
            let reverb_input = (left + right) * 0.5 * self.reverb_mix + reverb_bus;
            let rv = self.reverb.process(reverb_input);
            left += rv * 0.8;
            right += rv;

            // Stereo width
            if (self.stereo_width - 1.0).abs() > 0.01 {
                let mid = (left + right) * 0.5;
                let side = (left - right) * 0.5;
                let widened_side = side * self.stereo_width;
                left = mid + widened_side;
                right = mid - widened_side;
            }

            // Metronome
            if self.metronome_env > 0.001 {
                let met_freq = if self.seq.current_step % 4 == 0 && self.seq.current_step == 0 { 1200.0 } else { 800.0 };
                self.metronome_phase += met_freq / sr;
                let click = (self.metronome_phase * TWO_PI).sin() as f32 * self.metronome_env as f32 * 0.3;
                left += click;
                right += click;
                self.metronome_env *= (-1.0 / (sr * 0.01)).exp();
            }

            // Master filter
            left = self.filter_l.tick(left);
            right = self.filter_r.tick(right);

            // Tape saturation (subtle warmth)
            left = tape_saturate(left);
            right = tape_saturate(right);

            // Master enhancer (Soundgoodizer-style multiband saturation + compression)
            if self.enhancer_amount > 0.01 {
                let amt = self.enhancer_amount;
                let mono = (left + right) * 0.5;

                // Split into 3 bands (approximate with single-pole filters)
                let low = self.enhancer_lp.tick(mono);
                let high_input = self.enhancer_hp.tick(mono);
                let mid = mono - low - high_input;

                // Per-band envelope following (for compression)
                let att = 0.01f32;
                let rel = 0.995f32;
                self.enhancer_low_env = if low.abs() > self.enhancer_low_env {
                    low.abs() * att + self.enhancer_low_env * (1.0 - att)
                } else { self.enhancer_low_env * rel };
                self.enhancer_mid_env = if mid.abs() > self.enhancer_mid_env {
                    mid.abs() * att + self.enhancer_mid_env * (1.0 - att)
                } else { self.enhancer_mid_env * rel };
                self.enhancer_high_env = if high_input.abs() > self.enhancer_high_env {
                    high_input.abs() * att + self.enhancer_high_env * (1.0 - att)
                } else { self.enhancer_high_env * rel };

                // Compress + saturate each band
                let compress = |signal: f32, env: f32| -> f32 {
                    if env > 0.01 { signal / (1.0 + env * amt * 2.0) } else { signal }
                };
                let saturate = |x: f32| -> f32 { (x * (1.0 + amt * 3.0)).tanh() };

                let low_proc = saturate(compress(low, self.enhancer_low_env)) * (1.0 + amt * 0.3);
                let mid_proc = saturate(compress(mid, self.enhancer_mid_env));
                let high_proc = saturate(compress(high_input, self.enhancer_high_env)) * (1.0 + amt * 0.5);

                // Recombine
                let enhanced = low_proc + mid_proc + high_proc;
                let blend = amt;
                left = left * (1.0 - blend) + enhanced * blend;
                right = right * (1.0 - blend) + enhanced * blend;

                // Subtle stereo widening on highs
                let side = (left - right) * 0.5;
                left += side * amt * 0.2;
                right -= side * amt * 0.2;
            }

            // Soft clip (final limiter)
            frame[0] = soft_clip(left);
            frame[1] = soft_clip(right);

            // Track master peak
            let mono_out = (frame[0].abs() + frame[1].abs()) * 0.5;
            if mono_out > self.master_peak {
                self.master_peak = mono_out;
            } else {
                self.master_peak *= decay;
            }

            // Spectrum analyzer
            self.analyzer.push((frame[0] + frame[1]) * 0.5);

            // Record output
            self.recorder.push(frame[0], frame[1]);
        }

        // Write peak levels to shared state (once per buffer)
        for i in 0..NUM_PADS {
            self.shared.set_pad_level(i, self.pad_peaks[i]);
        }
        self.shared.set_master_level(self.master_peak);
    }

    fn handle_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play => {
                self.seq.playing = true;
            }
            Cmd::Stop => {
                self.seq.playing = false;
                self.seq.current_step = 0;
                self.seq.counter = 0.0;
                self.shared.step.store(-1, Ordering::Relaxed);
            }
            Cmd::SetBpm(bpm) => {
                self.seq.bpm = bpm;
                self.delay.set_time(60.0 / bpm as f64 / 2.0);
            }
            Cmd::SetSwing(s) => self.seq.swing = s,
            Cmd::SetSteps(n) => self.seq.num_steps = n.min(MAX_STEPS),
            Cmd::SetCell { pad, step, vel } => {
                if pad < NUM_PADS && step < MAX_STEPS {
                    self.seq.pattern[pad][step] = vel;
                }
            }
            Cmd::SetFullPattern(pat) => {
                for (i, row) in pat.iter().enumerate() {
                    if i >= NUM_PADS {
                        break;
                    }
                    for (j, &v) in row.iter().enumerate() {
                        if j >= MAX_STEPS {
                            break;
                        }
                        self.seq.pattern[i][j] = v;
                    }
                }
            }
            Cmd::TriggerPad(idx, vel) => {
                if idx < NUM_PADS {
                    self.pads[idx].trigger(vel);
                }
            }
            Cmd::SetPadVol(i, v) => {
                if i < NUM_PADS {
                    self.pads[i].vol = v;
                }
            }
            Cmd::SetPadPan(i, v) => {
                if i < NUM_PADS {
                    self.pads[i].pan = v;
                }
            }
            Cmd::SetPadPitch(i, v) => {
                if i < NUM_PADS {
                    self.pads[i].pitch = v;
                }
            }
            Cmd::SetPadFilter(i, freq) => {
                if i < NUM_PADS {
                    self.pads[i].filter = Biquad::lowpass(freq as f64, self.sr);
                }
            }
            Cmd::SetPadReverse(i, r) => {
                if i < NUM_PADS {
                    self.pads[i].reversed = r;
                }
            }
            Cmd::SetPadTrim(i, s, e) => {
                if i < NUM_PADS {
                    self.pads[i].trim_start = s;
                    self.pads[i].trim_end = e;
                }
            }
            Cmd::SetPadMute(i, m) => {
                if i < NUM_PADS {
                    self.pads[i].muted = m;
                }
            }
            Cmd::SetPadSolo(i, s) => {
                if i < NUM_PADS {
                    self.pads[i].soloed = s;
                }
            }
            Cmd::SetMasterVol(v) => self.master_vol = v,
            Cmd::SetMasterFilter(freq) => {
                self.filter_l = Biquad::lowpass(freq as f64, self.sr);
                self.filter_r = Biquad::lowpass(freq as f64, self.sr);
            }
            Cmd::SetReverb(v) => self.reverb_mix = v,
            Cmd::SetDelay(v) => self.delay_mix = v,
            Cmd::LoadSample {
                pad,
                data,
                original_sr,
            } => {
                if pad < NUM_PADS {
                    // Resample if necessary
                    let resampled = if original_sr as f64 != self.sr {
                        let ratio = self.sr / original_sr as f64;
                        let new_len = (data.len() as f64 * ratio) as usize;
                        let mut out = Vec::with_capacity(new_len);
                        for i in 0..new_len {
                            let src_pos = i as f64 / ratio;
                            let idx = src_pos as usize;
                            let frac = src_pos - idx as f64;
                            let a = data.get(idx).copied().unwrap_or(0.0);
                            let b = data.get(idx + 1).copied().unwrap_or(a);
                            out.push((a as f64 * (1.0 - frac) + b as f64 * frac) as f32);
                        }
                        Arc::new(out)
                    } else {
                        data
                    };
                    self.pads[pad].sample = Some(SampleData {
                        data: resampled,
                        position: 0.0,
                        playing: false,
                        velocity: 1.0,
                        rate: 1.0,
                        reversed: false,
                        trim_start_sample: 0,
                        trim_end_sample: 0, // will be set on trigger
                    });
                    self.pads[pad].voice = Voice::Silent;
                }
            }
            Cmd::RemoveSample(i) => {
                if i < NUM_PADS {
                    self.pads[i].sample = None;
                    self.pads[i].voice = default_voice(i, self.sr);
                }
            }
            Cmd::SetPadSynth(i, params) => {
                if i < NUM_PADS {
                    let mut synth = SubSynth::new();
                    synth.params = params;
                    self.synths[i] = Some(synth);
                    // Clear drum voice when synth is assigned
                    self.pads[i].voice = Voice::Silent;
                    self.pads[i].sample = None;
                }
            }
            Cmd::NoteOn { pad, note, velocity } => {
                if pad < NUM_PADS {
                    if let Some(ref mut s) = self.synths[pad] {
                        s.note_on(note, velocity);
                    }
                }
            }
            Cmd::NoteOff { pad, note } => {
                if pad < NUM_PADS {
                    if let Some(ref mut s) = self.synths[pad] {
                        s.note_off(note);
                    }
                }
            }
            Cmd::SetNotePattern { pad, pattern } => {
                if pad < NUM_PADS {
                    self.note_patterns[pad] = pattern;
                }
            }
            Cmd::StartRecording => {
                self.recorder.start();
                self.shared.is_recording.store(true, Ordering::Relaxed);
            }
            Cmd::StopRecording => {
                self.recorder.stop();
                self.shared.is_recording.store(false, Ordering::Relaxed);
                // Export to file
                let desktop = dirs_next_desktop();
                let filename = format!("beatforge-{}.wav",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs());
                let path = desktop.join(filename);
                if let Err(e) = self.recorder.export_wav(&path) {
                    eprintln!("Export error: {e}");
                } else {
                    eprintln!("Exported to: {}", path.display());
                }
            }
            Cmd::SetChokeGroup(i, group) => {
                if i < NUM_PADS { self.pads[i].choke_group = group; }
            }
            Cmd::SetMetronome(on) => {
                self.metronome_on = on;
            }
            Cmd::CopyBank { from: _, to: _ } => {
                // Bank management handled on UI side; audio just plays the active pattern
            }
            Cmd::SetPadEq(i, params) => {
                if i < NUM_PADS {
                    self.pads[i].eq.set_params(&params);
                }
            }
            Cmd::SetAutomation(data) => {
                self.automation = data;
            }
            Cmd::SetSidechain { source, target, amount } => {
                if target < NUM_PADS && source < NUM_PADS {
                    self.sidechain[target] = Some((source, amount, 0.0));
                }
            }
            Cmd::ClearSidechain(target) => {
                if target < NUM_PADS {
                    self.sidechain[target] = None;
                }
            }
            Cmd::SetGrossBeat(mode) => {
                self.gross_beat.set_mode(mode);
            }
            Cmd::SetStepProbability(prob) => {
                self.step_probability = prob;
            }
            Cmd::SetPadReverbSend(i, v) => {
                if i < NUM_PADS { self.pads[i].reverb_send = v; }
            }
            Cmd::SetPadDelaySend(i, v) => {
                if i < NUM_PADS { self.pads[i].delay_send = v; }
            }
            Cmd::SetStereoWidth(w) => {
                self.stereo_width = w;
            }
            Cmd::SetEnhancer(amt) => {
                self.enhancer_amount = amt;
            }
            Cmd::AllNotesOff => {
                for synth in &mut self.synths {
                    if let Some(ref mut s) = synth {
                        s.all_notes_off();
                    }
                }
                // Also silence all pad voices
                for pad in &mut self.pads {
                    pad.voice.silence();
                    if let Some(ref mut s) = pad.sample {
                        s.playing = false;
                    }
                }
            }
            Cmd::SetPadDistortion { pad, drive, mix } => {
                if pad < NUM_PADS {
                    self.pads[pad].fx.distortion.drive = drive;
                    self.pads[pad].fx.distortion.mix = mix;
                }
            }
            Cmd::SetPadBitcrush { pad, bits, rate, mix } => {
                if pad < NUM_PADS {
                    self.pads[pad].fx.bitcrusher.bits = bits;
                    self.pads[pad].fx.bitcrusher.rate = rate;
                    self.pads[pad].fx.bitcrusher.mix = mix;
                }
            }
            Cmd::SetPadChorus { pad, rate, depth, mix } => {
                if pad < NUM_PADS {
                    self.pads[pad].fx.chorus.rate = rate;
                    self.pads[pad].fx.chorus.depth = depth;
                    self.pads[pad].fx.chorus.mix = mix;
                }
            }
            Cmd::SetPadPhaser { pad, rate, depth, feedback, mix } => {
                if pad < NUM_PADS {
                    self.pads[pad].fx.phaser.rate = rate;
                    self.pads[pad].fx.phaser.depth = depth;
                    self.pads[pad].fx.phaser.feedback = feedback;
                    self.pads[pad].fx.phaser.mix = mix;
                }
            }
            Cmd::SetPadAttack(i, v) => { if i < NUM_PADS { self.pads[i].amp_attack = v; } }
            Cmd::SetPadRelease(i, v) => { if i < NUM_PADS { self.pads[i].amp_release = v; } }
            Cmd::SetPadLoopMode(i, m) => { if i < NUM_PADS { self.pads[i].loop_mode = m; } }
            Cmd::SetDrumParams(i, tune, decay, color) => {
                if i < NUM_PADS {
                    self.pads[i].drum_tune = tune;
                    self.pads[i].drum_decay = decay;
                    self.pads[i].drum_color = color;
                    // Apply to the voice if it's a kick
                    if let Voice::Kick(ref mut k) = self.pads[i].voice {
                        k.set_params(tune, decay, color);
                    }
                }
            }
            Cmd::SetLoopRegion(start, end) => {
                self.seq.loop_start = start;
                self.seq.loop_end = end;
            }
            Cmd::SetSongMode(patterns) => {
                self.seq.song_patterns = patterns;
                self.seq.song_position = 0;
                self.seq.song_mode = true;
                self.seq.load_song_pattern();
            }
            Cmd::ClearSongMode => {
                self.seq.song_mode = false;
                self.seq.song_patterns.clear();
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════
//  SEQUENCER
// ═══════════════════════════════════════════════════════════

struct Sequencer {
    playing: bool,
    bpm: f32,
    swing: f32,
    current_step: usize,
    num_steps: usize,
    counter: f64,
    next_step_time: f64,
    pattern: [[u8; MAX_STEPS]; NUM_PADS],
    last_triggers: Option<Vec<u8>>,
    // Loop region
    loop_start: Option<usize>,
    loop_end: Option<usize>,
    // Song mode
    song_patterns: Vec<Vec<Vec<u8>>>,
    song_position: usize,
    song_mode: bool,
}

impl Sequencer {
    fn new() -> Self {
        Sequencer {
            playing: false,
            bpm: 90.0,
            swing: 0.0,
            current_step: 0,
            num_steps: 16,
            counter: 0.0,
            next_step_time: 0.0,
            pattern: [[0u8; MAX_STEPS]; NUM_PADS],
            last_triggers: None,
            loop_start: None,
            loop_end: None,
            song_patterns: Vec::new(),
            song_position: 0,
            song_mode: false,
        }
    }

    fn tick(&mut self, sr: f64) -> Option<Vec<u8>> {
        if !self.playing {
            return None;
        }

        // First call — trigger immediately
        if self.next_step_time <= 0.0 {
            self.recalc_step_time(sr);
            return Some(self.collect_triggers());
        }

        self.counter += 1.0;
        if self.counter >= self.next_step_time {
            self.counter -= self.next_step_time;
            self.current_step += 1;

            // Check if we've hit the loop end or pattern end
            let effective_end = self.loop_end.map(|e| e + 1).unwrap_or(self.num_steps);
            if self.current_step >= effective_end {
                self.current_step = self.loop_start.unwrap_or(0);
                // Song mode: advance to next pattern
                if self.song_mode && !self.song_patterns.is_empty() {
                    self.song_position += 1;
                    if self.song_position >= self.song_patterns.len() {
                        self.song_position = 0; // loop
                    }
                    self.load_song_pattern();
                }
            }

            self.recalc_step_time(sr);
            let triggers = self.collect_triggers();
            self.last_triggers = Some(triggers.clone());
            Some(triggers)
        } else {
            None
        }
    }

    fn load_song_pattern(&mut self) {
        if let Some(pat) = self.song_patterns.get(self.song_position) {
            for (i, row) in pat.iter().enumerate() {
                if i >= NUM_PADS { break; }
                for (j, &v) in row.iter().enumerate() {
                    if j >= MAX_STEPS { break; }
                    self.pattern[i][j] = v;
                }
            }
        }
    }

    fn recalc_step_time(&mut self, sr: f64) {
        let base = sr * 60.0 / self.bpm as f64 / 4.0;
        let swing_amt = self.swing as f64 / 200.0;
        // Odd steps are delayed
        self.next_step_time = if self.current_step % 2 == 0 {
            base * (1.0 + swing_amt)
        } else {
            base * (1.0 - swing_amt).max(0.1)
        };
    }

    fn collect_triggers(&self) -> Vec<u8> {
        (0..NUM_PADS)
            .map(|p| {
                if self.current_step < self.num_steps {
                    self.pattern[p][self.current_step]
                } else {
                    0
                }
            })
            .collect()
    }
}

// ═══════════════════════════════════════════════════════════
//  PAD
// ═══════════════════════════════════════════════════════════

struct Pad {
    voice: Voice,
    sample: Option<SampleData>,
    filter: Biquad,
    eq: ChannelEq,
    fx: InsertFxChain,
    vol: f32,
    pan: f32,
    pitch: f32,
    muted: bool,
    soloed: bool,
    reversed: bool,
    trim_start: f32,
    trim_end: f32,
    choke_group: u8,
    reverb_send: f32,
    delay_send: f32,
    // Per-pad sample envelope
    amp_attack: f32,
    amp_release: f32,
    // Drum voice tuning (for synth pads only)
    drum_tune: f32,    // semitones offset (-24 to +24)
    drum_decay: f32,   // decay time multiplier (0.1 to 3.0, 1.0 = default)
    drum_color: f32,   // tonal color / noise blend (0 = tonal, 1 = noisy)
    amp_env_value: f64, // current envelope level
    amp_env_stage: u8,  // 0=idle, 1=attack, 2=sustain, 3=release
    // Sample loop mode
    loop_mode: bool,   // true = loop, false = one-shot
}

impl Pad {
    fn new(index: usize, sr: f64) -> Self {
        // Default choke groups: HH-C (index 2) and HH-O (index 3) choke each other
        let choke = match index { 2 | 3 => 1, _ => 0 };
        Pad {
            voice: default_voice(index, sr),
            sample: None,
            filter: Biquad::lowpass(20000.0, sr),
            eq: ChannelEq::new(sr),
            fx: InsertFxChain::new(sr),
            vol: 0.7,
            pan: 0.0,
            pitch: 0.0,
            muted: false,
            soloed: false,
            reversed: false,
            trim_start: 0.0,
            trim_end: 1.0,
            choke_group: choke,
            reverb_send: 0.0,
            delay_send: 0.0,
            amp_attack: 0.001,
            amp_release: 0.01,
            drum_tune: 0.0,
            drum_decay: 1.0,
            drum_color: 0.5,
            amp_env_value: 0.0,
            amp_env_stage: 0,
            loop_mode: false,
        }
    }

    fn trigger(&mut self, vel: f32) {
        // Start amplitude envelope
        self.amp_env_stage = 1; // attack
        self.amp_env_value = 0.0;

        if let Some(ref mut s) = self.sample {
            let rate = 2.0f64.powf(self.pitch as f64 / 12.0);
            let len = s.data.len() as f64;
            s.playing = true;
            s.velocity = vel;
            if self.reversed {
                s.position = len * self.trim_end as f64 - 1.0;
            } else {
                s.position = len * self.trim_start as f64;
            }
            s.rate = rate;
            s.reversed = self.reversed;
            s.trim_start_sample = (len * self.trim_start as f64) as usize;
            s.trim_end_sample = (len * self.trim_end as f64) as usize;
        } else {
            self.voice.trigger(vel);
        }
    }

    fn tick(&mut self, sr: f64) -> f32 {
        let raw = if let Some(ref mut s) = self.sample {
            let out = s.tick();
            // Handle loop mode
            if !s.playing && self.loop_mode && self.amp_env_stage > 0 {
                let len = s.data.len() as f64;
                s.playing = true;
                s.position = len * self.trim_start as f64;
            }
            out
        } else {
            self.voice.tick(sr)
        };
        // Amplitude envelope
        match self.amp_env_stage {
            1 => { // Attack
                let rate = if self.amp_attack > 0.001 { 1.0 / (self.amp_attack as f64 * sr) } else { 1.0 };
                self.amp_env_value = (self.amp_env_value + rate).min(1.0);
                if self.amp_env_value >= 1.0 { self.amp_env_stage = 2; }
            }
            2 => { /* Sustain — hold at 1.0 */ }
            3 => { // Release
                let rate = if self.amp_release > 0.001 { 1.0 / (self.amp_release as f64 * sr) } else { 1.0 };
                self.amp_env_value = (self.amp_env_value - rate).max(0.0);
                if self.amp_env_value <= 0.0 { self.amp_env_stage = 0; }
            }
            _ => {} // Idle
        }
        let env_gain = self.amp_env_value as f32;

        let filtered = self.filter.tick(raw * self.vol * env_gain);
        let fxed = self.fx.process(filtered);
        self.eq.process(fxed)
    }
}

// ═══════════════════════════════════════════════════════════
//  SAMPLE PLAYER
// ═══════════════════════════════════════════════════════════

struct SampleData {
    data: Arc<Vec<f32>>,
    position: f64,
    playing: bool,
    velocity: f32,
    rate: f64,
    reversed: bool,
    trim_start_sample: usize,
    trim_end_sample: usize,
}

impl SampleData {
    fn tick(&mut self) -> f32 {
        if !self.playing {
            return 0.0;
        }

        let idx = self.position as usize;
        if idx >= self.data.len() {
            self.playing = false;
            return 0.0;
        }

        // Bounds check
        if !self.reversed && idx >= self.trim_end_sample {
            self.playing = false;
            return 0.0;
        }
        if self.reversed && idx <= self.trim_start_sample {
            self.playing = false;
            return 0.0;
        }

        // Linear interpolation
        let frac = self.position - idx as f64;
        let a = self.data[idx];
        let b = self.data.get(idx + 1).copied().unwrap_or(a);
        let out = (a as f64 * (1.0 - frac) + b as f64 * frac) as f32;

        if self.reversed {
            self.position -= self.rate;
        } else {
            self.position += self.rate;
        }

        out * self.velocity
    }
}

// ═══════════════════════════════════════════════════════════
//  DRUM VOICES
// ═══════════════════════════════════════════════════════════

fn default_voice(index: usize, sr: f64) -> Voice {
    match index {
        0 => Voice::Kick(KickOsc::new(sr)),
        1 => Voice::Snare(SnareOsc::new(sr)),
        2 => Voice::HiHat(HiHatOsc::new(sr, false)),
        3 => Voice::HiHat(HiHatOsc::new(sr, true)),
        4 => Voice::Clap(ClapOsc::new(sr)),
        5 => Voice::Rim(RimOsc::new(sr)),
        6 => Voice::Tom(TomOsc::new(sr, 200.0, 100.0)),
        7 => Voice::Tom(TomOsc::new(sr, 120.0, 55.0)),
        8 => Voice::Perc(PercOsc::new(sr)),
        9 => Voice::Cowbell(CowbellOsc::new(sr)),
        _ => Voice::Silent,
    }
}

enum Voice {
    Kick(KickOsc),
    Snare(SnareOsc),
    HiHat(HiHatOsc),
    Clap(ClapOsc),
    Rim(RimOsc),
    Tom(TomOsc),
    Perc(PercOsc),
    Cowbell(CowbellOsc),
    Silent,
}

impl Voice {
    fn silence(&mut self) {
        match self {
            Voice::Kick(o) => o.env = 0.0,
            Voice::Snare(o) => { o.body_env = 0.0; o.noise_env = 0.0; }
            Voice::HiHat(o) => o.env = 0.0,
            Voice::Clap(o) => o.env = 0.0,
            Voice::Rim(o) => { o.env = 0.0; o.noise_env = 0.0; }
            Voice::Tom(o) => o.env = 0.0,
            Voice::Perc(o) => o.env = 0.0,
            Voice::Cowbell(o) => o.env = 0.0,
            Voice::Silent => {}
        }
    }

    fn trigger(&mut self, vel: f32) {
        match self {
            Voice::Kick(o) => o.trigger(vel),
            Voice::Snare(o) => o.trigger(vel),
            Voice::HiHat(o) => o.trigger(vel),
            Voice::Clap(o) => o.trigger(vel),
            Voice::Rim(o) => o.trigger(vel),
            Voice::Tom(o) => o.trigger(vel),
            Voice::Perc(o) => o.trigger(vel),
            Voice::Cowbell(o) => o.trigger(vel),
            Voice::Silent => {}
        }
    }

    fn tick(&mut self, sr: f64) -> f32 {
        match self {
            Voice::Kick(o) => o.tick(sr),
            Voice::Snare(o) => o.tick(sr),
            Voice::HiHat(o) => o.tick(sr),
            Voice::Clap(o) => o.tick(sr),
            Voice::Rim(o) => o.tick(sr),
            Voice::Tom(o) => o.tick(sr),
            Voice::Perc(o) => o.tick(sr),
            Voice::Cowbell(o) => o.tick(sr),
            Voice::Silent => 0.0,
        }
    }
}

// ── Kick ───────────────────────────────────────────────────
struct KickOsc {
    phase: f64,
    env: f64,
    pitch_env: f64,
    click_env: f64,
    amp_decay: f64,
    pitch_decay: f64,
    sr: f64,
    // Tunable parameters (set from pad)
    base_freq: f64,     // base frequency (default 42Hz)
    pitch_depth: f64,   // pitch envelope amount (default 130Hz)
    decay_mult: f64,    // decay time multiplier
    click_amount: f64,  // transient click (0-1)
    noise_state: u32,
}

impl KickOsc {
    fn new(sr: f64) -> Self {
        KickOsc {
            phase: 0.0,
            env: 0.0,
            pitch_env: 0.0,
            click_env: 0.0,
            amp_decay: (-1.0 / (sr * 0.3)).exp(),
            pitch_decay: (-1.0 / (sr * 0.04)).exp(),
            sr,
            base_freq: 42.0,
            pitch_depth: 130.0,
            decay_mult: 1.0,
            click_amount: 0.3,
            noise_state: 99999,
        }
    }

    fn set_params(&mut self, tune_semi: f32, decay_mult: f32, color: f32) {
        let ratio = 2.0f64.powf(tune_semi as f64 / 12.0);
        self.base_freq = 42.0 * ratio;
        self.pitch_depth = 130.0 * ratio;
        self.decay_mult = decay_mult as f64;
        self.amp_decay = (-1.0 / (self.sr * 0.3 * self.decay_mult)).exp();
        self.click_amount = color as f64; // color controls click amount
    }

    fn trigger(&mut self, vel: f32) {
        self.phase = 0.0;
        self.env = vel as f64;
        self.pitch_env = 1.0;
        self.click_env = vel as f64;
    }

    fn tick(&mut self, sr: f64) -> f32 {
        if self.env < 0.001 && self.click_env < 0.001 {
            return 0.0;
        }
        let freq = self.base_freq + self.pitch_depth * self.pitch_env;
        self.phase += freq / sr;
        let out = (self.phase * TWO_PI).sin() * self.env;
        self.pitch_env *= self.pitch_decay;
        self.env *= self.amp_decay;
        // Sub harmonics for body
        let sub = (self.phase * 0.5 * TWO_PI).sin() * self.env * 0.3;
        // Transient click (noise burst)
        let click = if self.click_env > 0.001 {
            self.noise_state ^= self.noise_state << 13;
            self.noise_state ^= self.noise_state >> 17;
            self.noise_state ^= self.noise_state << 5;
            let noise = (self.noise_state as f64 / u32::MAX as f64) * 2.0 - 1.0;
            let c = noise * self.click_env * self.click_amount;
            self.click_env *= (-1.0 / (sr * 0.003)).exp(); // 3ms click
            c
        } else { 0.0 };
        (out + sub + click) as f32
    }
}

// ── Snare ──────────────────────────────────────────────────
struct SnareOsc {
    body_phase: f64,
    body_env: f64,
    noise_env: f64,
    body_decay: f64,
    noise_decay: f64,
    noise_state: u32,
}

impl SnareOsc {
    fn new(sr: f64) -> Self {
        SnareOsc {
            body_phase: 0.0,
            body_env: 0.0,
            noise_env: 0.0,
            body_decay: (-1.0 / (sr * 0.08)).exp(),
            noise_decay: (-1.0 / (sr * 0.12)).exp(),
            noise_state: 12345,
        }
    }

    fn trigger(&mut self, vel: f32) {
        self.body_phase = 0.0;
        self.body_env = vel as f64 * 0.6;
        self.noise_env = vel as f64;
    }

    fn tick(&mut self, sr: f64) -> f32 {
        if self.body_env < 0.001 && self.noise_env < 0.001 {
            return 0.0;
        }
        // Body (sine at ~200Hz)
        self.body_phase += 200.0 / sr;
        let body = (self.body_phase * TWO_PI).sin() * self.body_env;
        self.body_env *= self.body_decay;

        // Noise
        let noise = white_noise(&mut self.noise_state) as f64 * self.noise_env * 0.7;
        self.noise_env *= self.noise_decay;

        (body + noise) as f32
    }
}

// ── Hi-Hat ─────────────────────────────────────────────────
struct HiHatOsc {
    env: f64,
    decay: f64,
    phases: [f64; 6],
    freqs: [f64; 6],
    noise_state: u32,
    _open: bool,
}

impl HiHatOsc {
    fn new(sr: f64, open: bool) -> Self {
        let decay_time = if open { 0.25 } else { 0.04 };
        HiHatOsc {
            env: 0.0,
            decay: (-1.0 / (sr * decay_time)).exp(),
            phases: [0.0; 6],
            freqs: [205.3, 304.4, 369.6, 522.7, 540.5, 587.2],
            noise_state: 67890,
            _open: open,
        }
    }

    fn trigger(&mut self, vel: f32) {
        self.env = vel as f64 * 0.5;
        self.phases = [0.0; 6];
    }

    fn tick(&mut self, sr: f64) -> f32 {
        if self.env < 0.001 {
            return 0.0;
        }
        // Metallic component: sum of inharmonic square waves
        let mut metallic = 0.0f64;
        for i in 0..6 {
            self.phases[i] += self.freqs[i] / sr;
            metallic += if (self.phases[i] * TWO_PI).sin() > 0.0 {
                1.0
            } else {
                -1.0
            };
        }
        metallic /= 6.0;

        // Filtered noise
        let noise = white_noise(&mut self.noise_state) as f64 * 0.3;

        let out = (metallic * 0.7 + noise * 0.3) * self.env;
        self.env *= self.decay;
        out as f32
    }
}

// ── Clap ───────────────────────────────────────────────────
struct ClapOsc {
    env: f64,
    decay: f64,
    burst_counter: f64,
    burst_env: f64,
    noise_state: u32,
}

impl ClapOsc {
    fn new(sr: f64) -> Self {
        ClapOsc {
            env: 0.0,
            decay: (-1.0 / (sr * 0.15)).exp(),
            burst_counter: 0.0,
            burst_env: 0.0,
            noise_state: 11111,
        }
    }

    fn trigger(&mut self, vel: f32) {
        self.env = vel as f64;
        self.burst_counter = 0.0;
        self.burst_env = 1.0;
    }

    fn tick(&mut self, sr: f64) -> f32 {
        if self.env < 0.001 {
            return 0.0;
        }
        let noise = white_noise(&mut self.noise_state) as f64;

        // Multiple short bursts for clap texture
        self.burst_counter += 1.0;
        let burst_period = sr * 0.01; // 10ms bursts
        if self.burst_counter < burst_period * 4.0 {
            let burst_phase = (self.burst_counter % burst_period) / burst_period;
            self.burst_env = if burst_phase < 0.3 { 1.0 } else { 0.0 };
        }

        let out = noise * self.env * (self.burst_env * 0.5 + 0.5);
        self.env *= self.decay;
        out as f32
    }
}

// ── Rim ────────────────────────────────────────────────────
struct RimOsc {
    phase: f64,
    env: f64,
    decay: f64,
    noise_env: f64,
    noise_decay: f64,
    noise_state: u32,
}

impl RimOsc {
    fn new(sr: f64) -> Self {
        RimOsc {
            phase: 0.0,
            env: 0.0,
            decay: (-1.0 / (sr * 0.015)).exp(),
            noise_env: 0.0,
            noise_decay: (-1.0 / (sr * 0.005)).exp(),
            noise_state: 22222,
        }
    }

    fn trigger(&mut self, vel: f32) {
        self.phase = 0.0;
        self.env = vel as f64 * 0.7;
        self.noise_env = vel as f64;
    }

    fn tick(&mut self, sr: f64) -> f32 {
        if self.env < 0.001 && self.noise_env < 0.001 {
            return 0.0;
        }
        self.phase += 820.0 / sr;
        let tone = (self.phase * TWO_PI).sin() * self.env;
        self.env *= self.decay;

        let click = white_noise(&mut self.noise_state) as f64 * self.noise_env * 0.5;
        self.noise_env *= self.noise_decay;

        (tone + click) as f32
    }
}

// ── Tom ────────────────────────────────────────────────────
struct TomOsc {
    phase: f64,
    env: f64,
    pitch_env: f64,
    amp_decay: f64,
    pitch_decay: f64,
    start_freq: f64,
    end_freq: f64,
}

impl TomOsc {
    fn new(sr: f64, start: f64, end: f64) -> Self {
        TomOsc {
            phase: 0.0,
            env: 0.0,
            pitch_env: 1.0,
            amp_decay: (-1.0 / (sr * 0.2)).exp(),
            pitch_decay: (-1.0 / (sr * 0.06)).exp(),
            start_freq: start,
            end_freq: end,
        }
    }

    fn trigger(&mut self, vel: f32) {
        self.phase = 0.0;
        self.env = vel as f64;
        self.pitch_env = 1.0;
    }

    fn tick(&mut self, sr: f64) -> f32 {
        if self.env < 0.001 {
            return 0.0;
        }
        let freq = self.end_freq + (self.start_freq - self.end_freq) * self.pitch_env;
        self.phase += freq / sr;
        let out = (self.phase * TWO_PI).sin() * self.env;
        self.pitch_env *= self.pitch_decay;
        self.env *= self.amp_decay;
        out as f32
    }
}

// ── Perc (metallic) ────────────────────────────────────────
struct PercOsc {
    phases: [f64; 3],
    env: f64,
    decay: f64,
}

impl PercOsc {
    fn new(sr: f64) -> Self {
        PercOsc {
            phases: [0.0; 3],
            env: 0.0,
            decay: (-1.0 / (sr * 0.06)).exp(),
        }
    }

    fn trigger(&mut self, vel: f32) {
        self.env = vel as f64 * 0.5;
        self.phases = [0.0; 3];
    }

    fn tick(&mut self, sr: f64) -> f32 {
        if self.env < 0.001 {
            return 0.0;
        }
        let freqs = [415.3, 587.3, 845.1];
        let mut out = 0.0f64;
        for i in 0..3 {
            self.phases[i] += freqs[i] / sr;
            out += (self.phases[i] * TWO_PI).sin();
        }
        out /= 3.0;
        out *= self.env;
        self.env *= self.decay;
        out as f32
    }
}

// ── Cowbell ────────────────────────────────────────────────
struct CowbellOsc {
    phase1: f64,
    phase2: f64,
    env: f64,
    decay: f64,
}

impl CowbellOsc {
    fn new(sr: f64) -> Self {
        CowbellOsc {
            phase1: 0.0,
            phase2: 0.0,
            env: 0.0,
            decay: (-1.0 / (sr * 0.1)).exp(),
        }
    }

    fn trigger(&mut self, vel: f32) {
        self.phase1 = 0.0;
        self.phase2 = 0.0;
        self.env = vel as f64 * 0.4;
    }

    fn tick(&mut self, sr: f64) -> f32 {
        if self.env < 0.001 {
            return 0.0;
        }
        self.phase1 += 562.0 / sr;
        self.phase2 += 845.0 / sr;
        // Square waves
        let sq1 = if (self.phase1 * TWO_PI).sin() > 0.0 {
            1.0
        } else {
            -1.0
        };
        let sq2 = if (self.phase2 * TWO_PI).sin() > 0.0 {
            1.0
        } else {
            -1.0
        };
        let out = (sq1 + sq2) * 0.5 * self.env;
        self.env *= self.decay;
        out as f32
    }
}

// ═══════════════════════════════════════════════════════════
//  GROSS BEAT (tape stop, half speed, stutter, gate, reverse)
// ═══════════════════════════════════════════════════════════

struct GrossBeatState {
    mode: GrossBeatMode,
    buffer: Vec<(f32, f32)>,
    write_pos: usize,
    read_pos: f64,
    rate: f64,       // playback rate (1.0 = normal)
    gate_phase: f64,
    counter: f64,
}

impl GrossBeatState {
    fn new(sr: f64) -> Self {
        let buf_size = (sr * 4.0) as usize; // 4 seconds buffer
        GrossBeatState {
            mode: GrossBeatMode::Off,
            buffer: vec![(0.0, 0.0); buf_size],
            write_pos: 0,
            read_pos: 0.0,
            rate: 1.0,
            gate_phase: 0.0,
            counter: 0.0,
        }
    }

    fn process(&mut self, left: f32, right: f32, sr: f64) -> (f32, f32) {
        let len = self.buffer.len();
        // Always write to buffer
        self.buffer[self.write_pos] = (left, right);
        self.write_pos = (self.write_pos + 1) % len;

        match self.mode {
            GrossBeatMode::Off => (left, right),

            GrossBeatMode::HalfSpeed => {
                // Read at half speed from buffer
                let idx = self.read_pos as usize % len;
                let frac = self.read_pos - self.read_pos.floor();
                let a = self.buffer[idx];
                let b = self.buffer[(idx + 1) % len];
                self.read_pos += 0.5; // half speed
                if self.read_pos >= len as f64 { self.read_pos -= len as f64; }
                let l = a.0 as f64 * (1.0 - frac) + b.0 as f64 * frac;
                let r = a.1 as f64 * (1.0 - frac) + b.1 as f64 * frac;
                (l as f32, r as f32)
            }

            GrossBeatMode::TapeStop => {
                // Gradually slow down playback (decelerate)
                self.rate *= 1.0 - (0.5 / sr); // slow decay
                self.rate = self.rate.max(0.01);
                let idx = self.read_pos as usize % len;
                let sample = self.buffer[idx];
                self.read_pos += self.rate;
                if self.read_pos >= len as f64 { self.read_pos -= len as f64; }
                (sample.0 * self.rate as f32, sample.1 * self.rate as f32)
            }

            GrossBeatMode::Gate => {
                // Rhythmic gating (1/8 note on/off)
                self.gate_phase += 1.0 / sr;
                let gate_period = 60.0 / 120.0 / 2.0; // assume ~120bpm 1/8 note
                let gate_on = (self.gate_phase % gate_period) < gate_period * 0.5;
                if gate_on { (left, right) } else { (0.0, 0.0) }
            }

            GrossBeatMode::Stutter => {
                // Repeat last 1/16 note worth of audio
                let stutter_len = (sr * 60.0 / 120.0 / 4.0) as usize; // ~1/16 at 120bpm
                self.counter += 1.0;
                let read_offset = (self.counter as usize) % stutter_len;
                let read_idx = (self.write_pos + len - stutter_len + read_offset) % len;
                self.buffer[read_idx]
            }

            GrossBeatMode::Reverse => {
                // Play buffer backwards
                if self.read_pos <= 0.0 {
                    self.read_pos = self.write_pos as f64;
                }
                self.read_pos -= 1.0;
                if self.read_pos < 0.0 { self.read_pos += len as f64; }
                let idx = self.read_pos as usize % len;
                self.buffer[idx]
            }
        }
    }

    fn set_mode(&mut self, mode: GrossBeatMode) {
        if mode != self.mode {
            self.mode = mode;
            self.rate = 1.0;
            self.read_pos = self.write_pos as f64;
            self.counter = 0.0;
            self.gate_phase = 0.0;
        }
    }
}

// ═══════════════════════════════════════════════════════════
//  DSP UTILITIES
// ═══════════════════════════════════════════════════════════

// ── Biquad filter ──────────────────────────────────────────
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn lowpass(freq: f64, sr: f64) -> Self {
        let freq = freq.min(sr * 0.49).max(20.0);
        let w0 = TWO_PI * freq / sr;
        let alpha = w0.sin() / (2.0 * 0.707);
        let cos_w0 = w0.cos();

        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Biquad {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn tick(&mut self, input: f32) -> f32 {
        let x = input as f64;
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y as f32
    }
}

// ── Delay ──────────────────────────────────────────────────
struct Delay {
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
    delay_samples: usize,
    feedback: f32,
    sr: f64,
}

impl Delay {
    fn new(sr: f64, time: f64) -> Self {
        let max_samples = (sr * 2.0) as usize; // 2 seconds max
        let delay = (sr * time) as usize;
        Delay {
            buffer_l: vec![0.0; max_samples],
            buffer_r: vec![0.0; max_samples],
            write_pos: 0,
            delay_samples: delay.min(max_samples - 1),
            feedback: 0.35,
            sr,
        }
    }

    fn set_time(&mut self, time: f64) {
        self.delay_samples = ((self.sr * time) as usize).min(self.buffer_l.len() - 1);
    }

    fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let len = self.buffer_l.len();
        let read_pos = (self.write_pos + len - self.delay_samples) % len;
        let out_l = self.buffer_l[read_pos];
        let out_r = self.buffer_r[read_pos];

        self.buffer_l[self.write_pos] = left + out_l * self.feedback;
        self.buffer_r[self.write_pos] = right + out_r * self.feedback;

        self.write_pos = (self.write_pos + 1) % len;
        (out_l, out_r)
    }
}

// ── Reverb (Schroeder) ────────────────────────────────────
struct Reverb {
    combs: [CombFilter; 8], // 8 combs for smoother diffusion
    allpass: [AllpassFilter; 4], // 4 allpasses for better density
    predelay: Vec<f32>,
    predelay_pos: usize,
    predelay_len: usize,
    dc_filter: f64, // DC offset removal
}

impl Reverb {
    fn new(sr: f64) -> Self {
        let predelay_ms = 20.0; // 20ms pre-delay
        let predelay_len = (sr * predelay_ms / 1000.0) as usize;
        Reverb {
            combs: [
                // Spread out delay times for less metallic sound (prime-ish numbers)
                CombFilter::new((sr * 0.0297) as usize, 0.84, 0.35),
                CombFilter::new((sr * 0.0371) as usize, 0.82, 0.35),
                CombFilter::new((sr * 0.0411) as usize, 0.80, 0.35),
                CombFilter::new((sr * 0.0437) as usize, 0.78, 0.35),
                CombFilter::new((sr * 0.0482) as usize, 0.76, 0.35),
                CombFilter::new((sr * 0.0513) as usize, 0.74, 0.35),
                CombFilter::new((sr * 0.0557) as usize, 0.72, 0.35),
                CombFilter::new((sr * 0.0617) as usize, 0.70, 0.35),
            ],
            allpass: [
                AllpassFilter::new((sr * 0.0051) as usize, 0.7),
                AllpassFilter::new((sr * 0.0126) as usize, 0.7),
                AllpassFilter::new((sr * 0.0100) as usize, 0.5),
                AllpassFilter::new((sr * 0.0077) as usize, 0.5),
            ],
            predelay: vec![0.0; predelay_len.max(1)],
            predelay_pos: 0,
            predelay_len,
            dc_filter: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        // Pre-delay
        let delayed = self.predelay[self.predelay_pos];
        self.predelay[self.predelay_pos] = input;
        self.predelay_pos = (self.predelay_pos + 1) % self.predelay.len();

        // Parallel combs (8 for smoother density)
        let mut sum = 0.0f32;
        for comb in &mut self.combs {
            sum += comb.process(delayed);
        }
        sum *= 0.125; // 1/8

        // Series allpasses (4 for better diffusion)
        for ap in &mut self.allpass {
            sum = ap.process(sum);
        }

        // DC offset removal (high-pass at ~5Hz)
        let dc_coeff = 0.9999;
        let out = sum as f64 - self.dc_filter;
        self.dc_filter = sum as f64 - out * dc_coeff;

        out as f32
    }
}

struct CombFilter {
    buffer: Vec<f32>,
    pos: usize,
    feedback: f32,
    damp: f32,
    prev: f32,
}

impl CombFilter {
    fn new(size: usize, feedback: f32, damp: f32) -> Self {
        CombFilter {
            buffer: vec![0.0; size.max(1)],
            pos: 0,
            feedback,
            damp,
            prev: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let out = self.buffer[self.pos];
        // Low-pass damping
        self.prev = out * (1.0 - self.damp) + self.prev * self.damp;
        self.buffer[self.pos] = input + self.prev * self.feedback;
        self.pos = (self.pos + 1) % self.buffer.len();
        out
    }
}

struct AllpassFilter {
    buffer: Vec<f32>,
    pos: usize,
    feedback: f32,
}

impl AllpassFilter {
    fn new(size: usize, feedback: f32) -> Self {
        AllpassFilter {
            buffer: vec![0.0; size.max(1)],
            pos: 0,
            feedback,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.pos];
        let out = -input + buffered;
        self.buffer[self.pos] = input + buffered * self.feedback;
        self.pos = (self.pos + 1) % self.buffer.len();
        out
    }
}

// ── Utilities ──────────────────────────────────────────────

fn white_noise(state: &mut u32) -> f32 {
    // xorshift32
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Tape saturation: subtle even-harmonic distortion for warmth
/// Uses a polynomial approximation of tape magnetization
fn tape_saturate(x: f32) -> f32 {
    if x.abs() < 0.001 { return x; }
    // Gentle S-curve that adds warmth without obvious distortion
    // f(x) = x - x³/6 (first two terms of sin approximation)
    let x_clamped = x.clamp(-1.5, 1.5);
    x_clamped - (x_clamped * x_clamped * x_clamped) / 6.0
}

fn soft_clip(x: f32) -> f32 {
    if x.abs() <= 0.5 {
        x
    } else {
        x.signum() * (1.0 - (1.0 / (1.0 + (x.abs() * 2.0 - 1.0).exp()))) * 0.5 + x.signum() * 0.5
    }
}

fn dirs_next_desktop() -> std::path::PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        std::path::PathBuf::from(home).join("Desktop")
    } else {
        std::path::PathBuf::from(".")
    }
}

// ═══════════════════════════════════════════════════════════
//  PUBLIC HELPERS
// ═══════════════════════════════════════════════════════════

/// Default pad info for UI
pub struct PadInfo {
    pub name: &'static str,
    pub color: (u8, u8, u8),
    pub has_voice: bool,
}

pub fn default_pad_info() -> Vec<PadInfo> {
    vec![
        PadInfo { name: "KICK", color: (255, 61, 90), has_voice: true },
        PadInfo { name: "SNARE", color: (0, 229, 255), has_voice: true },
        PadInfo { name: "HH-C", color: (255, 214, 0), has_voice: true },
        PadInfo { name: "HH-O", color: (255, 145, 0), has_voice: true },
        PadInfo { name: "CLAP", color: (179, 136, 255), has_voice: true },
        PadInfo { name: "RIM", color: (255, 128, 171), has_voice: true },
        PadInfo { name: "TOM-H", color: (105, 240, 174), has_voice: true },
        PadInfo { name: "TOM-L", color: (38, 166, 154), has_voice: true },
        PadInfo { name: "PERC", color: (68, 138, 255), has_voice: true },
        PadInfo { name: "COWB", color: (255, 171, 64), has_voice: true },
        PadInfo { name: "PAD 11", color: (80, 80, 90), has_voice: false },
        PadInfo { name: "PAD 12", color: (80, 80, 90), has_voice: false },
        PadInfo { name: "PAD 13", color: (80, 80, 90), has_voice: false },
        PadInfo { name: "PAD 14", color: (80, 80, 90), has_voice: false },
        PadInfo { name: "PAD 15", color: (80, 80, 90), has_voice: false },
        PadInfo { name: "PAD 16", color: (80, 80, 90), has_voice: false },
    ]
}

/// Load an audio file (WAV, MP3, FLAC, OGG, AAC) and return (mono samples, sample_rate).
/// Uses symphonia for broad format support, falls back to hound for WAV.
pub fn load_audio(path: &std::path::Path) -> Option<(Vec<f32>, u32)> {
    // Try symphonia first (handles MP3, FLAC, OGG, WAV, AAC)
    if let Some(result) = load_with_symphonia(path) {
        return Some(result);
    }
    // Fallback to hound for WAV
    load_with_hound(path)
}

/// Keep the old name as an alias
pub fn load_wav(path: &std::path::Path) -> Option<(Vec<f32>, u32)> {
    load_audio(path)
}

fn load_with_symphonia(path: &std::path::Path) -> Option<(Vec<f32>, u32)> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .ok()?;

    let mut format = probed.format;
    let track = format.default_track()?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

    let dec_opts = DecoderOptions::default();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .ok()?;

    let mut all_samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let spec = *decoded.spec();
        let num_frames = decoded.frames();
        let mut sample_buf = SampleBuffer::<f32>::new(num_frames as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        all_samples.extend_from_slice(sample_buf.samples());
    }

    if all_samples.is_empty() {
        return None;
    }

    // Downmix to mono
    let mono = downmix_to_mono(&all_samples, channels);
    Some((mono, sample_rate))
}

fn load_with_hound(path: &std::path::Path) -> Option<(Vec<f32>, u32)> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let sr = spec.sample_rate;

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max)
                .collect()
        }
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect(),
    };

    let mono = downmix_to_mono(&samples, spec.channels as usize);
    Some((mono, sr))
}

fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        samples.to_vec()
    } else {
        samples
            .chunks(channels)
            .map(|ch| ch.iter().sum::<f32>() / channels as f32)
            .collect()
    }
}

/// Compute waveform peaks for display
pub fn compute_peaks(data: &[f32], resolution: usize) -> Vec<f32> {
    if data.is_empty() {
        return vec![0.0; resolution];
    }
    let block = (data.len() / resolution).max(1);
    (0..resolution)
        .map(|i| {
            let start = i * block;
            let end = (start + block).min(data.len());
            data[start..end]
                .iter()
                .fold(0.0f32, |acc, &s| acc.max(s.abs()))
        })
        .collect()
}
