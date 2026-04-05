//! MIDI input handling via midir.
//! Connects to any available MIDI input device and routes
//! note/CC messages to pad triggers and synth notes.

use crossbeam_channel::Sender;
use midir::{MidiInput, MidiInputConnection};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// MIDI message types we care about
#[derive(Debug, Clone)]
pub enum MidiEvent {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8 },
    ControlChange { channel: u8, cc: u8, value: u8 },
}

/// MIDI input manager
pub struct MidiManager {
    _connection: Option<MidiInputConnection<()>>,
    pub connected: Arc<AtomicBool>,
    pub device_name: String,
}

impl MidiManager {
    /// Try to connect to the first available MIDI input device.
    /// Sends parsed events through the channel.
    pub fn new(event_tx: Sender<MidiEvent>) -> Self {
        let connected = Arc::new(AtomicBool::new(false));
        let conn_flag = connected.clone();
        let mut device_name = "No MIDI device".to_string();

        let connection = Self::try_connect(event_tx, conn_flag.clone(), &mut device_name);

        MidiManager {
            _connection: connection,
            connected,
            device_name,
        }
    }

    fn try_connect(
        tx: Sender<MidiEvent>,
        connected: Arc<AtomicBool>,
        device_name: &mut String,
    ) -> Option<MidiInputConnection<()>> {
        let midi_in = MidiInput::new("BeatForge MIDI").ok()?;
        let ports = midi_in.ports();

        if ports.is_empty() {
            return None;
        }

        // Use the first available port
        let port = &ports[0];
        *device_name = midi_in.port_name(port).unwrap_or_else(|_| "Unknown".to_string());

        let conn = midi_in.connect(
            port,
            "beatforge-input",
            move |_timestamp, message, _| {
                if message.len() < 2 { return; }
                let status = message[0] & 0xF0;
                let channel = message[0] & 0x0F;

                let event = match status {
                    0x90 if message.len() >= 3 && message[2] > 0 => {
                        Some(MidiEvent::NoteOn {
                            channel,
                            note: message[1],
                            velocity: message[2],
                        })
                    }
                    0x80 | 0x90 if message.len() >= 3 => {
                        Some(MidiEvent::NoteOff {
                            channel,
                            note: message[1],
                        })
                    }
                    0xB0 if message.len() >= 3 => {
                        Some(MidiEvent::ControlChange {
                            channel,
                            cc: message[1],
                            value: message[2],
                        })
                    }
                    _ => None,
                };

                if let Some(evt) = event {
                    let _ = tx.send(evt);
                }
            },
            (),
        ).ok()?;

        connected.store(true, Ordering::Relaxed);
        Some(conn)
    }

    /// List available MIDI input devices
    pub fn list_devices() -> Vec<String> {
        if let Ok(midi_in) = MidiInput::new("BeatForge MIDI Scan") {
            midi_in.ports().iter()
                .filter_map(|p| midi_in.port_name(p).ok())
                .collect()
        } else {
            Vec::new()
        }
    }
}

/// Map MIDI note to pad index (General MIDI drum map)
/// Notes 36-51 map to pads 0-15 (standard GM drum mapping)
pub fn midi_note_to_pad(note: u8) -> Option<usize> {
    if note >= 36 && note <= 51 {
        Some((note - 36) as usize)
    } else {
        None
    }
}

/// Map MIDI CC to parameter changes
pub fn midi_cc_to_param(cc: u8, value: u8) -> Option<MidiParam> {
    match cc {
        1 => Some(MidiParam::ModWheel(value as f32 / 127.0)),
        7 => Some(MidiParam::Volume(value as f32 / 127.0)),
        10 => Some(MidiParam::Pan((value as f32 / 63.5) - 1.0)),
        74 => Some(MidiParam::FilterCutoff(value as f32 / 127.0 * 20000.0)),
        _ => None,
    }
}

#[derive(Debug)]
pub enum MidiParam {
    ModWheel(f32),
    Volume(f32),
    Pan(f32),
    FilterCutoff(f32),
}
