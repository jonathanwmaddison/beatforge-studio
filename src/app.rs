use crate::audio::{self, Cmd, Engine, GrossBeatMode, NUM_PADS, MAX_STEPS};
use crate::mic::MicRecorder;
use crate::midi::{MidiManager, MidiEvent, midi_note_to_pad};
use crate::synth::{SynthParams, Waveform, FilterType, LfoTarget, NotePattern};
use crate::slicer;
use crate::presets;
use crate::project::ProjectData;
use crate::automation::{AutomationData, AutoTarget};
use crate::eq::EqParams;
use crate::analyzer::NUM_BINS;
use eframe::egui::{self, *};
use std::sync::Arc;
use std::sync::atomic::Ordering;

// ── Constants ──────────────────────────────────────────────
const BANK_LABELS: [&str; 8] = ["A", "B", "C", "D", "E", "F", "G", "H"];
const STEPS_OPTIONS: [usize; 3] = [16, 32, 64];

// MPC-style keyboard mapping (bottom-left = pad 0)
const KEY_MAP: [(Key, usize); 16] = [
    (Key::Z, 0), (Key::X, 1), (Key::C, 2), (Key::V, 3),
    (Key::A, 4), (Key::S, 5), (Key::D, 6), (Key::F, 7),
    (Key::Q, 8), (Key::W, 9), (Key::E, 10), (Key::R, 11),
    (Key::Num1, 12), (Key::Num2, 13), (Key::Num3, 14), (Key::Num4, 15),
];

const KEY_LABELS: [&str; 16] = [
    "Z","X","C","V", "A","S","D","F", "Q","W","E","R", "1","2","3","4",
];

// Display order: top-left = pad 12, bottom-left = pad 0 (MPC style)
const PAD_DISPLAY: [usize; 16] = [12,13,14,15, 8,9,10,11, 4,5,6,7, 0,1,2,3];

// ── Scale definitions ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq)]
enum Scale {
    Chromatic,
    Major,
    Minor,
    Pentatonic,
    Blues,
    Dorian,
    Mixolydian,
}

impl Scale {
    fn name(&self) -> &'static str {
        match self {
            Scale::Chromatic => "CHROM",
            Scale::Major => "MAJOR",
            Scale::Minor => "MINOR",
            Scale::Pentatonic => "PENTA",
            Scale::Blues => "BLUES",
            Scale::Dorian => "DORIAN",
            Scale::Mixolydian => "MIXO",
        }
    }

    /// Returns which semitones (0-11) are in the scale
    fn intervals(&self) -> &[u8] {
        match self {
            Scale::Chromatic => &[0,1,2,3,4,5,6,7,8,9,10,11],
            Scale::Major => &[0,2,4,5,7,9,11],
            Scale::Minor => &[0,2,3,5,7,8,10],
            Scale::Pentatonic => &[0,2,4,7,9],
            Scale::Blues => &[0,3,5,6,7,10],
            Scale::Dorian => &[0,2,3,5,7,9,10],
            Scale::Mixolydian => &[0,2,4,5,7,9,10],
        }
    }

    /// Check if a MIDI note is in the scale (root = C)
    fn contains(&self, note: u8) -> bool {
        let semitone = note % 12;
        self.intervals().contains(&semitone)
    }

    /// Snap a note to the nearest scale tone
    fn snap(&self, note: u8) -> u8 {
        if self.contains(note) { return note; }
        // Find nearest scale tone
        for offset in 1..=6u8 {
            if note >= offset && self.contains(note - offset) { return note - offset; }
            if self.contains((note + offset) % 128) { return (note + offset).min(127); }
        }
        note
    }
}

// ── Presets ────────────────────────────────────────────────
fn preset_boom_bap() -> Vec<Vec<u8>> {
    let mut g = vec![vec![0u8; 16]; NUM_PADS];
    g[0] = vec![3,0,0,0,0,0,0,0,3,0,0,0,0,0,0,0];
    g[1] = vec![0,0,0,0,3,0,0,0,0,0,0,0,3,0,0,0];
    g[2] = vec![3,0,2,0,3,0,2,0,3,0,2,0,3,0,2,0];
    g[3] = vec![0,0,0,0,0,0,0,0,0,0,0,0,0,0,2,0];
    g[4] = vec![0,0,0,0,3,0,0,0,0,0,0,0,0,0,0,0];
    g
}

fn preset_trap() -> Vec<Vec<u8>> {
    let mut g = vec![vec![0u8; 16]; NUM_PADS];
    g[0] = vec![3,0,0,0,0,0,3,0,0,0,0,0,3,0,0,0];
    g[1] = vec![0,0,0,0,3,0,0,0,0,0,0,0,3,0,0,2];
    g[2] = vec![3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3];
    g[4] = vec![0,0,0,0,3,0,0,0,0,0,0,0,3,0,0,0];
    g[5] = vec![0,0,2,0,0,0,0,0,0,0,2,0,0,0,0,0];
    g
}

fn preset_house() -> Vec<Vec<u8>> {
    let mut g = vec![vec![0u8; 16]; NUM_PADS];
    g[0] = vec![3,0,0,0,3,0,0,0,3,0,0,0,3,0,0,0];
    g[1] = vec![0,0,0,0,3,0,0,0,0,0,0,0,3,0,0,0];
    g[2] = vec![0,0,3,0,0,0,3,0,0,0,3,0,0,0,3,0];
    g[4] = vec![0,0,0,0,3,0,0,0,0,0,0,0,3,0,0,0];
    g
}

// ── Theme ──────────────────────────────────────────────────
pub fn dark_theme() -> Visuals {
    let mut v = Visuals::dark();
    v.panel_fill = Color32::from_rgb(19, 19, 22);
    v.window_fill = Color32::from_rgb(19, 19, 22);
    v.extreme_bg_color = Color32::from_rgb(10, 10, 12);
    v.faint_bg_color = Color32::from_rgb(26, 26, 31);
    v.widgets.inactive.bg_fill = Color32::from_rgb(30, 30, 36);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(30, 30, 36);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_gray(120));
    v.widgets.hovered.bg_fill = Color32::from_rgb(40, 40, 48);
    v.widgets.active.bg_fill = Color32::from_rgb(245, 158, 11);
    v.selection.bg_fill = Color32::from_rgb(245, 158, 11).gamma_multiply(0.3);
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(245, 158, 11));
    v.override_text_color = Some(Color32::from_gray(200));
    v.window_rounding = Rounding::same(6.0);
    v.window_stroke = Stroke::new(1.0, Color32::from_gray(40));
    v
}

fn accent() -> Color32 { Color32::from_rgb(245, 158, 11) }
fn red() -> Color32 { Color32::from_rgb(239, 68, 68) }
fn green() -> Color32 { Color32::from_rgb(34, 197, 94) }
fn dim() -> Color32 { Color32::from_gray(70) }
fn muted_color() -> Color32 { Color32::from_gray(50) }

fn pad_color(idx: usize) -> Color32 {
    let info = audio::default_pad_info();
    let (r, g, b) = info.get(idx).map(|p| p.color).unwrap_or((80, 80, 90));
    Color32::from_rgb(r, g, b)
}

fn color_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_premultiplied(
        (c.r() as u16 * a as u16 / 255) as u8,
        (c.g() as u16 * a as u16 / 255) as u8,
        (c.b() as u16 * a as u16 / 255) as u8,
        a,
    )
}

// ═══════════════════════════════════════════════════════════
//  APP STATE
// ═══════════════════════════════════════════════════════════

pub struct BeatForge {
    engine: Engine,

    // Transport
    playing: bool,
    bpm: f32,
    swing: f32,
    num_steps: usize,

    // Banks
    banks: Vec<Vec<Vec<u8>>>, // [bank][pad][step]
    active_bank: usize,

    // Pad state (UI mirrors)
    pad_names: Vec<String>,
    pad_colors: Vec<Color32>,
    pad_types: Vec<PadType>,
    pad_peaks: Vec<Option<Vec<f32>>>,
    volumes: Vec<f32>,
    pans: Vec<f32>,
    pitches: Vec<f32>,
    filters: Vec<f32>,
    reversed: Vec<bool>,
    trim_start: Vec<f32>,
    trim_end: Vec<f32>,
    muted: Vec<bool>,
    soloed: Vec<bool>,

    // Master
    master_vol: f32,
    master_filter: f32,
    reverb_mix: f32,
    delay_mix: f32,

    // Per-pad bus routing (0=master, 1-3=bus)
    channel_bus: Vec<u8>,

    // Per-pad drum voice tuning
    drum_tune: Vec<f32>,
    drum_decay: Vec<f32>,
    drum_color: Vec<f32>,

    // Per-pad attack/release/loop
    pad_attack: Vec<f32>,
    pad_release: Vec<f32>,
    pad_loop: Vec<bool>,

    // Audio recording (bounce)
    is_recording: bool,
    record_start_time: f64,

    // Export state
    show_export: bool,
    export_bars: usize,
    exporting: bool,
    export_steps_remaining: i32,

    // Zoom level for sequencer/piano roll (1.0 = default)
    seq_zoom: f32,

    // Step input mode (advance cursor one step at a time)
    step_input: bool,
    step_cursor: usize,

    // Detected BPM and sample info
    detected_bpm: Option<f32>,
    sample_info: Option<String>, // duration, sample rate, etc.

    // Live recording (record pad hits to grid while playing)
    live_rec: bool,
    overdub: bool,     // true = layer on top, false = replace existing hits
    count_in: bool,    // 1-bar count-in before live rec starts
    count_in_step: i32, // counts down from num_steps to 0

    // Synth params per pad
    synth_params: Vec<SynthParams>,
    synth_assigned: Vec<bool>, // whether pad has a subtractive synth

    // Piano roll
    keyboard_octave: i32, // octave offset for keyboard piano input (-2 to +2)
    note_patterns: Vec<NotePattern>,
    piano_scroll_y: f32,
    piano_scroll_x: f32,
    piano_scale: Scale,

    // Per-step probability (0-100, applied during playback)
    step_probability: Vec<Vec<u8>>, // [pad][step] = 0-100 probability

    // Slicer
    slicer_source: Option<Vec<f32>>,
    slicer_source_sr: u32,
    slicer_slices: Vec<slicer::Slice>,
    slicer_sensitivity: f32,

    // Arrangement (song mode): Vec of bars, each bar is Vec of bank indices per track
    arrangement: Vec<Vec<u8>>, // arrangement[bar][track] = bank_index (255 = empty)

    // Per-channel EQ
    eq_params: Vec<EqParams>,

    // Automation
    automation: AutomationData,
    show_automation: bool,
    auto_target: AutoTarget,

    // Per-pad send levels
    reverb_sends: Vec<f32>,
    delay_sends: Vec<f32>,

    // Per-pad insert FX params [pad] -> (dist_drive, dist_mix, bits, crush_rate, crush_mix,
    //                                     chorus_rate, chorus_depth, chorus_mix,
    //                                     phaser_rate, phaser_depth, phaser_fb, phaser_mix)
    fx_params: Vec<[f32; 12]>,

    // Master stereo width + enhancer
    stereo_width: f32,
    enhancer_amount: f32,

    // Velocity curve: 0=linear, 1=exponential (softer feel), 2=logarithmic (harder feel)
    velocity_curve: usize,

    // Delay time division (0=1/4, 1=1/8, 2=1/16, 3=dotted 1/8, 4=triplet 1/8)
    delay_division: usize,

    // Sidechain state per pad
    sidechain_active: Vec<bool>,

    // Gross Beat effect
    gross_beat_mode: GrossBeatMode,

    // Note repeat rate (0=off, 1=1/4, 2=1/8, 3=1/16, 4=1/32)
    note_repeat_rate: usize,
    note_repeat_counter: f64,
    note_repeat_held_pad: Option<usize>,

    // Tap tempo
    tap_times: Vec<f64>,

    // Pattern clipboard (for copy/paste between banks)
    pattern_clipboard: Option<Vec<Vec<u8>>>,

    // Metronome
    metronome_on: bool,

    // Undo/redo
    undo_stack: Vec<UndoState>,
    redo_stack: Vec<UndoState>,

    // Preset browser
    show_synth_presets: bool,

    // Automation recording — when enabled, knob/slider changes during playback get written to automation
    auto_rec: bool,

    // Piano roll snap grid
    piano_snap: f32,

    // Loop region (None = loop entire pattern)
    loop_start: Option<usize>,
    loop_end: Option<usize>,

    // Sequencer lane visibility
    show_velocity_lane: bool,
    show_probability_lane: bool,

    // Sequencer row context menu
    context_menu_row: Option<(usize, Pos2)>, // (pad_idx, screen_pos)

    // MIDI input
    midi_rx: crossbeam_channel::Receiver<MidiEvent>,
    midi_connected: std::sync::Arc<std::sync::atomic::AtomicBool>,
    midi_device_name: String,

    // Sample browser
    browser_path: std::path::PathBuf,
    browser_files: Vec<(String, std::path::PathBuf, bool)>, // (name, path, is_dir)
    browser_open: bool,

    // Mic recording
    mic: MicRecorder,
    mic_recording_for_pad: Option<usize>,

    // Pad context menu
    pad_context_menu: Option<(usize, Pos2)>, // (pad_idx, position)

    // Channel settings popup
    show_channel_settings: Option<usize>,

    // Project state
    project_name: String,
    project_dirty: bool,
    last_save_path: Option<std::path::PathBuf>,
    show_about: bool,

    // UI
    selected_pad: usize,
    flash_pad: Option<(usize, f64)>, // (pad, time)
    main_view: MainView,
    bottom_view: BottomView,
    show_help: bool,
    show_presets: bool,
}

#[derive(PartialEq, Clone, Copy)]
enum PadType { Synth, Sample, Empty, SubSynth }

#[derive(PartialEq, Clone, Copy)]
enum MainView { Sequencer, PianoRoll, Arrangement }

#[derive(PartialEq, Clone, Copy)]
enum BottomView { Editor, Mixer, Synth, Slicer, InsertFx }

/// Undo/redo state snapshot
#[derive(Clone)]
struct UndoState {
    banks: Vec<Vec<Vec<u8>>>,
    note_patterns: Vec<NotePattern>,
}

impl BeatForge {
    pub fn new() -> Self {
        let info = audio::default_pad_info();
        let engine = Engine::new();

        // MIDI input
        let (midi_tx, midi_rx) = crossbeam_channel::unbounded();
        let midi_mgr = MidiManager::new(midi_tx);
        let midi_connected = midi_mgr.connected.clone();
        let midi_device_name = midi_mgr.device_name.clone();
        std::mem::forget(midi_mgr); // Keep the connection alive

        let mut app = BeatForge {
            engine,
            playing: false,
            bpm: 90.0,
            swing: 0.0,
            num_steps: 16,
            banks: (0..8).map(|_| vec![vec![0u8; MAX_STEPS]; NUM_PADS]).collect(),
            active_bank: 0,
            pad_names: info.iter().map(|p| p.name.to_string()).collect(),
            pad_colors: info.iter().map(|p| Color32::from_rgb(p.color.0, p.color.1, p.color.2)).collect(),
            pad_types: info.iter().map(|p| if p.has_voice { PadType::Synth } else { PadType::Empty }).collect(),
            pad_peaks: vec![None; NUM_PADS],
            volumes: vec![0.7; NUM_PADS],
            pans: vec![0.0; NUM_PADS],
            pitches: vec![0.0; NUM_PADS],
            filters: vec![20000.0; NUM_PADS],
            reversed: vec![false; NUM_PADS],
            trim_start: vec![0.0; NUM_PADS],
            trim_end: vec![1.0; NUM_PADS],
            muted: vec![false; NUM_PADS],
            soloed: vec![false; NUM_PADS],
            master_vol: 0.8,
            master_filter: 20000.0,
            reverb_mix: 0.0,
            delay_mix: 0.0,
            reverb_sends: vec![0.0; NUM_PADS],
            delay_sends: vec![0.0; NUM_PADS],
            // [drive, dist_mix, bits, crush_rate, crush_mix, chorus_rate, chorus_depth, chorus_mix, ph_rate, ph_depth, ph_fb, ph_mix]
            fx_params: vec![[0.0, 0.0, 16.0, 1.0, 0.0, 0.5, 3.0, 0.0, 0.3, 0.5, 0.5, 0.0]; NUM_PADS],
            stereo_width: 1.0,
            enhancer_amount: 0.0,
            velocity_curve: 0, // 0=linear, 1=exp, 2=log
            delay_division: 1,
            browser_path: dirs_home().join("Desktop"),
            browser_files: Vec::new(),
            browser_open: false,
            mic: MicRecorder::new(),
            mic_recording_for_pad: None,
            pad_context_menu: None,
            show_channel_settings: None,
            project_name: "Untitled".to_string(),
            project_dirty: false,
            last_save_path: None,
            show_about: false,
            midi_rx,
            midi_connected,
            midi_device_name,
            auto_rec: false,
            piano_snap: 1.0,
            loop_start: None,
            loop_end: None,
            show_velocity_lane: true,
            show_probability_lane: false,
            context_menu_row: None,
            sidechain_active: vec![false; NUM_PADS],
            gross_beat_mode: GrossBeatMode::Off,
            note_repeat_rate: 0,
            note_repeat_counter: 0.0,
            note_repeat_held_pad: None,
            tap_times: Vec::new(),
            pattern_clipboard: None,
            arrangement: (0..4).map(|_| vec![0u8; 5]).collect(), // 4 bars, 5 tracks
            eq_params: (0..NUM_PADS).map(|_| EqParams::default()).collect(),
            automation: AutomationData::new(),
            show_automation: false,
            auto_target: AutoTarget::FilterCutoff,
            is_recording: false,
            record_start_time: 0.0,
            show_export: false,
            export_bars: 2,
            exporting: false,
            export_steps_remaining: -1,
            seq_zoom: 1.0,
            step_input: false,
            step_cursor: 0,
            detected_bpm: None,
            sample_info: None,
            channel_bus: vec![0u8; NUM_PADS],
            drum_tune: vec![0.0; NUM_PADS],
            drum_decay: vec![1.0; NUM_PADS],
            drum_color: vec![0.3; NUM_PADS],
            pad_attack: vec![0.001; NUM_PADS],
            pad_release: vec![0.01; NUM_PADS],
            pad_loop: vec![false; NUM_PADS],
            live_rec: false,
            overdub: true,
            count_in: false,
            count_in_step: -1,
            synth_params: (0..NUM_PADS).map(|_| SynthParams::default()).collect(),
            synth_assigned: vec![false; NUM_PADS],
            note_patterns: (0..NUM_PADS).map(|_| NotePattern::new()).collect(),
            piano_scroll_y: 60.0, // start around middle C
            piano_scroll_x: 0.0,
            keyboard_octave: 0,
            piano_scale: Scale::Chromatic,
            step_probability: (0..NUM_PADS).map(|_| vec![100u8; MAX_STEPS]).collect(),
            slicer_source: None,
            slicer_source_sr: 44100,
            slicer_slices: Vec::new(),
            slicer_sensitivity: 0.5,
            metronome_on: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            show_synth_presets: false,
            selected_pad: 0,
            flash_pad: None,
            main_view: MainView::Sequencer,
            bottom_view: BottomView::Editor,
            show_help: false,
            show_presets: false,
        };

        // Load a default beat so the app isn't empty on first launch
        app.load_default_beat();
        app
    }

    fn scan_browser(&mut self) {
        self.browser_files.clear();
        if let Ok(entries) = std::fs::read_dir(&self.browser_path) {
            let mut dirs = Vec::new();
            let mut files = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') { continue; } // skip hidden
                if path.is_dir() {
                    dirs.push((name, path, true));
                } else if is_audio_file(&path) {
                    files.push((name, path, false));
                }
            }
            dirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
            files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
            self.browser_files.extend(dirs);
            self.browser_files.extend(files);
        }
    }

    fn load_default_beat(&mut self) {
        // Boom Bap pattern in Bank A
        let bank = &mut self.banks[0];
        // Kick
        bank[0] = vec![3,0,0,0,0,0,0,0,3,0,0,0,0,0,0,0,
                       0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                       0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];
        // Snare
        bank[1] = vec![0,0,0,0,3,0,0,0,0,0,0,0,3,0,0,0,
                       0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                       0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];
        // Hi-hat closed
        bank[2] = vec![3,0,2,0,3,0,2,0,3,0,2,0,3,0,2,0,
                       0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                       0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];
        // Hi-hat open
        bank[3] = vec![0,0,0,0,0,0,0,0,0,0,0,0,0,0,2,0,
                       0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                       0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];
        // Trap pattern in Bank B
        let bank_b = &mut self.banks[1];
        bank_b[0] = vec![3,0,0,0,0,0,3,0,0,0,0,0,3,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];
        bank_b[1] = vec![0,0,0,0,3,0,0,0,0,0,0,0,3,0,0,2,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];
        bank_b[2] = vec![3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];
        bank_b[5] = vec![0,0,2,0,0,0,0,0,0,0,2,0,0,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];

        // House pattern in Bank C
        let bank_c = &mut self.banks[2];
        bank_c[0] = vec![3,0,0,0,3,0,0,0,3,0,0,0,3,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];
        bank_c[1] = vec![0,0,0,0,3,0,0,0,0,0,0,0,3,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];
        bank_c[2] = vec![0,0,3,0,0,0,3,0,0,0,3,0,0,0,3,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0];

        self.project_name = "Demo Beat".to_string();
    }

    fn trigger_pad(&mut self, idx: usize, ctx: &egui::Context) {
        self.engine.send(Cmd::TriggerPad(idx, 1.0));
        self.selected_pad = idx;
        self.flash_pad = Some((idx, ctx.input(|i| i.time)));

        // Step input mode: write to cursor and advance
        if self.step_input && !self.playing {
            let s = self.step_cursor;
            if s < self.num_steps {
                self.banks[self.active_bank][idx][s] = 3;
                self.engine.send(Cmd::SetCell { pad: idx, step: s, vel: 3 });
                self.step_cursor = (self.step_cursor + 1) % self.num_steps;
            }
        }

        // Live recording: write hit to current step in the grid
        if self.live_rec && self.playing && self.count_in_step < 0 {
            let step = self.engine.current_step();
            if step >= 0 && (step as usize) < self.num_steps {
                let s = step as usize;
                let existing = self.banks[self.active_bank][idx][s];
                if self.overdub || existing == 0 {
                    self.banks[self.active_bank][idx][s] = 3;
                    self.engine.send(Cmd::SetCell { pad: idx, step: s, vel: 3 });
                }
            }
        }
    }

    fn sync_pattern(&self) {
        self.engine.send(Cmd::SetFullPattern(
            self.banks[self.active_bank].clone()
        ));
    }

    fn load_preset(&mut self, pattern: Vec<Vec<u8>>) {
        for (i, row) in pattern.into_iter().enumerate() {
            if i < NUM_PADS {
                let bank = &mut self.banks[self.active_bank];
                for (j, v) in row.into_iter().enumerate() {
                    if j < MAX_STEPS {
                        bank[i][j] = v;
                    }
                }
            }
        }
        self.sync_pattern();
    }

    fn load_sample_file(&mut self, pad: usize, path: &std::path::Path) {
        if let Some((data, sr)) = audio::load_wav(path) {
            let peaks = audio::compute_peaks(&data, 200);
            self.pad_peaks[pad] = Some(peaks);

            // BPM detection + sample info
            self.detected_bpm = slicer::detect_bpm(&data, sr);
            let duration = data.len() as f32 / sr as f32;
            self.sample_info = Some(format!("{:.2}s · {}Hz · {} samples", duration, sr, data.len()));

            let name = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("SAMPLE")
                .chars().take(10)
                .collect::<String>()
                .to_uppercase();
            self.pad_names[pad] = name;
            self.pad_types[pad] = PadType::Sample;
            self.pitches[pad] = 0.0;
            self.reversed[pad] = false;
            self.trim_start[pad] = 0.0;
            self.trim_end[pad] = 1.0;

            self.engine.send(Cmd::LoadSample {
                pad,
                data: Arc::new(data),
                original_sr: sr,
            });
        }
    }
}

// ═══════════════════════════════════════════════════════════
//  EGUI APP IMPL
// ═══════════════════════════════════════════════════════════

impl eframe::App for BeatForge {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let current_step = self.engine.current_step();

        // Only request repaint when there's active animation
        // When truly idle, egui repaints only on user input (mouse/keyboard)
        if self.flash_pad.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }

        // Window title is set on save/load/new — not every frame

        // Read triggered pads from audio thread (for pad flash effect)
        let seq_triggered = self.engine.shared.get_triggered();

        // ── Keyboard input ─────────────────────────────────
        ctx.input(|input| {
            let piano_mode = self.main_view == MainView::PianoRoll
                && self.synth_assigned[self.selected_pad];

            if piano_mode {
                // Keyboard plays chromatic notes on the selected synth
                // Octave shift with [ and ]
                if input.key_pressed(Key::OpenBracket) {
                    self.keyboard_octave = (self.keyboard_octave - 1).max(-2);
                }
                if input.key_pressed(Key::CloseBracket) {
                    self.keyboard_octave = (self.keyboard_octave + 1).min(2);
                }

                // Bottom row = C3, next = C#3, etc (2 octaves mapped)
                const PIANO_KEYS: [(Key, u8); 17] = [
                    (Key::Z, 48), (Key::S, 49), (Key::X, 50), (Key::D, 51),
                    (Key::C, 52), (Key::V, 53), (Key::G, 54), (Key::B, 55),
                    (Key::H, 56), (Key::N, 57), (Key::J, 58), (Key::M, 59),
                    (Key::Q, 60), (Key::Num2, 61), (Key::W, 62), (Key::Num3, 63),
                    (Key::E, 64),
                ];
                let sp = self.selected_pad;
                let oct_offset = self.keyboard_octave * 12;
                for &(key, base_note) in &PIANO_KEYS {
                    let note = (base_note as i32 + oct_offset).clamp(0, 127) as u8;
                    if input.key_pressed(key) {
                        self.engine.send(Cmd::NoteOn { pad: sp, note, velocity: 0.8 });
                    }
                    if input.key_released(key) {
                        self.engine.send(Cmd::NoteOff { pad: sp, note });
                    }
                }
            } else {
                // Normal mode: pad triggers
                for &(key, pad) in &KEY_MAP {
                    if input.key_pressed(key) {
                        self.engine.send(Cmd::TriggerPad(pad, 1.0));
                        self.selected_pad = pad;
                        self.flash_pad = Some((pad, input.time));
                    }
                }
            }

            if input.key_pressed(Key::Space) {
                self.playing = !self.playing;
                if self.playing {
                    self.sync_pattern();
                    self.engine.send(Cmd::Play);
                } else {
                    self.engine.send(Cmd::Stop); self.engine.send(Cmd::AllNotesOff);
                }
            }
            if input.key_pressed(Key::Escape) {
                self.playing = false;
                self.engine.send(Cmd::Stop); self.engine.send(Cmd::AllNotesOff);
                self.show_help = false;
                self.show_presets = false;
            }
            if input.key_pressed(Key::ArrowUp) {
                let delta = if input.modifiers.shift { 10.0 } else { 1.0 };
                self.bpm = (self.bpm + delta).min(300.0);
                self.engine.send(Cmd::SetBpm(self.bpm));
            }
            if input.key_pressed(Key::ArrowDown) {
                let delta = if input.modifiers.shift { 10.0 } else { 1.0 };
                self.bpm = (self.bpm - delta).max(20.0);
                self.engine.send(Cmd::SetBpm(self.bpm));
            }
            if input.key_pressed(Key::Slash) {
                self.show_help = !self.show_help;
            }
            if input.key_pressed(Key::Tab) {
                self.main_view = match self.main_view {
                    MainView::Sequencer => MainView::PianoRoll,
                    MainView::PianoRoll => MainView::Arrangement,
                    MainView::Arrangement => MainView::Sequencer,
                };
            }
            // Undo/Redo (Cmd+Z / Cmd+Shift+Z on macOS)
            if input.modifiers.command && input.key_pressed(Key::Z) {
                if input.modifiers.shift {
                    self.redo();
                } else {
                    self.undo();
                }
            }
            // Cmd+S for quick save
            if input.modifiers.command && input.key_pressed(Key::S) {
                self.quick_save();
            }
            // Cmd+N for new project
            if input.modifiers.command && input.key_pressed(Key::N) {
                self.new_project();
            }
            // Cmd+C / Cmd+V for pattern copy/paste
            if input.modifiers.command && input.key_pressed(Key::C) {
                self.copy_pattern();
            }
            if input.modifiers.command && input.key_pressed(Key::V) {
                self.paste_pattern();
            }
            // Cmd+D to duplicate pattern to next bank
            if input.modifiers.command && input.key_pressed(Key::D) {
                self.duplicate_pattern();
            }
            // F1-F8 switch pattern banks
            let f_keys = [Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8];
            for (i, &fkey) in f_keys.iter().enumerate() {
                if input.key_pressed(fkey) {
                    self.active_bank = i;
                    self.sync_pattern();
                }
            }
            // T for tap tempo
            // M = mute selected pad, S = solo (only when not in piano mode)
            if !piano_mode {
                if input.key_pressed(Key::M) && !input.modifiers.command {
                    let sp = self.selected_pad;
                    self.muted[sp] = !self.muted[sp];
                    self.engine.send(Cmd::SetPadMute(sp, self.muted[sp]));
                }
                // S is already mapped to pad 5, so use Ctrl+S... actually let's not conflict
                // Use F9 for solo toggle instead
                if input.key_pressed(Key::F9) {
                    let sp = self.selected_pad;
                    self.soloed[sp] = !self.soloed[sp];
                    self.engine.send(Cmd::SetPadSolo(sp, self.soloed[sp]));
                }
            }
            if input.key_pressed(Key::T) {
                self.tap_tempo(input.time);
            }

            // Pad selection with arrow keys (when not in step input and not adjusting BPM)
            if !self.step_input && !input.modifiers.shift {
                if input.key_pressed(Key::ArrowLeft) && !input.modifiers.command {
                    self.selected_pad = self.selected_pad.checked_sub(1).unwrap_or(NUM_PADS - 1);
                }
                if input.key_pressed(Key::ArrowRight) && !input.modifiers.command {
                    self.selected_pad = (self.selected_pad + 1) % NUM_PADS;
                }
            }

            // Ctrl+scroll for zoom
            if input.modifiers.command {
                let scroll = input.smooth_scroll_delta.y;
                if scroll.abs() > 0.1 {
                    self.seq_zoom = (self.seq_zoom + scroll * 0.01).clamp(0.5, 3.0);
                }
            }

            // Step cursor navigation (when step input is active)
            if self.step_input && !self.playing {
                if input.key_pressed(Key::ArrowRight) {
                    self.step_cursor = (self.step_cursor + 1) % self.num_steps;
                }
                if input.key_pressed(Key::ArrowLeft) {
                    self.step_cursor = self.step_cursor.checked_sub(1).unwrap_or(self.num_steps - 1);
                }
                // Delete key clears the cell at cursor
                if input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace) {
                    let sp = self.selected_pad;
                    if self.step_cursor < self.num_steps {
                        self.banks[self.active_bank][sp][self.step_cursor] = 0;
                        self.engine.send(Cmd::SetCell { pad: sp, step: self.step_cursor, vel: 0 });
                    }
                }
                // Enter advances cursor without placing a hit (rest)
                if input.key_pressed(Key::Enter) {
                    self.step_cursor = (self.step_cursor + 1) % self.num_steps;
                }
            }

            // Global: Delete key clears selected pad's entire row
            if !self.step_input && input.key_pressed(Key::Delete) {
                self.push_undo();
                let sp = self.selected_pad;
                for s in 0..self.num_steps {
                    self.banks[self.active_bank][sp][s] = 0;
                }
                self.sync_pattern();
            }
        });

        // ── MIDI input processing ──────────────────────────
        while let Ok(evt) = self.midi_rx.try_recv() {
            match evt {
                MidiEvent::NoteOn { note, velocity, .. } => {
                    let vel_f = apply_velocity_curve(velocity as f32 / 127.0, self.velocity_curve);
                    let piano_mode = self.main_view == MainView::PianoRoll
                        && self.synth_assigned[self.selected_pad];

                    if piano_mode {
                        // Route to synth
                        self.engine.send(Cmd::NoteOn {
                            pad: self.selected_pad,
                            note,
                            velocity: vel_f,
                        });
                    } else if let Some(pad) = midi_note_to_pad(note) {
                        // Route to pad (GM drum map: notes 36-51)
                        self.engine.send(Cmd::TriggerPad(pad, vel_f));
                        self.selected_pad = pad;
                        self.flash_pad = Some((pad, ctx.input(|i| i.time)));
                        // Live recording
                        if self.live_rec && self.playing {
                            let step = self.engine.current_step();
                            if step >= 0 && (step as usize) < self.num_steps {
                                let vel_u8 = if vel_f < 0.33 { 1 } else if vel_f < 0.66 { 2 } else { 3 };
                                self.banks[self.active_bank][pad][step as usize] = vel_u8;
                                self.engine.send(Cmd::SetCell { pad, step: step as usize, vel: vel_u8 });
                            }
                        }
                    } else {
                        // For notes outside drum range, try playing the selected synth
                        if self.synth_assigned[self.selected_pad] {
                            self.engine.send(Cmd::NoteOn {
                                pad: self.selected_pad,
                                note,
                                velocity: vel_f,
                            });
                        }
                    }
                }
                MidiEvent::NoteOff { note, .. } => {
                    if self.synth_assigned[self.selected_pad] {
                        self.engine.send(Cmd::NoteOff {
                            pad: self.selected_pad,
                            note,
                        });
                    }
                }
                MidiEvent::ControlChange { cc, value, .. } => {
                    if let Some(param) = crate::midi::midi_cc_to_param(cc, value) {
                        match param {
                            crate::midi::MidiParam::Volume(v) => {
                                self.volumes[self.selected_pad] = v;
                                self.engine.send(Cmd::SetPadVol(self.selected_pad, v));
                            }
                            crate::midi::MidiParam::Pan(v) => {
                                self.pans[self.selected_pad] = v;
                                self.engine.send(Cmd::SetPadPan(self.selected_pad, v));
                            }
                            crate::midi::MidiParam::FilterCutoff(v) => {
                                self.filters[self.selected_pad] = v;
                                self.engine.send(Cmd::SetPadFilter(self.selected_pad, v));
                            }
                            crate::midi::MidiParam::ModWheel(v) => {
                                // Route mod wheel to master filter
                                self.master_filter = 200.0 + v * 19800.0;
                                self.engine.send(Cmd::SetMasterFilter(self.master_filter));
                            }
                        }
                    }
                }
            }
        }

        // ── Drag & drop files ──────────────────────────────
        ctx.input(|input| {
            if !input.raw.dropped_files.is_empty() {
                for (i, file) in input.raw.dropped_files.iter().enumerate() {
                    if let Some(ref path) = file.path {
                        let pad = (self.selected_pad + i).min(NUM_PADS - 1);
                        // Clone path for use after closure
                        let path = path.clone();
                        // We'll handle below
                        self.load_sample_file(pad, &path);
                    }
                }
            }
        });

        // Clear flash after 100ms
        if let Some((_, t)) = self.flash_pad {
            if ctx.input(|i| i.time) - t > 0.1 {
                self.flash_pad = None;
            }
        }

        // Count-in countdown
        if self.count_in_step > 0 && self.playing {
            let step = current_step;
            if step >= 0 {
                self.count_in_step -= 1;
                if self.count_in_step <= 0 {
                    // Count-in done, recording starts
                    self.count_in_step = -1;
                    if !self.metronome_on {
                        self.engine.send(Cmd::SetMetronome(false)); // turn off click if user didn't have it on
                    }
                }
            }
        }

        // Note repeat: retrigger held pad at selected rate
        if self.note_repeat_rate > 0 {
            if let Some(pad) = self.note_repeat_held_pad {
                let interval = match self.note_repeat_rate {
                    1 => 60.0 / self.bpm as f64,           // 1/4 note
                    2 => 60.0 / self.bpm as f64 / 2.0,     // 1/8
                    3 => 60.0 / self.bpm as f64 / 4.0,     // 1/16
                    4 => 60.0 / self.bpm as f64 / 8.0,     // 1/32
                    _ => 1.0,
                };
                let now = ctx.input(|i| i.time);
                if now - self.note_repeat_counter >= interval {
                    self.engine.send(Cmd::TriggerPad(pad, 0.8));
                    self.note_repeat_counter = now;
                    self.flash_pad = Some((pad, now));
                }
            }
            if self.note_repeat_held_pad.is_some() {
                ctx.request_repaint(); // Only repaint when actively repeating
            }
        }

        // Request repaint while playing for step animation (30fps is enough)
        if self.playing || self.exporting {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }

        // ══════════════════════════════════════════════════
        // TOP BAR
        // ══════════════════════════════════════════════════
        TopBottomPanel::top("topbar").exact_height(44.0).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;

                // Logo
                ui.label(RichText::new("◈").size(18.0).color(accent()));
                let dirty_marker = if self.project_dirty { " ●" } else { "" };
                ui.label(RichText::new(format!("BEATFORGE{dirty_marker}")).size(14.0).strong().color(accent()));
                ui.label(RichText::new("STUDIO").size(8.0).color(dim()));
                ui.add_space(12.0);

                // Transport
                let play_text = if self.playing { "■ STOP" } else { "▶ PLAY" };
                let play_color = if self.playing { red() } else { green() };
                if ui.add(Button::new(RichText::new(play_text).size(11.0).strong().color(Color32::BLACK))
                    .fill(play_color)
                    .min_size(vec2(70.0, 28.0))).clicked()
                {
                    self.playing = !self.playing;
                    if self.playing {
                        self.sync_pattern();
                        self.engine.send(Cmd::Play);
                    } else {
                        self.engine.send(Cmd::Stop); self.engine.send(Cmd::AllNotesOff);
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // BPM (slider + click-to-type value)
                ui.label(RichText::new("BPM").size(9.0).color(dim()).family(FontFamily::Monospace));
                let bpm_before = self.bpm;
                ui.add(Slider::new(&mut self.bpm, 20.0..=300.0).show_value(false).fixed_decimals(0));
                // Clickable BPM display — click to type exact value
                ui.add(egui::DragValue::new(&mut self.bpm)
                    .range(20.0..=300.0)
                    .speed(0.5)
                    .fixed_decimals(0)
                    .prefix("")
                    .custom_formatter(|v, _| format!("{:.0}", v)));
                if self.bpm != bpm_before {
                    self.engine.send(Cmd::SetBpm(self.bpm));
                }

                ui.add_space(4.0);

                // Swing
                ui.label(RichText::new("SWG").size(9.0).color(dim()).family(FontFamily::Monospace));
                let sw_before = self.swing;
                ui.add(Slider::new(&mut self.swing, 0.0..=100.0).show_value(false).fixed_decimals(0));
                ui.label(RichText::new(format!("{:.0}%", self.swing)).size(11.0).color(dim()).family(FontFamily::Monospace));
                if self.swing != sw_before {
                    self.engine.send(Cmd::SetSwing(self.swing));
                }

                ui.separator();

                // Pattern banks (with activity indicator)
                for (i, &label) in BANK_LABELS.iter().enumerate() {
                    let active = self.active_bank == i;
                    // Check if bank has any data
                    let has_data = i < self.banks.len() && self.banks[i].iter()
                        .any(|row| row.iter().any(|&v| v > 0));
                    let indicator = if has_data && !active { "·" } else { "" };
                    let text = format!("{label}{indicator}");
                    let fill = if active { accent() }
                        else if has_data { Color32::from_gray(38) }
                        else { Color32::from_gray(28) };
                    let btn = Button::new(RichText::new(&text).size(11.0).strong()
                        .color(if active { Color32::BLACK } else if has_data { Color32::from_gray(140) } else { dim() }))
                        .fill(fill)
                        .min_size(vec2(24.0, 24.0));
                    if ui.add(btn).clicked() {
                        self.active_bank = i;
                        self.sync_pattern();
                    }
                }

                ui.separator();

                // Step count
                for &count in &STEPS_OPTIONS {
                    let active = self.num_steps == count;
                    let btn = Button::new(RichText::new(format!("{count}")).size(10.0)
                        .color(if active { Color32::BLACK } else { dim() }))
                        .fill(if active { accent() } else { Color32::from_gray(28) })
                        .min_size(vec2(28.0, 22.0));
                    if ui.add(btn).clicked() {
                        self.num_steps = count;
                        self.engine.send(Cmd::SetSteps(count));
                    }
                }

                ui.separator();

                // Presets
                if ui.button(RichText::new("PRESETS").size(10.0).color(dim())).clicked() {
                    self.show_presets = !self.show_presets;
                }

                // Live record toggle
                let live_color = if self.live_rec { Color32::from_rgb(239, 68, 68) } else { dim() };
                if ui.add(Button::new(RichText::new("●REC").size(9.0)
                    .color(if self.live_rec { Color32::BLACK } else { live_color }))
                    .fill(if self.live_rec { Color32::from_rgb(239, 68, 68) } else { Color32::from_gray(28) })
                    .min_size(vec2(32.0, 18.0))).clicked() {
                    self.live_rec = !self.live_rec;
                    if self.live_rec {
                        // Push undo state before recording so entire take can be undone
                        self.push_undo();
                        if !self.playing {
                            self.sync_pattern();
                            self.engine.send(Cmd::SetMetronome(true));
                            self.engine.send(Cmd::Play);
                            self.playing = true;
                            if self.count_in {
                                self.count_in_step = self.num_steps as i32;
                            }
                        }
                    }
                }
                // Overdub toggle
                let od_color = if self.overdub { accent() } else { Color32::from_gray(28) };
                if ui.add(Button::new(RichText::new("OVR").size(8.0)
                    .color(if self.overdub { Color32::BLACK } else { dim() }))
                    .fill(od_color).min_size(vec2(24.0, 16.0))).clicked() {
                    self.overdub = !self.overdub;
                }
                // Count-in toggle
                let ci_color = if self.count_in { accent() } else { Color32::from_gray(28) };
                if ui.add(Button::new(RichText::new("C-IN").size(7.0)
                    .color(if self.count_in { Color32::BLACK } else { dim() }))
                    .fill(ci_color).min_size(vec2(24.0, 16.0))).clicked() {
                    self.count_in = !self.count_in;
                }
                // Step input toggle
                let si_color = if self.step_input { Color32::from_rgb(6, 182, 212) } else { Color32::from_gray(28) };
                if ui.add(Button::new(RichText::new("STEP").size(7.0)
                    .color(if self.step_input { Color32::BLACK } else { dim() }))
                    .fill(si_color).min_size(vec2(28.0, 16.0))).clicked() {
                    self.step_input = !self.step_input;
                    self.step_cursor = 0;
                }

                // Automation recording toggle
                let ar_color = if self.auto_rec { Color32::from_rgb(239, 68, 68) } else { Color32::from_gray(28) };
                if ui.add(Button::new(RichText::new("A.REC").size(7.0)
                    .color(if self.auto_rec { Color32::BLACK } else { dim() }))
                    .fill(ar_color).min_size(vec2(32.0, 16.0))).clicked() {
                    self.auto_rec = !self.auto_rec;
                }

                // Tap tempo
                if ui.button(RichText::new("TAP").size(10.0).color(dim())).clicked() {
                    let time = ui.input(|i| i.time);
                    self.tap_tempo(time);
                }

                // Note repeat rate selector
                let repeat_labels = ["RPT", "1/4", "1/8", "1/16", "1/32"];
                for (i, &label) in repeat_labels.iter().enumerate() {
                    let active = self.note_repeat_rate == i;
                    if ui.add(Button::new(RichText::new(label).size(8.0)
                        .color(if active { Color32::BLACK } else { dim() }))
                        .fill(if active { Color32::from_rgb(239, 68, 68) } else { Color32::from_gray(28) })
                        .min_size(vec2(22.0, 18.0))).clicked() {
                        self.note_repeat_rate = if active { 0 } else { i };
                    }
                }

                ui.separator();

                // Gross Beat FX
                let gb_modes = [
                    (GrossBeatMode::Off, "FX"),
                    (GrossBeatMode::HalfSpeed, "½×"),
                    (GrossBeatMode::TapeStop, "STOP"),
                    (GrossBeatMode::Gate, "GATE"),
                    (GrossBeatMode::Stutter, "STUT"),
                    (GrossBeatMode::Reverse, "REV"),
                ];
                for (mode, label) in gb_modes {
                    let active = self.gross_beat_mode == mode;
                    let btn_color = if mode == GrossBeatMode::Off {
                        if self.gross_beat_mode == GrossBeatMode::Off { dim() } else { Color32::from_rgb(239, 68, 68) }
                    } else if active { Color32::from_rgb(168, 85, 247) } else { Color32::from_gray(28) };
                    let text_color = if active && mode != GrossBeatMode::Off { Color32::BLACK }
                        else if mode == GrossBeatMode::Off && self.gross_beat_mode != GrossBeatMode::Off { Color32::BLACK }
                        else { dim() };
                    if ui.add(Button::new(RichText::new(label).size(8.0).color(text_color))
                        .fill(btn_color).min_size(vec2(24.0, 18.0))).clicked() {
                        self.gross_beat_mode = mode;
                        self.engine.send(Cmd::SetGrossBeat(mode));
                    }
                }

                ui.separator();

                // Humanize button
                if ui.button(RichText::new("HUM").size(9.0).color(dim())).clicked() {
                    self.humanize_pattern();
                }

                // Velocity curve
                let curve_names = ["LIN", "EXP", "LOG"];
                for (i, &name) in curve_names.iter().enumerate() {
                    let active = self.velocity_curve == i;
                    if ui.add(Button::new(RichText::new(name).size(7.0)
                        .color(if active { Color32::BLACK } else { dim() }))
                        .fill(if active { accent() } else { Color32::from_gray(28) })
                        .min_size(vec2(20.0, 14.0))).clicked() {
                        self.velocity_curve = i;
                    }
                }

                // Pattern copy/paste
                if ui.button(RichText::new("CPY").size(9.0).color(dim())).clicked() {
                    self.copy_pattern();
                }
                let paste_color = if self.pattern_clipboard.is_some() { dim() } else { muted_color() };
                if ui.add(Button::new(RichText::new("PST").size(9.0).color(paste_color))).clicked() {
                    self.paste_pattern();
                }

                // Metronome
                let _met_color = if self.metronome_on { accent() } else { dim() };
                if ui.add(Button::new(RichText::new("MET").size(10.0)
                    .color(if self.metronome_on { Color32::BLACK } else { dim() }))
                    .fill(if self.metronome_on { accent() } else { Color32::from_gray(28) })).clicked() {
                    self.metronome_on = !self.metronome_on;
                    self.engine.send(Cmd::SetMetronome(self.metronome_on));
                }

                // Help + Save/Load
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(RichText::new("?").size(12.0).color(dim())).clicked() {
                        self.show_help = !self.show_help;
                    }
                    if ui.button(RichText::new("ABOUT").size(8.0).color(muted_color())).clicked() {
                        self.show_about = !self.show_about;
                    }
                    if ui.button(RichText::new("NEW").size(9.0).color(dim())).clicked() {
                        self.new_project();
                    }
                    if ui.button(RichText::new("LOAD").size(9.0).color(dim())).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("BeatForge Project", &["bfp"])
                            .pick_file() {
                            if let Ok(proj) = ProjectData::load(&path) {
                                self.apply_project(proj);
                                self.project_name = path.file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Untitled").to_string();
                                self.last_save_path = Some(path);
                                self.project_dirty = false;
                            }
                        }
                    }
                    if ui.button(RichText::new("SAVE").size(9.0).color(dim())).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("BeatForge Project", &["bfp"])
                            .set_file_name("beat.bfp")
                            .save_file() {
                            let proj = self.to_project();
                            if let Err(e) = proj.save(&path) {
                                eprintln!("Save error: {e}");
                            } else {
                                self.project_dirty = false;
                                self.project_name = path.file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Untitled").to_string();
                                self.last_save_path = Some(path);
                            }
                        }
                    }
                    // Export WAV button
                    if ui.button(RichText::new("EXPORT").size(8.0).color(accent())).clicked() {
                        self.show_export = !self.show_export;
                    }
                });
            });
        });

        // ══════════════════════════════════════════════════
        // LEFT PANEL — PADS
        // ══════════════════════════════════════════════════
        SidePanel::left("pads").exact_width(280.0).show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("PADS").size(10.0).color(dim()).family(FontFamily::Monospace));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(RichText::new("LOAD +").size(9.0).color(dim())).clicked() {
                        if let Some(paths) = rfd::FileDialog::new()
                            .add_filter("Audio", &["wav", "wave", "mp3", "flac", "ogg", "aac", "m4a"])
                            .pick_files()
                        {
                            for (i, path) in paths.iter().enumerate() {
                                let pad = (self.selected_pad + i).min(NUM_PADS - 1);
                                self.load_sample_file(pad, path);
                            }
                        }
                    }
                });
            });
            ui.separator();
            ui.add_space(4.0);

            // 4×4 pad grid
            let pad_size = 56.0;
            let gap = 5.0;
            let total = pad_size * 4.0 + gap * 3.0;

            let (response, painter) = ui.allocate_painter(
                vec2(total, total),
                Sense::click(),
            );
            let origin = response.rect.min;

            for (di, &pad_idx) in PAD_DISPLAY.iter().enumerate() {
                let col = di % 4;
                let row = di / 4;
                let x = origin.x + col as f32 * (pad_size + gap);
                let y = origin.y + row as f32 * (pad_size + gap);
                let rect = Rect::from_min_size(pos2(x, y), vec2(pad_size, pad_size));

                let is_selected = pad_idx == self.selected_pad;
                let is_flash = self.flash_pad.map(|(p,_)| p == pad_idx).unwrap_or(false)
                    || (seq_triggered & (1 << pad_idx)) != 0;
                let color = self.pad_colors[pad_idx];

                // Background
                let bg = if is_flash {
                    color_alpha(color, 60)
                } else {
                    Color32::from_gray(if is_selected { 30 } else { 20 })
                };
                painter.rect_filled(rect, 8.0, bg);

                // Waveform background
                if let Some(ref peaks) = self.pad_peaks[pad_idx] {
                    let n = peaks.len();
                    let bar_w = rect.width() / n as f32;
                    for (i, &p) in peaks.iter().enumerate() {
                        let h = p * rect.height() * 0.7;
                        let bx = rect.left() + i as f32 * bar_w;
                        let by = rect.center().y - h / 2.0;
                        painter.rect_filled(
                            Rect::from_min_size(pos2(bx, by), vec2(bar_w.max(1.0), h.max(0.5))),
                            0.0,
                            color_alpha(color, 30),
                        );
                    }
                }

                // Border
                let border = if is_selected { color } else if is_flash { color } else { Color32::from_gray(40) };
                let stroke_w = if is_selected || is_flash { 1.5 } else { 1.0 };
                painter.rect_stroke(rect, 8.0, Stroke::new(stroke_w, border));

                // Glow
                if is_flash {
                    painter.rect_stroke(rect, 8.0, Stroke::new(3.0, color_alpha(color, 50)));
                }

                // Type dot
                let dot_color = match self.pad_types[pad_idx] {
                    PadType::Synth => color,
                    PadType::SubSynth => accent(),
                    PadType::Sample => green(),
                    PadType::Empty => Color32::from_gray(40),
                };
                painter.circle_filled(pos2(rect.right() - 6.0, rect.top() + 6.0), 3.0, dot_color);

                // Name
                painter.text(rect.center() - vec2(0.0, 5.0), Align2::CENTER_CENTER,
                    &self.pad_names[pad_idx], FontId::monospace(8.0), color);

                // Key label
                painter.text(pos2(rect.center().x, rect.bottom() - 8.0), Align2::CENTER_CENTER,
                    KEY_LABELS[pad_idx], FontId::monospace(7.0), Color32::from_gray(50));
            }

            // Handle pad clicks (and note repeat hold)
            if response.clicked() || response.drag_started() {
                if let Some(pos) = response.interact_pointer_pos() {
                    for (di, &pad_idx) in PAD_DISPLAY.iter().enumerate() {
                        let col = di % 4;
                        let row = di / 4;
                        let x = origin.x + col as f32 * (pad_size + gap);
                        let y = origin.y + row as f32 * (pad_size + gap);
                        let rect = Rect::from_min_size(pos2(x, y), vec2(pad_size, pad_size));
                        if rect.contains(pos) {
                            let alt = ctx.input(|i| i.modifiers.alt);
                            if alt {
                                // Alt+click: preview only (don't change selection)
                                self.engine.send(Cmd::TriggerPad(pad_idx, 1.0));
                                self.flash_pad = Some((pad_idx, ctx.input(|i| i.time)));
                            } else {
                                self.trigger_pad(pad_idx, ctx);
                            }
                            // Set note repeat hold
                            if self.note_repeat_rate > 0 {
                                self.note_repeat_held_pad = Some(pad_idx);
                                self.note_repeat_counter = ctx.input(|i| i.time);
                            }
                            break;
                        }
                    }
                }
            }
            // Double-click pad → jump to sample editor / synth editor
            if response.double_clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    for (di, &pad_idx) in PAD_DISPLAY.iter().enumerate() {
                        let col = di % 4;
                        let row = di / 4;
                        let x = origin.x + col as f32 * (pad_size + gap);
                        let y = origin.y + row as f32 * (pad_size + gap);
                        let rect = Rect::from_min_size(pos2(x, y), vec2(pad_size, pad_size));
                        if rect.contains(pos) {
                            self.selected_pad = pad_idx;
                            if self.synth_assigned[pad_idx] {
                                self.bottom_view = BottomView::Synth;
                            } else {
                                self.bottom_view = BottomView::Editor;
                            }
                            break;
                        }
                    }
                }
            }
            // Right-click on pad → context menu
            if response.secondary_clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    for (di, &pad_idx) in PAD_DISPLAY.iter().enumerate() {
                        let col = di % 4;
                        let row = di / 4;
                        let x = origin.x + col as f32 * (pad_size + gap);
                        let y = origin.y + row as f32 * (pad_size + gap);
                        let r = Rect::from_min_size(pos2(x, y), vec2(pad_size, pad_size));
                        if r.contains(pos) {
                            self.pad_context_menu = Some((pad_idx, pos));
                            self.selected_pad = pad_idx;
                            break;
                        }
                    }
                }
            }
            // Release note repeat on pointer up
            if response.drag_stopped() || (!response.is_pointer_button_down_on() && self.note_repeat_held_pad.is_some()) {
                self.note_repeat_held_pad = None;
            }

            ui.add_space(8.0);
            ui.separator();

            // ── Pad controls ───────────────────────────────
            let sp = self.selected_pad;
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.pad_names[sp]).size(11.0).strong().color(self.pad_colors[sp]).family(FontFamily::Monospace));
                ui.label(RichText::new(match self.pad_types[sp] {
                    PadType::Synth => "SYNTH",
                    PadType::SubSynth => "SUB SYNTH",
                    PadType::Sample => "SAMPLE",
                    PadType::Empty => "EMPTY",
                }).size(8.0).color(dim()));
            });
            ui.add_space(2.0);

            // VOL
            ui.horizontal(|ui| {
                ui.label(RichText::new("VOL").size(9.0).color(muted_color()).family(FontFamily::Monospace));
                let before = self.volumes[sp];
                ui.add(Slider::new(&mut self.volumes[sp], 0.0..=1.0).show_value(false));
                ui.label(RichText::new(format!("{}", (self.volumes[sp] * 100.0) as u32)).size(10.0).color(dim()).family(FontFamily::Monospace));
                if self.volumes[sp] != before {
                    self.engine.send(Cmd::SetPadVol(sp, self.volumes[sp]));
                    self.record_automation(AutoTarget::Volume, sp, self.volumes[sp]);
                }
            });

            // PAN
            ui.horizontal(|ui| {
                ui.label(RichText::new("PAN").size(9.0).color(muted_color()).family(FontFamily::Monospace));
                let before = self.pans[sp];
                ui.add(Slider::new(&mut self.pans[sp], -1.0..=1.0).show_value(false));
                let pan_text = if self.pans[sp].abs() < 0.01 { "C".to_string() }
                    else if self.pans[sp] < 0.0 { format!("L{}", (self.pans[sp].abs() * 100.0) as u32) }
                    else { format!("R{}", (self.pans[sp] * 100.0) as u32) };
                ui.label(RichText::new(pan_text).size(10.0).color(dim()).family(FontFamily::Monospace));
                if self.pans[sp] != before { self.engine.send(Cmd::SetPadPan(sp, self.pans[sp])); }
            });

            // PITCH
            ui.horizontal(|ui| {
                ui.label(RichText::new("PIT").size(9.0).color(muted_color()).family(FontFamily::Monospace));
                let before = self.pitches[sp];
                ui.add(Slider::new(&mut self.pitches[sp], -24.0..=24.0).show_value(false).step_by(1.0));
                let sign = if self.pitches[sp] > 0.0 { "+" } else { "" };
                ui.label(RichText::new(format!("{sign}{}st", self.pitches[sp] as i32)).size(10.0).color(dim()).family(FontFamily::Monospace));
                if self.pitches[sp] != before { self.engine.send(Cmd::SetPadPitch(sp, self.pitches[sp])); }
            });

            // FILTER
            ui.horizontal(|ui| {
                ui.label(RichText::new("FLT").size(9.0).color(muted_color()).family(FontFamily::Monospace));
                let before = self.filters[sp];
                ui.add(Slider::new(&mut self.filters[sp], 100.0..=20000.0).show_value(false).logarithmic(true));
                let freq_text = if self.filters[sp] >= 1000.0 { format!("{:.1}k", self.filters[sp] / 1000.0) }
                    else { format!("{:.0}", self.filters[sp]) };
                ui.label(RichText::new(freq_text).size(10.0).color(dim()).family(FontFamily::Monospace));
                if self.filters[sp] != before {
                    self.engine.send(Cmd::SetPadFilter(sp, self.filters[sp]));
                    self.record_automation(AutoTarget::FilterCutoff, sp, self.filters[sp]);
                }
            });

            // Reverse + Load buttons
            if self.pad_types[sp] == PadType::Sample {
                ui.horizontal(|ui| {
                    if ui.add(Button::new(RichText::new("REV").size(9.0).color(if self.reversed[sp] { Color32::BLACK } else { dim() }))
                        .fill(if self.reversed[sp] { accent() } else { Color32::from_gray(28) })).clicked()
                    {
                        self.reversed[sp] = !self.reversed[sp];
                        self.engine.send(Cmd::SetPadReverse(sp, self.reversed[sp]));
                    }
                    if ui.button(RichText::new("REMOVE").size(9.0).color(red())).clicked() {
                        self.engine.send(Cmd::RemoveSample(sp));
                        let info = audio::default_pad_info();
                        self.pad_names[sp] = info[sp].name.to_string();
                        self.pad_types[sp] = if info[sp].has_voice { PadType::Synth } else { PadType::Empty };
                        self.pad_peaks[sp] = None;
                    }
                });
            }

            // Drum voice tuning (only for synth pads 0-9)
            if self.pad_types[sp] == PadType::Synth && sp < 10 {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("TUNE").size(8.0).color(muted_color()).family(FontFamily::Monospace));
                    let before = self.drum_tune[sp];
                    ui.add(Slider::new(&mut self.drum_tune[sp], -24.0..=24.0).step_by(1.0).show_value(false));
                    ui.label(RichText::new(format!("{:+.0}st", self.drum_tune[sp])).size(9.0).color(dim()).family(FontFamily::Monospace));
                    if self.drum_tune[sp] != before {
                        self.engine.send(Cmd::SetDrumParams(sp, self.drum_tune[sp], self.drum_decay[sp], self.drum_color[sp]));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("DEC").size(8.0).color(muted_color()).family(FontFamily::Monospace));
                    let before = self.drum_decay[sp];
                    ui.add(Slider::new(&mut self.drum_decay[sp], 0.1..=3.0).show_value(false));
                    if self.drum_decay[sp] != before {
                        self.engine.send(Cmd::SetDrumParams(sp, self.drum_tune[sp], self.drum_decay[sp], self.drum_color[sp]));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("CLR").size(8.0).color(muted_color()).family(FontFamily::Monospace));
                    let before = self.drum_color[sp];
                    ui.add(Slider::new(&mut self.drum_color[sp], 0.0..=1.0).show_value(false));
                    if self.drum_color[sp] != before {
                        self.engine.send(Cmd::SetDrumParams(sp, self.drum_tune[sp], self.drum_decay[sp], self.drum_color[sp]));
                    }
                });
            }

            // Choke group
            ui.horizontal(|ui| {
                ui.label(RichText::new("CHOKE").size(8.0).color(muted_color()).family(FontFamily::Monospace));
                for group in 0..=4u8 {
                    let label = if group == 0 { "OFF".to_string() } else { format!("{}", group) };
                    // We track choke groups locally (would need a choke_groups vec but for now use pad index convention)
                    let is_default_choke = match sp { 2 | 3 => group == 1, _ => group == 0 };
                    if ui.add(Button::new(RichText::new(&label).size(8.0)
                        .color(if is_default_choke { Color32::BLACK } else { dim() }))
                        .fill(if is_default_choke { accent() } else { Color32::from_gray(28) })
                        .min_size(vec2(20.0, 14.0))).clicked() {
                        self.engine.send(Cmd::SetChokeGroup(sp, group));
                    }
                }
            });

            ui.add_space(4.0);
            ui.separator();

            // ── Master FX ──────────────────────────────────
            ui.label(RichText::new("MASTER FX").size(9.0).color(muted_color()).family(FontFamily::Monospace));
            ui.add_space(2.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("VOL").size(9.0).color(muted_color()).family(FontFamily::Monospace));
                let before = self.master_vol;
                ui.add(Slider::new(&mut self.master_vol, 0.0..=1.0).show_value(false));
                ui.label(RichText::new(format!("{}", (self.master_vol * 100.0) as u32)).size(10.0).color(dim()).family(FontFamily::Monospace));
                if self.master_vol != before { self.engine.send(Cmd::SetMasterVol(self.master_vol)); }
            });

            ui.horizontal(|ui| {
                ui.label(RichText::new("VRB").size(9.0).color(muted_color()).family(FontFamily::Monospace));
                let before = self.reverb_mix;
                ui.add(Slider::new(&mut self.reverb_mix, 0.0..=1.0).show_value(false));
                ui.label(RichText::new(format!("{}", (self.reverb_mix * 100.0) as u32)).size(10.0).color(dim()).family(FontFamily::Monospace));
                if self.reverb_mix != before { self.engine.send(Cmd::SetReverb(self.reverb_mix)); }
            });

            ui.horizontal(|ui| {
                ui.label(RichText::new("DLY").size(9.0).color(muted_color()).family(FontFamily::Monospace));
                let before = self.delay_mix;
                ui.add(Slider::new(&mut self.delay_mix, 0.0..=1.0).show_value(false));
                ui.label(RichText::new(format!("{}", (self.delay_mix * 100.0) as u32)).size(10.0).color(dim()).family(FontFamily::Monospace));
                if self.delay_mix != before { self.engine.send(Cmd::SetDelay(self.delay_mix)); }
            });
            // Delay time division
            ui.horizontal(|ui| {
                ui.add_space(32.0);
                let divisions = ["1/4", "1/8", "1/16", "D1/8", "T1/8"];
                for (i, &label) in divisions.iter().enumerate() {
                    let active = self.delay_division == i;
                    if ui.add(Button::new(RichText::new(label).size(7.0)
                        .color(if active { Color32::BLACK } else { dim() }))
                        .fill(if active { Color32::from_rgb(6, 182, 212) } else { Color32::from_gray(28) })
                        .min_size(vec2(22.0, 12.0))).clicked() {
                        self.delay_division = i;
                        // Calculate delay time from BPM and division
                        let beat_sec = 60.0 / self.bpm;
                        let delay_time = match i {
                            0 => beat_sec,           // 1/4
                            1 => beat_sec / 2.0,     // 1/8
                            2 => beat_sec / 4.0,     // 1/16
                            3 => beat_sec * 0.75,    // dotted 1/8
                            4 => beat_sec / 3.0,     // triplet 1/8
                            _ => beat_sec / 2.0,
                        };
                        self.engine.send(Cmd::SetBpm(self.bpm)); // triggers delay time recalc in engine
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label(RichText::new("FLT").size(9.0).color(muted_color()).family(FontFamily::Monospace));
                let before = self.master_filter;
                ui.add(Slider::new(&mut self.master_filter, 100.0..=20000.0).show_value(false).logarithmic(true));
                let freq_text = if self.master_filter >= 1000.0 { format!("{:.1}k", self.master_filter / 1000.0) }
                    else { format!("{:.0}", self.master_filter) };
                ui.label(RichText::new(freq_text).size(10.0).color(dim()).family(FontFamily::Monospace));
                if self.master_filter != before { self.engine.send(Cmd::SetMasterFilter(self.master_filter)); }
            });

            ui.horizontal(|ui| {
                ui.label(RichText::new("WID").size(9.0).color(muted_color()).family(FontFamily::Monospace));
                let before = self.stereo_width;
                ui.add(Slider::new(&mut self.stereo_width, 0.0..=2.0).show_value(false));
                let width_text = if self.stereo_width < 0.1 { "MONO".to_string() }
                    else if (self.stereo_width - 1.0).abs() < 0.05 { "NORM".to_string() }
                    else { format!("{:.0}%", self.stereo_width * 100.0) };
                ui.label(RichText::new(width_text).size(10.0).color(dim()).family(FontFamily::Monospace));
                if self.stereo_width != before { self.engine.send(Cmd::SetStereoWidth(self.stereo_width)); }
            });

            // Master enhancer (Soundgoodizer-style one-knob mastering)
            ui.horizontal(|ui| {
                ui.label(RichText::new("ENH").size(9.0).color(Color32::from_rgb(168, 85, 247)).family(FontFamily::Monospace));
                let before = self.enhancer_amount;
                ui.add(Slider::new(&mut self.enhancer_amount, 0.0..=1.0).show_value(false));
                ui.label(RichText::new(format!("{}", (self.enhancer_amount * 100.0) as u32))
                    .size(10.0).color(Color32::from_rgb(168, 85, 247)).family(FontFamily::Monospace));
                if self.enhancer_amount != before { self.engine.send(Cmd::SetEnhancer(self.enhancer_amount)); }
            });

            ui.add_space(4.0);
            ui.separator();

            // ── Sample Browser ──────────────────────────
            let browser_label = if self.browser_open { "▼ BROWSER" } else { "▶ BROWSER" };
            if ui.add(Button::new(RichText::new(browser_label).size(9.0).color(dim()).family(FontFamily::Monospace)).frame(false)).clicked() {
                self.browser_open = !self.browser_open;
                if self.browser_open && self.browser_files.is_empty() {
                    self.scan_browser();
                }
            }

            if self.browser_open {
                // Current path
                let path_str = self.browser_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("/")
                    .to_string();
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("↑").size(10.0).color(dim())).clicked() {
                        if let Some(parent) = self.browser_path.parent() {
                            self.browser_path = parent.to_path_buf();
                            self.scan_browser();
                        }
                    }
                    ui.label(RichText::new(&path_str).size(8.0).color(dim()).family(FontFamily::Monospace));
                    if ui.button(RichText::new("⟳").size(10.0).color(dim())).clicked() {
                        self.scan_browser();
                    }
                });

                // File list
                ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                    let files = self.browser_files.clone();
                    for (name, path, is_dir) in &files {
                        let icon = if *is_dir { "📁 " } else { "♪ " };
                        let color = if *is_dir { dim() } else { Color32::from_rgb(34, 197, 94) };
                        let resp = ui.add(Button::new(
                            RichText::new(format!("{icon}{name}")).size(9.0).color(color).family(FontFamily::Monospace)
                        ).frame(false));

                        if *is_dir && resp.clicked() {
                            self.browser_path = path.clone();
                            self.scan_browser();
                        } else if !is_dir {
                            if resp.clicked() {
                                // Single click: preview (trigger on selected pad temporarily)
                                let sp = self.selected_pad;
                                self.load_sample_file(sp, path);
                                self.engine.send(Cmd::TriggerPad(sp, 1.0));
                            }
                            if resp.double_clicked() {
                                // Double-click: load permanently
                                let sp = self.selected_pad;
                                self.load_sample_file(sp, path);
                            }
                        }
                    }
                    if files.is_empty() {
                        ui.label(RichText::new("No audio files").size(9.0).color(muted_color()));
                    }
                });
            }
        });

        // ══════════════════════════════════════════════════
        // BOTTOM PANEL
        // ══════════════════════════════════════════════════
        TopBottomPanel::bottom("bottom").exact_height(200.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (view, label) in [
                    (BottomView::Editor, "SAMPLE EDITOR"),
                    (BottomView::Synth, "SYNTH"),
                    (BottomView::Mixer, "MIXER"),
                    (BottomView::Slicer, "SLICER"),
                    (BottomView::InsertFx, "INSERT FX"),
                ] {
                    let color = if self.bottom_view == view { accent() } else { dim() };
                    if ui.add(Button::new(RichText::new(label).size(9.0).color(color)).frame(false)).clicked() {
                        self.bottom_view = view;
                    }
                    if view != BottomView::Slicer { ui.label("|"); }
                }

                // Recording controls
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let rec_active = self.engine.shared.is_recording.load(Ordering::Relaxed);
                    let rec_color = if rec_active { red() } else { dim() };
                    let rec_text = if rec_active { "■ STOP REC" } else { "⏺ RECORD" };
                    if ui.add(Button::new(RichText::new(rec_text).size(9.0).color(rec_color))).clicked() {
                        if rec_active {
                            self.engine.send(Cmd::StopRecording);
                            self.is_recording = false;
                        } else {
                            self.engine.send(Cmd::StartRecording);
                            self.is_recording = true;
                            self.record_start_time = ui.input(|i| i.time);
                        }
                    }
                    if rec_active {
                        let elapsed = ui.input(|i| i.time) - self.record_start_time;
                        ui.label(RichText::new(format!("● {:.1}s", elapsed)).size(10.0).color(red()).family(FontFamily::Monospace));
                    }
                });
            });
            ui.separator();

            match self.bottom_view {
                BottomView::Editor => self.draw_sample_editor(ui),
                BottomView::Mixer => self.draw_mixer(ui, current_step),
                BottomView::Synth => self.draw_synth_editor(ui),
                BottomView::Slicer => self.draw_slicer(ui),
                BottomView::InsertFx => self.draw_insert_fx(ui),
            }
        });

        // ══════════════════════════════════════════════════
        // CENTRAL PANEL
        // ══════════════════════════════════════════════════
        CentralPanel::default().show(ctx, |ui| {
            // View tabs
            ui.horizontal(|ui| {
                for (view, label) in [(MainView::Sequencer, "SEQUENCER"), (MainView::PianoRoll, "PIANO ROLL"), (MainView::Arrangement, "ARRANGE")] {
                    let color = if self.main_view == view { accent() } else { dim() };
                    if ui.add(Button::new(RichText::new(label).size(9.0).color(color)).frame(false)).clicked() {
                        self.main_view = view;
                    }
                    if view != MainView::Arrangement { ui.label("|"); }
                }

                // Undo/Redo buttons
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(RichText::new("REDO").size(9.0).color(if self.redo_stack.is_empty() { muted_color() } else { dim() })).clicked() {
                        self.redo();
                    }
                    if ui.button(RichText::new("UNDO").size(9.0).color(if self.undo_stack.is_empty() { muted_color() } else { dim() })).clicked() {
                        self.undo();
                    }
                });
            });
            ui.separator();

            match self.main_view {
                MainView::Sequencer => self.draw_sequencer(ui, current_step),
                MainView::PianoRoll => self.draw_piano_roll(ui, current_step),
                MainView::Arrangement => self.draw_arrangement(ui),
            }
        });

        // ══════════════════════════════════════════════════
        // MODALS
        // ══════════════════════════════════════════════════
        if self.show_presets {
            Window::new("Presets")
                .collapsible(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if ui.button("Boom Bap").clicked() { self.load_preset(preset_boom_bap()); self.show_presets = false; }
                    if ui.button("Trap").clicked() { self.load_preset(preset_trap()); self.show_presets = false; }
                    if ui.button("House").clicked() { self.load_preset(preset_house()); self.show_presets = false; }
                    if ui.button(RichText::new("Clear").color(red())).clicked() {
                        self.banks[self.active_bank] = vec![vec![0u8; MAX_STEPS]; NUM_PADS];
                        self.sync_pattern();
                        self.show_presets = false;
                    }
                    if ui.button("Close").clicked() { self.show_presets = false; }
                });
        }

        if self.show_help {
            Window::new("Keyboard Shortcuts")
                .collapsible(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    Grid::new("shortcuts").num_columns(2).spacing([20.0, 4.0]).show(ui, |ui| {
                        let k = |t: &str| RichText::new(t).family(FontFamily::Monospace).strong().size(11.0);
                        ui.label(k("Space")); ui.label("Play / Stop"); ui.end_row();
                        ui.label(k("Esc")); ui.label("Stop & reset"); ui.end_row();
                        ui.label(k("↑ / ↓")); ui.label("BPM ±1 (Shift: ±10)"); ui.end_row();
                        ui.label(k("← / →")); ui.label("Select pad"); ui.end_row();
                        ui.label(k("Tab")); ui.label("Cycle views"); ui.end_row();
                        ui.label(k("T")); ui.label("Tap tempo"); ui.end_row();
                        ui.label(k("M")); ui.label("Mute selected pad"); ui.end_row();
                        ui.label(k("F1-F8")); ui.label("Switch pattern bank"); ui.end_row();
                        ui.label(k("F9")); ui.label("Solo selected pad"); ui.end_row();
                        ui.label(k("⌘S")); ui.label("Quick save"); ui.end_row();
                        ui.label(k("⌘N")); ui.label("New project"); ui.end_row();
                        ui.label(k("⌘Z / ⌘⇧Z")); ui.label("Undo / Redo"); ui.end_row();
                        ui.label(k("⌘C / ⌘V")); ui.label("Copy / Paste pattern"); ui.end_row();
                        ui.label(k("⌘D")); ui.label("Duplicate pattern to next bank"); ui.end_row();
                        ui.label(k("/")); ui.label("This help"); ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new("PAD MODE").size(9.0).strong().color(accent()));
                    Grid::new("pad_keys").num_columns(2).spacing([20.0, 2.0]).show(ui, |ui| {
                        let k = |t: &str| RichText::new(t).family(FontFamily::Monospace).strong().size(10.0);
                        ui.label(k("Z X C V")); ui.label("Pads 1-4"); ui.end_row();
                        ui.label(k("A S D F")); ui.label("Pads 5-8"); ui.end_row();
                        ui.label(k("Q W E R")); ui.label("Pads 9-12"); ui.end_row();
                        ui.label(k("1 2 3 4")); ui.label("Pads 13-16"); ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new("PIANO ROLL MODE").size(9.0).strong().color(accent()));
                    Grid::new("piano_keys").num_columns(2).spacing([20.0, 2.0]).show(ui, |ui| {
                        let k = |t: &str| RichText::new(t).family(FontFamily::Monospace).strong().size(10.0);
                        ui.label(k("Z..M")); ui.label("C3 to B3 (chromatic)"); ui.end_row();
                        ui.label(k("Q..E")); ui.label("C4 to E4"); ui.end_row();
                        ui.label(k("[ / ]")); ui.label("Octave down / up"); ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new("Click cell to toggle · Shift+Click for velocity\nDrag & drop audio files onto pads · Right-click to delete\nProbability lane: draw per-step trigger chance").size(10.0).color(dim()));
                    ui.add_space(4.0);
                    if ui.button("Close").clicked() { self.show_help = false; }
                });
        }

        // Export dialog
        if self.show_export {
            Window::new("Export WAV")
                .collapsible(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(RichText::new("EXPORT TO WAV").size(12.0).strong().color(accent()));
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label("Bars to export:");
                        ui.add(egui::DragValue::new(&mut self.export_bars).range(1..=32).speed(0.1));
                    });

                    let total_steps = self.export_bars * self.num_steps;
                    let duration = total_steps as f32 * 60.0 / self.bpm / 4.0;
                    ui.label(RichText::new(format!("{} steps · {:.1}s at {:.0} BPM", total_steps, duration, self.bpm))
                        .size(10.0).color(dim()));

                    ui.add_space(8.0);

                    if self.exporting {
                        let progress = if self.export_steps_remaining > 0 {
                            1.0 - self.export_steps_remaining as f32 / (self.export_bars * self.num_steps) as f32
                        } else { 1.0 };
                        ui.add(egui::ProgressBar::new(progress).text(format!("Exporting... {:.0}%", progress * 100.0)));
                    } else {
                        if ui.add(Button::new(RichText::new("Export WAV").size(11.0).strong().color(Color32::BLACK))
                            .fill(accent()).min_size(vec2(120.0, 28.0))).clicked() {
                            // Start export: begin recording + playback for N bars
                            self.sync_pattern();
                            self.engine.send(Cmd::StartRecording);
                            self.engine.send(Cmd::Play);
                            self.playing = true;
                            self.exporting = true;
                            self.export_steps_remaining = (self.export_bars * self.num_steps) as i32;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_export = false;
                        }
                    }
                });
        }

        // Export state machine: count down steps, then stop and save
        if self.exporting && self.playing {
            let step = current_step;
            if step >= 0 {
                self.export_steps_remaining -= 1;
                if self.export_steps_remaining <= 0 {
                    // Stop and save
                    self.engine.send(Cmd::Stop); self.engine.send(Cmd::AllNotesOff);
                    self.engine.send(Cmd::StopRecording);
                    self.playing = false;
                    self.exporting = false;
                    self.show_export = false;
                }
            }
        }

        // Pad context menu
        if let Some((pad_idx, menu_pos)) = self.pad_context_menu {
            Window::new("pad_ctx")
                .title_bar(false)
                .fixed_pos(menu_pos)
                .auto_sized()
                .show(ctx, |ui| {
                    ui.label(RichText::new(&self.pad_names[pad_idx]).size(10.0).strong().color(self.pad_colors[pad_idx]));
                    ui.separator();
                    if ui.button("Load Sample").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Audio", &["wav","wave","mp3","flac","ogg","aac","m4a"])
                            .pick_file() {
                            self.load_sample_file(pad_idx, &path);
                        }
                        self.pad_context_menu = None;
                    }
                    if ui.button("Assign Synth").clicked() {
                        self.synth_assigned[pad_idx] = true;
                        self.pad_types[pad_idx] = PadType::SubSynth;
                        self.pad_names[pad_idx] = format!("SYNTH {}", pad_idx + 1);
                        self.engine.send(Cmd::SetPadSynth(pad_idx, self.synth_params[pad_idx].clone()));
                        self.bottom_view = BottomView::Synth;
                        self.pad_context_menu = None;
                    }
                    if self.pad_types[pad_idx] == PadType::Sample {
                        if ui.button(RichText::new("Remove Sample").color(red())).clicked() {
                            self.engine.send(Cmd::RemoveSample(pad_idx));
                            let info = audio::default_pad_info();
                            self.pad_names[pad_idx] = info[pad_idx].name.to_string();
                            self.pad_types[pad_idx] = if info[pad_idx].has_voice { PadType::Synth } else { PadType::Empty };
                            self.pad_peaks[pad_idx] = None;
                            self.pad_context_menu = None;
                        }
                    }
                    if ui.button("Channel Settings").clicked() {
                        self.show_channel_settings = Some(pad_idx);
                        self.pad_context_menu = None;
                    }
                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.pad_context_menu = None;
                    }
                });
        }

        // Channel settings popup
        if let Some(ch) = self.show_channel_settings {
            let ch_name = self.pad_names[ch].clone();
            let ch_color = self.pad_colors[ch];
            Window::new(format!("Channel: {}", ch_name))
                .collapsible(false)
                .resizable(true)
                .default_size([500.0, 350.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&ch_name).size(14.0).strong().color(ch_color).family(FontFamily::Monospace));
                        ui.label(RichText::new(match self.pad_types[ch] {
                            PadType::Synth => "DRUM SYNTH",
                            PadType::SubSynth => "SUB SYNTH",
                            PadType::Sample => "SAMPLE",
                            PadType::Empty => "EMPTY",
                        }).size(9.0).color(dim()));
                        // Level meter
                        let level = self.engine.shared.get_pad_level(ch);
                        let db = if level > 0.001 { 20.0 * level.log10() } else { -60.0 };
                        ui.label(RichText::new(format!("{:.1} dB", db)).size(10.0)
                            .color(if db > -3.0 { red() } else { dim() }).family(FontFamily::Monospace));
                    });
                    ui.separator();

                    ui.columns(2, |cols| {
                        // LEFT: Mixer params
                        cols[0].label(RichText::new("MIXER").size(9.0).color(accent()).family(FontFamily::Monospace));
                        cols[0].horizontal(|ui| {
                            ui.label(RichText::new("VOL").size(8.0).color(muted_color()));
                            if ui.add(Slider::new(&mut self.volumes[ch], 0.0..=1.0).show_value(true)).changed() {
                                self.engine.send(Cmd::SetPadVol(ch, self.volumes[ch]));
                            }
                        });
                        cols[0].horizontal(|ui| {
                            ui.label(RichText::new("PAN").size(8.0).color(muted_color()));
                            if ui.add(Slider::new(&mut self.pans[ch], -1.0..=1.0).show_value(true)).changed() {
                                self.engine.send(Cmd::SetPadPan(ch, self.pans[ch]));
                            }
                        });
                        cols[0].horizontal(|ui| {
                            ui.label(RichText::new("PITCH").size(8.0).color(muted_color()));
                            if ui.add(Slider::new(&mut self.pitches[ch], -24.0..=24.0).step_by(1.0).show_value(true).suffix("st")).changed() {
                                self.engine.send(Cmd::SetPadPitch(ch, self.pitches[ch]));
                            }
                        });
                        cols[0].horizontal(|ui| {
                            ui.label(RichText::new("FILT").size(8.0).color(muted_color()));
                            if ui.add(Slider::new(&mut self.filters[ch], 100.0..=20000.0).logarithmic(true).show_value(true).suffix("Hz")).changed() {
                                self.engine.send(Cmd::SetPadFilter(ch, self.filters[ch]));
                            }
                        });
                        cols[0].horizontal(|ui| {
                            ui.label(RichText::new("ATK").size(8.0).color(muted_color()));
                            if ui.add(Slider::new(&mut self.pad_attack[ch], 0.0..=0.5).logarithmic(true).show_value(true).suffix("s")).changed() {
                                self.engine.send(Cmd::SetPadAttack(ch, self.pad_attack[ch]));
                            }
                        });
                        cols[0].horizontal(|ui| {
                            ui.label(RichText::new("REL").size(8.0).color(muted_color()));
                            if ui.add(Slider::new(&mut self.pad_release[ch], 0.0..=2.0).logarithmic(true).show_value(true).suffix("s")).changed() {
                                self.engine.send(Cmd::SetPadRelease(ch, self.pad_release[ch]));
                            }
                        });
                        cols[0].horizontal(|ui| {
                            ui.label(RichText::new("VERB").size(8.0).color(muted_color()));
                            if ui.add(Slider::new(&mut self.reverb_sends[ch], 0.0..=1.0).show_value(false)).changed() {
                                self.engine.send(Cmd::SetPadReverbSend(ch, self.reverb_sends[ch]));
                            }
                            ui.label(RichText::new("DLY").size(8.0).color(muted_color()));
                            if ui.add(Slider::new(&mut self.delay_sends[ch], 0.0..=1.0).show_value(false)).changed() {
                                self.engine.send(Cmd::SetPadDelaySend(ch, self.delay_sends[ch]));
                            }
                        });

                        // RIGHT: FX params
                        cols[1].label(RichText::new("INSERT FX").size(9.0).color(accent()).family(FontFamily::Monospace));
                        let p = &mut self.fx_params[ch];
                        let mut fx_changed = false;
                        cols[1].horizontal(|ui| {
                            ui.label(RichText::new("DIST").size(8.0).color(Color32::from_rgb(239, 68, 68)));
                            fx_changed |= ui.add(Slider::new(&mut p[0], 0.0..=1.0).show_value(false)).changed();
                            fx_changed |= ui.add(Slider::new(&mut p[1], 0.0..=1.0).show_value(false)).changed();
                        });
                        cols[1].horizontal(|ui| {
                            ui.label(RichText::new("CRUSH").size(8.0).color(Color32::from_rgb(168, 85, 247)));
                            fx_changed |= ui.add(Slider::new(&mut p[2], 1.0..=16.0).step_by(1.0).show_value(true)).changed();
                            fx_changed |= ui.add(Slider::new(&mut p[4], 0.0..=1.0).show_value(false)).changed();
                        });
                        cols[1].horizontal(|ui| {
                            ui.label(RichText::new("CHOR").size(8.0).color(Color32::from_rgb(6, 182, 212)));
                            fx_changed |= ui.add(Slider::new(&mut p[7], 0.0..=1.0).show_value(false)).changed();
                        });
                        cols[1].horizontal(|ui| {
                            ui.label(RichText::new("PHAS").size(8.0).color(Color32::from_rgb(34, 197, 94)));
                            fx_changed |= ui.add(Slider::new(&mut p[11], 0.0..=1.0).show_value(false)).changed();
                        });
                        if fx_changed {
                            self.engine.send(Cmd::SetPadDistortion { pad: ch, drive: p[0], mix: p[1] });
                            self.engine.send(Cmd::SetPadBitcrush { pad: ch, bits: p[2], rate: p[3], mix: p[4] });
                            self.engine.send(Cmd::SetPadChorus { pad: ch, rate: p[5], depth: p[6], mix: p[7] });
                            self.engine.send(Cmd::SetPadPhaser { pad: ch, rate: p[8], depth: p[9], feedback: p[10], mix: p[11] });
                        }

                        // EQ
                        cols[1].label(RichText::new("EQ").size(9.0).color(accent()).family(FontFamily::Monospace));
                        let eq = &mut self.eq_params[ch];
                        let mut eq_changed = false;
                        cols[1].horizontal(|ui| {
                            ui.label(RichText::new("LOW").size(8.0).color(red()));
                            eq_changed |= ui.add(Slider::new(&mut eq.low_gain, -12.0..=12.0).show_value(true).suffix("dB")).changed();
                        });
                        cols[1].horizontal(|ui| {
                            ui.label(RichText::new("MID").size(8.0).color(green()));
                            eq_changed |= ui.add(Slider::new(&mut eq.mid_gain, -12.0..=12.0).show_value(true).suffix("dB")).changed();
                        });
                        cols[1].horizontal(|ui| {
                            ui.label(RichText::new("HIGH").size(8.0).color(Color32::from_rgb(6, 182, 212)));
                            eq_changed |= ui.add(Slider::new(&mut eq.high_gain, -12.0..=12.0).show_value(true).suffix("dB")).changed();
                        });
                        if eq_changed {
                            self.engine.send(Cmd::SetPadEq(ch, eq.clone()));
                        }

                        // EQ frequency response curve
                        let curve_data = crate::eq::freq_response(eq, 44100.0, 80);
                        let (_, curve_painter) = cols[1].allocate_painter(vec2(cols[1].available_width(), 60.0), Sense::hover());
                        let cr = curve_painter.clip_rect();
                        curve_painter.rect_filled(cr, 3.0, Color32::from_gray(12));

                        // 0dB line
                        let zero_y = cr.center().y;
                        curve_painter.line_segment(
                            [pos2(cr.left(), zero_y), pos2(cr.right(), zero_y)],
                            Stroke::new(0.5, Color32::from_gray(30)),
                        );

                        // Draw curve
                        if curve_data.len() >= 2 {
                            let points: Vec<Pos2> = curve_data.iter().enumerate().map(|(i, &(_freq, db))| {
                                let x = cr.left() + (i as f32 / (curve_data.len() - 1) as f32) * cr.width();
                                let y = zero_y - (db / 18.0) * (cr.height() * 0.45); // ±18dB range
                                pos2(x, y.clamp(cr.top(), cr.bottom()))
                            }).collect();

                            for pair in points.windows(2) {
                                let color = if pair[0].y < zero_y { green() } else { red() };
                                curve_painter.line_segment([pair[0], pair[1]], Stroke::new(1.5, color));
                            }
                        }

                        // Frequency labels
                        for &(freq, label) in &[(100.0, "100"), (1000.0, "1k"), (10000.0, "10k")] {
                            let t = (freq as f64).log10() / (20000.0f64).log10();
                            let x = cr.left() + t as f32 * cr.width();
                            curve_painter.text(pos2(x, cr.bottom() - 6.0), Align2::CENTER_BOTTOM,
                                label, FontId::monospace(6.0), Color32::from_gray(40));
                        }
                    });

                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.show_channel_settings = None;
                    }
                });
        }

        // About dialog
        if self.show_about {
            Window::new("About BeatForge Studio")
                .collapsible(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(RichText::new("◈ BEATFORGE STUDIO").size(16.0).strong().color(accent()));
                    ui.label(RichText::new("v0.1.0").size(11.0).color(dim()).family(FontFamily::Monospace));
                    ui.add_space(8.0);
                    ui.label("A native beatmaking DAW built entirely in Rust.");
                    ui.label(RichText::new("No Electron. No web tech. Pure native performance.").size(10.0).color(dim()));
                    ui.add_space(8.0);
                    ui.label(RichText::new("AUDIO").size(9.0).strong().color(accent()));
                    ui.label(RichText::new("cpal (CoreAudio) · symphonia (MP3/FLAC/OGG/AAC) · hound (WAV)").size(9.0).color(dim()));
                    ui.label(RichText::new("UI").size(9.0).strong().color(accent()));
                    ui.label(RichText::new("egui/eframe · JetBrains Mono · Custom dark theme").size(9.0).color(dim()));
                    ui.label(RichText::new("MIDI").size(9.0).strong().color(accent()));
                    ui.label(RichText::new("midir (CoreMIDI on macOS)").size(9.0).color(dim()));
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("{} lines of Rust · 13 modules · 6.4MB binary", 8300)).size(9.0).color(muted_color()));
                    ui.add_space(4.0);
                    if ui.button("Close").clicked() { self.show_about = false; }
                });
        }

        // Status bar with mini spectrum
        TopBottomPanel::bottom("status").exact_height(28.0).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                // Beat counter during playback
                if self.playing && current_step >= 0 {
                    let bar = current_step as usize / 16 + 1;
                    let beat = (current_step as usize % 16) / 4 + 1;
                    let tick = current_step as usize % 4 + 1;
                    ui.label(RichText::new(format!("{bar}.{beat}.{tick}"))
                        .size(14.0).strong().color(accent()).family(FontFamily::Monospace));
                    ui.label(RichText::new("·").size(9.0).color(muted_color()));
                }
                if self.live_rec {
                    ui.label(RichText::new("● LIVE REC").size(9.0).strong().color(red()).family(FontFamily::Monospace));
                    ui.label(RichText::new("·").size(9.0).color(muted_color()));
                }
                // Clip indicator
                let master_level = self.engine.shared.get_master_level();
                if master_level > 0.95 {
                    ui.label(RichText::new("CLIP").size(9.0).strong().color(Color32::BLACK)
                        .background_color(red()));
                    ui.label(RichText::new("·").size(9.0).color(muted_color()));
                }
                // MIDI indicator
                if self.midi_connected.load(Ordering::Relaxed) {
                    ui.label(RichText::new("MIDI ●").size(8.0).color(green()).family(FontFamily::Monospace));
                    ui.label(RichText::new("·").size(9.0).color(muted_color()));
                }
                ui.label(RichText::new(format!(
                    "BEATFORGE · {:.0} BPM · {} CH · {} STEPS · BANK {}",
                    self.bpm,
                    self.active_pads().len(),
                    self.num_steps,
                    BANK_LABELS[self.active_bank],
                )).size(9.0).color(muted_color()).family(FontFamily::Monospace));

                // Mini spectrum analyzer (gradient: green → amber → red)
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (_, painter) = ui.allocate_painter(vec2(220.0, 22.0), Sense::hover());
                    let rect = painter.clip_rect();
                    painter.rect_filled(rect, 2.0, Color32::from_gray(8));
                    let bar_w = rect.width() / NUM_BINS as f32;
                    for i in 0..NUM_BINS {
                        let val = self.engine.spectrum.get_bin(i);
                        let h = val * (rect.height() - 2.0);
                        let x = rect.left() + i as f32 * bar_w + 1.0;
                        // Color based on level: green → amber → red
                        let color = if val > 0.7 {
                            Color32::from_rgb(239, 68, 68) // red
                        } else if val > 0.4 {
                            Color32::from_rgb(245, 158, 11) // amber
                        } else {
                            Color32::from_rgb(34, 197, 94) // green
                        };
                        painter.rect_filled(
                            Rect::from_min_size(pos2(x, rect.bottom() - h - 1.0), vec2((bar_w - 1.0).max(1.0), h)),
                            1.0, color_alpha(color, 200),
                        );
                    }
                });
            });
        });
    }
}

// ═══════════════════════════════════════════════════════════
//  DRAWING HELPERS
// ═══════════════════════════════════════════════════════════

impl BeatForge {
    fn active_pads(&self) -> Vec<usize> {
        (0..NUM_PADS).filter(|&i| self.pad_types[i] != PadType::Empty).collect()
    }

    fn draw_sequencer(&mut self, ui: &mut Ui, current_step: i32) {
        let pads = self.active_pads();
        let num_rows = pads.len();
        let num_steps = self.num_steps;

        // Zoom controls + AI generate
        ui.horizontal(|ui| {
            // Generate complementary patterns from kick
            if ui.add(Button::new(RichText::new("⚡ GENERATE").size(8.0)
                .color(Color32::BLACK)).fill(Color32::from_rgb(168, 85, 247))
                .min_size(vec2(65.0, 16.0))).clicked() {
                self.push_undo();
                let kick = self.banks[self.active_bank][0].clone();
                let seed = simple_rng();
                let kit = crate::generate::generate_full_kit(&kick[..self.num_steps], 0.6, seed);
                for (i, pat) in kit.iter().enumerate() {
                    if i < NUM_PADS {
                        for (s, &v) in pat.iter().enumerate() {
                            if s < self.num_steps {
                                self.banks[self.active_bank][i][s] = v;
                            }
                        }
                    }
                }
                self.sync_pattern();
            }

            ui.separator();
            ui.label(RichText::new(format!("ZOOM {:.0}%", self.seq_zoom * 100.0))
                .size(8.0).color(muted_color()).family(FontFamily::Monospace));
            if ui.add(Button::new(RichText::new("−").size(10.0).color(dim()))
                .min_size(vec2(16.0, 14.0))).clicked() {
                self.seq_zoom = (self.seq_zoom - 0.25).max(0.5);
            }
            if ui.add(Button::new(RichText::new("+").size(10.0).color(dim()))
                .min_size(vec2(16.0, 14.0))).clicked() {
                self.seq_zoom = (self.seq_zoom + 0.25).min(3.0);
            }
            if ui.add(Button::new(RichText::new("1:1").size(8.0).color(dim()))
                .min_size(vec2(22.0, 14.0))).clicked() {
                self.seq_zoom = 1.0;
            }
            if self.step_input {
                ui.label(RichText::new(format!("STEP: {}", self.step_cursor + 1))
                    .size(9.0).color(Color32::from_rgb(6, 182, 212)).family(FontFamily::Monospace));
            }

            // Loop region controls
            if self.loop_start.is_some() || self.loop_end.is_some() {
                let ls = self.loop_start.unwrap_or(0) + 1;
                let le = self.loop_end.unwrap_or(self.num_steps - 1) + 1;
                ui.label(RichText::new(format!("LOOP: {ls}-{le}"))
                    .size(8.0).color(Color32::from_rgb(34, 197, 94)).family(FontFamily::Monospace));
                if ui.add(Button::new(RichText::new("×").size(9.0).color(red()))
                    .min_size(vec2(14.0, 14.0))).clicked() {
                    self.loop_start = None;
                    self.loop_end = None;
                    self.sync_loop_region();
                }
            } else {
                ui.label(RichText::new("Shift+click step # to set loop").size(7.0).color(muted_color()));
            }
        });

        ScrollArea::both().show(ui, |ui| {
            let label_w = 90.0;
            let avail_w = ui.available_width() - label_w;
            let cell_w = (avail_w / num_steps as f32 * self.seq_zoom).min(48.0).max(8.0);
            let row_h = 28.0;
            let header_h = 16.0;
            let total_h = header_h + row_h * num_rows as f32 + 12.0;
            let grid_w = cell_w * num_steps as f32;

            let (response, painter) = ui.allocate_painter(
                vec2(label_w + grid_w, total_h),
                Sense::click(),
            );
            let rect = response.rect;
            let grid_left = rect.left() + label_w;

            // Step numbers with bar markers
            for step in 0..num_steps {
                let x = grid_left + step as f32 * cell_w + cell_w / 2.0;
                let is_current = step as i32 == current_step;
                let is_beat = step % 4 == 0;
                let is_bar = step % 16 == 0;
                let color = if is_current { accent() }
                    else if is_bar { Color32::from_gray(160) }
                    else if is_beat { Color32::from_gray(100) }
                    else { Color32::from_gray(45) };
                let label = if is_bar {
                    format!("{}:{}", step / 16 + 1, step % 16 + 1)
                } else {
                    format!("{}", step + 1)
                };
                painter.text(
                    pos2(x, rect.top() + 8.0), Align2::CENTER_CENTER,
                    &label, FontId::monospace(if is_bar { 8.0 } else { 7.0 }), color,
                );

                // Vertical beat/bar lines through the grid area
                if is_beat && step > 0 {
                    let line_alpha = if is_bar { 40 } else { 20 };
                    painter.line_segment(
                        [pos2(grid_left + step as f32 * cell_w, rect.top() + header_h),
                         pos2(grid_left + step as f32 * cell_w, rect.top() + header_h + num_rows as f32 * row_h)],
                        Stroke::new(if is_bar { 1.0 } else { 0.5 }, Color32::from_white_alpha(line_alpha)),
                    );
                }
            }

            // Rows
            for (ri, &pad_idx) in pads.iter().enumerate() {
                let y = rect.top() + header_h + ri as f32 * row_h;
                let color = self.pad_colors[pad_idx];
                let any_soloed = self.soloed.iter().any(|&s| s);
                let is_muted = self.muted[pad_idx]
                    || (any_soloed && !self.soloed[pad_idx]); // solo-in-place: dim non-soloed

                // Selected pad highlight
                let is_selected = pad_idx == self.selected_pad;
                if is_selected && !is_muted {
                    let highlight_rect = Rect::from_min_size(
                        pos2(rect.left(), y),
                        vec2(label_w + grid_w, row_h),
                    );
                    painter.rect_filled(highlight_rect, 0.0, Color32::from_white_alpha(5));
                }

                // Row label
                let label_alpha = if is_muted { 80 } else { 255 };
                painter.text(
                    pos2(rect.left() + 4.0, y + row_h / 2.0), Align2::LEFT_CENTER,
                    &self.pad_names[pad_idx], FontId::monospace(9.0),
                    color_alpha(color, label_alpha),
                );

                // M/S buttons
                let m_rect = Rect::from_min_size(pos2(rect.left() + 60.0, y + 6.0), vec2(12.0, 14.0));
                let s_rect = Rect::from_min_size(pos2(rect.left() + 74.0, y + 6.0), vec2(12.0, 14.0));

                painter.rect_filled(m_rect, 2.0, if self.muted[pad_idx] { red() } else { Color32::from_gray(28) });
                painter.text(m_rect.center(), Align2::CENTER_CENTER, "M", FontId::monospace(7.0),
                    if self.muted[pad_idx] { Color32::BLACK } else { Color32::from_gray(60) });

                painter.rect_filled(s_rect, 2.0, if self.soloed[pad_idx] { accent() } else { Color32::from_gray(28) });
                painter.text(s_rect.center(), Align2::CENTER_CENTER, "S", FontId::monospace(7.0),
                    if self.soloed[pad_idx] { Color32::BLACK } else { Color32::from_gray(60) });

                // Cells
                let row_alpha = if is_muted { 80 } else { 255 };
                for step in 0..num_steps {
                    let vel = self.banks[self.active_bank][pad_idx].get(step).copied().unwrap_or(0);
                    let cx = grid_left + step as f32 * cell_w;
                    let cell_rect = Rect::from_min_size(
                        pos2(cx + 1.0, y + 1.0),
                        vec2(cell_w - 2.0, row_h - 2.0),
                    );

                    let bg = if vel > 0 {
                        let alpha = match vel { 1 => 80, 2 => 150, _ => 220 };
                        color_alpha(color, (alpha as u16 * row_alpha as u16 / 255) as u8)
                    } else if step % 4 == 0 {
                        Color32::from_gray(22)
                    } else {
                        Color32::from_gray(16)
                    };

                    painter.rect_filled(cell_rect, 2.0, bg);

                    // Mini waveform inside active cells (for sample pads)
                    if vel > 0 {
                        if let Some(ref peaks) = self.pad_peaks[pad_idx] {
                            let n = peaks.len().min(cell_rect.width() as usize);
                            if n > 2 {
                                let bar_w = cell_rect.width() / n as f32;
                                let cy = cell_rect.center().y;
                                for pi in 0..n {
                                    let h = peaks[pi * peaks.len() / n] * cell_rect.height() * 0.6;
                                    painter.rect_filled(
                                        Rect::from_min_size(
                                            pos2(cell_rect.left() + pi as f32 * bar_w, cy - h * 0.5),
                                            vec2(bar_w.max(0.5), h.max(0.5)),
                                        ),
                                        0.0,
                                        color_alpha(color, (row_alpha as u16 * 180 / 255) as u8),
                                    );
                                }
                            }
                        }
                        painter.rect_stroke(cell_rect, 2.0, Stroke::new(0.5, color_alpha(color, row_alpha)));
                    }

                    // Current step (playback)
                    if step as i32 == current_step && self.playing {
                        painter.rect_stroke(cell_rect, 2.0, Stroke::new(1.5, accent()));
                        if vel > 0 {
                            painter.rect_filled(cell_rect.shrink(1.0), 2.0, color_alpha(color, 200));
                        }
                    }
                    // Step cursor (step input mode)
                    if self.step_input && !self.playing && step == self.step_cursor {
                        painter.rect_stroke(cell_rect, 2.0, Stroke::new(2.0, Color32::from_rgb(6, 182, 212)));
                    }
                }
            }

            // Loop region markers
            if let (Some(ls), Some(le)) = (self.loop_start, self.loop_end) {
                let lx_start = grid_left + ls as f32 * cell_w;
                let lx_end = grid_left + (le + 1) as f32 * cell_w;
                let loop_rect = Rect::from_min_max(
                    pos2(lx_start, rect.top()),
                    pos2(lx_end, rect.top() + header_h + num_rows as f32 * row_h),
                );
                // Dim areas outside the loop
                if ls > 0 {
                    painter.rect_filled(
                        Rect::from_min_max(pos2(grid_left, rect.top()), pos2(lx_start, loop_rect.bottom())),
                        0.0, Color32::from_black_alpha(60),
                    );
                }
                if le + 1 < num_steps {
                    painter.rect_filled(
                        Rect::from_min_max(pos2(lx_end, rect.top()), pos2(grid_left + num_steps as f32 * cell_w, loop_rect.bottom())),
                        0.0, Color32::from_black_alpha(60),
                    );
                }
                // Loop boundary lines
                painter.line_segment([pos2(lx_start, rect.top()), pos2(lx_start, loop_rect.bottom())],
                    Stroke::new(1.5, Color32::from_rgb(34, 197, 94)));
                painter.line_segment([pos2(lx_end, rect.top()), pos2(lx_end, loop_rect.bottom())],
                    Stroke::new(1.5, Color32::from_rgb(239, 68, 68)));
            }

            // Sweeping playhead line (vertical line through entire grid)
            if self.playing && current_step >= 0 && (current_step as usize) < num_steps {
                let ph_x = grid_left + current_step as f32 * cell_w + cell_w * 0.5;
                painter.line_segment(
                    [pos2(ph_x, rect.top()), pos2(ph_x, rect.top() + header_h + num_rows as f32 * row_h + 8.0)],
                    Stroke::new(2.0, Color32::from_rgba_premultiplied(245, 158, 11, 180)),
                );
                // Playhead triangle at top
                let tri_size = 5.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        pos2(ph_x - tri_size, rect.top()),
                        pos2(ph_x + tri_size, rect.top()),
                        pos2(ph_x, rect.top() + tri_size * 1.5),
                    ],
                    accent(),
                    Stroke::NONE,
                ));
            }

            // Step indicator bar (bottom)
            for step in 0..num_steps {
                let x = grid_left + step as f32 * cell_w + 1.0;
                let y = rect.top() + header_h + num_rows as f32 * row_h + 4.0;
                let bar_rect = Rect::from_min_size(pos2(x, y), vec2(cell_w - 2.0, 3.0));
                if step as i32 == current_step && self.playing {
                    painter.rect_filled(bar_rect, 1.5, accent());
                }
            }

            // Handle clicks
            if response.clicked() || response.secondary_clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    // Check M/S button clicks
                    for (ri, &pad_idx) in pads.iter().enumerate() {
                        let y = rect.top() + header_h + ri as f32 * row_h;
                        let m_rect = Rect::from_min_size(pos2(rect.left() + 60.0, y + 6.0), vec2(12.0, 14.0));
                        let s_rect = Rect::from_min_size(pos2(rect.left() + 74.0, y + 6.0), vec2(12.0, 14.0));

                        if m_rect.contains(pos) {
                            self.muted[pad_idx] = !self.muted[pad_idx];
                            self.engine.send(Cmd::SetPadMute(pad_idx, self.muted[pad_idx]));
                            return;
                        }
                        if s_rect.contains(pos) {
                            self.soloed[pad_idx] = !self.soloed[pad_idx];
                            self.engine.send(Cmd::SetPadSolo(pad_idx, self.soloed[pad_idx]));
                            return;
                        }
                    }

                    // Shift+click on step header → set loop region
                    let in_header = pos.y < rect.top() + header_h;
                    if response.clicked() && pos.x >= grid_left && in_header {
                        let step = ((pos.x - grid_left) / cell_w) as usize;
                        if step < num_steps && ui.input(|i| i.modifiers.shift) {
                            if self.loop_start.is_none() {
                                self.loop_start = Some(step);
                            } else if self.loop_end.is_none() {
                                let start = self.loop_start.unwrap();
                                if step > start {
                                    self.loop_end = Some(step);
                                } else {
                                    self.loop_end = self.loop_start;
                                    self.loop_start = Some(step);
                                }
                            } else {
                                // Reset and start new selection
                                self.loop_start = Some(step);
                                self.loop_end = None;
                            }
                            self.sync_loop_region();
                        }
                    }

                    // Click on row label → select pad
                    if response.clicked() && pos.x < grid_left {
                        let row = ((pos.y - rect.top() - header_h) / row_h) as usize;
                        if row < pads.len() {
                            self.selected_pad = pads[row];
                        }
                    }

                    // Right-click on row label → context menu
                    if response.secondary_clicked() && pos.x < grid_left {
                        let row = ((pos.y - rect.top() - header_h) / row_h) as usize;
                        if row < pads.len() {
                            self.context_menu_row = Some((pads[row], pos));
                            self.selected_pad = pads[row];
                        }
                    }

                    // Left-click OR left-drag on grid cells (paint mode)
                    let left_button = ui.input(|i| i.pointer.primary_down());
                    if (response.clicked() || (response.dragged() && left_button)) && pos.x >= grid_left {
                        let step = ((pos.x - grid_left) / cell_w) as usize;
                        let row = ((pos.y - rect.top() - header_h) / row_h) as usize;
                        if step < num_steps && row < pads.len() {
                            let pad_idx = pads[row];
                            let current = self.banks[self.active_bank][pad_idx][step];
                            let shift = ui.input(|i| i.modifiers.shift);

                            // On first click, push undo and decide paint mode (on or off)
                            if response.clicked() {
                                self.push_undo();
                            }

                            let new_val = if shift {
                                (current + 1) % 4
                            } else if response.clicked() {
                                // Toggle: if on → off, if off → on
                                if current > 0 { 0 } else { 3 }
                            } else {
                                // Drag: paint with the same mode as the initial click
                                // If dragging, always paint ON (most common use case)
                                3
                            };

                            if self.banks[self.active_bank][pad_idx][step] != new_val {
                                self.banks[self.active_bank][pad_idx][step] = new_val;
                                self.engine.send(Cmd::SetCell { pad: pad_idx, step, vel: new_val });
                            }
                        }
                    }

                    // Right-click (click OR drag) on grid cells = ERASE
                    let right_button_down = ui.input(|i| i.pointer.secondary_down());
                    if (response.secondary_clicked() || (response.dragged() && right_button_down)) && pos.x >= grid_left {
                        let step = ((pos.x - grid_left) / cell_w) as usize;
                        let row = ((pos.y - rect.top() - header_h) / row_h) as usize;
                        if step < num_steps && row < pads.len() {
                            let pad_idx = pads[row];
                            if self.banks[self.active_bank][pad_idx][step] > 0 {
                                if response.secondary_clicked() { self.push_undo(); }
                                self.banks[self.active_bank][pad_idx][step] = 0;
                                self.engine.send(Cmd::SetCell { pad: pad_idx, step, vel: 0 });
                            }
                        }
                    }
                }
            }
        });

        // ── Row context menu popup ──────────────────────
        if let Some((pad_idx, menu_pos)) = self.context_menu_row {
            let bank = self.active_bank;
            let num_steps = self.num_steps;
            let pad_name = self.pad_names[pad_idx].clone();
            let color = self.pad_colors[pad_idx];

            Window::new("row_menu")
                .title_bar(false)
                .fixed_pos(menu_pos)
                .auto_sized()
                .show(ui.ctx(), |ui| {
                    ui.label(RichText::new(&pad_name).size(10.0).strong().color(color));
                    ui.separator();

                    // Euclidean rhythm generator
                    ui.label(RichText::new("EUCLIDEAN").size(8.0).color(Color32::from_rgb(168, 85, 247)).family(FontFamily::Monospace));
                    ui.horizontal(|ui| {
                        let euclidean_presets = [
                            (3, "E(3)", "tresillo"),
                            (4, "E(4)", "4-on-floor"),
                            (5, "E(5)", "cinquillo"),
                            (7, "E(7)", "west african"),
                        ];
                        for (hits, label, _desc) in euclidean_presets {
                            if ui.add(Button::new(RichText::new(label).size(8.0).color(dim()))
                                .min_size(vec2(28.0, 16.0))).clicked() {
                                self.push_undo();
                                let pattern = euclidean_rhythm(hits, num_steps);
                                for (s, &v) in pattern.iter().enumerate() {
                                    if s < num_steps {
                                        self.banks[bank][pad_idx][s] = v;
                                    }
                                }
                                self.sync_pattern();
                                self.context_menu_row = None;
                            }
                        }
                    });
                    ui.separator();

                    if ui.button("Randomize (25%)").clicked() {
                        self.push_undo();
                        let mut rng = simple_rng();
                        for s in 0..num_steps {
                            self.banks[bank][pad_idx][s] = if rng_next(&mut rng) % 4 == 0 {
                                (rng_next(&mut rng) % 3 + 1) as u8
                            } else { 0 };
                        }
                        self.sync_pattern();
                        self.context_menu_row = None;
                    }
                    if ui.button("Randomize (50%)").clicked() {
                        self.push_undo();
                        let mut rng = simple_rng();
                        for s in 0..num_steps {
                            self.banks[bank][pad_idx][s] = if rng_next(&mut rng) % 2 == 0 {
                                (rng_next(&mut rng) % 3 + 1) as u8
                            } else { 0 };
                        }
                        self.sync_pattern();
                        self.context_menu_row = None;
                    }
                    if ui.button("Shift Left").clicked() {
                        self.push_undo();
                        let first = self.banks[bank][pad_idx][0];
                        for s in 0..num_steps - 1 {
                            self.banks[bank][pad_idx][s] = self.banks[bank][pad_idx][s + 1];
                        }
                        self.banks[bank][pad_idx][num_steps - 1] = first;
                        self.sync_pattern();
                        self.context_menu_row = None;
                    }
                    if ui.button("Shift Right").clicked() {
                        self.push_undo();
                        let last = self.banks[bank][pad_idx][num_steps - 1];
                        for s in (1..num_steps).rev() {
                            self.banks[bank][pad_idx][s] = self.banks[bank][pad_idx][s - 1];
                        }
                        self.banks[bank][pad_idx][0] = last;
                        self.sync_pattern();
                        self.context_menu_row = None;
                    }
                    if ui.button("Double").clicked() {
                        self.push_undo();
                        let half = num_steps / 2;
                        for s in 0..half.min(self.banks[bank][pad_idx].len()) {
                            let v = self.banks[bank][pad_idx][s];
                            if s + half < self.banks[bank][pad_idx].len() {
                                self.banks[bank][pad_idx][s + half] = v;
                            }
                        }
                        self.sync_pattern();
                        self.context_menu_row = None;
                    }
                    if ui.button("Halve").clicked() {
                        self.push_undo();
                        for s in (0..num_steps).step_by(2) {
                            let v = self.banks[bank][pad_idx][s];
                            self.banks[bank][pad_idx][s / 2] = v;
                        }
                        for s in num_steps / 2..num_steps {
                            self.banks[bank][pad_idx][s] = 0;
                        }
                        self.sync_pattern();
                        self.context_menu_row = None;
                    }
                    if ui.button(RichText::new("Clear Row").color(red())).clicked() {
                        self.push_undo();
                        for s in 0..num_steps {
                            self.banks[bank][pad_idx][s] = 0;
                        }
                        self.sync_pattern();
                        self.context_menu_row = None;
                    }
                    ui.separator();
                    // Clone pad settings to another pad
                    ui.label(RichText::new("CLONE TO:").size(8.0).color(muted_color()));
                    ui.horizontal(|ui| {
                        for target in 0..NUM_PADS {
                            if target == pad_idx { continue; }
                            if self.pad_types[target] == PadType::Empty || target >= 10 {
                                if ui.add(Button::new(RichText::new(format!("{}", target + 1)).size(8.0).color(dim()))
                                    .min_size(vec2(18.0, 14.0))).clicked() {
                                    // Clone mixer settings
                                    self.volumes[target] = self.volumes[pad_idx];
                                    self.pans[target] = self.pans[pad_idx];
                                    self.pitches[target] = self.pitches[pad_idx];
                                    self.filters[target] = self.filters[pad_idx];
                                    self.reversed[target] = self.reversed[pad_idx];
                                    self.fx_params[target] = self.fx_params[pad_idx];
                                    self.pad_names[target] = format!("{} (clone)", self.pad_names[pad_idx]);
                                    // Clone pattern
                                    for b in 0..self.banks.len() {
                                        self.banks[b][target] = self.banks[b][pad_idx].clone();
                                    }
                                    // Sync
                                    self.engine.send(Cmd::SetPadVol(target, self.volumes[target]));
                                    self.engine.send(Cmd::SetPadPan(target, self.pans[target]));
                                    self.engine.send(Cmd::SetPadPitch(target, self.pitches[target]));
                                    self.engine.send(Cmd::SetPadFilter(target, self.filters[target]));
                                    self.sync_pattern();
                                    self.context_menu_row = None;
                                }
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.context_menu_row = None;
                    }
                });

            // Close on click elsewhere
            if ui.ctx().input(|i| i.pointer.any_pressed()) {
                // Let the menu handle its own clicks first, close on next frame if still open
            }
        }

        // ── Lane visibility toggles ──
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let vel_color = if self.show_velocity_lane { accent() } else { dim() };
            if ui.add(Button::new(RichText::new("VEL").size(8.0).color(if self.show_velocity_lane { Color32::BLACK } else { vel_color }))
                .fill(if self.show_velocity_lane { accent() } else { Color32::from_gray(28) })
                .min_size(vec2(28.0, 14.0))).clicked() {
                self.show_velocity_lane = !self.show_velocity_lane;
            }
            let prob_color = if self.show_probability_lane { Color32::from_rgb(168, 85, 247) } else { dim() };
            if ui.add(Button::new(RichText::new("PROB").size(8.0).color(if self.show_probability_lane { Color32::BLACK } else { prob_color }))
                .fill(if self.show_probability_lane { Color32::from_rgb(168, 85, 247) } else { Color32::from_gray(28) })
                .min_size(vec2(28.0, 14.0))).clicked() {
                self.show_probability_lane = !self.show_probability_lane;
            }
            let auto_color = if self.show_automation { accent() } else { dim() };
            if ui.add(Button::new(RichText::new("AUTO").size(8.0).color(if self.show_automation { Color32::BLACK } else { auto_color }))
                .fill(if self.show_automation { accent() } else { Color32::from_gray(28) })
                .min_size(vec2(28.0, 14.0))).clicked() {
                self.show_automation = !self.show_automation;
            }
        });

        // ── Velocity lane (per-step velocity bars for selected pad) ──
        if self.show_velocity_lane {
            let sp = self.selected_pad;
            let num_steps = self.num_steps;
            let vel_h = 40.0;
            let label_w = 90.0;
            let avail_w = ui.available_width() - label_w;
            let cell_w = (avail_w / num_steps as f32 * self.seq_zoom).min(48.0).max(8.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("VEL — {}", self.pad_names[sp]))
                    .size(8.0).color(self.pad_colors[sp]).family(FontFamily::Monospace));
            });

            let (response, painter) = ui.allocate_painter(
                vec2(label_w + cell_w * num_steps as f32, vel_h),
                Sense::click_and_drag(),
            );
            let rect = response.rect;
            let grid_left = rect.left() + label_w;
            let color = self.pad_colors[sp];

            // Background
            painter.rect_filled(rect, 2.0, Color32::from_gray(12));

            // Velocity bars
            for step in 0..num_steps {
                let vel = self.banks[self.active_bank][sp].get(step).copied().unwrap_or(0);
                if vel > 0 {
                    let norm = vel as f32 / 3.0;
                    let x = grid_left + step as f32 * cell_w + 1.0;
                    let bar_h = norm * (vel_h - 4.0);
                    let bar_rect = Rect::from_min_size(
                        pos2(x, rect.bottom() - bar_h - 2.0),
                        vec2(cell_w - 2.0, bar_h),
                    );
                    let alpha = (norm * 200.0 + 55.0) as u8;
                    painter.rect_filled(bar_rect, 1.0, color_alpha(color, alpha));
                }

                // Grid line on beats
                if step % 4 == 0 {
                    painter.line_segment(
                        [pos2(grid_left + step as f32 * cell_w, rect.top()),
                         pos2(grid_left + step as f32 * cell_w, rect.bottom())],
                        Stroke::new(0.5, Color32::from_gray(30)),
                    );
                }
            }

            // Horizontal guide lines at each velocity level
            for level in 1..=3 {
                let y = rect.bottom() - (level as f32 / 3.0) * (vel_h - 4.0) - 2.0;
                painter.line_segment(
                    [pos2(grid_left, y), pos2(grid_left + cell_w * num_steps as f32, y)],
                    Stroke::new(0.3, Color32::from_gray(25)),
                );
            }

            // Click/drag to set velocity
            if response.dragged() || response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if pos.x >= grid_left {
                        let step = ((pos.x - grid_left) / cell_w) as usize;
                        let norm = 1.0 - ((pos.y - rect.top()) / vel_h).clamp(0.0, 1.0);
                        let vel = if norm < 0.1 { 0u8 } else if norm < 0.4 { 1 } else if norm < 0.7 { 2 } else { 3 };
                        if step < num_steps {
                            self.banks[self.active_bank][sp][step] = vel;
                            self.engine.send(Cmd::SetCell { pad: sp, step, vel });
                        }
                    }
                }
            }

            // Label
            painter.text(pos2(rect.left() + 4.0, rect.center().y), Align2::LEFT_CENTER,
                "VEL", FontId::monospace(8.0), dim());
        } // end velocity lane

        ui.add_space(2.0);

        // ── Probability lane ────────────────────────────
        if self.show_probability_lane {
            let sp = self.selected_pad;
            let num_steps = self.num_steps;
            let prob_h = 30.0;
            let label_w = 90.0;
            let avail_w = ui.available_width() - label_w;
            let cell_w = (avail_w / num_steps as f32 * self.seq_zoom).min(48.0).max(8.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("PROB — {}", self.pad_names[sp]))
                    .size(8.0).color(Color32::from_rgb(168, 85, 247)).family(FontFamily::Monospace));
            });

            let (response, painter) = ui.allocate_painter(
                vec2(label_w + cell_w * num_steps as f32, prob_h),
                Sense::click_and_drag(),
            );
            let rect = response.rect;
            let grid_left = rect.left() + label_w;

            painter.rect_filled(rect, 2.0, Color32::from_gray(10));

            for step in 0..num_steps {
                let prob = self.step_probability[sp].get(step).copied().unwrap_or(100);
                let norm = prob as f32 / 100.0;
                let x = grid_left + step as f32 * cell_w + 1.0;
                let bar_h = norm * (prob_h - 4.0);
                let bar_rect = Rect::from_min_size(
                    pos2(x, rect.bottom() - bar_h - 2.0),
                    vec2(cell_w - 2.0, bar_h),
                );
                let color = if prob >= 100 { Color32::from_rgb(168, 85, 247) }
                    else if prob >= 50 { Color32::from_rgb(120, 60, 180) }
                    else { Color32::from_rgb(80, 40, 120) };
                if self.banks[self.active_bank][sp].get(step).copied().unwrap_or(0) > 0 {
                    painter.rect_filled(bar_rect, 1.0, color);
                }

                if step % 4 == 0 {
                    painter.line_segment(
                        [pos2(grid_left + step as f32 * cell_w, rect.top()),
                         pos2(grid_left + step as f32 * cell_w, rect.bottom())],
                        Stroke::new(0.3, Color32::from_gray(25)),
                    );
                }
            }

            // Click/drag to set probability
            if response.dragged() || response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if pos.x >= grid_left {
                        let step = ((pos.x - grid_left) / cell_w) as usize;
                        let norm = 1.0 - ((pos.y - rect.top()) / prob_h).clamp(0.0, 1.0);
                        let prob = (norm * 100.0).round() as u8;
                        if step < num_steps && sp < self.step_probability.len() {
                            while self.step_probability[sp].len() <= step {
                                self.step_probability[sp].push(100);
                            }
                            self.step_probability[sp][step] = prob;
                            self.engine.send(Cmd::SetStepProbability(self.step_probability.clone()));
                        }
                    }
                }
            }

            painter.text(pos2(rect.left() + 4.0, rect.center().y), Align2::LEFT_CENTER,
                "PROB", FontId::monospace(8.0), Color32::from_rgb(120, 60, 180));
        }

        ui.add_space(4.0);

        // ── Automation lane ─────────────────────────────
        if self.show_automation {
            ui.horizontal(|ui| {
                let targets = [
                    (AutoTarget::FilterCutoff, "FILTER"),
                    (AutoTarget::Volume, "VOL"),
                    (AutoTarget::Pan, "PAN"),
                ];
                for (target, name) in targets {
                    let active = self.auto_target == target;
                    if ui.add(Button::new(RichText::new(name).size(8.0)
                        .color(if active { Color32::BLACK } else { dim() }))
                        .fill(if active { accent() } else { Color32::from_gray(28) })
                        .min_size(vec2(28.0, 16.0))).clicked() {
                        self.auto_target = target;
                    }
                }
                ui.label(RichText::new(format!("PAD: {}", self.pad_names[self.selected_pad]))
                    .size(8.0).color(self.pad_colors[self.selected_pad]).family(FontFamily::Monospace));
            });
            let sp = self.selected_pad;
            let num_steps = self.num_steps;
            let lane_h = 60.0;
            let label_w = 90.0;
            let avail_w = ui.available_width() - label_w;
            let cell_w = (avail_w / num_steps as f32 * self.seq_zoom).min(48.0).max(8.0);

            let (response, painter) = ui.allocate_painter(
                vec2(label_w + cell_w * num_steps as f32, lane_h),
                Sense::click_and_drag(),
            );
            let rect = response.rect;
            let grid_left = rect.left() + label_w;

            // Background
            painter.rect_filled(rect, 2.0, Color32::from_gray(12));

            // Find or create automation lane for this pad + target
            let lane_idx = self.automation.lanes.iter().position(|l| {
                l.pad == sp && l.target == self.auto_target
            });

            let lane_idx = lane_idx.unwrap_or_else(|| {
                self.automation.add_lane(self.auto_target, sp, num_steps);
                self.automation.lanes.len() - 1
            });

            // Draw automation points and lines
            let (min_val, max_val) = self.auto_target.range();
            let mut prev_point: Option<Pos2> = None;
            let color = self.pad_colors[sp];

            for step in 0..num_steps {
                let x = grid_left + step as f32 * cell_w + cell_w / 2.0;

                if let Some(val) = self.automation.lanes[lane_idx].get_interpolated(step) {
                    let norm = (val - min_val) / (max_val - min_val);
                    let y = rect.bottom() - norm * rect.height();
                    let point = pos2(x, y);

                    // Draw line from previous point
                    if let Some(prev) = prev_point {
                        painter.line_segment([prev, point], Stroke::new(1.5, color_alpha(color, 150)));
                    }

                    // Draw point if it's a set value (not interpolated)
                    if self.automation.lanes[lane_idx].values[step].is_some() {
                        painter.circle_filled(point, 4.0, color);
                        painter.circle_stroke(point, 4.0, Stroke::new(1.0, Color32::WHITE));
                    } else {
                        painter.circle_filled(point, 2.0, color_alpha(color, 100));
                    }

                    prev_point = Some(point);
                } else {
                    prev_point = None;
                }
            }

            // Grid lines
            for step in 0..num_steps {
                if step % 4 == 0 {
                    let x = grid_left + step as f32 * cell_w;
                    painter.line_segment(
                        [pos2(x, rect.top()), pos2(x, rect.bottom())],
                        Stroke::new(0.5, Color32::from_gray(30)),
                    );
                }
            }

            // Handle click/drag to set automation values
            if response.clicked() || response.dragged() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if pos.x >= grid_left {
                        let step = ((pos.x - grid_left) / cell_w) as usize;
                        let norm = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
                        let val = min_val + norm * (max_val - min_val);
                        if step < num_steps {
                            self.automation.lanes[lane_idx].set(step, val);
                            self.engine.send(Cmd::SetAutomation(self.automation.clone()));
                        }
                    }
                }
            }

            // Right-click to clear a point
            if response.secondary_clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if pos.x >= grid_left {
                        let step = ((pos.x - grid_left) / cell_w) as usize;
                        if step < num_steps {
                            self.automation.lanes[lane_idx].clear(step);
                            self.engine.send(Cmd::SetAutomation(self.automation.clone()));
                        }
                    }
                }
            }

            // Label
            painter.text(pos2(rect.left() + 4.0, rect.center().y), Align2::LEFT_CENTER,
                self.auto_target.name(), FontId::monospace(8.0), dim());
        }
    }

    fn draw_arrangement(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("SONG ARRANGEMENT").size(10.0).color(accent()).family(FontFamily::Monospace));

            // PLAY SONG button — builds the pattern chain and starts playback
            let play_color = if self.playing { red() } else { green() };
            if ui.add(Button::new(RichText::new(if self.playing { "■ STOP" } else { "▶ PLAY SONG" }).size(10.0)
                .strong().color(Color32::BLACK)).fill(play_color)).clicked() {
                if self.playing {
                    self.engine.send(Cmd::ClearSongMode);
                    self.engine.send(Cmd::Stop); self.engine.send(Cmd::AllNotesOff);
                    self.playing = false;
                } else {
                    // Build pattern chain from arrangement
                    let mut song: Vec<Vec<Vec<u8>>> = Vec::new();
                    for bar in &self.arrangement {
                        // Merge all tracks for this bar into one combined pattern
                        let mut combined = vec![vec![0u8; self.num_steps]; NUM_PADS];
                        for (track_idx, &bank_idx) in bar.iter().enumerate() {
                            if bank_idx < 8 {
                                // Map track to pad range: track 0=pads 0-3 (drums), 1=4-7, etc
                                let pad_start = track_idx * 3;
                                let bank = &self.banks[bank_idx as usize];
                                for pad in pad_start..(pad_start + 4).min(NUM_PADS) {
                                    for step in 0..self.num_steps.min(bank[pad].len()) {
                                        if bank[pad][step] > combined[pad][step] {
                                            combined[pad][step] = bank[pad][step];
                                        }
                                    }
                                }
                            }
                        }
                        song.push(combined);
                    }
                    self.engine.send(Cmd::SetSongMode(song));
                    self.sync_pattern();
                    self.engine.send(Cmd::Play);
                    self.playing = true;
                }
            }

            ui.separator();

            if ui.button(RichText::new("+ BAR").size(9.0).color(dim())).clicked() {
                if !self.arrangement.is_empty() {
                    let last = self.arrangement.last().unwrap().clone();
                    self.arrangement.push(last);
                } else {
                    self.arrangement.push(vec![0]);
                }
            }
            if self.arrangement.len() > 1 {
                if ui.button(RichText::new("- BAR").size(9.0).color(red())).clicked() {
                    self.arrangement.pop();
                }
            }
            ui.label(RichText::new(format!("{} bars", self.arrangement.len()))
                .size(9.0).color(dim()).family(FontFamily::Monospace));
        });
        ui.add_space(4.0);

        ScrollArea::both().show(ui, |ui| {
            let bar_w = 80.0;
            let row_h = 32.0;
            let label_w = 50.0;
            let num_bars = self.arrangement.len();

            // Header — bar numbers
            ui.horizontal(|ui| {
                ui.add_space(label_w);
                for bar in 0..num_bars {
                    let (r, p) = ui.allocate_painter(vec2(bar_w, 16.0), Sense::hover());
                    p.text(r.rect.center(), Align2::CENTER_CENTER,
                        &format!("{}", bar + 1), FontId::monospace(8.0),
                        if bar % 4 == 0 { Color32::from_gray(120) } else { Color32::from_gray(60) });
                }
            });

            // One row per track — click to cycle bank assignment
            let track_names = ["DRUMS", "BASS", "LEAD", "PAD", "FX"];
            let track_colors = [red(), Color32::from_rgb(0, 229, 255), accent(), Color32::from_rgb(179, 136, 255), Color32::from_rgb(105, 240, 174)];

            for (track_idx, (name, color)) in track_names.iter().zip(track_colors.iter()).enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(*name).size(9.0).color(*color).family(FontFamily::Monospace));
                    ui.add_space(label_w - 40.0);

                    for bar_idx in 0..num_bars {
                        // Ensure arrangement has enough tracks
                        while self.arrangement[bar_idx].len() <= track_idx {
                            self.arrangement[bar_idx].push(255); // 255 = empty
                        }

                        let bank_idx = self.arrangement[bar_idx][track_idx];
                        let is_empty = bank_idx >= 8;

                        let (resp, painter) = ui.allocate_painter(vec2(bar_w - 2.0, row_h - 2.0), Sense::click());
                        let rect = resp.rect;

                        if is_empty {
                            painter.rect_filled(rect, 3.0, Color32::from_gray(16));
                            painter.rect_stroke(rect, 3.0, Stroke::new(0.5, Color32::from_gray(30)));
                        } else {
                            let bg = color_alpha(*color, 40 + bank_idx as u8 * 15);
                            painter.rect_filled(rect, 3.0, bg);
                            painter.rect_stroke(rect, 3.0, Stroke::new(0.5, *color));
                            painter.text(rect.center(), Align2::CENTER_CENTER,
                                BANK_LABELS[bank_idx as usize], FontId::monospace(11.0), *color);
                        }

                        if resp.clicked() {
                            // Left click: cycle bank assignment
                            let new_val = if is_empty { 0 } else if bank_idx < 7 { bank_idx + 1 } else { 255 };
                            self.arrangement[bar_idx][track_idx] = new_val;
                        }
                        if resp.double_clicked() && !is_empty {
                            // Double-click: jump to this bank for editing
                            self.active_bank = bank_idx as usize;
                            self.sync_pattern();
                            self.main_view = MainView::Sequencer;
                        }
                    }
                });
            }

            ui.add_space(8.0);
            ui.label(RichText::new("Click cells to assign pattern banks (A-H). Empty = silent.")
                .size(9.0).color(muted_color()));
        });
    }

    fn draw_sample_editor(&mut self, ui: &mut Ui) {
        let sp = self.selected_pad;
        ui.horizontal(|ui| {
            // Waveform display
            let avail = ui.available_size();
            let wave_w = avail.x - 200.0;
            let wave_h = avail.y - 4.0;

            let (response, painter) = ui.allocate_painter(
                vec2(wave_w.max(100.0), wave_h.max(50.0)),
                Sense::click_and_drag(),
            );
            let rect = response.rect;

            // Background
            painter.rect_filled(rect, 4.0, Color32::from_rgb(10, 10, 12));

            if let Some(ref peaks) = self.pad_peaks[sp] {
                let color = self.pad_colors[sp];
                let n = peaks.len();
                let bar_w = rect.width() / n as f32;
                let center_y = rect.center().y;

                // Dim regions outside trim
                let trim_left = rect.left() + self.trim_start[sp] * rect.width();
                let trim_right = rect.left() + self.trim_end[sp] * rect.width();
                painter.rect_filled(
                    Rect::from_min_max(rect.left_top(), pos2(trim_left, rect.bottom())),
                    0.0, Color32::from_black_alpha(140),
                );
                painter.rect_filled(
                    Rect::from_min_max(pos2(trim_right, rect.top()), rect.right_bottom()),
                    0.0, Color32::from_black_alpha(140),
                );

                // Waveform bars
                for (i, &p) in peaks.iter().enumerate() {
                    let x = rect.left() + i as f32 * bar_w;
                    let h = p * rect.height() * 0.8;
                    let in_trim = (i as f32 / n as f32) >= self.trim_start[sp]
                        && (i as f32 / n as f32) <= self.trim_end[sp];
                    let c = if in_trim { color } else { color_alpha(color, 60) };
                    painter.rect_filled(
                        Rect::from_min_size(pos2(x, center_y - h / 2.0), vec2(bar_w.max(1.0), h.max(0.5))),
                        0.0, c,
                    );
                }

                // Center line
                painter.line_segment(
                    [pos2(rect.left(), center_y), pos2(rect.right(), center_y)],
                    Stroke::new(0.5, Color32::from_gray(40)),
                );

                // Trim handles
                painter.rect_filled(
                    Rect::from_min_size(pos2(trim_left - 2.0, rect.top()), vec2(4.0, rect.height())),
                    0.0, green(),
                );
                painter.rect_filled(
                    Rect::from_min_size(pos2(trim_right - 2.0, rect.top()), vec2(4.0, rect.height())),
                    0.0, red(),
                );

                // Playhead (shows current playback position)
                let play_pos = self.engine.shared.get_pad_play_pos(sp);
                if play_pos > 0.001 && play_pos < 0.999 {
                    let ph_x = rect.left() + play_pos * rect.width();
                    painter.line_segment(
                        [pos2(ph_x, rect.top()), pos2(ph_x, rect.bottom())],
                        Stroke::new(2.0, accent()),
                    );
                    // Request repaint to animate the playhead
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
                }

                // Click to preview from position
                if response.clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let x_frac = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                        // Set trim start to click position and trigger preview
                        self.trim_start[sp] = x_frac.min(self.trim_end[sp] - 0.01);
                        self.engine.send(Cmd::SetPadTrim(sp, self.trim_start[sp], self.trim_end[sp]));
                        self.engine.send(Cmd::TriggerPad(sp, 1.0));
                    }
                }

                // Drag for trim adjustment
                if response.dragged() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let x_frac = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                        let start_dist = (x_frac - self.trim_start[sp]).abs();
                        let end_dist = (x_frac - self.trim_end[sp]).abs();
                        if start_dist < end_dist {
                            self.trim_start[sp] = x_frac.min(self.trim_end[sp] - 0.01);
                        } else {
                            self.trim_end[sp] = x_frac.max(self.trim_start[sp] + 0.01);
                        }
                        self.engine.send(Cmd::SetPadTrim(sp, self.trim_start[sp], self.trim_end[sp]));
                    }
                }
            } else {
                // Empty state
                let text = if self.pad_types[sp] == PadType::Synth {
                    format!("{} — SYNTHESIZED", self.pad_names[sp])
                } else {
                    "DROP WAV FILE OR CLICK LOAD".to_string()
                };
                painter.text(rect.center(), Align2::CENTER_CENTER, &text,
                    FontId::monospace(11.0), muted_color());
            }

            // Right side controls
            ui.vertical(|ui| {
                ui.set_min_width(190.0);
                ui.label(RichText::new(&self.pad_names[sp]).size(10.0).strong().color(self.pad_colors[sp]).family(FontFamily::Monospace));
                ui.add_space(2.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("PITCH").size(8.0).color(muted_color()).family(FontFamily::Monospace));
                    let before = self.pitches[sp];
                    ui.add(Slider::new(&mut self.pitches[sp], -24.0..=24.0).show_value(true).step_by(1.0).suffix("st"));
                    if self.pitches[sp] != before { self.engine.send(Cmd::SetPadPitch(sp, self.pitches[sp])); }
                });

                ui.horizontal(|ui| {
                    ui.label(RichText::new("FILTER").size(8.0).color(muted_color()).family(FontFamily::Monospace));
                    let before = self.filters[sp];
                    ui.add(Slider::new(&mut self.filters[sp], 100.0..=20000.0).show_value(true).logarithmic(true).suffix("Hz"));
                    if self.filters[sp] != before { self.engine.send(Cmd::SetPadFilter(sp, self.filters[sp])); }
                });

                ui.horizontal(|ui| {
                    ui.label(RichText::new("VOL").size(8.0).color(muted_color()).family(FontFamily::Monospace));
                    let before = self.volumes[sp];
                    ui.add(Slider::new(&mut self.volumes[sp], 0.0..=1.0).show_value(true));
                    if self.volumes[sp] != before { self.engine.send(Cmd::SetPadVol(sp, self.volumes[sp])); }
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if self.pad_types[sp] == PadType::Sample {
                        let rev_fill = if self.reversed[sp] { accent() } else { Color32::from_gray(28) };
                        if ui.add(Button::new(RichText::new("REV").size(8.0)
                            .color(if self.reversed[sp] { Color32::BLACK } else { dim() }))
                            .fill(rev_fill)).clicked() {
                            self.reversed[sp] = !self.reversed[sp];
                            self.engine.send(Cmd::SetPadReverse(sp, self.reversed[sp]));
                        }
                        // Loop mode toggle
                        let loop_fill = if self.pad_loop[sp] { Color32::from_rgb(6, 182, 212) } else { Color32::from_gray(28) };
                        if ui.add(Button::new(RichText::new("LOOP").size(8.0)
                            .color(if self.pad_loop[sp] { Color32::BLACK } else { dim() }))
                            .fill(loop_fill)).clicked() {
                            self.pad_loop[sp] = !self.pad_loop[sp];
                            self.engine.send(Cmd::SetPadLoopMode(sp, self.pad_loop[sp]));
                        }
                    }
                    // Attack / Release
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("ATK").size(8.0).color(muted_color()));
                        let before = self.pad_attack[sp];
                        ui.add(Slider::new(&mut self.pad_attack[sp], 0.0..=0.5).show_value(false).logarithmic(true));
                        if self.pad_attack[sp] != before { self.engine.send(Cmd::SetPadAttack(sp, self.pad_attack[sp])); }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("REL").size(8.0).color(muted_color()));
                        let before = self.pad_release[sp];
                        ui.add(Slider::new(&mut self.pad_release[sp], 0.0..=2.0).show_value(false).logarithmic(true));
                        if self.pad_release[sp] != before { self.engine.send(Cmd::SetPadRelease(sp, self.pad_release[sp])); }
                    });
                    if ui.button(RichText::new("LOAD").size(9.0).color(accent())).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Audio", &["wav", "wave", "mp3", "flac", "ogg", "aac", "m4a"])
                            .pick_file()
                        {
                            self.load_sample_file(sp, &path);
                        }
                    }
                    // Mic record to pad
                    if self.mic.available {
                        if let Some(target_pad) = self.mic_recording_for_pad {
                            // Currently recording
                            let dur = self.mic.duration();
                            if ui.add(Button::new(RichText::new(format!("■ STOP ({:.1}s)", dur)).size(8.0)
                                .color(Color32::BLACK)).fill(red())).clicked() {
                                let data = self.mic.stop();
                                if !data.is_empty() {
                                    let sr = self.mic.sample_rate;
                                    let peaks = audio::compute_peaks(&data, 200);
                                    self.pad_peaks[target_pad] = Some(peaks);
                                    self.pad_names[target_pad] = "MIC REC".to_string();
                                    self.pad_types[target_pad] = PadType::Sample;
                                    self.sample_info = Some(format!("{:.2}s · {}Hz · {} samples", data.len() as f32 / sr as f32, sr, data.len()));
                                    self.engine.send(Cmd::LoadSample {
                                        pad: target_pad,
                                        data: std::sync::Arc::new(data),
                                        original_sr: sr,
                                    });
                                }
                                self.mic_recording_for_pad = None;
                            }
                        } else {
                            if ui.button(RichText::new("🎤 REC").size(8.0).color(red())).clicked() {
                                self.mic_recording_for_pad = Some(sp);
                                self.mic.start();
                            }
                        }
                    }
                    // Normalize (boost to peak)
                    if self.pad_peaks[sp].is_some() {
                        if ui.button(RichText::new("NORM").size(9.0).color(dim())).clicked() {
                            let peak = self.pad_peaks[sp].as_ref()
                                .map(|p| p.iter().fold(0.0f32, |a, &b| a.max(b)))
                                .unwrap_or(1.0);
                            if peak > 0.01 {
                                self.volumes[sp] = (self.volumes[sp] / peak).min(1.0);
                                self.engine.send(Cmd::SetPadVol(sp, self.volumes[sp]));
                            }
                        }
                    }
                    // Sample info
                    if let Some(ref info) = self.sample_info {
                        ui.label(RichText::new(info).size(8.0).color(muted_color()).family(FontFamily::Monospace));
                    }
                    // BPM detection result + match button
                    if let Some(detected) = self.detected_bpm {
                        ui.label(RichText::new(format!("~{:.0}BPM", detected))
                            .size(9.0).color(Color32::from_rgb(6, 182, 212)).family(FontFamily::Monospace));
                        if ui.button(RichText::new("MATCH").size(8.0).color(accent())).clicked() {
                            self.bpm = detected;
                            self.engine.send(Cmd::SetBpm(self.bpm));
                        }
                    }
                });
            });
        });
    }

    fn draw_mixer(&mut self, ui: &mut Ui, _current_step: i32) {
        let pads = self.active_pads();

        ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                let strip_w = 52.0;
                let avail_h = ui.available_height() - 8.0;

                for &pad_idx in &pads {
                    ui.vertical(|ui| {
                        ui.set_width(strip_w);
                        let color = self.pad_colors[pad_idx];

                        // Name
                        if ui.add(Button::new(RichText::new(&self.pad_names[pad_idx]).size(8.0).strong().color(color).family(FontFamily::Monospace)).frame(false)).double_clicked() {
                            self.show_channel_settings = Some(pad_idx);
                        }

                        // Fader (vertical)
                        let fader_h = (avail_h - 80.0).max(40.0);
                        let (fader_resp, fader_painter) = ui.allocate_painter(
                            vec2(strip_w, fader_h),
                            Sense::click_and_drag(),
                        );
                        let fr = fader_resp.rect;

                        // Level meter (left side)
                        let level = self.engine.shared.get_pad_level(pad_idx).clamp(0.0, 1.0);
                        let meter_rect = Rect::from_min_size(
                            pos2(fr.left(), fr.top()),
                            vec2(3.0, fr.height()),
                        );
                        fader_painter.rect_filled(meter_rect, 1.0, Color32::from_gray(10));
                        let meter_h = level * fr.height();
                        let meter_color = if level > 0.8 { red() } else if level > 0.5 { accent() } else { green() };
                        fader_painter.rect_filled(
                            Rect::from_min_size(pos2(fr.left(), fr.bottom() - meter_h), vec2(3.0, meter_h)),
                            1.0, meter_color,
                        );

                        // Fader track
                        let track = Rect::from_min_size(
                            pos2(fr.center().x, fr.top()),
                            vec2(6.0, fr.height()),
                        );
                        fader_painter.rect_filled(track, 3.0, Color32::from_gray(16));

                        // Fill
                        let fill_h = self.volumes[pad_idx] * fr.height();
                        fader_painter.rect_filled(
                            Rect::from_min_size(
                                pos2(track.left(), fr.bottom() - fill_h),
                                vec2(6.0, fill_h),
                            ),
                            3.0, color_alpha(color, 100),
                        );

                        // Handle
                        let handle_y = fr.bottom() - fill_h;
                        fader_painter.rect_filled(
                            Rect::from_min_size(pos2(fr.center().x - 6.0, handle_y - 3.0), vec2(16.0, 6.0)),
                            3.0, Color32::from_gray(180),
                        );

                        // Drag
                        if fader_resp.dragged() || fader_resp.clicked() {
                            if let Some(pos) = fader_resp.interact_pointer_pos() {
                                let val = 1.0 - ((pos.y - fr.top()) / fr.height()).clamp(0.0, 1.0);
                                self.volumes[pad_idx] = val;
                                self.engine.send(Cmd::SetPadVol(pad_idx, val));
                            }
                        }

                        // Value
                        ui.label(RichText::new(format!("{}", (self.volumes[pad_idx] * 100.0) as u32))
                            .size(9.0).color(dim()).family(FontFamily::Monospace));

                        // Pan slider
                        let before = self.pans[pad_idx];
                        ui.add(Slider::new(&mut self.pans[pad_idx], -1.0..=1.0).show_value(false));
                        if self.pans[pad_idx] != before { self.engine.send(Cmd::SetPadPan(pad_idx, self.pans[pad_idx])); }

                        // Mini EQ — 3 knobs (L/M/H gain)
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 1.0;
                            let eq = &mut self.eq_params[pad_idx];
                            let mut eq_changed = false;
                            // Low
                            let (r, p) = ui.allocate_painter(vec2(14.0, 14.0), Sense::click_and_drag());
                            let low_norm = (eq.low_gain + 12.0) / 24.0;
                            p.rect_filled(r.rect, 2.0, Color32::from_gray(20));
                            p.rect_filled(Rect::from_min_size(
                                pos2(r.rect.left(), r.rect.bottom() - low_norm * r.rect.height()),
                                vec2(r.rect.width(), low_norm * r.rect.height())),
                                2.0, color_alpha(red(), 100));
                            p.text(r.rect.center(), Align2::CENTER_CENTER, "L", FontId::monospace(6.0), dim());
                            if r.dragged() { if let Some(pos) = r.interact_pointer_pos() {
                                eq.low_gain = (1.0 - (pos.y - r.rect.top()) / r.rect.height()).clamp(0.0, 1.0) * 24.0 - 12.0;
                                eq_changed = true;
                            }}
                            // Mid
                            let (r, p) = ui.allocate_painter(vec2(14.0, 14.0), Sense::click_and_drag());
                            let mid_norm = (eq.mid_gain + 12.0) / 24.0;
                            p.rect_filled(r.rect, 2.0, Color32::from_gray(20));
                            p.rect_filled(Rect::from_min_size(
                                pos2(r.rect.left(), r.rect.bottom() - mid_norm * r.rect.height()),
                                vec2(r.rect.width(), mid_norm * r.rect.height())),
                                2.0, color_alpha(green(), 100));
                            p.text(r.rect.center(), Align2::CENTER_CENTER, "M", FontId::monospace(6.0), dim());
                            if r.dragged() { if let Some(pos) = r.interact_pointer_pos() {
                                eq.mid_gain = (1.0 - (pos.y - r.rect.top()) / r.rect.height()).clamp(0.0, 1.0) * 24.0 - 12.0;
                                eq_changed = true;
                            }}
                            // High
                            let (r, p) = ui.allocate_painter(vec2(14.0, 14.0), Sense::click_and_drag());
                            let high_norm = (eq.high_gain + 12.0) / 24.0;
                            p.rect_filled(r.rect, 2.0, Color32::from_gray(20));
                            p.rect_filled(Rect::from_min_size(
                                pos2(r.rect.left(), r.rect.bottom() - high_norm * r.rect.height()),
                                vec2(r.rect.width(), high_norm * r.rect.height())),
                                2.0, color_alpha(Color32::from_rgb(0, 229, 255), 100));
                            p.text(r.rect.center(), Align2::CENTER_CENTER, "H", FontId::monospace(6.0), dim());
                            if r.dragged() { if let Some(pos) = r.interact_pointer_pos() {
                                eq.high_gain = (1.0 - (pos.y - r.rect.top()) / r.rect.height()).clamp(0.0, 1.0) * 24.0 - 12.0;
                                eq_changed = true;
                            }}
                            if eq_changed {
                                self.engine.send(Cmd::SetPadEq(pad_idx, eq.clone()));
                            }
                        });

                        // Send knobs (reverb/delay)
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            // Reverb send
                            let rv_before = self.reverb_sends[pad_idx];
                            let (r, p) = ui.allocate_painter(vec2(14.0, 14.0), Sense::click_and_drag());
                            let rv_norm = self.reverb_sends[pad_idx];
                            p.rect_filled(r.rect, 2.0, Color32::from_gray(20));
                            p.rect_filled(Rect::from_min_size(
                                pos2(r.rect.left(), r.rect.bottom() - rv_norm * r.rect.height()),
                                vec2(r.rect.width(), rv_norm * r.rect.height())),
                                2.0, color_alpha(Color32::from_rgb(168, 85, 247), 120));
                            p.text(r.rect.center(), Align2::CENTER_CENTER, "R", FontId::monospace(6.0), dim());
                            if r.dragged() { if let Some(pos) = r.interact_pointer_pos() {
                                self.reverb_sends[pad_idx] = (1.0 - (pos.y - r.rect.top()) / r.rect.height()).clamp(0.0, 1.0);
                                self.engine.send(Cmd::SetPadReverbSend(pad_idx, self.reverb_sends[pad_idx]));
                            }}
                            // Delay send
                            let (r, p) = ui.allocate_painter(vec2(14.0, 14.0), Sense::click_and_drag());
                            let dl_norm = self.delay_sends[pad_idx];
                            p.rect_filled(r.rect, 2.0, Color32::from_gray(20));
                            p.rect_filled(Rect::from_min_size(
                                pos2(r.rect.left(), r.rect.bottom() - dl_norm * r.rect.height()),
                                vec2(r.rect.width(), dl_norm * r.rect.height())),
                                2.0, color_alpha(Color32::from_rgb(6, 182, 212), 120));
                            p.text(r.rect.center(), Align2::CENTER_CENTER, "D", FontId::monospace(6.0), dim());
                            if r.dragged() { if let Some(pos) = r.interact_pointer_pos() {
                                self.delay_sends[pad_idx] = (1.0 - (pos.y - r.rect.top()) / r.rect.height()).clamp(0.0, 1.0);
                                self.engine.send(Cmd::SetPadDelaySend(pad_idx, self.delay_sends[pad_idx]));
                            }}
                        });

                        // M/S/SC
                        ui.horizontal(|ui| {
                            let m_fill = if self.muted[pad_idx] { red() } else { Color32::from_gray(28) };
                            let s_fill = if self.soloed[pad_idx] { accent() } else { Color32::from_gray(28) };
                            if ui.add(Button::new(RichText::new("M").size(7.0)
                                .color(if self.muted[pad_idx] { Color32::BLACK } else { dim() }))
                                .fill(m_fill).min_size(vec2(14.0, 13.0))).clicked() {
                                self.muted[pad_idx] = !self.muted[pad_idx];
                                self.engine.send(Cmd::SetPadMute(pad_idx, self.muted[pad_idx]));
                            }
                            if ui.add(Button::new(RichText::new("S").size(7.0)
                                .color(if self.soloed[pad_idx] { Color32::BLACK } else { dim() }))
                                .fill(s_fill).min_size(vec2(14.0, 13.0))).clicked() {
                                self.soloed[pad_idx] = !self.soloed[pad_idx];
                                self.engine.send(Cmd::SetPadSolo(pad_idx, self.soloed[pad_idx]));
                            }
                            // Sidechain from kick (pad 0)
                            if pad_idx != 0 {
                                let sc_active = self.sidechain_active[pad_idx];
                                let sc_fill = if sc_active { Color32::from_rgb(168, 85, 247) } else { Color32::from_gray(28) };
                                if ui.add(Button::new(RichText::new("SC").size(6.0)
                                    .color(if sc_active { Color32::BLACK } else { Color32::from_gray(60) }))
                                    .fill(sc_fill).min_size(vec2(16.0, 13.0))).clicked() {
                                    self.sidechain_active[pad_idx] = !sc_active;
                                    if !sc_active {
                                        self.engine.send(Cmd::SetSidechain {
                                            source: 0, target: pad_idx, amount: 0.8,
                                        });
                                    } else {
                                        self.engine.send(Cmd::ClearSidechain(pad_idx));
                                    }
                                }
                            }
                        });
                        // Bus routing selector
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 1.0;
                            let labels = ["M", "B1", "B2", "B3"];
                            for (b, &lbl) in labels.iter().enumerate() {
                                let active = self.channel_bus[pad_idx] == b as u8;
                                let fill = if active { Color32::from_rgb(168, 85, 247) } else { Color32::from_gray(24) };
                                if ui.add(Button::new(RichText::new(lbl).size(6.0)
                                    .color(if active { Color32::BLACK } else { Color32::from_gray(55) }))
                                    .fill(fill).min_size(vec2(12.0, 11.0))).clicked() {
                                    self.channel_bus[pad_idx] = b as u8;
                                    self.engine.send(Cmd::SetChannelBus(pad_idx, b as u8));
                                }
                            }
                        });
                    });
                }

                // Bus channels (13/14/15) — show as labeled separators
                for bus in 0..3u8 {
                    let bus_pad = 13 + bus as usize;
                    if bus_pad < NUM_PADS {
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.set_width(40.0);
                            ui.label(RichText::new(format!("BUS{}", bus + 1)).size(7.0)
                                .color(Color32::from_rgb(168, 85, 247)).family(FontFamily::Monospace));
                            // Compact fader
                            let (r, p) = ui.allocate_painter(vec2(40.0, 60.0), Sense::click_and_drag());
                            let fr = r.rect;
                            p.rect_filled(Rect::from_min_size(pos2(fr.center().x - 2.0, fr.top()), vec2(4.0, fr.height())),
                                2.0, Color32::from_gray(16));
                            let fill_h = self.volumes[bus_pad] * fr.height();
                            p.rect_filled(Rect::from_min_size(pos2(fr.center().x - 2.0, fr.bottom() - fill_h), vec2(4.0, fill_h)),
                                2.0, color_alpha(Color32::from_rgb(168, 85, 247), 100));
                            if r.dragged() || r.clicked() {
                                if let Some(pos) = r.interact_pointer_pos() {
                                    self.volumes[bus_pad] = 1.0 - ((pos.y - fr.top()) / fr.height()).clamp(0.0, 1.0);
                                    self.engine.send(Cmd::SetPadVol(bus_pad, self.volumes[bus_pad]));
                                }
                            }
                            // Count routed channels
                            let routed = (0..13).filter(|&i| self.channel_bus[i] == bus + 1).count();
                            ui.label(RichText::new(format!("{} ch", routed)).size(7.0).color(dim()).family(FontFamily::Monospace));
                        });
                    }
                }

                // Master strip
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(60.0);
                    ui.label(RichText::new("MASTER").size(8.0).strong().color(accent()).family(FontFamily::Monospace));

                    let fader_h = (ui.available_height() - 50.0).max(40.0);
                    let (fader_resp, fader_painter) = ui.allocate_painter(
                        vec2(60.0, fader_h),
                        Sense::click_and_drag(),
                    );
                    let fr = fader_resp.rect;

                    // Master level meter
                    let m_level = self.engine.shared.get_master_level().clamp(0.0, 1.0);
                    let m_meter_h = m_level * fr.height();
                    let m_meter_color = if m_level > 0.8 { red() } else if m_level > 0.5 { accent() } else { green() };
                    fader_painter.rect_filled(
                        Rect::from_min_size(pos2(fr.left(), fr.top()), vec2(3.0, fr.height())),
                        1.0, Color32::from_gray(10),
                    );
                    fader_painter.rect_filled(
                        Rect::from_min_size(pos2(fr.left(), fr.bottom() - m_meter_h), vec2(3.0, m_meter_h)),
                        1.0, m_meter_color,
                    );
                    // Right meter (stereo pair)
                    fader_painter.rect_filled(
                        Rect::from_min_size(pos2(fr.right() - 3.0, fr.top()), vec2(3.0, fr.height())),
                        1.0, Color32::from_gray(10),
                    );
                    fader_painter.rect_filled(
                        Rect::from_min_size(pos2(fr.right() - 3.0, fr.bottom() - m_meter_h * 0.95), vec2(3.0, m_meter_h * 0.95)),
                        1.0, m_meter_color,
                    );

                    let track = Rect::from_min_size(pos2(fr.center().x - 3.0, fr.top()), vec2(6.0, fr.height()));
                    fader_painter.rect_filled(track, 3.0, Color32::from_gray(16));
                    let fill_h = self.master_vol * fr.height();
                    fader_painter.rect_filled(
                        Rect::from_min_size(pos2(track.left(), fr.bottom() - fill_h), vec2(6.0, fill_h)),
                        3.0, color_alpha(accent(), 100),
                    );
                    let handle_y = fr.bottom() - fill_h;
                    fader_painter.rect_filled(
                        Rect::from_min_size(pos2(fr.center().x - 8.0, handle_y - 3.0), vec2(16.0, 6.0)),
                        3.0, Color32::from_gray(200),
                    );
                    if fader_resp.dragged() || fader_resp.clicked() {
                        if let Some(pos) = fader_resp.interact_pointer_pos() {
                            self.master_vol = 1.0 - ((pos.y - fr.top()) / fr.height()).clamp(0.0, 1.0);
                            self.engine.send(Cmd::SetMasterVol(self.master_vol));
                        }
                    }
                    ui.label(RichText::new(format!("{}", (self.master_vol * 100.0) as u32))
                        .size(10.0).color(accent()).family(FontFamily::Monospace));
                    // Master dB readout
                    let m_level = self.engine.shared.get_master_level();
                    let db = if m_level > 0.001 { 20.0 * m_level.log10() } else { -60.0 };
                    let db_color = if db > -3.0 { red() } else if db > -12.0 { accent() } else { green() };
                    ui.label(RichText::new(format!("{:.1}dB", db)).size(9.0).color(db_color).family(FontFamily::Monospace));
                });
            });
        });
    }

    // ═══════════════════════════════════════════════════════
    //  PIANO ROLL
    // ═══════════════════════════════════════════════════════

    fn draw_piano_roll(&mut self, ui: &mut Ui, current_step: i32) {
        let sp = self.selected_pad;
        let num_steps = self.num_steps;
        let note_range = 36..=96; // C2 to C7 (5 octaves)
        let num_notes = *note_range.end() - *note_range.start() + 1;

        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("PIANO ROLL — {}", self.pad_names[sp]))
                .size(10.0).color(self.pad_colors[sp]).family(FontFamily::Monospace));

            if !self.synth_assigned[sp] {
                if ui.button(RichText::new("ASSIGN SYNTH").size(9.0).color(accent())).clicked() {
                    self.synth_assigned[sp] = true;
                    self.pad_types[sp] = PadType::SubSynth;
                    self.pad_names[sp] = format!("SYNTH {}", sp + 1);
                    self.engine.send(Cmd::SetPadSynth(sp, self.synth_params[sp].clone()));
                }
            }

            ui.separator();

            // Octave indicator
            let oct_text = match self.keyboard_octave {
                0 => "C3-E4".to_string(),
                n => format!("C{}-E{}", 3 + n, 4 + n),
            };
            ui.label(RichText::new(format!("OCT: {} [/]", oct_text))
                .size(8.0).color(Color32::from_rgb(6, 182, 212)).family(FontFamily::Monospace));

            ui.separator();

            // Snap grid selector
            ui.label(RichText::new("SNAP").size(8.0).color(muted_color()).family(FontFamily::Monospace));
            let snaps = [(1.0, "1"), (0.5, "1/2"), (0.25, "1/4")];
            for (val, label) in snaps {
                let active = (self.piano_snap - val).abs() < 0.01;
                if ui.add(Button::new(RichText::new(label).size(7.0)
                    .color(if active { Color32::BLACK } else { dim() }))
                    .fill(if active { accent() } else { Color32::from_gray(28) })
                    .min_size(vec2(20.0, 14.0))).clicked() {
                    self.piano_snap = val;
                }
            }

            ui.separator();

            // Scale selector
            ui.label(RichText::new("SCALE").size(8.0).color(muted_color()).family(FontFamily::Monospace));
            let scales = [Scale::Chromatic, Scale::Major, Scale::Minor, Scale::Pentatonic, Scale::Blues, Scale::Dorian, Scale::Mixolydian];
            for scale in scales {
                let active = self.piano_scale == scale;
                if ui.add(Button::new(RichText::new(scale.name()).size(7.0)
                    .color(if active { Color32::BLACK } else { dim() }))
                    .fill(if active { accent() } else { Color32::from_gray(28) })
                    .min_size(vec2(28.0, 14.0))).clicked() {
                    self.piano_scale = scale;
                }
            }
            // Quantize button
            if ui.button(RichText::new("QUANT").size(8.0).color(dim())).clicked() {
                self.push_undo();
                for note in &mut self.note_patterns[sp].notes {
                    note.start = note.start.round();
                    note.duration = note.duration.round().max(1.0);
                }
                self.engine.send(Cmd::SetNotePattern {
                    pad: sp,
                    pattern: self.note_patterns[sp].clone(),
                });
            }

            // Chord stamps (one-click chord insertion)
            ui.separator();
            ui.label(RichText::new("CHORD").size(8.0).color(muted_color()).family(FontFamily::Monospace));
            let chord_types: &[(&str, &[i32])] = &[
                ("MAJ", &[0, 4, 7]),
                ("MIN", &[0, 3, 7]),
                ("7th", &[0, 4, 7, 10]),
                ("m7", &[0, 3, 7, 10]),
                ("dim", &[0, 3, 6]),
                ("aug", &[0, 4, 8]),
                ("sus4", &[0, 5, 7]),
            ];
            // Find the last placed note as the root
            let last_note = self.note_patterns[sp].notes.last().map(|n| (n.note, n.start));
            for &(name, intervals) in chord_types {
                if ui.add(Button::new(RichText::new(name).size(7.0).color(dim()))
                    .min_size(vec2(24.0, 14.0))).clicked() {
                    if let Some((root, start)) = last_note {
                        self.push_undo();
                        // Remove the single root note (we'll replace with chord)
                        self.note_patterns[sp].notes.retain(|n| !(n.note == root && n.start == start));
                        // Add all chord tones
                        let snap = self.piano_snap;
                        for &interval in intervals {
                            let note = (root as i32 + interval).clamp(0, 127) as u8;
                            self.note_patterns[sp].add_note(note, start, snap, 0.8);
                        }
                        self.engine.send(Cmd::SetNotePattern {
                            pad: sp,
                            pattern: self.note_patterns[sp].clone(),
                        });
                        // Preview the chord
                        for &interval in intervals {
                            let note = (root as i32 + interval).clamp(0, 127) as u8;
                            self.engine.send(Cmd::NoteOn { pad: sp, note, velocity: 0.7 });
                        }
                    }
                }
            }

            // Clear piano roll
            if ui.button(RichText::new("CLEAR").size(8.0).color(red())).clicked() {
                self.push_undo();
                self.note_patterns[sp].notes.clear();
                self.engine.send(Cmd::SetNotePattern {
                    pad: sp,
                    pattern: self.note_patterns[sp].clone(),
                });
            }
        });

        ScrollArea::both().show(ui, |ui| {
            let key_w = 40.0;
            let cell_w = 24.0;
            let cell_h = 12.0;
            let total_w = key_w + cell_w * num_steps as f32;
            let total_h = cell_h * num_notes as f32;

            let (response, painter) = ui.allocate_painter(
                vec2(total_w, total_h),
                Sense::click(),
            );
            let rect = response.rect;
            let grid_left = rect.left() + key_w;

            // Draw piano keys
            for note in note_range.clone() {
                let row = (*note_range.end() - note) as f32;
                let y = rect.top() + row * cell_h;
                let is_black = matches!(note % 12, 1 | 3 | 6 | 8 | 10);
                let key_color = if is_black { Color32::from_gray(30) } else { Color32::from_gray(45) };
                let key_rect = Rect::from_min_size(pos2(rect.left(), y), vec2(key_w, cell_h));
                painter.rect_filled(key_rect, 0.0, key_color);
                painter.rect_stroke(key_rect, 0.0, Stroke::new(0.5, Color32::from_gray(20)));

                // Note name + keyboard mapping
                let note_names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
                let nn = note_names[(note % 12) as usize];
                let octave = note / 12 - 1;
                if note % 12 == 0 || is_black {
                    let label = if note % 12 == 0 { format!("{nn}{octave}") } else { nn.to_string() };
                    let text_color = if note % 12 == 0 { Color32::from_gray(140) } else { Color32::from_gray(80) };
                    painter.text(pos2(rect.left() + 4.0, y + cell_h / 2.0), Align2::LEFT_CENTER,
                        &label, FontId::monospace(6.0), text_color);
                }

                // Show keyboard mapping for notes 48-64 (C3-E4)
                if self.synth_assigned[sp] {
                    let kb_map: &[(u8, &str)] = &[
                        (48,"Z"),(49,"S"),(50,"X"),(51,"D"),(52,"C"),(53,"V"),(54,"G"),
                        (55,"B"),(56,"H"),(57,"N"),(58,"J"),(59,"M"),
                        (60,"Q"),(61,"2"),(62,"W"),(63,"3"),(64,"E"),
                    ];
                    if let Some((_, key)) = kb_map.iter().find(|(n, _)| *n == note as u8) {
                        painter.text(pos2(rect.left() + key_w - 4.0, y + cell_h / 2.0), Align2::RIGHT_CENTER,
                            key, FontId::monospace(6.0), accent());
                    }
                }

                // Grid row (dim out-of-scale notes)
                let in_scale = self.piano_scale.contains(note as u8);
                let row_color = if !in_scale {
                    Color32::from_gray(8) // very dim for out-of-scale
                } else if is_black {
                    Color32::from_gray(14)
                } else {
                    Color32::from_gray(18)
                };
                for step in 0..num_steps {
                    let cx = grid_left + step as f32 * cell_w;
                    let cell_rect = Rect::from_min_size(pos2(cx, y), vec2(cell_w - 1.0, cell_h - 1.0));
                    let bg = if step % 4 == 0 && in_scale { Color32::from_gray(22) } else { row_color };
                    painter.rect_filled(cell_rect, 1.0, bg);

                    // Current step line
                    if step as i32 == current_step && self.playing {
                        painter.rect_stroke(cell_rect, 1.0, Stroke::new(0.5, accent()));
                    }
                }
            }

            // Draw ghost notes (other pads' notes shown dimmed)
            for (pad_idx, pattern) in self.note_patterns.iter().enumerate() {
                if pad_idx == sp || !self.synth_assigned[pad_idx] { continue; }
                let ghost_color = color_alpha(self.pad_colors[pad_idx], 25);
                for note_evt in &pattern.notes {
                    if note_evt.note < *note_range.start() as u8 || note_evt.note > *note_range.end() as u8 { continue; }
                    let row = (*note_range.end() as u8 - note_evt.note) as f32;
                    let y = rect.top() + row * cell_h;
                    let x = grid_left + note_evt.start * cell_w;
                    let w = note_evt.duration * cell_w;
                    painter.rect_filled(
                        Rect::from_min_size(pos2(x, y + 1.0), vec2(w - 1.0, cell_h - 2.0)),
                        1.0, ghost_color,
                    );
                }
            }

            // Draw notes (active pad)
            let color = self.pad_colors[sp];
            for note_evt in &self.note_patterns[sp].notes {
                if note_evt.note < *note_range.start() as u8 || note_evt.note > *note_range.end() as u8 {
                    continue;
                }
                let row = (*note_range.end() as u8 - note_evt.note) as f32;
                let y = rect.top() + row * cell_h;
                let x = grid_left + note_evt.start * cell_w;
                let w = note_evt.duration * cell_w;
                let note_rect = Rect::from_min_size(pos2(x, y + 1.0), vec2(w - 1.0, cell_h - 2.0));
                let alpha = (note_evt.velocity * 200.0 + 55.0) as u8;
                painter.rect_filled(note_rect, 2.0, color_alpha(color, alpha));
                painter.rect_stroke(note_rect, 2.0, Stroke::new(0.5, color));
            }

            // Handle click to add/remove notes
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if pos.x >= grid_left {
                        let step = ((pos.x - grid_left) / cell_w) as f32;
                        let row = ((pos.y - rect.top()) / cell_h) as usize;
                        let note = (*note_range.end() as usize).saturating_sub(row) as u8;

                        if note >= *note_range.start() as u8 && note <= *note_range.end() as u8 {
                            self.push_undo();

                            // Check if note exists at this position
                            let existing = self.note_patterns[sp].notes.iter()
                                .position(|n| n.note == note && n.start <= step && (n.start + n.duration) > step);

                            if let Some(idx) = existing {
                                // Send NoteOff before removing
                                let removed_note = self.note_patterns[sp].notes[idx].note;
                                self.engine.send(Cmd::NoteOff { pad: sp, note: removed_note });
                                self.note_patterns[sp].notes.remove(idx);
                            } else {
                                let snapped = self.piano_scale.snap(note);
                                let snap = self.piano_snap;
                                let snapped_step = (step / snap).round() * snap;
                                self.note_patterns[sp].add_note(snapped, snapped_step, snap.max(0.25), 0.8);
                                // Preview the note
                                self.engine.send(Cmd::NoteOn { pad: sp, note: snapped, velocity: 0.8 });
                            }

                            self.engine.send(Cmd::SetNotePattern {
                                pad: sp,
                                pattern: self.note_patterns[sp].clone(),
                            });
                        }
                    }
                }
            }

            // Drag right edge of notes to resize duration
            if response.dragged() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if pos.x >= grid_left {
                        let step = (pos.x - grid_left) / cell_w;
                        let row = ((pos.y - rect.top()) / cell_h) as usize;
                        let note = (*note_range.end() as usize).saturating_sub(row) as u8;

                        // Find note whose right edge is near the cursor
                        if let Some(idx) = self.note_patterns[sp].notes.iter().position(|n| {
                            n.note == note && ((n.start + n.duration) - step).abs() < 0.5
                        }) {
                            let new_dur = (step - self.note_patterns[sp].notes[idx].start).max(0.25);
                            self.note_patterns[sp].notes[idx].duration = new_dur;
                            self.engine.send(Cmd::SetNotePattern {
                                pad: sp,
                                pattern: self.note_patterns[sp].clone(),
                            });
                        }
                    }
                }
            }

            // Right-click to delete note
            if response.secondary_clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if pos.x >= grid_left {
                        let step = ((pos.x - grid_left) / cell_w) as f32;
                        let row = ((pos.y - rect.top()) / cell_h) as usize;
                        let note = (*note_range.end() as usize).saturating_sub(row) as u8;

                        if let Some(idx) = self.note_patterns[sp].notes.iter()
                            .position(|n| n.note == note && n.start <= step && (n.start + n.duration) > step) {
                            self.push_undo();
                            self.engine.send(Cmd::NoteOff { pad: sp, note });
                            self.note_patterns[sp].notes.remove(idx);
                            self.engine.send(Cmd::SetNotePattern {
                                pad: sp,
                                pattern: self.note_patterns[sp].clone(),
                            });
                        }
                    }
                }
            }
        });

        // Velocity lane below the piano roll
        if !self.note_patterns[sp].notes.is_empty() {
            self.draw_piano_velocity_lane(ui);
        }
    }

    /// Draw a velocity lane for piano roll notes
    fn draw_piano_velocity_lane(&mut self, ui: &mut Ui) {
        let sp = self.selected_pad;
        let num_steps = self.num_steps;
        let lane_h = 40.0;
        let cell_w = 24.0;
        let key_w = 40.0;

        let (response, painter) = ui.allocate_painter(
            vec2(key_w + cell_w * num_steps as f32, lane_h),
            Sense::click_and_drag(),
        );
        let rect = response.rect;
        let grid_left = rect.left() + key_w;

        painter.rect_filled(rect, 2.0, Color32::from_gray(10));
        painter.text(pos2(rect.left() + 4.0, rect.center().y), Align2::LEFT_CENTER,
            "VEL", FontId::monospace(7.0), muted_color());

        // Collect max velocity per step from note pattern
        let color = self.pad_colors[sp];
        for note in &self.note_patterns[sp].notes {
            let step = note.start as usize;
            if step < num_steps {
                let norm = note.velocity;
                let x = grid_left + step as f32 * cell_w + 1.0;
                let bar_h = norm * (lane_h - 4.0);
                painter.rect_filled(
                    Rect::from_min_size(pos2(x, rect.bottom() - bar_h - 2.0), vec2(cell_w - 2.0, bar_h)),
                    1.0, color_alpha(color, (norm * 200.0 + 55.0) as u8),
                );
            }
        }

        // Grid lines
        for step in 0..num_steps {
            if step % 4 == 0 {
                let x = grid_left + step as f32 * cell_w;
                painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())],
                    Stroke::new(0.3, Color32::from_gray(25)));
            }
        }

        // Click/drag to adjust note velocities at a step
        if response.dragged() || response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if pos.x >= grid_left {
                    let step = ((pos.x - grid_left) / cell_w) as usize;
                    let norm = (1.0 - (pos.y - rect.top()) / lane_h).clamp(0.05, 1.0);
                    // Set velocity of all notes at this step
                    let mut changed = false;
                    for note in &mut self.note_patterns[sp].notes {
                        if note.start as usize == step {
                            note.velocity = norm;
                            changed = true;
                        }
                    }
                    if changed {
                        self.engine.send(Cmd::SetNotePattern {
                            pad: sp,
                            pattern: self.note_patterns[sp].clone(),
                        });
                    }
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════
    //  SYNTH EDITOR
    // ═══════════════════════════════════════════════════════

    fn draw_synth_editor(&mut self, ui: &mut Ui) {
        let sp = self.selected_pad;

        if !self.synth_assigned[sp] {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("SELECT A PAD AND ASSIGN A SYNTH FROM THE PIANO ROLL VIEW")
                    .size(10.0).color(muted_color()).family(FontFamily::Monospace));
            });
            return;
        }

        let mut changed = false;

        // Header + presets (before borrowing params)
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("SYNTH — {}", self.pad_names[sp]))
                .size(10.0).strong().color(self.pad_colors[sp]).family(FontFamily::Monospace));

            if ui.button(RichText::new("PRESETS ▾").size(9.0).color(accent())).clicked() {
                self.show_synth_presets = !self.show_synth_presets;
            }
        });

        // Preset browser dropdown
        if self.show_synth_presets {
            let mut preset_selected: Option<(String, SynthParams)> = None;
            ui.horizontal_wrapped(|ui| {
                for (cat, cat_presets) in presets::presets_by_category() {
                    ui.label(RichText::new(cat).size(8.0).strong().color(accent()).family(FontFamily::Monospace));
                    for preset in &cat_presets {
                        if ui.add(Button::new(RichText::new(preset.name).size(9.0).color(dim()))
                            .min_size(vec2(0.0, 18.0))).clicked() {
                            preset_selected = Some((preset.name.to_string(), preset.params.clone()));
                        }
                    }
                    ui.label(" | ");
                }
            });
            if let Some((name, params)) = preset_selected {
                self.synth_params[sp] = params;
                self.synth_assigned[sp] = true;
                self.pad_types[sp] = PadType::SubSynth;
                self.pad_names[sp] = name;
                self.engine.send(Cmd::SetPadSynth(sp, self.synth_params[sp].clone()));
                self.show_synth_presets = false;
                changed = true;
            }
        }

        // Now borrow params for the knob UI
        let params = &mut self.synth_params[sp];

        ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                // OSC 1
                ui.vertical(|ui| {
                    ui.set_width(120.0);
                    ui.label(RichText::new("OSC 1").size(9.0).color(accent()).family(FontFamily::Monospace));
                    let wave_names = ["SIN", "SAW", "SQR", "TRI", "NSE", "FM"];
                    let waves = [Waveform::Sine, Waveform::Saw, Waveform::Square, Waveform::Triangle, Waveform::Noise, Waveform::Fm];
                    ui.horizontal(|ui| {
                        for (w, name) in waves.iter().zip(wave_names.iter()) {
                            let active = params.osc1_wave == *w;
                            if ui.add(Button::new(RichText::new(*name).size(8.0)
                                .color(if active { Color32::BLACK } else { dim() }))
                                .fill(if active { accent() } else { Color32::from_gray(28) })
                                .min_size(vec2(22.0, 16.0))).clicked() {
                                params.osc1_wave = *w;
                                changed = true;
                            }
                        }
                    });
                });

                ui.separator();

                // OSC 2
                ui.vertical(|ui| {
                    ui.set_width(140.0);
                    ui.label(RichText::new("OSC 2").size(9.0).color(accent()).family(FontFamily::Monospace));
                    let wave_names = ["SIN", "SAW", "SQR", "TRI", "NSE", "FM"];
                    let waves = [Waveform::Sine, Waveform::Saw, Waveform::Square, Waveform::Triangle, Waveform::Noise, Waveform::Fm];
                    ui.horizontal(|ui| {
                        for (w, name) in waves.iter().zip(wave_names.iter()) {
                            let active = params.osc2_wave == *w;
                            if ui.add(Button::new(RichText::new(*name).size(8.0)
                                .color(if active { Color32::BLACK } else { dim() }))
                                .fill(if active { accent() } else { Color32::from_gray(28) })
                                .min_size(vec2(22.0, 16.0))).clicked() {
                                params.osc2_wave = *w;
                                changed = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("MIX").size(8.0).color(muted_color()));
                        changed |= ui.add(Slider::new(&mut params.osc_mix, 0.0..=1.0).show_value(false)).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("DET").size(8.0).color(muted_color()));
                        changed |= ui.add(Slider::new(&mut params.osc2_detune, -100.0..=100.0).show_value(true).suffix("c")).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("SEM").size(8.0).color(muted_color()));
                        let mut semi = params.osc2_semi as f32;
                        if ui.add(Slider::new(&mut semi, -24.0..=24.0).step_by(1.0).show_value(true)).changed() {
                            params.osc2_semi = semi as i32;
                            changed = true;
                        }
                    });
                });

                ui.separator();

                // FILTER
                ui.vertical(|ui| {
                    ui.set_width(140.0);
                    ui.label(RichText::new("FILTER").size(9.0).color(accent()).family(FontFamily::Monospace));
                    ui.horizontal(|ui| {
                        for (ft, name) in [(FilterType::Lowpass, "LP"), (FilterType::Highpass, "HP"), (FilterType::Bandpass, "BP")] {
                            let active = params.filter_type == ft;
                            if ui.add(Button::new(RichText::new(name).size(8.0)
                                .color(if active { Color32::BLACK } else { dim() }))
                                .fill(if active { accent() } else { Color32::from_gray(28) })).clicked() {
                                params.filter_type = ft;
                                changed = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("CUT").size(8.0).color(muted_color()));
                        changed |= ui.add(Slider::new(&mut params.filter_cutoff, 20.0..=20000.0).logarithmic(true).show_value(true).suffix("Hz")).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("RES").size(8.0).color(muted_color()));
                        changed |= ui.add(Slider::new(&mut params.filter_resonance, 0.0..=1.0).show_value(false)).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("ENV").size(8.0).color(muted_color()));
                        changed |= ui.add(Slider::new(&mut params.filt_env_amount, -1.0..=1.0).show_value(false)).changed();
                    });
                });

                ui.separator();

                // AMP ENVELOPE
                ui.vertical(|ui| {
                    ui.set_width(120.0);
                    ui.label(RichText::new("AMP ENV").size(9.0).color(accent()).family(FontFamily::Monospace));
                    ui.horizontal(|ui| { ui.label(RichText::new("A").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut params.amp_attack, 0.001..=2.0).show_value(false).logarithmic(true)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("D").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut params.amp_decay, 0.001..=2.0).show_value(false).logarithmic(true)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("S").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut params.amp_sustain, 0.0..=1.0).show_value(false)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("R").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut params.amp_release, 0.001..=5.0).show_value(false).logarithmic(true)).changed(); });
                });

                ui.separator();

                // FILTER ENVELOPE
                ui.vertical(|ui| {
                    ui.set_width(120.0);
                    ui.label(RichText::new("FILT ENV").size(9.0).color(accent()).family(FontFamily::Monospace));
                    ui.horizontal(|ui| { ui.label(RichText::new("A").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut params.filt_attack, 0.001..=2.0).show_value(false).logarithmic(true)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("D").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut params.filt_decay, 0.001..=2.0).show_value(false).logarithmic(true)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("S").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut params.filt_sustain, 0.0..=1.0).show_value(false)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("R").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut params.filt_release, 0.001..=5.0).show_value(false).logarithmic(true)).changed(); });
                });

                ui.separator();

                // LFO
                ui.vertical(|ui| {
                    ui.set_width(120.0);
                    ui.label(RichText::new("LFO").size(9.0).color(accent()).family(FontFamily::Monospace));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("RATE").size(8.0).color(muted_color()));
                        changed |= ui.add(Slider::new(&mut params.lfo_rate, 0.1..=20.0).show_value(true).suffix("Hz").logarithmic(true)).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("AMT").size(8.0).color(muted_color()));
                        changed |= ui.add(Slider::new(&mut params.lfo_amount, 0.0..=1.0).show_value(false)).changed();
                    });
                    ui.horizontal(|ui| {
                        for (t, name) in [(LfoTarget::FilterCutoff, "FLT"), (LfoTarget::Pitch, "PIT"), (LfoTarget::Amplitude, "AMP")] {
                            let active = params.lfo_target == t;
                            if ui.add(Button::new(RichText::new(name).size(8.0)
                                .color(if active { Color32::BLACK } else { dim() }))
                                .fill(if active { accent() } else { Color32::from_gray(28) })).clicked() {
                                params.lfo_target = t;
                                changed = true;
                            }
                        }
                    });
                });

                ui.separator();

                // UNISON / SUB / RING MOD
                ui.vertical(|ui| {
                    ui.set_width(140.0);
                    ui.label(RichText::new("UNISON / SUB").size(9.0).color(accent()).family(FontFamily::Monospace));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("VOC").size(8.0).color(muted_color()));
                        let mut voices = params.unison_voices as f32;
                        if ui.add(Slider::new(&mut voices, 1.0..=7.0).step_by(1.0).show_value(true)).changed() {
                            params.unison_voices = voices as u8;
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("DET").size(8.0).color(muted_color()));
                        changed |= ui.add(Slider::new(&mut params.unison_detune, 0.0..=50.0).show_value(true).suffix("c")).changed();
                    });
                    ui.horizontal(|ui| {
                        // Sub osc toggle
                        let sub_fill = if params.sub_osc { accent() } else { Color32::from_gray(28) };
                        if ui.add(Button::new(RichText::new("SUB").size(8.0)
                            .color(if params.sub_osc { Color32::BLACK } else { dim() }))
                            .fill(sub_fill)).clicked() {
                            params.sub_osc = !params.sub_osc;
                            changed = true;
                        }
                        if params.sub_osc {
                            changed |= ui.add(Slider::new(&mut params.sub_level, 0.0..=1.0).show_value(false)).changed();
                        }
                    });
                    ui.horizontal(|ui| {
                        let ring_fill = if params.ring_mod { Color32::from_rgb(239, 68, 68) } else { Color32::from_gray(28) };
                        if ui.add(Button::new(RichText::new("RING").size(8.0)
                            .color(if params.ring_mod { Color32::BLACK } else { dim() }))
                            .fill(ring_fill)).clicked() {
                            params.ring_mod = !params.ring_mod;
                            changed = true;
                        }
                    });
                });
            });
        });

        if changed {
            self.engine.send(Cmd::SetPadSynth(sp, params.clone()));
        }
    }

    // ═══════════════════════════════════════════════════════
    //  SLICER
    // ═══════════════════════════════════════════════════════

    fn draw_insert_fx(&mut self, ui: &mut Ui) {
        let sp = self.selected_pad;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("INSERT FX — {}", self.pad_names[sp]))
                .size(10.0).strong().color(self.pad_colors[sp]).family(FontFamily::Monospace));
            ui.label(RichText::new("DIST → CRUSH → CHORUS → PHASER").size(7.0).color(muted_color()).family(FontFamily::Monospace));
        });

        // Use persistent per-pad FX params
        let p = &mut self.fx_params[sp];
        let mut changed = false;

        ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                let w = 150.0;

                // DISTORTION
                ui.vertical(|ui| {
                    ui.set_width(w);
                    ui.label(RichText::new("DISTORTION").size(9.0).color(Color32::from_rgb(239, 68, 68)).family(FontFamily::Monospace));
                    ui.horizontal(|ui| { ui.label(RichText::new("DRV").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[0], 0.0..=1.0).show_value(false)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("MIX").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[1], 0.0..=1.0).show_value(false)).changed(); });
                });
                ui.separator();

                // BITCRUSHER
                ui.vertical(|ui| {
                    ui.set_width(w);
                    ui.label(RichText::new("BITCRUSHER").size(9.0).color(Color32::from_rgb(168, 85, 247)).family(FontFamily::Monospace));
                    ui.horizontal(|ui| { ui.label(RichText::new("BIT").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[2], 1.0..=16.0).show_value(true).step_by(1.0)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("RATE").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[3], 1.0..=32.0).show_value(false)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("MIX").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[4], 0.0..=1.0).show_value(false)).changed(); });
                });
                ui.separator();

                // CHORUS
                ui.vertical(|ui| {
                    ui.set_width(w);
                    ui.label(RichText::new("CHORUS").size(9.0).color(Color32::from_rgb(6, 182, 212)).family(FontFamily::Monospace));
                    ui.horizontal(|ui| { ui.label(RichText::new("RATE").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[5], 0.1..=5.0).show_value(false)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("DEP").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[6], 0.5..=10.0).show_value(false)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("MIX").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[7], 0.0..=1.0).show_value(false)).changed(); });
                });
                ui.separator();

                // PHASER
                ui.vertical(|ui| {
                    ui.set_width(w);
                    ui.label(RichText::new("PHASER").size(9.0).color(Color32::from_rgb(34, 197, 94)).family(FontFamily::Monospace));
                    ui.horizontal(|ui| { ui.label(RichText::new("RATE").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[8], 0.05..=5.0).show_value(false)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("DEP").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[9], 0.0..=1.0).show_value(false)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("FB").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[10], 0.0..=0.9).show_value(false)).changed(); });
                    ui.horizontal(|ui| { ui.label(RichText::new("MIX").size(8.0).color(muted_color())); changed |= ui.add(Slider::new(&mut p[11], 0.0..=1.0).show_value(false)).changed(); });
                });
            });
        });

        if changed {
            let p = &self.fx_params[sp];
            self.engine.send(Cmd::SetPadDistortion { pad: sp, drive: p[0], mix: p[1] });
            self.engine.send(Cmd::SetPadBitcrush { pad: sp, bits: p[2], rate: p[3], mix: p[4] });
            self.engine.send(Cmd::SetPadChorus { pad: sp, rate: p[5], depth: p[6], mix: p[7] });
            self.engine.send(Cmd::SetPadPhaser { pad: sp, rate: p[8], depth: p[9], feedback: p[10], mix: p[11] });
        }
    }

    fn draw_slicer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("SAMPLE SLICER").size(10.0).color(accent()).family(FontFamily::Monospace));

            if ui.button(RichText::new("LOAD SAMPLE").size(9.0).color(accent())).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Audio", &["wav", "wave", "mp3", "flac", "ogg", "aac", "m4a"])
                    .pick_file()
                {
                    if let Some((data, sr)) = audio::load_wav(&path) {
                        self.slicer_slices = slicer::detect_slices(&data, sr, self.slicer_sensitivity);
                        self.slicer_source_sr = sr;
                        self.slicer_source = Some(data);
                    }
                }
            }

            ui.label(RichText::new("SENS").size(8.0).color(muted_color()));
            let before = self.slicer_sensitivity;
            ui.add(Slider::new(&mut self.slicer_sensitivity, 0.0..=1.0).show_value(false));
            if self.slicer_sensitivity != before {
                if let Some(ref data) = self.slicer_source {
                    self.slicer_slices = slicer::detect_slices(data, self.slicer_source_sr, self.slicer_sensitivity);
                }
            }

            if !self.slicer_slices.is_empty() {
                ui.label(RichText::new(format!("{} slices", self.slicer_slices.len()))
                    .size(9.0).color(dim()).family(FontFamily::Monospace));

                if ui.button(RichText::new("MAP TO PADS").size(9.0).color(green())).clicked() {
                    if let Some(ref data) = self.slicer_source {
                        for (i, slice) in self.slicer_slices.iter().enumerate() {
                            if i >= NUM_PADS { break; }
                            let slice_data = slicer::extract_slice(data, slice);
                            let peaks = audio::compute_peaks(&slice_data, 200);
                            self.pad_peaks[i] = Some(peaks);
                            self.pad_names[i] = slice.name.clone();
                            self.pad_types[i] = PadType::Sample;
                            self.engine.send(Cmd::LoadSample {
                                pad: i,
                                data: Arc::new(slice_data),
                                original_sr: self.slicer_source_sr,
                            });
                        }
                    }
                }
            }
        });

        // Waveform with slice markers
        if let Some(ref data) = self.slicer_source {
            let peaks = audio::compute_peaks(data, 400);
            let avail = ui.available_size();
            let (response, painter) = ui.allocate_painter(
                vec2(avail.x, (avail.y - 4.0).max(40.0)),
                Sense::click(),
            );
            let rect = response.rect;

            // Background
            painter.rect_filled(rect, 4.0, Color32::from_rgb(10, 10, 12));

            // Waveform
            let n = peaks.len();
            let bar_w = rect.width() / n as f32;
            let center_y = rect.center().y;
            for (i, &p) in peaks.iter().enumerate() {
                let x = rect.left() + i as f32 * bar_w;
                let h = p * rect.height() * 0.8;
                painter.rect_filled(
                    Rect::from_min_size(pos2(x, center_y - h / 2.0), vec2(bar_w.max(1.0), h.max(0.5))),
                    0.0, Color32::from_rgb(100, 100, 120),
                );
            }

            // Slice markers
            let total_samples = data.len() as f32;
            let slice_colors = [
                Color32::from_rgb(255, 61, 90), Color32::from_rgb(0, 229, 255),
                Color32::from_rgb(255, 214, 0), Color32::from_rgb(179, 136, 255),
                Color32::from_rgb(105, 240, 174), Color32::from_rgb(255, 128, 171),
                Color32::from_rgb(68, 138, 255), Color32::from_rgb(255, 171, 64),
            ];

            for (i, slice) in self.slicer_slices.iter().enumerate() {
                let x = rect.left() + (slice.start as f32 / total_samples) * rect.width();
                let color = slice_colors[i % slice_colors.len()];

                // Vertical line
                painter.line_segment(
                    [pos2(x, rect.top()), pos2(x, rect.bottom())],
                    Stroke::new(1.5, color),
                );

                // Label
                painter.text(pos2(x + 3.0, rect.top() + 4.0), Align2::LEFT_TOP,
                    &slice.name, FontId::monospace(7.0), color);
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("LOAD A SAMPLE TO SLICE")
                    .size(10.0).color(muted_color()).family(FontFamily::Monospace));
            });
        }
    }

    // ═══════════════════════════════════════════════════════
    //  UNDO / REDO
    // ═══════════════════════════════════════════════════════

    /// Humanize: slightly randomize velocities of active steps
    fn humanize_pattern(&mut self) {
        self.push_undo();
        let bank = &mut self.banks[self.active_bank];
        let mut rng = simple_rng();
        for row in bank.iter_mut() {
            for step in row.iter_mut() {
                if *step > 0 {
                    // Randomly adjust velocity ±1 level, staying in 1-3 range
                    let r = rng_next(&mut rng) % 100;
                    if r < 20 && *step > 1 {
                        *step -= 1;
                    } else if r > 80 && *step < 3 {
                        *step += 1;
                    }
                }
            }
        }
        self.sync_pattern();
    }

    fn tap_tempo(&mut self, time: f64) {
        // Remove taps older than 3 seconds
        self.tap_times.retain(|&t| time - t < 3.0);
        self.tap_times.push(time);

        if self.tap_times.len() >= 2 {
            let intervals: Vec<f64> = self.tap_times.windows(2)
                .map(|w| w[1] - w[0])
                .collect();
            let avg_interval = intervals.iter().sum::<f64>() / intervals.len() as f64;
            if avg_interval > 0.15 && avg_interval < 3.0 {
                self.bpm = (60.0 / avg_interval as f32).round().clamp(20.0, 300.0);
                self.engine.send(Cmd::SetBpm(self.bpm));
            }
        }
    }

    fn copy_pattern(&mut self) {
        self.pattern_clipboard = Some(self.banks[self.active_bank].clone());
    }

    fn merge_patterns(&mut self, source: usize) {
        // Merge source bank ON TOP of current bank (max velocity wins)
        if source >= self.banks.len() || source == self.active_bank { return; }
        self.push_undo();
        for pad in 0..NUM_PADS {
            for step in 0..MAX_STEPS {
                let src = self.banks[source][pad].get(step).copied().unwrap_or(0);
                let dst = self.banks[self.active_bank][pad].get(step).copied().unwrap_or(0);
                self.banks[self.active_bank][pad][step] = src.max(dst);
            }
        }
        self.sync_pattern();
    }

    fn duplicate_pattern(&mut self) {
        // Copy current bank to the next bank
        let next = (self.active_bank + 1) % 8;
        self.push_undo();
        self.banks[next] = self.banks[self.active_bank].clone();
        self.active_bank = next;
        self.sync_pattern();
    }

    fn paste_pattern(&mut self) {
        if let Some(clip) = self.pattern_clipboard.clone() {
            self.push_undo();
            self.banks[self.active_bank] = clip;
            self.sync_pattern();
        }
    }

    fn new_project(&mut self) {
        // Reset all state to defaults
        self.project_name = "Untitled".to_string();
        self.project_dirty = false;
        self.last_save_path = None;
        self.bpm = 90.0;
        self.swing = 0.0;
        self.num_steps = 16;
        self.active_bank = 0;
        self.master_vol = 0.8;
        self.master_filter = 20000.0;
        self.reverb_mix = 0.0;
        self.delay_mix = 0.0;
        self.stereo_width = 1.0;
        self.enhancer_amount = 0.0;

        let info = audio::default_pad_info();
        self.banks = (0..8).map(|_| vec![vec![0u8; MAX_STEPS]; NUM_PADS]).collect();
        self.pad_names = info.iter().map(|p| p.name.to_string()).collect();
        self.pad_types = info.iter().map(|p| if p.has_voice { PadType::Synth } else { PadType::Empty }).collect();
        self.pad_peaks = vec![None; NUM_PADS];
        self.volumes = vec![0.7; NUM_PADS];
        self.pans = vec![0.0; NUM_PADS];
        self.pitches = vec![0.0; NUM_PADS];
        self.filters = vec![20000.0; NUM_PADS];
        self.reversed = vec![false; NUM_PADS];
        self.trim_start = vec![0.0; NUM_PADS];
        self.trim_end = vec![1.0; NUM_PADS];
        self.muted = vec![false; NUM_PADS];
        self.soloed = vec![false; NUM_PADS];
        self.synth_assigned = vec![false; NUM_PADS];
        self.reverb_sends = vec![0.0; NUM_PADS];
        self.delay_sends = vec![0.0; NUM_PADS];
        self.sidechain_active = vec![false; NUM_PADS];
        self.fx_params = vec![[0.0, 0.0, 16.0, 1.0, 0.0, 0.5, 3.0, 0.0, 0.3, 0.5, 0.5, 0.0]; NUM_PADS];
        self.note_patterns = (0..NUM_PADS).map(|_| NotePattern::new()).collect();
        self.step_probability = (0..NUM_PADS).map(|_| vec![100u8; MAX_STEPS]).collect();
        self.eq_params = (0..NUM_PADS).map(|_| EqParams::default()).collect();
        self.undo_stack.clear();
        self.redo_stack.clear();

        // Stop playback and reset engine
        if self.playing {
            self.engine.send(Cmd::Stop); self.engine.send(Cmd::AllNotesOff);
            self.playing = false;
        }
        self.engine.send(Cmd::SetBpm(self.bpm));
        self.engine.send(Cmd::SetSwing(self.swing));
        self.engine.send(Cmd::SetSteps(self.num_steps));
        self.engine.send(Cmd::SetMasterVol(self.master_vol));
        self.engine.send(Cmd::SetMasterFilter(self.master_filter));
        self.engine.send(Cmd::SetReverb(self.reverb_mix));
        self.engine.send(Cmd::SetDelay(self.delay_mix));
        self.engine.send(Cmd::SetStereoWidth(self.stereo_width));
        self.sync_pattern();
        for i in 0..NUM_PADS {
            self.engine.send(Cmd::RemoveSample(i));
            self.engine.send(Cmd::SetPadVol(i, 0.7));
            self.engine.send(Cmd::SetPadPan(i, 0.0));
        }
    }

    fn quick_save(&mut self) {
        if let Some(ref path) = self.last_save_path.clone() {
            let proj = self.to_project();
            if proj.save(path).is_ok() {
                self.project_dirty = false;
            }
        } else {
            // No previous save — open file dialog
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("BeatForge Project", &["bfp"])
                .set_file_name("beat.bfp")
                .save_file() {
                let proj = self.to_project();
                if proj.save(&path).is_ok() {
                    self.project_dirty = false;
                    self.last_save_path = Some(path);
                }
            }
        }
    }

    fn sync_loop_region(&self) {
        self.engine.send(Cmd::SetLoopRegion(self.loop_start, self.loop_end));
    }

    /// Record a parameter change to automation if auto_rec is enabled
    fn record_automation(&mut self, target: AutoTarget, pad: usize, value: f32) {
        if !self.auto_rec || !self.playing { return; }
        let step = self.engine.current_step();
        if step < 0 { return; }
        let step = step as usize;

        // Find or create lane
        let lane_idx = self.automation.lanes.iter().position(|l| l.pad == pad && l.target == target);
        let lane_idx = lane_idx.unwrap_or_else(|| {
            self.automation.add_lane(target, pad, self.num_steps);
            self.automation.lanes.len() - 1
        });

        self.automation.lanes[lane_idx].set(step, value);
        self.engine.send(Cmd::SetAutomation(self.automation.clone()));
        self.project_dirty = true;
    }

    fn mark_dirty(&mut self) {
        self.project_dirty = true;
    }

    fn update_title(&self, ctx: &egui::Context) {
        let dirty_mark = if self.project_dirty { " \u{25cf}" } else { "" };
        let title = format!("{}{dirty_mark} \u{2014} BeatForge Studio", self.project_name);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    fn push_undo(&mut self) {
        self.project_dirty = true;
        let state = UndoState {
            banks: self.banks.clone(),
            note_patterns: self.note_patterns.clone(),
        };
        self.undo_stack.push(state);
        if self.undo_stack.len() > 50 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if let Some(state) = self.undo_stack.pop() {
            let current = UndoState {
                banks: self.banks.clone(),
                note_patterns: self.note_patterns.clone(),
            };
            self.redo_stack.push(current);
            self.banks = state.banks;
            self.note_patterns = state.note_patterns;
            self.sync_pattern();
        }
    }

    fn redo(&mut self) {
        if let Some(state) = self.redo_stack.pop() {
            let current = UndoState {
                banks: self.banks.clone(),
                note_patterns: self.note_patterns.clone(),
            };
            self.undo_stack.push(current);
            self.banks = state.banks;
            self.note_patterns = state.note_patterns;
            self.sync_pattern();
        }
    }

    // ═══════════════════════════════════════════════════════
    //  PROJECT SAVE / LOAD
    // ═══════════════════════════════════════════════════════

    fn to_project(&self) -> ProjectData {
        ProjectData {
            version: 1,
            name: "Untitled".to_string(),
            bpm: self.bpm,
            swing: self.swing,
            num_steps: self.num_steps,
            active_bank: self.active_bank,
            master_vol: self.master_vol,
            master_filter: self.master_filter,
            reverb_mix: self.reverb_mix,
            delay_mix: self.delay_mix,
            banks: self.banks.clone(),
            pad_names: self.pad_names.clone(),
            volumes: self.volumes.clone(),
            pans: self.pans.clone(),
            pitches: self.pitches.clone(),
            filters: self.filters.clone(),
            reversed: self.reversed.clone(),
            trim_start: self.trim_start.clone(),
            trim_end: self.trim_end.clone(),
            muted: self.muted.clone(),
            soloed: self.soloed.clone(),
            synth_assigned: self.synth_assigned.clone(),
            reverb_sends: self.reverb_sends.clone(),
            delay_sends: self.delay_sends.clone(),
            stereo_width: self.stereo_width,
            enhancer_amount: self.enhancer_amount,
            sidechain_active: self.sidechain_active.clone(),
            fx_params: self.fx_params.clone(),
        }
    }

    fn apply_project(&mut self, proj: ProjectData) {
        self.bpm = proj.bpm;
        self.swing = proj.swing;
        self.num_steps = proj.num_steps;
        self.active_bank = proj.active_bank;
        self.master_vol = proj.master_vol;
        self.master_filter = proj.master_filter;
        self.reverb_mix = proj.reverb_mix;
        self.delay_mix = proj.delay_mix;
        self.banks = proj.banks;
        self.pad_names = proj.pad_names;
        self.volumes = proj.volumes;
        self.pans = proj.pans;
        self.pitches = proj.pitches;
        self.filters = proj.filters;
        self.reversed = proj.reversed;
        self.trim_start = proj.trim_start;
        self.trim_end = proj.trim_end;
        self.muted = proj.muted;
        self.soloed = proj.soloed;
        self.synth_assigned = proj.synth_assigned;
        self.reverb_sends = proj.reverb_sends;
        self.delay_sends = proj.delay_sends;
        self.stereo_width = proj.stereo_width;
        self.enhancer_amount = proj.enhancer_amount;
        self.sidechain_active = proj.sidechain_active;
        self.fx_params = proj.fx_params;

        // Sync to engine
        self.engine.send(Cmd::SetBpm(self.bpm));
        self.engine.send(Cmd::SetSwing(self.swing));
        self.engine.send(Cmd::SetSteps(self.num_steps));
        self.engine.send(Cmd::SetMasterVol(self.master_vol));
        self.engine.send(Cmd::SetMasterFilter(self.master_filter));
        self.engine.send(Cmd::SetReverb(self.reverb_mix));
        self.engine.send(Cmd::SetDelay(self.delay_mix));
        self.sync_pattern();

        for i in 0..NUM_PADS {
            self.engine.send(Cmd::SetPadVol(i, self.volumes[i]));
            self.engine.send(Cmd::SetPadPan(i, self.pans[i]));
            self.engine.send(Cmd::SetPadPitch(i, self.pitches[i]));
            self.engine.send(Cmd::SetPadFilter(i, self.filters[i]));
            self.engine.send(Cmd::SetPadReverse(i, self.reversed[i]));
            self.engine.send(Cmd::SetPadTrim(i, self.trim_start[i], self.trim_end[i]));
            self.engine.send(Cmd::SetPadMute(i, self.muted[i]));
            self.engine.send(Cmd::SetPadSolo(i, self.soloed[i]));
            self.engine.send(Cmd::SetPadReverbSend(i, self.reverb_sends[i]));
            self.engine.send(Cmd::SetPadDelaySend(i, self.delay_sends[i]));
            // Restore FX params
            let p = &self.fx_params[i];
            self.engine.send(Cmd::SetPadDistortion { pad: i, drive: p[0], mix: p[1] });
            self.engine.send(Cmd::SetPadBitcrush { pad: i, bits: p[2], rate: p[3], mix: p[4] });
            self.engine.send(Cmd::SetPadChorus { pad: i, rate: p[5], depth: p[6], mix: p[7] });
            self.engine.send(Cmd::SetPadPhaser { pad: i, rate: p[8], depth: p[9], feedback: p[10], mix: p[11] });
            // Restore sidechain
            if self.sidechain_active[i] {
                self.engine.send(Cmd::SetSidechain { source: 0, target: i, amount: 0.8 });
            }
        }
        self.engine.send(Cmd::SetStereoWidth(self.stereo_width));
        self.engine.send(Cmd::SetEnhancer(self.enhancer_amount));
    }
}

// ═══════════════════════════════════════════════════════════
//  SIMPLE RNG (xorshift32, no crate needed)
// ═══════════════════════════════════════════════════════════

/// Apply velocity curve to a 0-1 velocity value
fn apply_velocity_curve(vel: f32, curve: usize) -> f32 {
    match curve {
        1 => vel * vel,                    // Exponential: softer response
        2 => vel.sqrt(),                   // Logarithmic: harder response
        _ => vel,                          // Linear
    }
}

/// Euclidean rhythm generator (Bjorklund algorithm)
/// Distributes `hits` evenly across `steps` — creates musically interesting patterns
/// E(3,8) = [x . . x . . x .] (Cuban tresillo)
/// E(5,8) = [x . x x . x x .] (Cinquillo)
/// E(4,12) = [x . . x . . x . . x . .] (standard 4-on-floor)
fn euclidean_rhythm(hits: usize, steps: usize) -> Vec<u8> {
    if hits == 0 || steps == 0 { return vec![0; steps]; }
    if hits >= steps { return vec![3; steps]; }

    // Bjorklund algorithm
    let mut pattern = Vec::new();
    let mut counts = Vec::new();
    let mut remainders = Vec::new();

    let mut divisor = steps - hits;
    remainders.push(hits as i32);
    let mut level = 0;

    loop {
        counts.push(divisor / remainders[level] as usize);
        let new_rem = divisor % remainders[level] as usize;
        remainders.push(new_rem as i32);
        divisor = remainders[level] as usize;
        level += 1;
        if remainders[level] <= 1 { break; }
    }

    counts.push(divisor);

    fn build(level: usize, counts: &[usize], remainders: &[i32], pattern: &mut Vec<bool>) {
        if level == usize::MAX { // underflow guard
            pattern.push(false);
        } else if level == 0 {
            pattern.push(true);
        } else {
            // Fallback: simple even distribution
            return;
        }
    }

    // Simpler implementation: direct computation
    pattern.clear();
    for i in 0..steps {
        // Bresenham-style even distribution
        let threshold = (i * hits) % steps;
        let prev_threshold = if i > 0 { ((i - 1) * hits) % steps } else { steps };
        let is_hit = (i * hits / steps) != ((i.wrapping_sub(1)) * hits / steps) || i == 0;
        pattern.push(if is_hit && pattern.iter().filter(|&&v| v > 0).count() < hits { 3 } else { 0 });
    }

    // Verify hit count and fix if needed
    let actual_hits = pattern.iter().filter(|&&v| v > 0).count();
    if actual_hits != hits {
        // Fallback: simple even spacing
        pattern = vec![0u8; steps];
        for i in 0..hits {
            let pos = (i * steps) / hits;
            pattern[pos] = 3;
        }
    }

    pattern
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

const AUDIO_EXTS: &[&str] = &["wav", "wave", "mp3", "flac", "ogg", "aac", "m4a", "aif", "aiff"];

fn is_audio_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn simple_rng() -> u32 {
    // Seed from time
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    t.wrapping_mul(2654435761)
}

fn rng_next(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}
