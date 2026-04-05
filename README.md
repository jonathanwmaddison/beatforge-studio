# BeatForge Studio

A native desktop beatmaking DAW built entirely in Rust. No Electron, no web tech — pure native performance.

## Features

**Sequencer** — 16/32/64 step grid with velocity, probability, automation lanes, per-row tools (randomize/shift/double/halve), drag paint/erase, 8 pattern banks

**Piano Roll** — 5-octave note editor with scale snap (7 scales), ghost notes, velocity lane, keyboard piano with octave shift, quantize

**Synthesis** — Dual oscillator subtractive synth with PolyBLEP anti-aliasing, FM synthesis, 8-voice polyphony, unison (1-7 voices), sub oscillator, ring modulation, resonant filter (LP/HP/BP), 2x ADSR, LFO, portamento. 16 presets across Bass/Lead/Pad/Keys/FM/FX

**Sampling** — Load WAV/MP3/FLAC/OGG/AAC, waveform editor with trim, pitch, reverse, filter, BPM detection, auto-slicer with transient detection, normalize

**Mixer** — Per-channel: fader + VU meter + pan + 3-band parametric EQ + insert FX (distortion, bitcrusher, chorus, phaser) + reverb/delay sends + sidechain from kick

**Effects** — Reverb (8-comb + 4-allpass + pre-delay), tempo-synced delay (5 divisions), LP filter, compressor, limiter, tape saturation, stereo width, Gross Beat (half-speed/tape stop/gate/stutter/reverse)

**Live Input** — MIDI controller input (CoreMIDI), live recording to grid, overdub mode, count-in, step input mode, note repeat (1/4-1/32), tap tempo

**Arrangement** — 5-track song mode, per-bar bank assignment, sequential playback

**I/O** — Project save/load (.bfp), WAV recording/export, Cmd+S quick save, unsaved changes indicator

## Build & Run

Requires Rust and Xcode Command Line Tools on macOS.

```bash
# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env

# Build and run
DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo run --release
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| Space | Play / Stop |
| Esc | Stop & reset |
| Cmd+S | Quick save |
| Cmd+N | New project |
| Cmd+Z | Undo |
| Cmd+Shift+Z | Redo |
| Cmd+C / Cmd+V | Copy / paste pattern |
| T | Tap tempo |
| Tab | Cycle views |
| Up / Down | BPM +/- 1 |
| Left / Right | Select pad |
| [ / ] | Octave shift (piano mode) |

**Pad Mode:** Z X C V / A S D F / Q W E R / 1 2 3 4

**Piano Mode** (in Piano Roll): Z-M = C3-B3, Q-E = C4-E4

## Tech Stack

- **Audio**: cpal (CoreAudio), symphonia (MP3/FLAC/OGG), hound (WAV)
- **UI**: egui/eframe, JetBrains Mono
- **MIDI**: midir (CoreMIDI)
- **DSP**: Custom — PolyBLEP oscillators, Schroeder reverb, biquad filters, state-variable filter, tape saturation
