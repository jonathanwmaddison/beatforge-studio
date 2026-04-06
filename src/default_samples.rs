//! Embedded default sample pack — loaded on startup for instant beat-making.
//! Samples are compiled into the binary via include_bytes!

pub struct DefaultSample {
    pub name: &'static str,
    pub data: &'static [u8], // raw WAV bytes
    pub pad_index: usize,
}

pub fn kit_names() -> Vec<&'static str> {
    vec!["808 KIT", "MODERN KIT"]
}

pub fn default_samples() -> Vec<DefaultSample> {
    vec![
        DefaultSample {
            name: "KICK",
            data: include_bytes!("../samples/kick.wav"),
            pad_index: 0,
        },
        DefaultSample {
            name: "SNARE",
            data: include_bytes!("../samples/snare.wav"),
            pad_index: 1,
        },
        DefaultSample {
            name: "HH-C",
            data: include_bytes!("../samples/hihat_closed.wav"),
            pad_index: 2,
        },
        DefaultSample {
            name: "HH-O",
            data: include_bytes!("../samples/hihat_open.wav"),
            pad_index: 3,
        },
        DefaultSample {
            name: "CLAP",
            data: include_bytes!("../samples/clap.wav"),
            pad_index: 4,
        },
        DefaultSample {
            name: "RIM",
            data: include_bytes!("../samples/rim.wav"),
            pad_index: 5,
        },
        DefaultSample {
            name: "PERC",
            data: include_bytes!("../samples/perc.wav"),
            pad_index: 8,
        },
        DefaultSample {
            name: "COWB",
            data: include_bytes!("../samples/cowbell.wav"),
            pad_index: 9,
        },
    ]
}

pub fn modern_kit() -> Vec<DefaultSample> {
    vec![
        DefaultSample { name: "KICK", data: include_bytes!("../samples/kit2/kick.wav"), pad_index: 0 },
        DefaultSample { name: "SNARE", data: include_bytes!("../samples/kit2/snare.wav"), pad_index: 1 },
        DefaultSample { name: "HH-C", data: include_bytes!("../samples/kit2/hihat_closed.wav"), pad_index: 2 },
        DefaultSample { name: "HH-O", data: include_bytes!("../samples/kit2/hihat_open.wav"), pad_index: 3 },
        DefaultSample { name: "CLAP", data: include_bytes!("../samples/kit2/clap.wav"), pad_index: 4 },
        DefaultSample { name: "PERC", data: include_bytes!("../samples/kit2/perc.wav"), pad_index: 8 },
    ]
}

pub fn chops_kit() -> Vec<DefaultSample> {
    vec![
        // Amen break slices (the most sampled break in music history)
        DefaultSample { name: "AMEN 1", data: include_bytes!("../samples/chops/amen_01.wav"), pad_index: 0 },
        DefaultSample { name: "AMEN 2", data: include_bytes!("../samples/chops/amen_02.wav"), pad_index: 1 },
        DefaultSample { name: "AMEN 3", data: include_bytes!("../samples/chops/amen_03.wav"), pad_index: 2 },
        DefaultSample { name: "AMEN 4", data: include_bytes!("../samples/chops/amen_04.wav"), pad_index: 3 },
        DefaultSample { name: "AMEN 5", data: include_bytes!("../samples/chops/amen_05.wav"), pad_index: 4 },
        DefaultSample { name: "AMEN 6", data: include_bytes!("../samples/chops/amen_06.wav"), pad_index: 5 },
        DefaultSample { name: "AMEN 7", data: include_bytes!("../samples/chops/amen_07.wav"), pad_index: 6 },
        DefaultSample { name: "AMEN 8", data: include_bytes!("../samples/chops/amen_08.wav"), pad_index: 7 },
        // Stabs and bass
        DefaultSample { name: "STAB 1", data: include_bytes!("../samples/chops/stab_01.wav"), pad_index: 8 },
        DefaultSample { name: "STAB 2", data: include_bytes!("../samples/chops/stab_02.wav"), pad_index: 9 },
        DefaultSample { name: "BASS 1", data: include_bytes!("../samples/chops/bass_01.wav"), pad_index: 10 },
        DefaultSample { name: "BASS 2", data: include_bytes!("../samples/chops/bass_02.wav"), pad_index: 11 },
        // Pad (for lo-fi chopping)
        DefaultSample { name: "PAD", data: include_bytes!("../samples/chops/pad_angel.wav"), pad_index: 12 },
    ]
}

pub fn synth_kit() -> Vec<DefaultSample> {
    vec![
        // Pre-rendered high-quality synth sounds (pitched to A4=440Hz)
        // Load onto pads 10-14, use PITCH control to play different notes
        DefaultSample { name: "SUPERSAW", data: include_bytes!("../samples/synth_supersaw.wav"), pad_index: 10 },
        DefaultSample { name: "PLUCK", data: include_bytes!("../samples/synth_pluck.wav"), pad_index: 11 },
        DefaultSample { name: "PAD", data: include_bytes!("../samples/synth_pad.wav"), pad_index: 12 },
        DefaultSample { name: "BASS", data: include_bytes!("../samples/synth_bass.wav"), pad_index: 13 },
        DefaultSample { name: "LEAD", data: include_bytes!("../samples/synth_lead.wav"), pad_index: 14 },
    ]
}

/// Decode a WAV from raw bytes into f32 samples + sample rate
pub fn decode_wav_bytes(data: &[u8]) -> Option<(Vec<f32>, u32)> {
    // Simple WAV parser for 16-bit PCM mono
    if data.len() < 44 { return None; }
    if &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" { return None; }

    // Find data chunk
    let mut pos = 12;
    while pos + 8 < data.len() {
        let chunk_id = &data[pos..pos+4];
        let chunk_size = u32::from_le_bytes([data[pos+4], data[pos+5], data[pos+6], data[pos+7]]) as usize;

        if chunk_id == b"fmt " {
            let _format = u16::from_le_bytes([data[pos+8], data[pos+9]]);
            let channels = u16::from_le_bytes([data[pos+10], data[pos+11]]);
            let sample_rate = u32::from_le_bytes([data[pos+12], data[pos+13], data[pos+14], data[pos+15]]);
            let bits = u16::from_le_bytes([data[pos+22], data[pos+23]]);

            if chunk_id == b"fmt " {
                // Skip to data chunk
                pos += 8 + chunk_size;
                // Find data
                while pos + 8 < data.len() {
                    if &data[pos..pos+4] == b"data" {
                        let data_size = u32::from_le_bytes([data[pos+4], data[pos+5], data[pos+6], data[pos+7]]) as usize;
                        let audio_data = &data[pos+8..pos+8+data_size.min(data.len()-pos-8)];

                        let samples: Vec<f32> = if bits == 16 {
                            audio_data.chunks(2)
                                .map(|chunk| {
                                    if chunk.len() == 2 {
                                        i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0
                                    } else { 0.0 }
                                })
                                .collect()
                        } else {
                            return None;
                        };

                        // Downmix to mono if stereo
                        let mono = if channels == 2 {
                            samples.chunks(2).map(|c| (c[0] + c.get(1).copied().unwrap_or(0.0)) * 0.5).collect()
                        } else {
                            samples
                        };

                        return Some((mono, sample_rate));
                    }
                    pos += 8 + u32::from_le_bytes([data[pos+4], data[pos+5], data[pos+6], data[pos+7]]) as usize;
                }
            }
        }
        pos += 8 + chunk_size;
    }
    None
}
