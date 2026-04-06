//! Microphone input recording — capture audio from the default input device
//! and return it as a Vec<f32> for loading onto a pad.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

pub struct MicRecorder {
    buffer: Arc<Mutex<Vec<f32>>>,
    recording: Arc<AtomicBool>,
    _stream: Option<cpal::Stream>,
    pub sample_rate: u32,
    pub available: bool,
}

impl MicRecorder {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host.default_input_device();

        let (stream, sr, available) = if let Some(dev) = device {
            if let Ok(config) = dev.default_input_config() {
                let sr = config.sample_rate().0;
                let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
                let buf_clone = buffer.clone();
                let recording = Arc::new(AtomicBool::new(false));
                let rec_clone = recording.clone();

                let stream_config = cpal::StreamConfig {
                    channels: 1,
                    sample_rate: config.sample_rate(),
                    buffer_size: cpal::BufferSize::Default,
                };

                let stream = dev.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if rec_clone.load(Ordering::Relaxed) {
                            if let Ok(mut buf) = buf_clone.lock() {
                                buf.extend_from_slice(data);
                            }
                        }
                    },
                    |err| eprintln!("Mic error: {err}"),
                    None,
                ).ok();

                if let Some(ref s) = stream {
                    let _ = s.play();
                }

                (stream, sr, true)
            } else {
                (None, 44100, false)
            }
        } else {
            (None, 44100, false)
        };

        MicRecorder {
            buffer: Arc::new(Mutex::new(Vec::new())),
            recording: Arc::new(AtomicBool::new(false)),
            _stream: stream,
            sample_rate: sr,
            available,
        }
    }

    pub fn start(&self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
        self.recording.store(true, Ordering::Relaxed);
    }

    pub fn stop(&self) -> Vec<f32> {
        self.recording.store(false, Ordering::Relaxed);
        if let Ok(buf) = self.buffer.lock() {
            buf.clone()
        } else {
            Vec::new()
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::Relaxed)
    }

    pub fn duration(&self) -> f32 {
        if let Ok(buf) = self.buffer.lock() {
            buf.len() as f32 / self.sample_rate as f32
        } else {
            0.0
        }
    }
}
