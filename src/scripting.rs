//! BeatForge Script (BFS) — a live coding pattern language for the DAW.
//! Inspired by Strudel/TidalCycles but designed for step sequencer integration.
//!
//! Syntax:
//!   kick  "x...x...x...x..."     # tracker-style: x=hit .=rest
//!   snare "....X.......X..."     # X=hard x=medium o=soft
//!   hh    "x.x.x.x.x.x.x.x."
//!
//!   bpm 140                       # set tempo
//!   swing 25                      # set swing percent
//!   reverb 0.3                    # master reverb
//!   delay 0.2                     # master delay
//!
//!   euclidean kick 3 16           # euclidean rhythm: 3 hits in 16 steps
//!   euclidean snare 5 16          # 5 hits in 16
//!
//!   every 4 kick "x..x..x..x..x..x"  # change pattern every 4 bars (future)
//!
//! Mini-notation (Strudel-style):
//!   "kick snare [hh hh] clap"    # evenly divided across steps
//!   "kick*2 ~ snare hh*4"        # repeat, ~ = rest

use crate::audio::NUM_PADS;

/// Pad name → pad index mapping
fn pad_index(name: &str) -> Option<usize> {
    match name.to_lowercase().as_str() {
        "kick" | "bd" | "k" => Some(0),
        "snare" | "sd" | "sn" | "s" => Some(1),
        "hh" | "hihat" | "ch" | "hat" => Some(2),
        "oh" | "openhat" | "ho" => Some(3),
        "clap" | "cp" | "cl" => Some(4),
        "rim" | "rs" | "r" => Some(5),
        "tom1" | "th" => Some(6),
        "tom2" | "tl" => Some(7),
        "perc" | "pc" | "clave" => Some(8),
        "cowbell" | "cb" | "cow" => Some(9),
        "pad10" | "p10" => Some(10),
        "pad11" | "p11" => Some(11),
        "pad12" | "p12" => Some(12),
        "pad13" | "p13" => Some(13),
        "pad14" | "p14" => Some(14),
        "pad15" | "p15" => Some(15),
        _ => None,
    }
}

/// Result of evaluating a script
#[derive(Default)]
pub struct ScriptResult {
    /// Pattern data: [pad][step] = velocity (0-3)
    pub pattern: Vec<Vec<u8>>,
    /// Melody notes: (midi_note, start_step, duration)
    pub melody_notes: Vec<(u8, f32, f32)>,
    /// Commands to execute
    pub commands: Vec<ScriptCommand>,
    /// Error messages
    pub errors: Vec<String>,
    /// Effective step count (may differ from input if 'steps' command used)
    pub effective_steps: usize,
}

#[derive(Clone)]
pub enum ScriptCommand {
    SetBpm(f32),
    SetSwing(f32),
    SetReverb(f32),
    SetDelay(f32),
    Euclidean(usize, usize, usize),
    SetPadVol(usize, f32),
    SetPadPan(usize, f32),
    SetPadPitch(usize, f32),
    SetPadFilter(usize, f32),
    SetSteps(usize),
    SetBank(usize),
}

/// Parse and evaluate a BeatForge Script
pub fn evaluate(script: &str, num_steps: usize) -> ScriptResult {
    // First pass: find 'steps' command to get correct pattern length
    let mut effective_steps = num_steps;
    for line in script.lines() {
        let line = line.trim();
        let parts: Vec<&str> = line.splitn(2, |c: char| c.is_whitespace()).collect();
        if let Some(&cmd) = parts.first() {
            if cmd == "steps" || cmd == "length" {
                if let Some(Ok(s)) = parts.get(1).map(|s| s.trim().parse::<usize>()) {
                    effective_steps = s.clamp(1, 64);
                }
            }
        }
    }

    let mut result = ScriptResult {
        pattern: vec![vec![0u8; effective_steps]; NUM_PADS],
        melody_notes: Vec::new(),
        commands: Vec::new(),
        errors: Vec::new(),
        effective_steps,
    };

    for (line_num, line) in script.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }

        // Try to parse each line
        if let Err(e) = parse_line(line, num_steps, &mut result) {
            result.errors.push(format!("Line {}: {}", line_num + 1, e));
        }
    }

    result
}

fn parse_line(line: &str, num_steps: usize, result: &mut ScriptResult) -> Result<(), String> {
    let parts: Vec<&str> = line.splitn(2, |c: char| c.is_whitespace()).collect();
    if parts.is_empty() { return Ok(()); }

    let cmd = parts[0].to_lowercase();
    let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

    match cmd.as_str() {
        "steps" | "length" => {
            if let Ok(s) = arg.parse::<usize>() {
                result.commands.push(ScriptCommand::SetSteps(s.clamp(1, 64)));
            }
            return Ok(());
        }
        "section" => {
            if let Ok(b) = arg.parse::<usize>() {
                result.commands.push(ScriptCommand::SetBank(b.saturating_sub(1).min(7)));
            }
            return Ok(());
        }
        _ => {}
    }

    // Skip lines handled by the extended command parser
    const EXT_COMMANDS: &[&str] = &[
        "kit", "loadkit", "synth", "preset", "dist", "distortion", "drive",
        "crush", "bitcrush", "chorus", "phaser", "send_reverb", "sendverb", "srv",
        "send_delay", "senddly", "sdl", "sidechain", "sc", "enhancer", "enhance", "enh",
        "width", "stereo", "master_filter", "mfilter", "mlpf", "export", "bounce",
    ];
    if EXT_COMMANDS.contains(&cmd.as_str()) {
        return Ok(()); // handled by extract_extended_commands
    }

    match cmd.as_str() {
        // Global commands
        "bpm" | "tempo" => {
            let bpm: f32 = arg.parse().map_err(|_| format!("Invalid BPM: {arg}"))?;
            result.commands.push(ScriptCommand::SetBpm(bpm.clamp(20.0, 300.0)));
        }
        "swing" | "swg" => {
            let swing: f32 = arg.parse().map_err(|_| format!("Invalid swing: {arg}"))?;
            result.commands.push(ScriptCommand::SetSwing(swing.clamp(0.0, 100.0)));
        }
        "reverb" | "verb" | "rev" => {
            let val: f32 = arg.parse().map_err(|_| format!("Invalid reverb: {arg}"))?;
            result.commands.push(ScriptCommand::SetReverb(val.clamp(0.0, 1.0)));
        }
        "delay" | "dly" | "del" => {
            let val: f32 = arg.parse().map_err(|_| format!("Invalid delay: {arg}"))?;
            result.commands.push(ScriptCommand::SetDelay(val.clamp(0.0, 1.0)));
        }

        // Euclidean rhythm
        "euclidean" | "euclid" | "euc" => {
            let eparts: Vec<&str> = arg.split_whitespace().collect();
            if eparts.len() < 3 {
                return Err("Usage: euclidean <pad> <hits> <steps>".to_string());
            }
            let pad = pad_index(eparts[0]).ok_or(format!("Unknown pad: {}", eparts[0]))?;
            let hits: usize = eparts[1].parse().map_err(|_| "Invalid hits count")?;
            let steps: usize = eparts[2].parse().map_err(|_| "Invalid steps count")?;
            result.commands.push(ScriptCommand::Euclidean(pad, hits, steps));
        }

        // Melody notation
        "note" | "melody" | "notes" => {
            let notes = parse_melody(arg, result.effective_steps);
            result.melody_notes.extend(notes);
        }

        // Pattern transforms
        "reverse" => {
            if let Some(pad) = pad_index(arg) {
                reverse_pattern(&mut result.pattern[pad], num_steps);
            }
        }
        "rotate" | "rot" => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Some(pad), Ok(amount)) = (pad_index(parts[0]), parts[1].parse::<i32>()) {
                    rotate_pattern(&mut result.pattern[pad], num_steps, amount);
                }
            }
        }
        "prob" | "probability" => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Some(pad), Ok(p)) = (pad_index(parts[0]), parts[1].parse::<f32>()) {
                    apply_probability(&mut result.pattern[pad], num_steps, p, 42);
                }
            }
        }

        // Per-pad effect: "pad_name fx_name value"
        "vol" | "volume" => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Some(pad), Ok(v)) = (pad_index(parts[0]), parts[1].parse::<f32>()) {
                    result.commands.push(ScriptCommand::SetPadVol(pad, v.clamp(0.0, 1.0)));
                }
            }
        }
        "pan" => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Some(pad), Ok(v)) = (pad_index(parts[0]), parts[1].parse::<f32>()) {
                    result.commands.push(ScriptCommand::SetPadPan(pad, v.clamp(-1.0, 1.0)));
                }
            }
        }
        "pitch" => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Some(pad), Ok(v)) = (pad_index(parts[0]), parts[1].parse::<f32>()) {
                    result.commands.push(ScriptCommand::SetPadPitch(pad, v.clamp(-24.0, 24.0)));
                }
            }
        }
        "filter" | "lpf" => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Some(pad), Ok(v)) = (pad_index(parts[0]), parts[1].parse::<f32>()) {
                    result.commands.push(ScriptCommand::SetPadFilter(pad, v.clamp(20.0, 20000.0)));
                }
            }
        }

        // Strudel-style chain: s("...") or n("...")
        _ if cmd.starts_with("s(") || cmd.starts_with("n(") => {
            // Extract the pattern string from s("...") or n("...")
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start+1..].find('"') {
                    let inner = &line[start+1..start+1+end];
                    let grid = crate::mini::to_grid(inner, result.effective_steps, 0);
                    for (name, step) in &grid {
                        if let Some(pad) = pad_index(name) {
                            if *step < result.effective_steps {
                                result.pattern[pad][*step] = 3;
                            }
                        } else if let Some(note) = parse_note_name(name) {
                            result.melody_notes.push((note, *step as f32, 1.0));
                        }
                    }
                }
            }
        }

        // Mini-notation (quoted string on its own line)
        _ if line.starts_with('"') => {
            parse_mini_notation(line, num_steps, result)?;
        }

        // Tracker-style: padname "pattern"
        _ => {
            if let Some(pad) = pad_index(&cmd) {
                if arg.starts_with('"') && arg.ends_with('"') && arg.len() > 2 {
                    let pattern_str = &arg[1..arg.len()-1];
                    parse_tracker_pattern(pad, pattern_str, num_steps, result)?;
                } else if !arg.is_empty() {
                    // Try without quotes
                    parse_tracker_pattern(pad, arg, num_steps, result)?;
                } else {
                    return Err(format!("Expected pattern after '{cmd}'"));
                }
            } else {
                return Err(format!("Unknown command: '{cmd}'"));
            }
        }
    }

    Ok(())
}

/// Parse tracker-style pattern: x=hard, o=medium, .=rest, -=rest
fn parse_tracker_pattern(pad: usize, pattern: &str, _num_steps: usize, result: &mut ScriptResult) -> Result<(), String> {
    let max_steps = result.pattern[pad].len(); // use actual buffer size
    for (i, ch) in pattern.chars().enumerate() {
        if i >= max_steps { break; }
        let vel = match ch {
            'x' | 'X' => 3,   // hard hit
            'o' | 'O' => 2,   // medium
            '+' => 1,          // soft (ghost note)
            '.' | '-' | ' ' | '~' => 0,  // rest
            _ => 0,
        };
        result.pattern[pad][i] = vel;
    }
    Ok(())
}

/// Parse Strudel-style mini-notation: "kick snare [hh hh] clap"
fn parse_mini_notation(line: &str, _num_steps: usize, result: &mut ScriptResult) -> Result<(), String> {
    let inner = line.trim_matches('"').trim();
    if inner.is_empty() { return Ok(()); }

    let effective_steps = result.effective_steps;

    // Use the proper mini-notation parser
    let grid = crate::mini::to_grid(inner, effective_steps, 0);
    for (name, step) in &grid {
        if let Some(pad) = pad_index(name) {
            if *step < effective_steps {
                result.pattern[pad][*step] = 3;
            }
        } else if let Some(note) = parse_note_name(name) {
            result.melody_notes.push((note, *step as f32, 1.0));
        }
    }
    return Ok(());
}

/// Format a pattern as BFS script (reverse: grid → code)
pub fn pattern_to_script(pattern: &[Vec<u8>], num_steps: usize, pad_names: &[String]) -> String {
    let mut lines = Vec::new();

    for (i, row) in pattern.iter().enumerate() {
        let steps = num_steps.min(row.len());
        let has_hits = row[..steps].iter().any(|&v| v > 0);
        if !has_hits { continue; }

        let name = pad_names.get(i).map(|s| s.to_lowercase()).unwrap_or(format!("pad{}", i));
        let pat: String = row[..steps].iter().map(|&v| match v {
            3 => 'x',
            2 => 'o',
            1 => '+',
            _ => '.',
        }).collect();

        lines.push(format!("{:<8} \"{}\"", name, pat));
    }

    if lines.is_empty() {
        "# Empty pattern — click cells in the SEQUENCER to add hits".to_string()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracker_pattern() {
        let result = evaluate("kick \"x...x...x...x...\"", 16);
        assert!(result.errors.is_empty());
        assert_eq!(result.pattern[0][0], 3);
        assert_eq!(result.pattern[0][1], 0);
        assert_eq!(result.pattern[0][4], 3);
    }

    #[test]
    fn test_bpm_command() {
        let result = evaluate("bpm 140", 16);
        assert!(result.errors.is_empty());
        assert_eq!(result.commands.len(), 1);
    }

    #[test]
    fn test_mini_notation() {
        let result = evaluate("\"kick snare hh clap\"", 16);
        assert!(result.errors.is_empty());
        assert_eq!(result.pattern[0][0], 3);  // kick at 0
        assert_eq!(result.pattern[1][4], 3);  // snare at 4
        assert_eq!(result.pattern[2][8], 3);  // hh at 8
        assert_eq!(result.pattern[4][12], 3); // clap at 12
    }

    #[test]
    fn test_repeat() {
        let result = evaluate("\"hh*4\"", 16);
        assert!(result.errors.is_empty());
        // Should have 4 hh hits evenly spread
        let hh_hits: usize = result.pattern[2].iter().filter(|&&v| v > 0).count();
        assert_eq!(hh_hits, 4);
    }

    #[test]
    fn test_comments() {
        let result = evaluate("# this is a comment\nkick \"x...x...\"", 16);
        assert!(result.errors.is_empty());
        assert_eq!(result.pattern[0][0], 3);
    }

    #[test]
    fn test_pattern_to_script() {
        let mut pattern = vec![vec![0u8; 16]; 16];
        pattern[0] = vec![3,0,0,0,3,0,0,0,3,0,0,0,3,0,0,0];
        let names: Vec<String> = (0..16).map(|i| format!("PAD{}", i)).collect();
        let script = pattern_to_script(&pattern, 16, &names);
        assert!(script.contains("x...x...x...x..."));
    }
}

// ═══════════════════════════════════════════════════════════
//  MELODY NOTATION — write synth melodies as note names
// ═══════════════════════════════════════════════════════════
//
//  note "C4 E4 G4 C5"          # quarter notes
//  note "C4*2 ~ E4 G4*2 ~ B3"  # with repeats and rests
//  note "C4.8 E4.4 G4.2"       # with length (steps)
//

/// Parse a note name like "C4", "F#3", "Bb5" into MIDI note number
pub fn parse_note_name(name: &str) -> Option<u8> {
    let name = name.trim();
    if name.len() < 2 { return None; }

    let chars: Vec<char> = name.chars().collect();
    let (note_base, idx) = match chars[0].to_uppercase().next()? {
        'C' => (0, 1),
        'D' => (2, 1),
        'E' => (4, 1),
        'F' => (5, 1),
        'G' => (7, 1),
        'A' => (9, 1),
        'B' => (11, 1),
        _ => return None,
    };

    let (semitone_offset, idx) = if idx < chars.len() {
        match chars[idx] {
            '#' | '♯' => (1i8, idx + 1),
            'b' | '♭' => (-1, idx + 1),
            _ => (0, idx),
        }
    } else {
        (0, idx)
    };

    // Parse octave number
    let octave_str: String = chars[idx..].iter().collect();
    let octave: i8 = octave_str.parse().ok()?;

    let midi = (octave + 1) as i16 * 12 + note_base as i16 + semitone_offset as i16;
    if midi >= 0 && midi <= 127 {
        Some(midi as u8)
    } else {
        None
    }
}

/// Parse a melody line: note "C4 E4 G4 C5"
/// Returns Vec of (midi_note, start_step, duration)
pub fn parse_melody(melody_str: &str, num_steps: usize) -> Vec<(u8, f32, f32)> {
    let inner = melody_str.trim().trim_matches('"');
    let tokens: Vec<&str> = inner.split_whitespace().collect();
    if tokens.is_empty() { return Vec::new(); }

    let steps_per_token = num_steps as f32 / tokens.len() as f32;
    let mut notes = Vec::new();

    for (i, token) in tokens.iter().enumerate() {
        let step = i as f32 * steps_per_token;

        if *token == "~" || *token == "." || *token == "-" {
            continue; // rest
        }

        // Handle note.duration syntax: C4.8 means C4 for 8 steps
        let (note_str, duration) = if let Some((n, d)) = token.split_once('.') {
            if let Ok(dur) = d.parse::<f32>() {
                (n, dur)
            } else {
                (*token, steps_per_token)
            }
        } else {
            (*token, steps_per_token)
        };

        // Handle repeat: C4*2
        if let Some((n, rep_str)) = note_str.split_once('*') {
            if let (Some(midi), Ok(reps)) = (parse_note_name(n), rep_str.parse::<usize>()) {
                let sub_step = steps_per_token / reps as f32;
                for r in 0..reps {
                    notes.push((midi, step + r as f32 * sub_step, sub_step * 0.9));
                }
            }
        } else if let Some(midi) = parse_note_name(note_str) {
            notes.push((midi, step, duration * 0.9));
        }
    }

    notes
}

// ═══════════════════════════════════════════════════════════
//  PATTERN TRANSFORMS
// ═══════════════════════════════════════════════════════════

/// Reverse a pattern row
pub fn reverse_pattern(row: &mut Vec<u8>, num_steps: usize) {
    let slice = &mut row[..num_steps];
    slice.reverse();
}

/// Rotate a pattern row by N steps
pub fn rotate_pattern(row: &mut Vec<u8>, num_steps: usize, amount: i32) {
    let n = num_steps;
    if n == 0 { return; }
    let shift = ((amount % n as i32) + n as i32) as usize % n;
    let temp = row[..n].to_vec();
    for i in 0..n {
        row[i] = temp[(i + n - shift) % n];
    }
}

/// Apply probability: randomly remove hits with given chance (0.0 = remove all, 1.0 = keep all)
pub fn apply_probability(row: &mut Vec<u8>, num_steps: usize, prob: f32, seed: u32) {
    let mut rng = seed;
    for i in 0..num_steps {
        if row[i] > 0 {
            rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;
            if (rng % 100) as f32 / 100.0 > prob {
                row[i] = 0;
            }
        }
    }
}

#[cfg(test)]
mod melody_tests {
    use super::*;

    #[test]
    fn test_parse_note_name() {
        assert_eq!(parse_note_name("C4"), Some(60));
        assert_eq!(parse_note_name("A4"), Some(69));
        assert_eq!(parse_note_name("C#4"), Some(61));
        assert_eq!(parse_note_name("Bb3"), Some(58));
        assert_eq!(parse_note_name("C0"), Some(12));
    }

    #[test]
    fn test_parse_melody() {
        let notes = parse_melody("C4 E4 G4 C5", 16);
        assert_eq!(notes.len(), 4);
        assert_eq!(notes[0].0, 60); // C4
        assert_eq!(notes[1].0, 64); // E4
        assert_eq!(notes[2].0, 67); // G4
        assert_eq!(notes[3].0, 72); // C5
    }

    #[test]
    fn test_reverse_pattern() {
        let mut row = vec![3, 0, 0, 0, 2, 0, 0, 0];
        reverse_pattern(&mut row, 8);
        assert_eq!(row[..8], [0, 0, 0, 2, 0, 0, 0, 3]);
    }

    #[test]
    fn test_rotate_pattern() {
        let mut row = vec![3, 0, 0, 0, 2, 0, 0, 0];
        rotate_pattern(&mut row, 8, 2);
        assert_eq!(row[..8], [0, 0, 3, 0, 0, 0, 2, 0]);
    }
}

// ═══════════════════════════════════════════════════════════
//  ADVANCED SCRIPT COMMANDS
// ═══════════════════════════════════════════════════════════

/// Extended commands for full DAW control from code
#[derive(Clone)]
pub enum ExtCommand {
    // Synth control
    AssignSynth(usize, String),      // pad, preset_name
    // Sample control
    LoadKit(String),                  // "808", "modern", "chops"
    // Effects per pad
    SetDistortion(usize, f32),       // pad, drive
    SetBitcrush(usize, f32, f32),    // pad, bits, rate
    SetChorus(usize, f32),           // pad, mix
    SetPhaser(usize, f32),           // pad, mix
    SetReverbSend(usize, f32),       // pad, amount
    SetDelaySend(usize, f32),        // pad, amount
    // Sidechain
    SetSidechain(usize, usize),      // source, target
    // Master
    SetEnhancer(f32),
    SetStereoWidth(f32),
    SetMasterFilter(f32),
    // Arrangement
    SetBank(usize),                   // switch to bank
    // Export
    ExportBars(usize),                // bounce N bars
}

/// Parse extended commands that give full DAW control
pub fn parse_extended(line: &str) -> Option<ExtCommand> {
    let parts: Vec<&str> = line.trim().splitn(3, |c: char| c.is_whitespace()).collect();
    if parts.is_empty() { return None; }

    let cmd = parts[0].to_lowercase();
    match cmd.as_str() {
        // Synth preset: synth kick "808 Sub"
        "synth" | "preset" => {
            if parts.len() >= 3 {
                let pad = pad_index(parts[1])?;
                let preset = parts[2].trim_matches('"').to_string();
                Some(ExtCommand::AssignSynth(pad, preset))
            } else { None }
        }

        // Kit loading: kit 808 / kit modern / kit chops
        "kit" | "loadkit" => {
            Some(ExtCommand::LoadKit(parts.get(1).unwrap_or(&"808").to_string()))
        }

        // Per-pad effects
        "dist" | "distortion" | "drive" => {
            if parts.len() >= 3 {
                let pad = pad_index(parts[1])?;
                let v: f32 = parts[2].parse().ok()?;
                Some(ExtCommand::SetDistortion(pad, v.clamp(0.0, 1.0)))
            } else { None }
        }
        "crush" | "bitcrush" => {
            if parts.len() >= 3 {
                let pad = pad_index(parts[1])?;
                let bits: f32 = parts[2].parse().ok()?;
                Some(ExtCommand::SetBitcrush(pad, bits.clamp(1.0, 16.0), 1.0))
            } else { None }
        }
        "chorus" => {
            if parts.len() >= 3 {
                let pad = pad_index(parts[1])?;
                let v: f32 = parts[2].parse().ok()?;
                Some(ExtCommand::SetChorus(pad, v.clamp(0.0, 1.0)))
            } else { None }
        }
        "phaser" => {
            if parts.len() >= 3 {
                let pad = pad_index(parts[1])?;
                let v: f32 = parts[2].parse().ok()?;
                Some(ExtCommand::SetPhaser(pad, v.clamp(0.0, 1.0)))
            } else { None }
        }

        // Send levels
        "send_reverb" | "sendverb" | "srv" => {
            if parts.len() >= 3 {
                let pad = pad_index(parts[1])?;
                let v: f32 = parts[2].parse().ok()?;
                Some(ExtCommand::SetReverbSend(pad, v.clamp(0.0, 1.0)))
            } else { None }
        }
        "send_delay" | "senddly" | "sdl" => {
            if parts.len() >= 3 {
                let pad = pad_index(parts[1])?;
                let v: f32 = parts[2].parse().ok()?;
                Some(ExtCommand::SetDelaySend(pad, v.clamp(0.0, 1.0)))
            } else { None }
        }

        // Sidechain
        "sidechain" | "sc" => {
            if parts.len() >= 3 {
                let src = pad_index(parts[1])?;
                let tgt = pad_index(parts[2])?;
                Some(ExtCommand::SetSidechain(src, tgt))
            } else { None }
        }

        // Master
        "enhancer" | "enhance" | "enh" => {
            let v: f32 = parts.get(1)?.parse().ok()?;
            Some(ExtCommand::SetEnhancer(v.clamp(0.0, 1.0)))
        }
        "width" | "stereo" => {
            let v: f32 = parts.get(1)?.parse().ok()?;
            Some(ExtCommand::SetStereoWidth(v.clamp(0.0, 2.0)))
        }
        "master_filter" | "mfilter" | "mlpf" => {
            let v: f32 = parts.get(1)?.parse().ok()?;
            Some(ExtCommand::SetMasterFilter(v.clamp(20.0, 20000.0)))
        }

        // Bank
        "bank" => {
            let b: usize = parts.get(1)?.parse::<usize>().ok()?.checked_sub(1)?;
            Some(ExtCommand::SetBank(b))
        }

        // Export
        "export" | "bounce" => {
            let bars: usize = parts.get(1)?.parse().ok()?;
            Some(ExtCommand::ExportBars(bars))
        }

        _ => None,
    }
}

/// Process all extended commands in a script, returning them as a list
pub fn extract_extended_commands(script: &str) -> Vec<ExtCommand> {
    script.lines()
        .filter_map(|line| parse_extended(line.trim()))
        .collect()
}

#[cfg(test)]
mod eval_tests {
    use super::*;

    #[test]
    fn test_steps_command_sets_length() {
        let result = evaluate("steps 32\nkick \"x...x...x...x...x...x...x...x...\"", 16);
        assert_eq!(result.effective_steps, 32);
        assert_eq!(result.pattern[0].len(), 32);
    }

    #[test]
    fn test_32_step_pattern_fully_parsed() {
        let result = evaluate("steps 32\nkick \"x...x...x...x...x...x...x...x...\"", 16);
        // Should have hits at 0, 4, 8, 12, 16, 20, 24, 28
        let hits: Vec<usize> = result.pattern[0].iter().enumerate()
            .filter(|(_, &v)| v > 0).map(|(i, _)| i).collect();
        assert_eq!(hits, vec![0, 4, 8, 12, 16, 20, 24, 28], "Expected 8 hits across 32 steps");
    }

    #[test]
    fn test_pattern_without_steps_uses_default() {
        let result = evaluate("kick \"x...x...x...x...\"", 16);
        assert_eq!(result.effective_steps, 16);
        assert_eq!(result.pattern[0].len(), 16);
        let hits: usize = result.pattern[0].iter().filter(|&&v| v > 0).count();
        assert_eq!(hits, 4);
    }

    #[test]
    fn test_edm_script_parses_correctly() {
        let script = include_str!("../songs/edm_supersaw.bfs");
        let result = evaluate(script, 16);
        assert_eq!(result.effective_steps, 32, "EDM should set steps to 32");
        assert!(result.errors.is_empty(), "EDM script errors: {:?}", result.errors);

        // Kick should have hits across full 32 steps
        let kick_hits: Vec<usize> = result.pattern[0].iter().enumerate()
            .filter(|(_, &v)| v > 0).map(|(i, _)| i).collect();
        assert!(kick_hits.len() >= 8, "Kick should have 8+ hits, got {:?}", kick_hits);
        assert!(*kick_hits.last().unwrap() >= 24, "Last kick hit should be at step 24+, got {:?}", kick_hits);
    }

    #[test]
    fn test_full_demo_parses_correctly() {
        let script = include_str!("../songs/full_production.bfs");
        let result = evaluate(script, 16);
        assert_eq!(result.effective_steps, 32);
        assert!(result.errors.is_empty(), "Full demo errors: {:?}", result.errors);
    }

    #[test]
    fn test_all_demo_songs_parse_without_errors() {
        let songs = [
            ("lofi", include_str!("../songs/lofi_chill.bfs")),
            ("trap", include_str!("../songs/trap_banger.bfs")),
            ("boom_bap", include_str!("../songs/boom_bap_soul.bfs")),
            ("house", include_str!("../songs/house_groove.bfs")),
            ("jungle", include_str!("../songs/jungle_breaks.bfs")),
            ("edm", include_str!("../songs/edm_supersaw.bfs")),
            ("full", include_str!("../songs/full_production.bfs")),
            ("tutorial", include_str!("../songs/strudel_showcase.bfs")),
        ];
        for (name, script) in songs {
            let result = evaluate(script, 16);
            assert!(result.errors.is_empty(), "Song '{}' has errors: {:?}", name, result.errors);
        }
    }

    #[test]
    fn test_velocity_chars() {
        let result = evaluate("kick \"xo+.xo+.\"", 8);
        assert_eq!(result.pattern[0][0], 3); // x = hard
        assert_eq!(result.pattern[0][1], 2); // o = medium
        assert_eq!(result.pattern[0][2], 1); // + = soft
        assert_eq!(result.pattern[0][3], 0); // . = rest
    }

    #[test]
    fn test_extended_commands_dont_error() {
        let script = "kit 808\nsynth pad10 \"808 Sub\"\nsidechain kick snare\nenhancer 0.3\nwidth 1.2";
        let result = evaluate(script, 16);
        assert!(result.errors.is_empty(), "Extended commands should not produce errors: {:?}", result.errors);
    }
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    fn check_song(name: &str, script: &str) {
        let result = evaluate(script, 16);
        assert!(result.errors.is_empty(), "Song '{}' errors: {:?}", name, result.errors);

        // Check that at least some pads have data
        let total_hits: usize = result.pattern.iter()
            .map(|row| row.iter().filter(|&&v| v > 0).count())
            .sum();
        assert!(total_hits > 0, "Song '{}' has no hits at all!", name);

        // Check that pattern length matches effective_steps
        for (i, row) in result.pattern.iter().enumerate() {
            assert_eq!(row.len(), result.effective_steps,
                "Song '{}' pad {} has {} steps, expected {}", name, i, row.len(), result.effective_steps);
        }

        // Print summary
        let steps = result.effective_steps;
        let active_pads: Vec<usize> = result.pattern.iter().enumerate()
            .filter(|(_, row)| row.iter().any(|&v| v > 0))
            .map(|(i, _)| i)
            .collect();
        let melody_notes = result.melody_notes.len();
        let commands = result.commands.len();
        eprintln!("  {} — {} steps, {} active pads {:?}, {} melody notes, {} commands",
            name, steps, active_pads.len(), active_pads, melody_notes, commands);
    }

    #[test]
    fn test_lofi_song() {
        check_song("lofi", include_str!("../songs/lofi_chill.bfs"));
    }

    #[test]
    fn test_trap_song() {
        check_song("trap", include_str!("../songs/trap_banger.bfs"));
    }

    #[test]
    fn test_boom_bap_song() {
        check_song("boom_bap", include_str!("../songs/boom_bap_soul.bfs"));
    }

    #[test]
    fn test_house_song() {
        check_song("house", include_str!("../songs/house_groove.bfs"));
    }

    #[test]
    fn test_jungle_song() {
        check_song("jungle", include_str!("../songs/jungle_breaks.bfs"));
    }

    #[test]
    fn test_edm_song() {
        check_song("edm", include_str!("../songs/edm_supersaw.bfs"));
    }

    #[test]
    fn test_full_demo_song() {
        check_song("full_demo", include_str!("../songs/full_production.bfs"));
    }
}
