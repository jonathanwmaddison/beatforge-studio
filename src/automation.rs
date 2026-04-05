//! Automation system — per-step parameter automation for filter sweeps,
//! volume changes, pan movements, etc. Similar to FL Studio automation clips.

pub const MAX_AUTO_STEPS: usize = 64;

#[derive(Clone, Copy, PartialEq)]
pub enum AutoTarget {
    FilterCutoff,
    Volume,
    Pan,
    ReverbSend,
    DelaySend,
    Pitch,
    MasterFilter,
    MasterVolume,
}

impl AutoTarget {
    pub fn name(&self) -> &'static str {
        match self {
            AutoTarget::FilterCutoff => "FILTER",
            AutoTarget::Volume => "VOLUME",
            AutoTarget::Pan => "PAN",
            AutoTarget::ReverbSend => "REVERB",
            AutoTarget::DelaySend => "DELAY",
            AutoTarget::Pitch => "PITCH",
            AutoTarget::MasterFilter => "M.FILTER",
            AutoTarget::MasterVolume => "M.VOL",
        }
    }

    pub fn range(&self) -> (f32, f32) {
        match self {
            AutoTarget::FilterCutoff => (100.0, 20000.0),
            AutoTarget::Volume => (0.0, 1.0),
            AutoTarget::Pan => (-1.0, 1.0),
            AutoTarget::ReverbSend => (0.0, 1.0),
            AutoTarget::DelaySend => (0.0, 1.0),
            AutoTarget::Pitch => (-24.0, 24.0),
            AutoTarget::MasterFilter => (100.0, 20000.0),
            AutoTarget::MasterVolume => (0.0, 1.0),
        }
    }

    pub fn is_logarithmic(&self) -> bool {
        matches!(self, AutoTarget::FilterCutoff | AutoTarget::MasterFilter)
    }
}

/// A single automation lane — stores per-step values for one parameter on one pad
#[derive(Clone)]
pub struct AutomationLane {
    pub target: AutoTarget,
    pub pad: usize, // 0-15 for pad, 255 for master
    pub values: Vec<Option<f32>>, // None = no automation at this step, Some = value
    pub enabled: bool,
}

impl AutomationLane {
    pub fn new(target: AutoTarget, pad: usize, num_steps: usize) -> Self {
        AutomationLane {
            target,
            pad,
            values: vec![None; num_steps],
            enabled: true,
        }
    }

    /// Set a value at a specific step
    pub fn set(&mut self, step: usize, value: f32) {
        if step < self.values.len() {
            let (min, max) = self.target.range();
            self.values[step] = Some(value.clamp(min, max));
        }
    }

    /// Clear a value at a specific step
    pub fn clear(&mut self, step: usize) {
        if step < self.values.len() {
            self.values[step] = None;
        }
    }

    /// Get interpolated value at a step (linear interpolation between defined points)
    pub fn get_interpolated(&self, step: usize) -> Option<f32> {
        if step >= self.values.len() { return None; }

        // If this step has a value, return it
        if let Some(v) = self.values[step] {
            return Some(v);
        }

        // Find previous and next defined points for interpolation
        let mut prev_step = None;
        let mut prev_val = None;
        for i in (0..step).rev() {
            if let Some(v) = self.values[i] {
                prev_step = Some(i);
                prev_val = Some(v);
                break;
            }
        }

        let mut next_step = None;
        let mut next_val = None;
        for i in (step + 1)..self.values.len() {
            if let Some(v) = self.values[i] {
                next_step = Some(i);
                next_val = Some(v);
                break;
            }
        }

        match (prev_val, next_val) {
            (Some(pv), Some(nv)) => {
                let ps = prev_step.unwrap() as f32;
                let ns = next_step.unwrap() as f32;
                let t = (step as f32 - ps) / (ns - ps);

                if self.target.is_logarithmic() {
                    // Logarithmic interpolation for frequency
                    Some((pv.ln() + t * (nv.ln() - pv.ln())).exp())
                } else {
                    Some(pv + t * (nv - pv))
                }
            }
            (Some(pv), None) => Some(pv),
            (None, Some(nv)) => Some(nv),
            (None, None) => None,
        }
    }

    /// Resize to new step count
    pub fn resize(&mut self, new_steps: usize) {
        self.values.resize(new_steps, None);
    }
}

/// Collection of automation lanes for a project
#[derive(Clone, Default)]
pub struct AutomationData {
    pub lanes: Vec<AutomationLane>,
}

impl AutomationData {
    pub fn new() -> Self {
        AutomationData { lanes: Vec::new() }
    }

    /// Add a new automation lane
    pub fn add_lane(&mut self, target: AutoTarget, pad: usize, num_steps: usize) -> usize {
        let lane = AutomationLane::new(target, pad, num_steps);
        self.lanes.push(lane);
        self.lanes.len() - 1
    }

    /// Get all lanes for a specific pad
    pub fn lanes_for_pad(&self, pad: usize) -> Vec<(usize, &AutomationLane)> {
        self.lanes.iter().enumerate()
            .filter(|(_, l)| l.pad == pad && l.enabled)
            .collect()
    }

    /// Get all master lanes
    pub fn master_lanes(&self) -> Vec<(usize, &AutomationLane)> {
        self.lanes.iter().enumerate()
            .filter(|(_, l)| l.pad == 255 && l.enabled)
            .collect()
    }

    /// Apply automation at a given step, returns (target, pad, value) tuples
    pub fn values_at_step(&self, step: usize) -> Vec<(AutoTarget, usize, f32)> {
        self.lanes.iter()
            .filter(|l| l.enabled)
            .filter_map(|l| l.get_interpolated(step).map(|v| (l.target, l.pad, v)))
            .collect()
    }

    /// Resize all lanes
    pub fn resize_all(&mut self, num_steps: usize) {
        for lane in &mut self.lanes {
            lane.resize(num_steps);
        }
    }
}
