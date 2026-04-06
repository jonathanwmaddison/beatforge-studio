//! Pattern generation algorithms — goes beyond FL Studio.
//! Generates complementary patterns based on existing ones using
//! musical rules and probability distributions.

/// Generate a complementary pattern for a given role based on an existing pattern.
/// Uses musical heuristics: snares typically fall on backbeats relative to kicks,
/// hats fill the gaps, percussion syncopates against the main groove.
pub fn generate_complement(
    existing: &[u8],       // existing pattern (e.g., kick)
    role: PatternRole,     // what kind of pattern to generate
    density: f32,          // 0.0-1.0 how dense the generated pattern should be
    seed: u32,             // random seed for variation
) -> Vec<u8> {
    let steps = existing.len();
    let mut result = vec![0u8; steps];
    let mut rng = seed;

    match role {
        PatternRole::Backbeat => {
            // Place hits on steps where the existing pattern is silent
            // Prefer beats 2 and 4 (steps 4, 12 in 16-step)
            for i in 0..steps {
                let is_backbeat = (i % 8) == 4;
                let existing_hit = existing[i] > 0;
                rng = xor_rng(rng);
                let roll = (rng % 100) as f32 / 100.0;

                if is_backbeat && !existing_hit {
                    result[i] = 3; // strong hit on backbeats
                } else if !existing_hit && roll < density * 0.3 {
                    result[i] = if roll < density * 0.1 { 2 } else { 1 }; // ghost notes
                }
            }
        }
        PatternRole::Offbeat => {
            // Hi-hat style: fill between existing hits
            for i in 0..steps {
                let existing_hit = existing[i] > 0;
                rng = xor_rng(rng);
                let roll = (rng % 100) as f32 / 100.0;

                if i % 2 == 0 {
                    // On-beat: stronger hits
                    if roll < density {
                        result[i] = if existing_hit { 2 } else { 3 };
                    }
                } else {
                    // Off-beat: lighter hits
                    if roll < density * 0.7 {
                        result[i] = if roll < density * 0.3 { 1 } else { 2 };
                    }
                }
            }
        }
        PatternRole::Syncopation => {
            // Percussion: hit where NEITHER kick nor standard backbeats land
            for i in 0..steps {
                let on_beat = i % 4 == 0;
                let existing_hit = existing[i] > 0;
                rng = xor_rng(rng);
                let roll = (rng % 100) as f32 / 100.0;

                if !on_beat && !existing_hit && roll < density {
                    result[i] = if roll < density * 0.4 { 2 } else { 1 };
                }
            }
        }
        PatternRole::Variation => {
            // Create a variation of the existing pattern
            // Keep ~70% of hits, shift some, add some new ones
            for i in 0..steps {
                rng = xor_rng(rng);
                let roll = (rng % 100) as f32 / 100.0;

                if existing[i] > 0 {
                    if roll < 0.7 {
                        result[i] = existing[i]; // keep
                    } else if roll < 0.85 && i + 1 < steps {
                        result[i + 1] = existing[i]; // shift right
                    }
                } else if roll < density * 0.15 {
                    result[i] = 2; // add new ghost note
                }
            }
        }
    }

    result
}

#[derive(Clone, Copy, PartialEq)]
pub enum PatternRole {
    Backbeat,     // Snare-like: hits on 2 and 4
    Offbeat,      // Hi-hat-like: fills between beats
    Syncopation,  // Perc-like: off-grid accents
    Variation,    // Copy with modifications
}

impl PatternRole {
    pub fn name(&self) -> &'static str {
        match self {
            PatternRole::Backbeat => "SNARE",
            PatternRole::Offbeat => "HI-HAT",
            PatternRole::Syncopation => "PERC",
            PatternRole::Variation => "VARIATION",
        }
    }
}

fn xor_rng(state: u32) -> u32 {
    let mut s = state;
    s ^= s << 13;
    s ^= s >> 17;
    s ^= s << 5;
    s
}

/// Generate a full drum kit pattern from a single kick pattern
pub fn generate_full_kit(kick: &[u8], density: f32, seed: u32) -> Vec<Vec<u8>> {
    vec![
        kick.to_vec(),
        generate_complement(kick, PatternRole::Backbeat, density, seed),
        generate_complement(kick, PatternRole::Offbeat, density, seed.wrapping_mul(2654435761)),
        generate_complement(kick, PatternRole::Offbeat, density * 0.3, seed.wrapping_mul(1013904223)),
        generate_complement(kick, PatternRole::Backbeat, density * 0.5, seed.wrapping_mul(1664525)),
        generate_complement(kick, PatternRole::Syncopation, density * 0.4, seed.wrapping_mul(1836311903)),
    ]
}
