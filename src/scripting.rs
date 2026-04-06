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
    /// Commands to execute
    pub commands: Vec<ScriptCommand>,
    /// Error messages
    pub errors: Vec<String>,
}

#[derive(Clone)]
pub enum ScriptCommand {
    SetBpm(f32),
    SetSwing(f32),
    SetReverb(f32),
    SetDelay(f32),
    Euclidean(usize, usize, usize), // pad, hits, steps
}

/// Parse and evaluate a BeatForge Script
pub fn evaluate(script: &str, num_steps: usize) -> ScriptResult {
    let mut result = ScriptResult {
        pattern: vec![vec![0u8; num_steps]; NUM_PADS],
        commands: Vec::new(),
        errors: Vec::new(),
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
fn parse_tracker_pattern(pad: usize, pattern: &str, num_steps: usize, result: &mut ScriptResult) -> Result<(), String> {
    for (i, ch) in pattern.chars().enumerate() {
        if i >= num_steps { break; }
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
fn parse_mini_notation(line: &str, num_steps: usize, result: &mut ScriptResult) -> Result<(), String> {
    let inner = line.trim_matches('"').trim();
    if inner.is_empty() { return Ok(()); }

    // Split into tokens
    let tokens: Vec<&str> = inner.split_whitespace().collect();
    if tokens.is_empty() { return Ok(()); }

    // Each token gets an equal share of the steps
    let steps_per_token = num_steps / tokens.len().max(1);

    for (i, token) in tokens.iter().enumerate() {
        let step = i * steps_per_token;
        if step >= num_steps { break; }

        // Handle repeat: kick*4
        if let Some((name, repeat_str)) = token.split_once('*') {
            if let (Some(pad), Ok(repeats)) = (pad_index(name), repeat_str.parse::<usize>()) {
                let sub_step = steps_per_token / repeats.max(1);
                for r in 0..repeats {
                    let s = step + r * sub_step;
                    if s < num_steps {
                        result.pattern[pad][s] = 3;
                    }
                }
            }
        }
        // Handle rest
        else if *token == "~" || *token == "." || *token == "-" {
            // rest — do nothing
        }
        // Handle [group] — subdivide
        else if token.starts_with('[') && token.ends_with(']') {
            let group = &token[1..token.len()-1];
            let sub_tokens: Vec<&str> = group.split_whitespace().collect();
            let sub_step = steps_per_token / sub_tokens.len().max(1);
            for (j, sub_token) in sub_tokens.iter().enumerate() {
                let s = step + j * sub_step;
                if s < num_steps {
                    if let Some(pad) = pad_index(sub_token) {
                        result.pattern[pad][s] = 3;
                    }
                }
            }
        }
        // Simple pad name
        else if let Some(pad) = pad_index(token) {
            result.pattern[pad][step] = 3;
        }
    }

    Ok(())
}

/// Format a pattern as BFS script (reverse: grid → code)
pub fn pattern_to_script(pattern: &[Vec<u8>], num_steps: usize, pad_names: &[String]) -> String {
    let mut lines = Vec::new();

    for (i, row) in pattern.iter().enumerate() {
        let has_hits = row[..num_steps].iter().any(|&v| v > 0);
        if !has_hits { continue; }

        let name = pad_names.get(i).map(|s| s.to_lowercase()).unwrap_or(format!("pad{}", i));
        let pat: String = row[..num_steps].iter().map(|&v| match v {
            3 => 'x',
            2 => 'o',
            1 => '+',
            _ => '.',
        }).collect();

        lines.push(format!("{:<8} \"{}\"", name, pat));
    }

    lines.join("\n")
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
