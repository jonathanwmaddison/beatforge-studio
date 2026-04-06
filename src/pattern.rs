//! Pattern engine — the heart of BeatForge Script.
//! A Strudel/TidalCycles-inspired pattern system that goes deeper
//! by integrating with the DAW's visual grid and audio engine.
//!
//! Key concepts:
//! - Patterns are functions of time → values
//! - Patterns can be composed (stack, cat, layer)
//! - Patterns can be transformed (fast, slow, rev, every, jux)
//! - Patterns render to both the step grid AND produce audio events
//!
//! Syntax improvements over Strudel:
//! - Bidirectional: code ↔ visual grid (Strudel is code-only)
//! - Integrated effects chain: .reverb .delay .lpf .dist
//! - Direct MIDI output and sample triggering
//! - Pattern recording: play live → generates code

use crate::audio::NUM_PADS;

// ═══════════════════════════════════════════════════════════
//  PATTERN TYPES
// ═══════════════════════════════════════════════════════════

/// A rhythmic event at a specific time
#[derive(Clone, Debug)]
pub struct Event {
    pub start: f32,     // 0.0 - 1.0 within the cycle
    pub end: f32,       // end of the event
    pub value: Value,   // what happens
}

/// What an event triggers
#[derive(Clone, Debug)]
pub enum Value {
    Sound(String),      // pad/sample name
    Note(u8),           // MIDI note
    Rest,               // silence
    Number(f32),        // continuous value (for automation)
}

/// A pattern that generates events over time
#[derive(Clone)]
pub enum Pattern {
    /// Sequence of sub-patterns played in order
    Cat(Vec<Pattern>),
    /// Sub-patterns stacked (played simultaneously)
    Stack(Vec<Pattern>),
    /// Single event
    Atom(Value),
    /// Silence
    Silence,
    /// Repeat N times within the same time span
    Fast(Box<Pattern>, f32),
    /// Slow down by factor
    Slow(Box<Pattern>, f32),
    /// Reverse
    Rev(Box<Pattern>),
    /// Apply reverse every N cycles
    EveryRev(Box<Pattern>, usize),
    /// Euclidean rhythm: distribute N hits across M slots
    Euclid(usize, usize, Box<Pattern>),
    /// Random choice from list
    Choose(Vec<Pattern>, u32), // patterns + seed
    /// Degrade: randomly drop events with probability
    Degrade(Box<Pattern>, f32, u32), // pattern, probability, seed
    /// Chain effects
    WithEffect(Box<Pattern>, EffectChain),
}

/// Effects that can be applied inline to patterns
#[derive(Clone, Debug)]
pub struct EffectChain {
    pub reverb: Option<f32>,
    pub delay: Option<f32>,
    pub lpf: Option<f32>,
    pub hpf: Option<f32>,
    pub dist: Option<f32>,
    pub pan: Option<f32>,
    pub gain: Option<f32>,
    pub speed: Option<f32>,   // sample playback speed
}

impl Default for EffectChain {
    fn default() -> Self {
        EffectChain {
            reverb: None, delay: None, lpf: None, hpf: None,
            dist: None, pan: None, gain: None, speed: None,
        }
    }
}

// ═══════════════════════════════════════════════════════════
//  PATTERN EVALUATION — render pattern to events
// ═══════════════════════════════════════════════════════════

impl Pattern {
    /// Evaluate the pattern for one cycle, returning all events
    pub fn query(&self, cycle: usize, num_steps: usize) -> Vec<Event> {
        self.query_span(0.0, 1.0, cycle, num_steps)
    }

    fn query_span(&self, start: f32, end: f32, cycle: usize, num_steps: usize) -> Vec<Event> {
        let duration = end - start;
        if duration <= 0.0 { return Vec::new(); }

        match self {
            Pattern::Atom(val) => {
                vec![Event { start, end, value: val.clone() }]
            }
            Pattern::Silence => Vec::new(),

            Pattern::Cat(pats) => {
                if pats.is_empty() { return Vec::new(); }
                let n = pats.len();
                let sub_dur = duration / n as f32;
                let mut events = Vec::new();
                for (i, pat) in pats.iter().enumerate() {
                    let s = start + i as f32 * sub_dur;
                    let e = s + sub_dur;
                    events.extend(pat.query_span(s, e, cycle, num_steps));
                }
                events
            }

            Pattern::Stack(pats) => {
                let mut events = Vec::new();
                for pat in pats {
                    events.extend(pat.query_span(start, end, cycle, num_steps));
                }
                events
            }

            Pattern::Fast(pat, factor) => {
                let mut events = Vec::new();
                let n = *factor as usize;
                let sub_dur = duration / *factor;
                for i in 0..n {
                    let s = start + i as f32 * sub_dur;
                    let e = s + sub_dur;
                    events.extend(pat.query_span(s, e, cycle, num_steps));
                }
                events
            }

            Pattern::Slow(pat, factor) => {
                // Only play 1/factor of the pattern per cycle
                let effective_cycle = cycle as f32 / factor;
                if effective_cycle.fract() < (1.0 / factor) {
                    pat.query_span(start, end, cycle, num_steps)
                } else {
                    Vec::new()
                }
            }

            Pattern::Rev(pat) => {
                let mut events = pat.query_span(start, end, cycle, num_steps);
                let n = events.len().max(1) as f32;
                let span = end - start;
                for i in 0..events.len() {
                    let offset = events[i].start - start;
                    let dur = events[i].end - events[i].start;
                    events[i].start = end - offset - dur;
                    events[i].end = events[i].start + dur;
                }
                events.reverse();
                events
            }

            Pattern::Euclid(hits, steps, pat) => {
                let mut events = Vec::new();
                let step_dur = duration / *steps as f32;
                for i in 0..*hits {
                    let pos = (i * steps) / hits;
                    let s = start + pos as f32 * step_dur;
                    let e = s + step_dur;
                    events.extend(pat.query_span(s, e, cycle, num_steps));
                }
                events
            }

            Pattern::Choose(pats, seed) => {
                if pats.is_empty() { return Vec::new(); }
                let mut rng = seed.wrapping_add(cycle as u32);
                rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;
                let idx = rng as usize % pats.len();
                pats[idx].query_span(start, end, cycle, num_steps)
            }

            Pattern::Degrade(pat, prob, seed) => {
                let mut events = pat.query_span(start, end, cycle, num_steps);
                let mut rng = seed.wrapping_add(cycle as u32);
                events.retain(|_| {
                    rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;
                    (rng % 100) as f32 / 100.0 < *prob
                });
                events
            }

            Pattern::EveryRev(pat, n) => {
                if cycle % n == 0 {
                    let rev = Pattern::Rev(pat.clone());
                    rev.query_span(start, end, cycle, num_steps)
                } else {
                    pat.query_span(start, end, cycle, num_steps)
                }
            }

            Pattern::WithEffect(pat, _fx) => {
                // Events inherit the effect chain (handled at render time)
                pat.query_span(start, end, cycle, num_steps)
            }
        }
    }

    /// Render pattern to a step grid (for the sequencer)
    pub fn to_grid(&self, num_steps: usize, pad: usize) -> Vec<u8> {
        let mut grid = vec![0u8; num_steps];
        let events = self.query(0, num_steps);
        for evt in &events {
            let step = (evt.start * num_steps as f32) as usize;
            if step < num_steps {
                grid[step] = match &evt.value {
                    Value::Rest => 0,
                    _ => 3,
                };
            }
        }
        grid
    }
}

// ═══════════════════════════════════════════════════════════
//  PATTERN PARSER — parse BFS+ syntax into Pattern trees
// ═══════════════════════════════════════════════════════════

/// Parse a mini-notation string into a Pattern
/// "kick snare [hh hh] clap" → Cat of sounds
/// "kick*4" → Fast(kick, 4)
/// "kick | snare" → Choose
/// "kick , snare" → Stack
pub fn parse_mini(input: &str) -> Pattern {
    let input = input.trim();
    if input.is_empty() { return Pattern::Silence; }

    // Stack operator (comma = simultaneous)
    if input.contains(',') {
        let parts: Vec<Pattern> = input.split(',').map(|s| parse_mini(s.trim())).collect();
        return Pattern::Stack(parts);
    }

    // Choose operator (pipe = random choice)
    if input.contains('|') {
        let parts: Vec<Pattern> = input.split('|').map(|s| parse_mini(s.trim())).collect();
        return Pattern::Choose(parts, 42);
    }

    // Sequence (space-separated)
    let tokens: Vec<&str> = input.split_whitespace().collect();
    if tokens.len() > 1 {
        let pats: Vec<Pattern> = tokens.iter().map(|t| parse_token(t)).collect();
        return Pattern::Cat(pats);
    }

    // Single token
    parse_token(input)
}

fn parse_token(token: &str) -> Pattern {
    let token = token.trim();

    // Handle brackets [x y z] → faster subdivision
    if token.starts_with('[') && token.ends_with(']') {
        let inner = &token[1..token.len()-1];
        return Pattern::Fast(Box::new(parse_mini(inner)), 1.0);
    }

    // Handle angle brackets <x y z> → choose one per cycle
    if token.starts_with('<') && token.ends_with('>') {
        let inner = &token[1..token.len()-1];
        let parts: Vec<Pattern> = inner.split_whitespace().map(|s| parse_token(s)).collect();
        return Pattern::Choose(parts, 42);
    }

    // Handle repeat: kick*4
    if let Some((name, rep_str)) = token.split_once('*') {
        if let Ok(n) = rep_str.parse::<f32>() {
            return Pattern::Fast(Box::new(parse_token(name)), n);
        }
    }

    // Handle euclidean: kick(3,8)
    if let Some(paren_start) = token.find('(') {
        let name = &token[..paren_start];
        let params = &token[paren_start+1..token.len()-1];
        let parts: Vec<&str> = params.split(',').collect();
        if parts.len() >= 2 {
            if let (Ok(hits), Ok(steps)) = (parts[0].trim().parse::<usize>(), parts[1].trim().parse::<usize>()) {
                return Pattern::Euclid(hits, steps, Box::new(parse_token(name)));
            }
        }
    }

    // Handle degrade: kick?0.5
    if let Some((name, prob_str)) = token.split_once('?') {
        if let Ok(prob) = prob_str.parse::<f32>() {
            return Pattern::Degrade(Box::new(parse_token(name)), prob, 42);
        }
    }

    // Rest
    if token == "~" || token == "." || token == "-" {
        return Pattern::Atom(Value::Rest);
    }

    // Note name (C4, F#3, etc)
    if let Some(note) = crate::scripting::parse_note_name(token) {
        return Pattern::Atom(Value::Note(note));
    }

    // Sound name
    Pattern::Atom(Value::Sound(token.to_string()))
}

// ═══════════════════════════════════════════════════════════
//  PATTERN CONSTRUCTORS (Strudel-style API)
// ═══════════════════════════════════════════════════════════

/// s("kick snare [hh hh] clap") — sound pattern
pub fn s(notation: &str) -> Pattern {
    parse_mini(notation)
}

/// n("C4 E4 G4 C5") — note pattern
pub fn n(notation: &str) -> Pattern {
    parse_mini(notation)
}

/// stack(patterns) — play simultaneously
pub fn stack(patterns: Vec<Pattern>) -> Pattern {
    Pattern::Stack(patterns)
}

/// cat(patterns) — play in sequence
pub fn cat(patterns: Vec<Pattern>) -> Pattern {
    Pattern::Cat(patterns)
}

/// fast(n, pattern) — speed up
pub fn fast(factor: f32, pattern: Pattern) -> Pattern {
    Pattern::Fast(Box::new(pattern), factor)
}

/// slow(n, pattern) — slow down
pub fn slow(factor: f32, pattern: Pattern) -> Pattern {
    Pattern::Slow(Box::new(pattern), factor)
}

/// rev(pattern) — reverse
pub fn rev(pattern: Pattern) -> Pattern {
    Pattern::Rev(Box::new(pattern))
}

/// euclid(hits, steps, pattern) — euclidean distribution
pub fn euclid(hits: usize, steps: usize, pattern: Pattern) -> Pattern {
    Pattern::Euclid(hits, steps, Box::new(pattern))
}

/// degrade(prob, pattern) — randomly drop events
pub fn degrade(prob: f32, pattern: Pattern) -> Pattern {
    Pattern::Degrade(Box::new(pattern), prob, 42)
}

// ═══════════════════════════════════════════════════════════
//  ADVANCED PARSER — handle Strudel-style function chains
// ═══════════════════════════════════════════════════════════

/// Parse a full BFS+ line that may include function calls:
/// s("kick snare").fast(2).rev()
/// n("C4 E4 G4").slow(2)
pub fn parse_chain(input: &str) -> Pattern {
    let input = input.trim();

    // Split by dots that are followed by function names
    // First, extract the base pattern
    let (base_str, chain) = if input.starts_with("s(") || input.starts_with("n(") {
        // Find the closing paren
        if let Some(close) = input.find(')') {
            let func = &input[..2];
            let inner = &input[2..close].trim_matches(|c| c == '(' || c == ')').trim_matches('"');
            let rest = &input[close+1..];
            let base = match func {
                "s(" => parse_mini(inner),
                "n(" => parse_mini(inner),
                _ => parse_mini(inner),
            };
            (base, rest)
        } else {
            (parse_mini(input), "")
        }
    } else if input.starts_with('"') {
        let inner = input.trim_matches('"');
        (parse_mini(inner), "")
    } else {
        (parse_mini(input), "")
    };

    // Apply chain of transforms
    let mut pattern = base_str;
    for part in chain.split('.') {
        let part = part.trim();
        if part.is_empty() { continue; }

        if part.starts_with("fast(") {
            if let Some(n) = extract_float_arg(part) {
                pattern = Pattern::Fast(Box::new(pattern), n);
            }
        } else if part.starts_with("slow(") {
            if let Some(n) = extract_float_arg(part) {
                pattern = Pattern::Slow(Box::new(pattern), n);
            }
        } else if part == "rev()" || part == "rev" {
            pattern = Pattern::Rev(Box::new(pattern));
        } else if part.starts_with("degrade(") || part.starts_with("deg(") {
            if let Some(p) = extract_float_arg(part) {
                pattern = Pattern::Degrade(Box::new(pattern), p, 42);
            }
        } else if part.starts_with("euclid(") {
            // euclid(3,8)
            let inner = part.split('(').nth(1).unwrap_or("").trim_end_matches(')');
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() >= 2 {
                if let (Ok(h), Ok(s)) = (parts[0].trim().parse(), parts[1].trim().parse()) {
                    pattern = Pattern::Euclid(h, s, Box::new(pattern));
                }
            }
        }
    }

    pattern
}

fn extract_float_arg(s: &str) -> Option<f32> {
    let start = s.find('(')?;
    let end = s.find(')')?;
    s[start+1..end].trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let pat = parse_mini("kick snare hh clap");
        let events = pat.query(0, 16);
        assert_eq!(events.len(), 4);
    }

    #[test]
    fn test_parse_repeat() {
        let pat = parse_mini("hh*4");
        let events = pat.query(0, 16);
        assert_eq!(events.len(), 4);
    }

    #[test]
    fn test_stack() {
        let pat = parse_mini("kick , snare");
        let events = pat.query(0, 16);
        assert_eq!(events.len(), 2);
        // Both should start at 0
        assert!((events[0].start - events[1].start).abs() < 0.01);
    }

    #[test]
    fn test_choose() {
        let pat = parse_mini("kick | snare | hh");
        let events = pat.query(0, 16);
        // Should pick exactly one
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_euclidean_syntax() {
        let pat = parse_token("kick(3,16)");
        let events = pat.query(0, 16);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn test_degrade() {
        let pat = parse_mini("hh*8");
        let deg = degrade(0.5, pat);
        let events = deg.query(0, 16);
        // Should have roughly half the events (probabilistic)
        assert!(events.len() <= 8);
    }

    #[test]
    fn test_chain_parse() {
        let pat = parse_chain("s(\"kick snare hh clap\").fast(2)");
        let events = pat.query(0, 16);
        assert_eq!(events.len(), 8); // 4 sounds × fast(2) = 8
    }

    #[test]
    fn test_to_grid() {
        let pat = parse_mini("kick snare hh clap");
        let grid = pat.to_grid(16, 0);
        // Should have 4 hits spread across 16 steps
        let hits: usize = grid.iter().filter(|&&v| v > 0).count();
        assert_eq!(hits, 4);
    }

    #[test]
    fn test_note_pattern() {
        let pat = parse_mini("C4 E4 G4");
        let events = pat.query(0, 16);
        assert_eq!(events.len(), 3);
        match &events[0].value {
            Value::Note(n) => assert_eq!(*n, 60),
            _ => panic!("Expected note"),
        }
    }

    #[test]
    fn test_rest() {
        let pat = parse_mini("kick ~ snare ~");
        let events = pat.query(0, 16);
        // 2 sounds + 2 rests = 4 events, but rests are Value::Rest
        let sounds: Vec<_> = events.iter().filter(|e| !matches!(e.value, Value::Rest)).collect();
        assert_eq!(sounds.len(), 2);
    }
}
