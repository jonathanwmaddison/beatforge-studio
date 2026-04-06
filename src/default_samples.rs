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
