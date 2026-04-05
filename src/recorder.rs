//! WAV recording — capture audio output to a buffer and export to WAV file.

use std::path::Path;

/// Ring buffer that captures stereo audio output
pub struct AudioRecorder {
    buffer: Vec<f32>,     // interleaved stereo samples
    recording: bool,
    sample_rate: u32,
    max_seconds: f32,
    max_samples: usize,
}

impl AudioRecorder {
    pub fn new(sample_rate: u32, max_seconds: f32) -> Self {
        let max_samples = (sample_rate as f32 * max_seconds * 2.0) as usize; // stereo
        AudioRecorder {
            buffer: Vec::with_capacity(max_samples),
            recording: false,
            sample_rate,
            max_seconds,
            max_samples,
        }
    }

    pub fn start(&mut self) {
        self.buffer.clear();
        self.recording = true;
    }

    pub fn stop(&mut self) {
        self.recording = false;
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// Push a stereo sample pair into the buffer
    pub fn push(&mut self, left: f32, right: f32) {
        if !self.recording {
            return;
        }
        if self.buffer.len() < self.max_samples {
            self.buffer.push(left);
            self.buffer.push(right);
        } else {
            // Auto-stop when buffer is full
            self.recording = false;
        }
    }

    /// Get recording duration in seconds
    pub fn duration(&self) -> f32 {
        self.buffer.len() as f32 / (self.sample_rate as f32 * 2.0)
    }

    /// Export the recording to a WAV file
    pub fn export_wav(&self, path: &Path) -> Result<(), String> {
        if self.buffer.is_empty() {
            return Err("Nothing recorded".to_string());
        }

        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let mut writer = hound::WavWriter::create(path, spec)
            .map_err(|e| format!("Failed to create WAV file: {e}"))?;

        for &sample in &self.buffer {
            writer.write_sample(sample)
                .map_err(|e| format!("Failed to write sample: {e}"))?;
        }

        writer.finalize()
            .map_err(|e| format!("Failed to finalize WAV: {e}"))?;

        Ok(())
    }

    /// Get the recorded data as a mono mixdown (for loading back as a sample)
    pub fn to_mono(&self) -> Vec<f32> {
        self.buffer
            .chunks(2)
            .map(|pair| {
                let l = pair[0];
                let r = pair.get(1).copied().unwrap_or(l);
                (l + r) * 0.5
            })
            .collect()
    }

    /// Get buffer reference for waveform display
    pub fn buffer(&self) -> &[f32] {
        &self.buffer
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

/// Export a mono buffer as a WAV file (for stem export)
pub fn export_mono_wav(path: &std::path::Path, data: &[f32], sample_rate: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| format!("Failed to create WAV: {e}"))?;
    for &s in data {
        writer.write_sample(s).map_err(|e| format!("Write error: {e}"))?;
    }
    writer.finalize().map_err(|e| format!("Finalize error: {e}"))?;
    Ok(())
}
