//! Music theory engine — scales, chords, voicings, progressions.
//! Provides the .scale(), .chord(), and .voicing() capabilities
//! that make Strudel songs musical.

/// Note names to semitone offset from C
pub fn note_to_semitone(name: &str) -> Option<i32> {
    let name = name.trim();
    if name.is_empty() { return None; }
    let chars: Vec<char> = name.chars().collect();

    let base = match chars[0].to_uppercase().next()? {
        'C' => 0, 'D' => 2, 'E' => 4, 'F' => 5,
        'G' => 7, 'A' => 9, 'B' => 11, _ => return None,
    };

    let (accidental, rest_start) = if chars.len() > 1 {
        match chars[1] {
            '#' => (1, 2),
            'b' => (-1, 2),
            _ => (0, 1),
        }
    } else {
        (0, 1)
    };

    // Parse octave if present
    let octave: i32 = if rest_start < chars.len() {
        chars[rest_start..].iter().collect::<String>().parse().unwrap_or(4)
    } else {
        4 // default octave
    };

    Some((octave + 1) * 12 + base + accidental)
}

/// MIDI note to note name
pub fn midi_to_name(midi: i32) -> String {
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = midi / 12 - 1;
    let note = (midi % 12) as usize;
    format!("{}{}", names[note], octave)
}

// ═══════════════════════════════════════════════════════════
//  SCALES
// ═══════════════════════════════════════════════════════════

/// Scale definition: name → interval pattern (semitones from root)
pub fn get_scale(name: &str) -> Option<Vec<i32>> {
    let intervals = match name.to_lowercase().as_str() {
        "major" | "ionian" => vec![0, 2, 4, 5, 7, 9, 11],
        "minor" | "aeolian" | "natural_minor" => vec![0, 2, 3, 5, 7, 8, 10],
        "dorian" => vec![0, 2, 3, 5, 7, 9, 10],
        "phrygian" => vec![0, 1, 3, 5, 7, 8, 10],
        "lydian" => vec![0, 2, 4, 6, 7, 9, 11],
        "mixolydian" => vec![0, 2, 4, 5, 7, 9, 10],
        "locrian" => vec![0, 1, 3, 5, 6, 8, 10],
        "harmonic_minor" => vec![0, 2, 3, 5, 7, 8, 11],
        "melodic_minor" => vec![0, 2, 3, 5, 7, 9, 11],
        "pentatonic" | "major_pentatonic" => vec![0, 2, 4, 7, 9],
        "minor_pentatonic" => vec![0, 3, 5, 7, 10],
        "blues" => vec![0, 3, 5, 6, 7, 10],
        "whole_tone" => vec![0, 2, 4, 6, 8, 10],
        "chromatic" => vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "diminished" => vec![0, 2, 3, 5, 6, 8, 9, 11],
        _ => return None,
    };
    Some(intervals)
}

/// Map a scale degree (0-based) to a MIDI note in a given scale and root
pub fn scale_degree_to_midi(degree: i32, root: i32, scale: &[i32]) -> i32 {
    if scale.is_empty() { return root + degree; }
    let len = scale.len() as i32;
    let octave_offset = if degree >= 0 { degree / len } else { (degree - len + 1) / len };
    let idx = ((degree % len) + len) % len;
    root + scale[idx as usize] + octave_offset * 12
}

// ═══════════════════════════════════════════════════════════
//  CHORDS
// ═══════════════════════════════════════════════════════════

/// Parse a chord symbol into intervals from root
pub fn parse_chord(symbol: &str) -> Option<(i32, Vec<i32>)> {
    let symbol = symbol.trim();
    if symbol.is_empty() { return None; }

    // Find where the root note ends and quality begins
    let chars: Vec<char> = symbol.chars().collect();
    let mut i = 1; // skip first letter
    if i < chars.len() && (chars[i] == '#' || chars[i] == 'b') { i += 1; }

    // Only consume digit as octave if followed by nothing or a quality string
    // "G7" → G is root (no octave), "7" is quality (dominant 7th)
    // "C4" → C4 is root (C octave 4), "" is quality (major)
    // "C4m7" → C4 is root, "m7" is quality
    // Heuristic: if the digit is followed by more chars, it's an octave; if alone or
    // followed by a quality suffix, check if it's a known chord quality
    let has_octave = if i < chars.len() && chars[i].is_ascii_digit() {
        let rest_after_digit = &symbol[i+1..];
        // "7" alone after a note letter = chord quality, not octave
        // Only treat as octave if followed by nothing or a quality (m, maj, dim, etc.)
        // If the digit+rest forms a known quality, it's NOT an octave
        let digit_and_rest = &symbol[i..];
        let known_qualities = ["7", "9", "11", "13", "6", "5"];
        !known_qualities.contains(&digit_and_rest)
    } else {
        false
    };
    if has_octave { i += 1; }

    let root_str = &symbol[..i];
    let quality = if i < symbol.len() { &symbol[i..] } else { "" };

    let root = note_to_semitone(root_str)?;

    let intervals = match quality.to_lowercase().as_str() {
        "" | "maj" | "major" => vec![0, 4, 7],
        "m" | "min" | "minor" => vec![0, 3, 7],
        "7" | "dom7" => vec![0, 4, 7, 10],
        "maj7" | "M7" => vec![0, 4, 7, 11],
        "m7" | "min7" => vec![0, 3, 7, 10],
        "dim" | "o" => vec![0, 3, 6],
        "dim7" | "o7" => vec![0, 3, 6, 9],
        "aug" | "+" => vec![0, 4, 8],
        "sus2" => vec![0, 2, 7],
        "sus4" | "sus" => vec![0, 5, 7],
        "6" => vec![0, 4, 7, 9],
        "m6" => vec![0, 3, 7, 9],
        "9" => vec![0, 4, 7, 10, 14],
        "m9" => vec![0, 3, 7, 10, 14],
        "add9" => vec![0, 4, 7, 14],
        "7sus4" => vec![0, 5, 7, 10],
        "11" => vec![0, 4, 7, 10, 14, 17],
        "m11" => vec![0, 3, 7, 10, 14, 17],
        "13" => vec![0, 4, 7, 10, 14, 17, 21],
        "5" | "power" => vec![0, 7],
        _ => vec![0, 4, 7], // default to major
    };

    Some((root, intervals))
}

/// Generate MIDI notes for a chord
pub fn chord_notes(symbol: &str) -> Vec<i32> {
    if let Some((root, intervals)) = parse_chord(symbol) {
        intervals.iter().map(|&i| root + i).collect()
    } else {
        Vec::new()
    }
}

/// Generate a voicing (spread notes across a range, with inversions)
pub fn voice_chord(symbol: &str, anchor: i32, spread: i32) -> Vec<i32> {
    let mut notes = chord_notes(symbol);
    if notes.is_empty() { return notes; }

    // Move notes close to the anchor
    for note in &mut notes {
        while *note < anchor - spread { *note += 12; }
        while *note > anchor + spread { *note -= 12; }
    }
    notes.sort();
    notes.dedup();
    notes
}

// ═══════════════════════════════════════════════════════════
//  CHORD PROGRESSIONS
// ═══════════════════════════════════════════════════════════

/// Parse a chord progression string: "Am Dm G C" or "Am7 Dm7 G7 Cmaj7"
pub fn parse_progression(prog: &str) -> Vec<Vec<i32>> {
    prog.split_whitespace()
        .map(|s| voice_chord(s, 60, 12)) // anchor around middle C
        .collect()
}

/// Roman numeral to chord in a key
pub fn roman_to_chord(numeral: &str, key_root: i32, scale: &[i32]) -> Vec<i32> {
    let (degree, quality) = match numeral.to_lowercase().trim() {
        "i" => (0, ""),
        "ii" => (1, "m"),
        "iii" => (2, "m"),
        "iv" => (3, ""),
        "v" => (4, ""),
        "vi" => (5, "m"),
        "vii" => (6, "dim"),
        // Minor key
        "i_m" => (0, "m"),
        "iv_m" => (3, "m"),
        "v_m" => (4, ""), // dominant is usually major even in minor
        _ => return Vec::new(),
    };

    let root = scale_degree_to_midi(degree, key_root, scale);
    let chord_sym = format!("{}{}", midi_to_name(root), quality);
    voice_chord(&chord_sym, 60, 12)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_to_semitone() {
        assert_eq!(note_to_semitone("C4"), Some(60));
        assert_eq!(note_to_semitone("A4"), Some(69));
        assert_eq!(note_to_semitone("C#4"), Some(61));
        assert_eq!(note_to_semitone("Bb3"), Some(58));
    }

    #[test]
    fn test_midi_to_name() {
        assert_eq!(midi_to_name(60), "C4");
        assert_eq!(midi_to_name(69), "A4");
    }

    #[test]
    fn test_scale() {
        let major = get_scale("major").unwrap();
        assert_eq!(major, vec![0, 2, 4, 5, 7, 9, 11]);

        let minor = get_scale("minor").unwrap();
        assert_eq!(minor, vec![0, 2, 3, 5, 7, 8, 10]);
    }

    #[test]
    fn test_scale_degree() {
        let major = get_scale("major").unwrap();
        // C major: degree 0 = C4(60), degree 2 = E4(64), degree 4 = G4(67)
        assert_eq!(scale_degree_to_midi(0, 60, &major), 60); // C
        assert_eq!(scale_degree_to_midi(2, 60, &major), 64); // E
        assert_eq!(scale_degree_to_midi(4, 60, &major), 67); // G
        // Octave up
        assert_eq!(scale_degree_to_midi(7, 60, &major), 72); // C5
    }

    #[test]
    fn test_parse_chord() {
        let (root, intervals) = parse_chord("C4").unwrap();
        assert_eq!(root, 60);
        assert_eq!(intervals, vec![0, 4, 7]); // major triad

        let (root, intervals) = parse_chord("Am").unwrap();
        assert_eq!(intervals, vec![0, 3, 7]); // minor triad

        let (_, intervals) = parse_chord("G7").unwrap();
        assert_eq!(intervals, vec![0, 4, 7, 10]); // dominant 7th
    }

    #[test]
    fn test_chord_notes() {
        let notes = chord_notes("C4");
        assert_eq!(notes, vec![60, 64, 67]); // C E G

        let notes = chord_notes("Am");
        assert!(notes.contains(&(69 + 0))); // A
        assert!(notes.contains(&(69 + 3))); // C
        assert!(notes.contains(&(69 + 7))); // E
    }

    #[test]
    fn test_voice_chord() {
        let notes = voice_chord("C4", 60, 12);
        assert!(notes.len() >= 3);
        // All notes should be within 12 semitones of anchor
        for &n in &notes {
            assert!((n - 60).abs() <= 12, "Note {} too far from anchor 60", n);
        }
    }

    #[test]
    fn test_progression() {
        let prog = parse_progression("Am Dm G C");
        assert_eq!(prog.len(), 4);
        for chord in &prog {
            assert!(chord.len() >= 3, "Chord should have 3+ notes");
        }
    }

    #[test]
    fn test_chord_qualities() {
        // Test various chord types parse correctly
        let types = ["C", "Cm", "C7", "Cmaj7", "Cm7", "Cdim", "Caug",
                     "Csus2", "Csus4", "C6", "C9", "C5"];
        for t in types {
            let notes = chord_notes(t);
            assert!(!notes.is_empty(), "Failed to parse chord: {}", t);
        }
    }
}
