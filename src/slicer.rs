//! Sample slicer — auto-detect transients and chop samples into slices
//! that can be mapped to pads, similar to FL Studio's Slicex.

/// A slice of audio from a larger sample
#[derive(Clone)]
pub struct Slice {
    pub start: usize,   // start sample index in original buffer
    pub end: usize,     // end sample index
    pub name: String,   // auto-generated name
}

/// Detect transients in audio data and return slice points.
///
/// Uses a simple onset detection algorithm:
/// 1. Compute short-term energy in windows
/// 2. Compare to running average
/// 3. Mark peaks above threshold as transients
pub fn detect_slices(data: &[f32], sample_rate: u32, sensitivity: f32) -> Vec<Slice> {
    if data.is_empty() {
        return Vec::new();
    }

    let window_size = (sample_rate as usize / 100).max(64); // ~10ms windows
    let hop = window_size / 2;
    let threshold = 0.05 + (1.0 - sensitivity) * 0.4; // sensitivity 0-1

    // Compute energy per window
    let mut energies: Vec<f32> = Vec::new();
    let mut pos = 0;
    while pos + window_size <= data.len() {
        let energy: f32 = data[pos..pos + window_size]
            .iter()
            .map(|s| s * s)
            .sum::<f32>()
            / window_size as f32;
        energies.push(energy.sqrt());
        pos += hop;
    }

    if energies.is_empty() {
        return vec![Slice {
            start: 0,
            end: data.len(),
            name: "SLICE 1".to_string(),
        }];
    }

    // Detect onsets: energy significantly above local average
    let avg_window = 8;
    let mut onsets: Vec<usize> = Vec::new();
    onsets.push(0); // Always include start

    for i in avg_window..energies.len() {
        let local_avg: f32 = energies[i.saturating_sub(avg_window)..i]
            .iter()
            .sum::<f32>()
            / avg_window as f32;

        if energies[i] > local_avg + threshold && energies[i] > 0.01 {
            let sample_pos = i * hop;
            // Minimum distance between slices (50ms)
            let min_dist = sample_rate as usize / 20;
            if let Some(&last) = onsets.last() {
                if sample_pos - last >= min_dist {
                    onsets.push(sample_pos);
                }
            }
        }
    }

    // Convert onset points to slices
    let mut slices = Vec::new();
    for (i, &start) in onsets.iter().enumerate() {
        let end = onsets.get(i + 1).copied().unwrap_or(data.len());
        slices.push(Slice {
            start,
            end,
            name: format!("SLICE {}", i + 1),
        });
    }

    // Limit to 16 slices (one per pad)
    if slices.len() > 16 {
        // Merge smallest slices
        while slices.len() > 16 {
            // Find shortest slice
            let min_idx = slices.iter().enumerate()
                .min_by_key(|(_, s)| s.end - s.start)
                .map(|(i, _)| i)
                .unwrap_or(0);

            if min_idx + 1 < slices.len() {
                slices[min_idx].end = slices[min_idx + 1].end;
                slices.remove(min_idx + 1);
            } else if min_idx > 0 {
                slices[min_idx - 1].end = slices[min_idx].end;
                slices.remove(min_idx);
            } else {
                break;
            }
        }
        // Re-name
        for (i, slice) in slices.iter_mut().enumerate() {
            slice.name = format!("SLICE {}", i + 1);
        }
    }

    slices
}

/// Extract a slice from the original data as a new Vec
pub fn extract_slice(data: &[f32], slice: &Slice) -> Vec<f32> {
    let start = slice.start.min(data.len());
    let end = slice.end.min(data.len());
    data[start..end].to_vec()
}

/// Divide a sample evenly into N slices
pub fn even_slices(data_len: usize, num_slices: usize) -> Vec<Slice> {
    let slice_len = data_len / num_slices.max(1);
    (0..num_slices)
        .map(|i| Slice {
            start: i * slice_len,
            end: if i == num_slices - 1 { data_len } else { (i + 1) * slice_len },
            name: format!("SLICE {}", i + 1),
        })
        .collect()
}

/// Normalize audio data to peak amplitude
pub fn normalize(data: &mut [f32]) {
    let peak = data.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
    if peak > 0.001 {
        let gain = 1.0 / peak;
        for s in data.iter_mut() {
            *s *= gain;
        }
    }
}

/// Apply fade-in to audio data
pub fn fade_in(data: &mut [f32], samples: usize) {
    let len = samples.min(data.len());
    for i in 0..len {
        data[i] *= i as f32 / len as f32;
    }
}

/// Apply fade-out to audio data
pub fn fade_out(data: &mut [f32], samples: usize) {
    let len = samples.min(data.len());
    let start = data.len() - len;
    for i in 0..len {
        data[start + i] *= 1.0 - (i as f32 / len as f32);
    }
}

/// Reverse audio data in place
pub fn reverse(data: &mut [f32]) {
    data.reverse();
}

/// Detect BPM of a sample by finding average onset interval.
/// Returns estimated BPM or None if detection fails.
pub fn detect_bpm(data: &[f32], sample_rate: u32) -> Option<f32> {
    if data.len() < sample_rate as usize { return None; } // need at least 1 second

    let window = (sample_rate as usize / 50).max(64); // 20ms windows
    let hop = window / 2;

    // Compute onset strength (energy difference between windows)
    let mut onsets: Vec<f32> = Vec::new();
    let mut prev_energy = 0.0f32;
    let mut pos = 0;
    while pos + window <= data.len() {
        let energy: f32 = data[pos..pos + window].iter().map(|s| s * s).sum::<f32>() / window as f32;
        let onset = (energy - prev_energy).max(0.0);
        onsets.push(onset);
        prev_energy = energy;
        pos += hop;
    }

    if onsets.len() < 10 { return None; }

    // Find peaks in onset strength
    let threshold = onsets.iter().fold(0.0f32, |a, &b| a.max(b)) * 0.3;
    let mut peak_positions: Vec<usize> = Vec::new();
    for i in 1..onsets.len() - 1 {
        if onsets[i] > threshold && onsets[i] > onsets[i - 1] && onsets[i] > onsets[i + 1] {
            peak_positions.push(i);
        }
    }

    if peak_positions.len() < 4 { return None; }

    // Calculate average interval between peaks
    let intervals: Vec<f32> = peak_positions.windows(2)
        .map(|w| (w[1] - w[0]) as f32 * hop as f32 / sample_rate as f32)
        .collect();

    // Filter outliers (keep intervals within 2x of median)
    let mut sorted = intervals.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[sorted.len() / 2];
    let filtered: Vec<f32> = intervals.iter()
        .filter(|&&i| i > median * 0.5 && i < median * 2.0)
        .copied().collect();

    if filtered.is_empty() { return None; }

    let avg_interval = filtered.iter().sum::<f32>() / filtered.len() as f32;
    let bpm = 60.0 / avg_interval;

    // Constrain to reasonable BPM range
    let bpm = if bpm > 200.0 { bpm / 2.0 }
        else if bpm < 60.0 { bpm * 2.0 }
        else { bpm };

    if bpm >= 50.0 && bpm <= 250.0 { Some(bpm.round()) } else { None }
}

/// Time-stretch audio by simple resampling (changes pitch)
pub fn resample(data: &[f32], ratio: f64) -> Vec<f32> {
    let new_len = (data.len() as f64 * ratio) as usize;
    let mut out = Vec::with_capacity(new_len);
    for i in 0..new_len {
        let src = i as f64 / ratio;
        let idx = src as usize;
        let frac = src - idx as f64;
        let a = data.get(idx).copied().unwrap_or(0.0);
        let b = data.get(idx + 1).copied().unwrap_or(a);
        out.push((a as f64 * (1.0 - frac) + b as f64 * frac) as f32);
    }
    out
}
