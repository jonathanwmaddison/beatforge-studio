# BeatForge Studio

A native desktop beatmaking DAW built entirely in Rust. No Electron, no web tech — pure native performance.

**9,200+ lines of Rust · 13 modules · 6.6MB binary · Zero dependencies on Node/Electron/web**

## Features

### Sequencer
16/32/64 step grid, drag paint (left-click) / drag erase (right-click), velocity lane, probability lane, automation lanes (filter/volume/pan), loop region (shift+click), per-row tools (randomize/shift/double/halve/clear/clone), zoom (50-300%), sweeping playhead, step input mode, live recording with overdub and count-in, collapsible lanes

### Piano Roll
5-octave note editor, 7 scale snaps (Major/Minor/Pentatonic/Blues/Dorian/Mixolydian), 3 time snaps, ghost notes from other channels, velocity lane, keyboard piano with octave shift ([/]), quantize, note resize by drag

### Synthesis
- **Subtractive**: Dual oscillator (PolyBLEP anti-aliased Sine/Saw/Square/Triangle/Noise), unison (1-7 voices with detune), sub oscillator, ring modulation, resonant filter (LP/HP/BP), 2× ADSR, LFO, portamento
- **FM**: Carrier-modulator with controllable index and ratio
- **16 presets** across Bass, Lead, Pad, Keys, FM, FX categories

### Sampling
WAV/MP3/FLAC/OGG/AAC/M4A loading, waveform editor with draggable trim handles, pitch (±24 semitones), reverse, filter, per-pad ADSR (attack/release), loop mode, normalize, BPM detection with match button, auto-slicer (transient detection → map to pads), sample info display

### Mixer
16 channels × (fader + real-time VU meter + pan + 3-band parametric EQ + 4 insert FX + reverb/delay sends + sidechain from kick), channel settings window (double-click strip), master dB readout, clip indicator

### Insert FX (per channel)
Distortion (soft saturation with tone), Bitcrusher (bit depth + rate reduction), Chorus (LFO-modulated delay), Phaser (4-stage allpass with feedback)

### Master Chain
Volume → Gross Beat (6 modes: half-speed/tape stop/gate/stutter/reverse) → tempo-synced delay (1/4, 1/8, 1/16, dotted, triplet) → 8-comb reverb (pre-delay, HF damping) → LP filter → stereo width → 3-band enhancer (Soundgoodizer-style) → tape saturation → soft clip limiter

### Live Input
MIDI controller input (CoreMIDI), live recording to grid, overdub mode, count-in (1 bar), step input mode, note repeat (1/4-1/32), tap tempo, velocity curves (Linear/Exponential/Logarithmic)

### Arrangement
8 pattern banks (F1-F8), copy/paste/duplicate/merge, bank activity indicators, 5-track song mode with per-bar bank assignment, sequential playback with PLAY SONG button

### I/O
WAV recording/export (32-bit float), project save/load (.bfp with full state), Cmd+S quick save, Cmd+N new project, drag & drop audio files, automation recording, unsaved changes indicator, dynamic window title

## Build & Run

### macOS

```bash
# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env

# Build and run
cargo run --release

# Or create a .app bundle
./bundle-macos.sh
# Then: cp -r "target/BeatForge Studio.app" /Applications/
```

### Linux

```bash
# Install dependencies (Ubuntu/Debian)
sudo apt install libasound2-dev libxkbcommon-dev libwayland-dev

# Build and run
cargo run --release
```

### Windows

```bash
# Just needs Rust installed
cargo run --release
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| Space | Play / Stop |
| Esc | Stop & reset |
| ↑ / ↓ | BPM ±1 (Shift: ±10) |
| ← / → | Select pad |
| M | Mute selected pad |
| F1-F8 | Switch pattern bank |
| F9 | Solo selected pad |
| T | Tap tempo |
| Tab | Cycle views (Sequencer/Piano Roll/Arrange) |
| [ / ] | Octave shift (piano mode) |
| Cmd+S | Quick save |
| Cmd+N | New project |
| Cmd+Z / Cmd+Shift+Z | Undo / Redo |
| Cmd+C / Cmd+V | Copy / Paste pattern |
| Cmd+D | Duplicate pattern to next bank |
| Shift+click step # | Set loop region |
| Ctrl+scroll | Zoom sequencer |
| Alt+click pad | Preview without selecting |
| / | Toggle help |

**Pad Mode:** Z X C V / A S D F / Q W E R / 1 2 3 4

**Piano Mode** (in Piano Roll): Z-M = C3-B3, Q-E = C4-E4, [ ] = octave shift

## Tech Stack

| Component | Crate | Purpose |
|-----------|-------|---------|
| Audio output | cpal | CoreAudio (macOS), WASAPI (Windows), ALSA (Linux) |
| Audio decode | symphonia | MP3, FLAC, OGG, AAC decoding |
| WAV I/O | hound | WAV read/write |
| GUI | eframe/egui | Immediate-mode GPU-accelerated UI |
| MIDI | midir | CoreMIDI (macOS), WinMM (Windows), ALSA (Linux) |
| Font | JetBrains Mono | Embedded at compile time |

## Architecture

```
src/app.rs        4,373  UI: all views, modals, input, state management
src/audio.rs      2,131  Audio engine: DSP, sequencer, mixing, routing
src/synth.rs        608  Subtractive + FM synthesis, 8-voice polyphony
src/presets.rs      538  16 synth presets across 6 categories
src/fx.rs           269  Insert FX: distortion, bitcrusher, chorus, phaser
src/project.rs      253  Project save/load (.bfp format)
src/slicer.rs       235  Sample slicer, BPM detection
src/eq.rs           194  3-band parametric EQ per channel
src/automation.rs   185  Per-step automation with interpolation
src/midi.rs         140  MIDI input via CoreMIDI
src/analyzer.rs     133  64-band spectrum analyzer
src/recorder.rs     123  WAV recording/export
src/main.rs          48  Entry point, font, theme
```

## License

MIT
