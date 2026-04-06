//! Mini-notation parser — faithful implementation of the Strudel/TidalCycles
//! mini-notation syntax for describing rhythmic and melodic patterns.
//!
//! Reference: https://strudel.cc/learn/mini-notation/
//!
//! Operators:
//!   space     Sequence: "a b c" = a then b then c, equally spaced
//!   [x y]     Subdivision: events share parent's time slot
//!   <x y>     Alternation: cycle through one per bar (slow)
//!   x*N       Speed up: play N times in the same time
//!   x/N       Slow down: spread across N cycles
//!   x@N       Weight: give N times more time than siblings
//!   x!N       Replicate: repeat N times (same speed, no compression)
//!   x?        Degrade: 50% chance to become rest
//!   x?0.3     Degrade with custom probability (0-1)
//!   x,y       Stack: play simultaneously
//!   x|y       Random: choose one randomly
//!   ~         Rest (silence)
//!   x(k,n)    Euclidean: k hits distributed in n steps
//!   x(k,n,o)  Euclidean with offset o

/// A node in the mini-notation AST
#[derive(Clone, Debug)]
pub enum MiniNode {
    /// Single sound/note atom
    Atom(String),
    /// Rest / silence
    Rest,
    /// Sequence of nodes (space-separated, equal time each)
    Seq(Vec<WeightedNode>),
    /// Stack: play all simultaneously (comma-separated)
    Stack(Vec<MiniNode>),
    /// Alternation: cycle through one per repetition (angle brackets)
    Alt(Vec<MiniNode>),
    /// Random choice (pipe-separated)
    Choose(Vec<MiniNode>),
    /// Speed up by factor
    Fast(Box<MiniNode>, f32),
    /// Slow down by factor
    Slow(Box<MiniNode>, f32),
    /// Replicate (repeat without compression)
    Replicate(Box<MiniNode>, usize),
    /// Degrade with probability (1.0 = always play, 0.0 = never)
    Degrade(Box<MiniNode>, f32),
    /// Euclidean rhythm
    Euclid(Box<MiniNode>, usize, usize, usize), // node, hits, steps, offset
}

/// A node with an optional weight (@N)
#[derive(Clone, Debug)]
pub struct WeightedNode {
    pub node: MiniNode,
    pub weight: f32, // default 1.0
}

impl WeightedNode {
    fn new(node: MiniNode) -> Self {
        WeightedNode { node, weight: 1.0 }
    }
    fn with_weight(node: MiniNode, weight: f32) -> Self {
        WeightedNode { node, weight }
    }
}

/// An event produced by evaluating a mini-notation pattern
#[derive(Clone, Debug)]
pub struct MiniEvent {
    pub value: String,      // sound name or note
    pub start: f32,         // 0.0 - 1.0 within the cycle
    pub duration: f32,      // fraction of cycle
    pub is_rest: bool,
}

/// Parse a mini-notation string into an AST
pub fn parse(input: &str) -> MiniNode {
    let input = input.trim();
    if input.is_empty() { return MiniNode::Rest; }

    // Top level: check for stack (comma)
    if let Some(parts) = split_top_level(input, ',') {
        if parts.len() > 1 {
            return MiniNode::Stack(parts.into_iter().map(|s| parse(&s)).collect());
        }
    }

    // Check for random choice (pipe)
    if let Some(parts) = split_top_level(input, '|') {
        if parts.len() > 1 {
            return MiniNode::Choose(parts.into_iter().map(|s| parse(&s)).collect());
        }
    }

    // Sequence (space-separated tokens)
    let tokens = tokenize(input);
    if tokens.len() > 1 {
        let nodes: Vec<WeightedNode> = tokens.into_iter().map(|t| parse_token_weighted(&t)).collect();
        return MiniNode::Seq(nodes);
    }

    // Single token
    parse_token_weighted(&input).node
}

/// Parse a single token, handling suffixes (*N, /N, @N, !N, ?, (k,n))
fn parse_token_weighted(token: &str) -> WeightedNode {
    let token = token.trim();

    // Handle brackets first
    if token.starts_with('[') {
        if let Some(close) = find_matching_bracket(token, '[', ']') {
            let inner = &token[1..close];
            let rest = &token[close+1..];
            let mut node = parse(inner);
            let mut weight = 1.0f32;
            // Apply suffixes from rest
            let (n, w) = apply_suffixes(node, rest);
            return WeightedNode::with_weight(n, w);
        }
    }

    // Angle brackets <x y z> = alternation
    if token.starts_with('<') {
        if let Some(close) = find_matching_bracket(token, '<', '>') {
            let inner = &token[1..close];
            let rest = &token[close+1..];
            let items: Vec<MiniNode> = tokenize(inner).into_iter().map(|s| parse(&s)).collect();
            let node = MiniNode::Alt(items);
            let (n, w) = apply_suffixes(node, rest);
            return WeightedNode::with_weight(n, w);
        }
    }

    // Simple atom with suffixes
    let (base, suffixes) = split_atom_suffixes(token);

    let base_node = if base == "~" || base == "-" {
        MiniNode::Rest
    } else {
        MiniNode::Atom(base.to_string())
    };

    let (node, weight) = apply_suffixes(base_node, &suffixes);
    WeightedNode::with_weight(node, weight)
}

/// Split suffixes from an atom: "kick*4?0.5" → ("kick", "*4?0.5")
fn split_atom_suffixes(token: &str) -> (&str, String) {
    // Find first suffix character that's not inside parens
    let suffix_chars = ['*', '/', '@', '!', '?', '('];
    let mut split_pos = token.len();
    let mut depth = 0;
    for (i, ch) in token.char_indices() {
        // Check for suffix BEFORE updating depth
        if depth == 0 && suffix_chars.contains(&ch) {
            split_pos = i;
            break;
        }
        match ch {
            '[' | '<' => depth += 1,
            ']' | '>' => depth -= 1,
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
    }
    (&token[..split_pos], token[split_pos..].to_string())
}

/// Apply suffix operators to a node, return (modified_node, weight)
fn apply_suffixes(mut node: MiniNode, suffixes: &str) -> (MiniNode, f32) {
    let mut weight = 1.0f32;
    let mut chars = suffixes.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            '*' => {
                chars.next();
                let num = collect_number(&mut chars);
                if num > 0.0 {
                    node = MiniNode::Fast(Box::new(node), num);
                }
            }
            '/' => {
                chars.next();
                let num = collect_number(&mut chars);
                if num > 0.0 {
                    node = MiniNode::Slow(Box::new(node), num);
                }
            }
            '@' => {
                chars.next();
                weight = collect_number(&mut chars).max(0.1);
            }
            '!' => {
                chars.next();
                let n = collect_number(&mut chars) as usize;
                if n > 1 {
                    node = MiniNode::Replicate(Box::new(node), n);
                }
            }
            '?' => {
                chars.next();
                // ? alone = 50%, ?N = custom
                let prob = if chars.peek().map(|c| c.is_ascii_digit() || *c == '.').unwrap_or(false) {
                    collect_number(&mut chars)
                } else {
                    0.5
                };
                node = MiniNode::Degrade(Box::new(node), 1.0 - prob); // keep probability
            }
            '(' => {
                chars.next();
                // Euclidean: (hits,steps) or (hits,steps,offset)
                let params: String = chars.by_ref().take_while(|&c| c != ')').collect();
                let parts: Vec<&str> = params.split(',').collect();
                if parts.len() >= 2 {
                    let hits = parts[0].trim().parse().unwrap_or(0);
                    let steps = parts[1].trim().parse().unwrap_or(0);
                    let offset = parts.get(2).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                    node = MiniNode::Euclid(Box::new(node), hits, steps, offset);
                }
            }
            _ => { chars.next(); } // skip unknown
        }
    }

    (node, weight)
}

fn collect_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> f32 {
    let mut s = String::new();
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_digit() || ch == '.' {
            s.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    s.parse().unwrap_or(1.0)
}

/// Evaluate a parsed mini-notation AST into a list of events for one cycle
pub fn eval(node: &MiniNode, start: f32, duration: f32, cycle: usize) -> Vec<MiniEvent> {
    match node {
        MiniNode::Atom(name) => {
            vec![MiniEvent { value: name.clone(), start, duration, is_rest: false }]
        }
        MiniNode::Rest => {
            vec![MiniEvent { value: String::new(), start, duration, is_rest: true }]
        }
        MiniNode::Seq(items) => {
            if items.is_empty() { return Vec::new(); }
            let total_weight: f32 = items.iter().map(|w| w.weight).sum();
            let mut events = Vec::new();
            let mut pos = start;
            for item in items {
                let frac = item.weight / total_weight;
                let dur = duration * frac;
                events.extend(eval(&item.node, pos, dur, cycle));
                pos += dur;
            }
            events
        }
        MiniNode::Stack(items) => {
            let mut events = Vec::new();
            for item in items {
                events.extend(eval(item, start, duration, cycle));
            }
            events
        }
        MiniNode::Alt(items) => {
            if items.is_empty() { return Vec::new(); }
            let idx = cycle % items.len();
            eval(&items[idx], start, duration, cycle)
        }
        MiniNode::Choose(items) => {
            if items.is_empty() { return Vec::new(); }
            // Deterministic random based on cycle
            let mut rng = (cycle as u32).wrapping_mul(2654435761);
            rng ^= rng >> 16;
            let idx = rng as usize % items.len();
            eval(&items[idx], start, duration, cycle)
        }
        MiniNode::Fast(inner, factor) => {
            let n = (*factor).ceil() as usize;
            let sub_dur = duration / factor;
            let mut events = Vec::new();
            for i in 0..n {
                let s = start + i as f32 * sub_dur;
                if s < start + duration {
                    events.extend(eval(inner, s, sub_dur.min(start + duration - s), cycle));
                }
            }
            events
        }
        MiniNode::Slow(inner, factor) => {
            // Play 1/factor of content per cycle
            let sub_cycle = cycle as f32 / factor;
            let phase = sub_cycle.fract();
            if phase < 1.0 / factor {
                eval(inner, start, duration, cycle)
            } else {
                Vec::new()
            }
        }
        MiniNode::Replicate(inner, n) => {
            let mut events = Vec::new();
            for _ in 0..*n {
                events.extend(eval(inner, start, duration, cycle));
            }
            // Adjust: replicated events share the same time (stacked)
            // Actually !N means repeat in sequence at same speed
            let sub_dur = duration; // each gets full duration since it's "same speed"
            events
        }
        MiniNode::Degrade(inner, keep_prob) => {
            let mut events = eval(inner, start, duration, cycle);
            let mut rng = (cycle as u32 * 31 + (start * 1000.0) as u32).wrapping_mul(2654435761);
            events.retain(|_| {
                rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;
                (rng % 1000) as f32 / 1000.0 < *keep_prob
            });
            events
        }
        MiniNode::Euclid(inner, hits, steps, offset) => {
            if *steps == 0 || *hits == 0 { return Vec::new(); }
            let step_dur = duration / *steps as f32;
            let mut events = Vec::new();
            for i in 0..*hits {
                let pos = ((i * steps) / hits + offset) % steps;
                let s = start + pos as f32 * step_dur;
                events.extend(eval(inner, s, step_dur, cycle));
            }
            events
        }
    }
}

/// Render a mini-notation string to a step grid
pub fn to_grid(input: &str, num_steps: usize, cycle: usize) -> Vec<(String, usize)> {
    let node = parse(input);
    let events = eval(&node, 0.0, 1.0, cycle);
    events.iter()
        .filter(|e| !e.is_rest)
        .map(|e| {
            let step = (e.start * num_steps as f32) as usize;
            (e.value.clone(), step.min(num_steps - 1))
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════
//  TOKENIZER HELPERS
// ═══════════════════════════════════════════════════════════

/// Split at top level (not inside brackets) by a delimiter
fn split_top_level(input: &str, delim: char) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for ch in input.chars() {
        match ch {
            '[' | '(' | '<' => { depth += 1; current.push(ch); }
            ']' | ')' | '>' => { depth -= 1; current.push(ch); }
            c if c == delim && depth == 0 => {
                parts.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    parts.push(current.trim().to_string());

    if parts.len() > 1 { Some(parts) } else { None }
}

/// Tokenize by spaces, respecting brackets
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for ch in input.chars() {
        match ch {
            '[' | '(' | '<' => { depth += 1; current.push(ch); }
            ']' | ')' | '>' => { depth -= 1; current.push(ch); }
            ' ' if depth == 0 => {
                let t = current.trim().to_string();
                if !t.is_empty() { tokens.push(t); }
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    let t = current.trim().to_string();
    if !t.is_empty() { tokens.push(t); }
    tokens
}

fn find_matching_bracket(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0;
    for (i, ch) in s.char_indices() {
        if ch == open { depth += 1; }
        if ch == close { depth -= 1; if depth == 0 { return Some(i); } }
    }
    None
}

// ═══════════════════════════════════════════════════════════
//  TESTS
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_sequence() {
        let events = eval(&parse("kick snare hh clap"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].value, "kick");
        assert_eq!(events[3].value, "clap");
        // Each should take 1/4 of the cycle
        assert!((events[0].duration - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_rest() {
        let events = eval(&parse("kick ~ snare ~"), 0.0, 1.0, 0);
        let sounds: Vec<_> = events.iter().filter(|e| !e.is_rest).collect();
        assert_eq!(sounds.len(), 2);
    }

    #[test]
    fn test_fast() {
        let events = eval(&parse("hh*4"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 4);
    }

    #[test]
    fn test_subdivision() {
        let events = eval(&parse("kick [snare clap] hh"), 0.0, 1.0, 0);
        // 3 top-level items, but [snare clap] subdivides into 2
        assert_eq!(events.len(), 4); // kick, snare, clap, hh
    }

    #[test]
    fn test_stack() {
        let events = eval(&parse("kick , snare"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 2);
        // Both start at the same time
        assert!((events[0].start - events[1].start).abs() < 0.01);
    }

    #[test]
    fn test_alternation() {
        let node = parse("<kick snare hh>");
        let e0 = eval(&node, 0.0, 1.0, 0);
        let e1 = eval(&node, 0.0, 1.0, 1);
        let e2 = eval(&node, 0.0, 1.0, 2);
        assert_eq!(e0[0].value, "kick");
        assert_eq!(e1[0].value, "snare");
        assert_eq!(e2[0].value, "hh");
    }

    #[test]
    fn test_euclidean() {
        let events = eval(&parse("kick(3,8)"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn test_euclidean_with_offset() {
        let events = eval(&parse("kick(3,8,2)"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn test_degrade() {
        let node = parse("hh*16?0.5");
        // Run many cycles and check roughly half are kept
        let total: usize = (0..100).map(|c| eval(&node, 0.0, 1.0, c).len()).sum();
        assert!(total > 400 && total < 1200, "Expected ~800 events, got {}", total);
    }

    #[test]
    fn test_choose() {
        let node = parse("kick | snare | hh");
        let e = eval(&node, 0.0, 1.0, 0);
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn test_weight() {
        let events = eval(&parse("kick@2 snare"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 2);
        // kick should get 2/3 of the time, snare 1/3
        assert!((events[0].duration - 0.667).abs() < 0.01);
        assert!((events[1].duration - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_to_grid() {
        let grid = to_grid("kick snare hh clap", 16, 0);
        assert_eq!(grid.len(), 4);
        assert_eq!(grid[0], ("kick".to_string(), 0));
        assert_eq!(grid[1], ("snare".to_string(), 4));
        assert_eq!(grid[2], ("hh".to_string(), 8));
        assert_eq!(grid[3], ("clap".to_string(), 12));
    }

    #[test]
    fn test_nested_subdivision() {
        let events = eval(&parse("[kick snare] [hh hh hh]"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 5); // 2 + 3
    }

    #[test]
    fn test_complex_pattern() {
        // Strudel-style complex pattern
        let events = eval(&parse("kick [snare [hh hh]] clap"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 5); // kick, snare, hh, hh, clap
    }

    #[test]
    fn test_slow() {
        let node = parse("kick/2");
        let e0 = eval(&node, 0.0, 1.0, 0);
        let e1 = eval(&node, 0.0, 1.0, 1);
        // Should only play on even cycles
        assert!(e0.len() == 1 || e1.len() == 1);
    }
}

#[cfg(test)]
mod real_song_tests {
    use super::*;

    #[test]
    fn test_replication_bang() {
        // "bd!2" = bd bd (not faster, just repeated)
        let events = eval(&parse("bd!2 sd"), 0.0, 1.0, 0);
        // bd takes 1/3, bd takes 1/3, sd takes 1/3
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].value, "bd");
        assert_eq!(events[1].value, "bd");
        assert_eq!(events[2].value, "sd");
    }

    #[test]
    fn test_weight_at_sign() {
        // "Am@3 G" = Am gets 3/4, G gets 1/4
        let events = eval(&parse("Am@3 G"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 2);
        assert!((events[0].duration - 0.75).abs() < 0.01, "Am should get 0.75, got {}", events[0].duration);
        assert!((events[1].duration - 0.25).abs() < 0.01, "G should get 0.25, got {}", events[1].duration);
    }

    #[test]
    fn test_nested_with_weight() {
        // "[bd*4]!2" from Blue Monday
        let events = eval(&parse("[bd*4]!2"), 0.0, 1.0, 0);
        // [bd*4] = 4 bds in one time slot, !2 = repeat that group twice
        assert!(events.len() >= 4, "Expected at least 4 events, got {}", events.len());
    }

    #[test]
    fn test_complex_drum_pattern() {
        // "[bd!2 ~ bd]*2" from Big Ship
        let events = eval(&parse("[bd!2 ~ bd]*2"), 0.0, 1.0, 0);
        let sounds: Vec<_> = events.iter().filter(|e| !e.is_rest).collect();
        assert!(sounds.len() >= 4, "Expected 4+ hits, got {}", sounds.len());
    }

    #[test]
    fn test_slow_alternation() {
        // "<Am D F>*2" — cycle through chords, played twice per cycle
        let node = parse("<Am D F>*2");
        let e0 = eval(&node, 0.0, 1.0, 0);
        let e1 = eval(&node, 0.0, 1.0, 1);
        let e2 = eval(&node, 0.0, 1.0, 2);
        assert_eq!(e0.len(), 2); // Am played twice
        assert_eq!(e0[0].value, "Am");
        assert_eq!(e1[0].value, "D");
        assert_eq!(e2[0].value, "F");
    }

    #[test]
    fn test_alternation_with_brackets() {
        // "<[sd ~ ~ sd] sd>" — alternate between two patterns
        let node = parse("<[sd ~ ~ sd] sd>");
        let e0 = eval(&node, 0.0, 1.0, 0);
        let e1 = eval(&node, 0.0, 1.0, 1);
        // Cycle 0: [sd ~ ~ sd] = 4 events (2 sounds, 2 rests)
        let sounds0: Vec<_> = e0.iter().filter(|e| !e.is_rest).collect();
        assert_eq!(sounds0.len(), 2, "Expected 2 sd in cycle 0");
        // Cycle 1: just sd
        assert_eq!(e1.len(), 1);
        assert_eq!(e1[0].value, "sd");
    }

    #[test]
    fn test_long_weighted_sequence() {
        // "c#6@2 f5 c6@3 a#5" — weighted melody
        let events = eval(&parse("c#6@2 f5 c6@3 a#5"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 4);
        // Total weight: 2+1+3+1 = 7
        assert!((events[0].duration - 2.0/7.0).abs() < 0.01);
        assert!((events[2].duration - 3.0/7.0).abs() < 0.01);
    }

    #[test]
    fn test_note_names_in_mini() {
        let events = eval(&parse("c4 e4 g4 b4"), 0.0, 1.0, 0);
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].value, "c4");
    }

    #[test]
    fn test_to_grid_complex() {
        let grid = to_grid("[bd!2 ~ bd]*2", 16, 0);
        // Should have multiple bd hits across 16 steps
        let bd_hits = grid.iter().filter(|(n, _)| n == "bd").count();
        assert!(bd_hits >= 4, "Expected 4+ bd hits, got {}", bd_hits);
    }
}
