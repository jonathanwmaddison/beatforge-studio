//! Project save/load — serialize entire project state to JSON.
//! Saves patterns, pad assignments, synth params, mixer settings, FX, BPM, etc.

use std::path::Path;
use std::fs;

/// Serializable project state
#[derive(Clone)]
pub struct ProjectData {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub num_steps: usize,
    pub active_bank: usize,
    pub master_vol: f32,
    pub master_filter: f32,
    pub reverb_mix: f32,
    pub delay_mix: f32,
    pub banks: Vec<Vec<Vec<u8>>>,
    pub pad_names: Vec<String>,
    pub volumes: Vec<f32>,
    pub pans: Vec<f32>,
    pub pitches: Vec<f32>,
    pub filters: Vec<f32>,
    pub reversed: Vec<bool>,
    pub trim_start: Vec<f32>,
    pub trim_end: Vec<f32>,
    pub muted: Vec<bool>,
    pub soloed: Vec<bool>,
    pub synth_assigned: Vec<bool>,
    pub reverb_sends: Vec<f32>,
    pub delay_sends: Vec<f32>,
    pub stereo_width: f32,
    pub enhancer_amount: f32,
    pub sidechain_active: Vec<bool>,
    pub fx_params: Vec<[f32; 12]>,
}

impl ProjectData {
    pub fn new() -> Self {
        ProjectData {
            version: 1,
            name: "Untitled".to_string(),
            bpm: 90.0,
            swing: 0.0,
            num_steps: 16,
            active_bank: 0,
            master_vol: 0.8,
            master_filter: 20000.0,
            reverb_mix: 0.0,
            delay_mix: 0.0,
            banks: Vec::new(),
            pad_names: Vec::new(),
            volumes: Vec::new(),
            pans: Vec::new(),
            pitches: Vec::new(),
            filters: Vec::new(),
            reversed: Vec::new(),
            trim_start: Vec::new(),
            trim_end: Vec::new(),
            muted: Vec::new(),
            soloed: Vec::new(),
            synth_assigned: Vec::new(),
            reverb_sends: Vec::new(),
            delay_sends: Vec::new(),
            stereo_width: 1.0,
            enhancer_amount: 0.0,
            sidechain_active: Vec::new(),
            fx_params: Vec::new(),
        }
    }

    /// Save to a simple text format (key=value, one section per pad)
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut out = String::new();

        out.push_str(&format!("[project]\n"));
        out.push_str(&format!("version={}\n", self.version));
        out.push_str(&format!("name={}\n", self.name));
        out.push_str(&format!("bpm={}\n", self.bpm));
        out.push_str(&format!("swing={}\n", self.swing));
        out.push_str(&format!("num_steps={}\n", self.num_steps));
        out.push_str(&format!("active_bank={}\n", self.active_bank));
        out.push_str(&format!("master_vol={}\n", self.master_vol));
        out.push_str(&format!("master_filter={}\n", self.master_filter));
        out.push_str(&format!("reverb_mix={}\n", self.reverb_mix));
        out.push_str(&format!("delay_mix={}\n", self.delay_mix));
        out.push_str("\n");

        // Banks
        for (bi, bank) in self.banks.iter().enumerate() {
            out.push_str(&format!("[bank.{}]\n", bi));
            for (pi, row) in bank.iter().enumerate() {
                let vals: Vec<String> = row.iter().map(|v| v.to_string()).collect();
                out.push_str(&format!("pad.{}={}\n", pi, vals.join(",")));
            }
            out.push_str("\n");
        }

        // Pads
        for i in 0..self.pad_names.len() {
            out.push_str(&format!("[pad.{}]\n", i));
            out.push_str(&format!("name={}\n", self.pad_names[i]));
            out.push_str(&format!("vol={}\n", self.volumes.get(i).unwrap_or(&0.7)));
            out.push_str(&format!("pan={}\n", self.pans.get(i).unwrap_or(&0.0)));
            out.push_str(&format!("pitch={}\n", self.pitches.get(i).unwrap_or(&0.0)));
            out.push_str(&format!("filter={}\n", self.filters.get(i).unwrap_or(&20000.0)));
            out.push_str(&format!("reversed={}\n", self.reversed.get(i).unwrap_or(&false)));
            out.push_str(&format!("trim_start={}\n", self.trim_start.get(i).unwrap_or(&0.0)));
            out.push_str(&format!("trim_end={}\n", self.trim_end.get(i).unwrap_or(&1.0)));
            out.push_str(&format!("muted={}\n", self.muted.get(i).unwrap_or(&false)));
            out.push_str(&format!("soloed={}\n", self.soloed.get(i).unwrap_or(&false)));
            out.push_str(&format!("synth={}\n", self.synth_assigned.get(i).unwrap_or(&false)));
            out.push_str(&format!("reverb_send={}\n", self.reverb_sends.get(i).unwrap_or(&0.0)));
            out.push_str(&format!("delay_send={}\n", self.delay_sends.get(i).unwrap_or(&0.0)));
            out.push_str(&format!("sidechain={}\n", self.sidechain_active.get(i).unwrap_or(&false)));
            // FX params (12 floats)
            if let Some(fx) = self.fx_params.get(i) {
                let fx_str: Vec<String> = fx.iter().map(|v| format!("{:.3}", v)).collect();
                out.push_str(&format!("fx={}\n", fx_str.join(",")));
            }
            out.push_str("\n");
        }

        // Global extras
        out.push_str("[global]\n");
        out.push_str(&format!("stereo_width={}\n", self.stereo_width));
        out.push_str(&format!("enhancer={}\n", self.enhancer_amount));
        out.push_str("\n");

        fs::write(path, out).map_err(|e| format!("Save failed: {e}"))
    }

    /// Load from file
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path).map_err(|e| format!("Load failed: {e}"))?;
        let mut project = ProjectData::new();

        let mut current_section = String::new();
        let mut current_bank: Option<usize> = None;
        let mut current_pad: Option<usize> = None;

        // Ensure banks exist
        for _ in 0..8 {
            project.banks.push(vec![vec![0u8; 64]; 16]);
        }
        for _ in 0..16 {
            project.pad_names.push("".to_string());
            project.volumes.push(0.7);
            project.pans.push(0.0);
            project.pitches.push(0.0);
            project.filters.push(20000.0);
            project.reversed.push(false);
            project.trim_start.push(0.0);
            project.trim_end.push(1.0);
            project.muted.push(false);
            project.soloed.push(false);
            project.synth_assigned.push(false);
            project.reverb_sends.push(0.0);
            project.delay_sends.push(0.0);
            project.sidechain_active.push(false);
            project.fx_params.push([0.0, 0.0, 16.0, 1.0, 0.0, 0.5, 3.0, 0.0, 0.3, 0.5, 0.5, 0.0]);
        }

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }

            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len()-1].to_string();
                if current_section.starts_with("bank.") {
                    current_bank = current_section[5..].parse().ok();
                    current_pad = None;
                } else if current_section.starts_with("pad.") {
                    current_pad = current_section[4..].parse().ok();
                    current_bank = None;
                } else {
                    current_bank = None;
                    current_pad = None;
                }
                continue;
            }

            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim();

                if current_section == "project" || current_section == "global" {
                    match key {
                        "name" => project.name = val.to_string(),
                        "bpm" => project.bpm = val.parse().unwrap_or(90.0),
                        "swing" => project.swing = val.parse().unwrap_or(0.0),
                        "num_steps" => project.num_steps = val.parse().unwrap_or(16),
                        "active_bank" => project.active_bank = val.parse().unwrap_or(0),
                        "master_vol" => project.master_vol = val.parse().unwrap_or(0.8),
                        "master_filter" => project.master_filter = val.parse().unwrap_or(20000.0),
                        "reverb_mix" => project.reverb_mix = val.parse().unwrap_or(0.0),
                        "delay_mix" => project.delay_mix = val.parse().unwrap_or(0.0),
                        "stereo_width" => project.stereo_width = val.parse().unwrap_or(1.0),
                        "enhancer" => project.enhancer_amount = val.parse().unwrap_or(0.0),
                        _ => {}
                    }
                } else if let Some(bi) = current_bank {
                    if key.starts_with("pad.") {
                        if let Ok(pi) = key[4..].parse::<usize>() {
                            if bi < project.banks.len() && pi < 16 {
                                let vals: Vec<u8> = val.split(',')
                                    .filter_map(|v| v.trim().parse().ok())
                                    .collect();
                                for (j, &v) in vals.iter().enumerate() {
                                    if j < 64 {
                                        project.banks[bi][pi][j] = v;
                                    }
                                }
                            }
                        }
                    }
                } else if let Some(pi) = current_pad {
                    if pi < 16 {
                        match key {
                            "name" => project.pad_names[pi] = val.to_string(),
                            "vol" => project.volumes[pi] = val.parse().unwrap_or(0.7),
                            "pan" => project.pans[pi] = val.parse().unwrap_or(0.0),
                            "pitch" => project.pitches[pi] = val.parse().unwrap_or(0.0),
                            "filter" => project.filters[pi] = val.parse().unwrap_or(20000.0),
                            "reversed" => project.reversed[pi] = val.parse().unwrap_or(false),
                            "trim_start" => project.trim_start[pi] = val.parse().unwrap_or(0.0),
                            "trim_end" => project.trim_end[pi] = val.parse().unwrap_or(1.0),
                            "muted" => project.muted[pi] = val.parse().unwrap_or(false),
                            "soloed" => project.soloed[pi] = val.parse().unwrap_or(false),
                            "synth" => project.synth_assigned[pi] = val.parse().unwrap_or(false),
                            "reverb_send" => project.reverb_sends[pi] = val.parse().unwrap_or(0.0),
                            "delay_send" => project.delay_sends[pi] = val.parse().unwrap_or(0.0),
                            "sidechain" => project.sidechain_active[pi] = val.parse().unwrap_or(false),
                            "fx" => {
                                let vals: Vec<f32> = val.split(',')
                                    .filter_map(|v| v.trim().parse().ok())
                                    .collect();
                                for (j, &v) in vals.iter().enumerate() {
                                    if j < 12 { project.fx_params[pi][j] = v; }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_project() {
        let p = ProjectData::new();
        assert_eq!(p.version, 1);
        assert_eq!(p.bpm, 90.0);
        assert_eq!(p.name, "Untitled");
        assert_eq!(p.stereo_width, 1.0);
    }

    #[test]
    fn test_save_load_roundtrip() {
        let mut p = ProjectData::new();
        p.bpm = 145.0;
        p.name = "Test".to_string();
        p.master_vol = 0.65;
        p.reverb_mix = 0.25;
        p.stereo_width = 1.5;
        p.enhancer_amount = 0.4;

        // Need some pad data
        for _ in 0..8 {
            p.banks.push(vec![vec![0u8; 64]; 16]);
        }
        for _ in 0..16 {
            p.pad_names.push("TEST".to_string());
            p.volumes.push(0.7);
            p.pans.push(0.0);
            p.pitches.push(0.0);
            p.filters.push(20000.0);
            p.reversed.push(false);
            p.trim_start.push(0.0);
            p.trim_end.push(1.0);
            p.muted.push(false);
            p.soloed.push(false);
            p.synth_assigned.push(false);
            p.reverb_sends.push(0.0);
            p.delay_sends.push(0.0);
            p.sidechain_active.push(false);
            p.fx_params.push([0.0; 12]);
        }

        let tmp = std::env::temp_dir().join("beatforge_test_roundtrip.bfp");
        p.save(&tmp).unwrap();

        let loaded = ProjectData::load(&tmp).unwrap();
        assert!((loaded.bpm - 145.0).abs() < 0.1);
        assert_eq!(loaded.name, "Test");
        assert!((loaded.master_vol - 0.65).abs() < 0.01);
        assert!((loaded.reverb_mix - 0.25).abs() < 0.01);
        assert!((loaded.stereo_width - 1.5).abs() < 0.01);
        assert!((loaded.enhancer_amount - 0.4).abs() < 0.01);

        std::fs::remove_file(&tmp).ok();
    }
}
