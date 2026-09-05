//! Microphone capture.
//!
//! `cpal::Stream` is `!Send`, so it cannot be parked in shared application state
//! and started/stopped from whichever thread happens to handle the hotkey. The
//! fix is to give the stream a thread of its own and talk to it over a channel:
//! the stream is created, held and dropped entirely inside `audio_thread`, and
//! callers only ever exchange plain data with it.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

/// A finished recording: interleaved channels already mixed down to mono.
pub struct Capture {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

enum Cmd {
    Start(Sender<Result<()>>),
    Stop(Sender<Capture>),
    DeviceName(Sender<String>),
}

pub struct Recorder {
    // `mpsc::Sender` is `Send` but not `Sync`, and Tauri's managed state must be
    // both. The mutex is uncontended in practice - one lock per hotkey press.
    tx: Mutex<Sender<Cmd>>,
}

impl Recorder {
    pub fn new() -> Self {
        let (tx, rx) = channel::<Cmd>();

        std::thread::spawn(move || {
            // The live stream, its shared buffer, and the rate it is capturing at.
            let mut active: Option<(cpal::Stream, Arc<Mutex<Vec<f32>>>, u32)> = None;

            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Cmd::Start(reply) => {
                        // Starting twice would leak the first stream.
                        if active.is_some() {
                            let _ = reply.send(Ok(()));
                            continue;
                        }
                        match open_stream() {
                            Ok(stream) => {
                                active = Some(stream);
                                let _ = reply.send(Ok(()));
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e));
                            }
                        }
                    }

                    Cmd::Stop(reply) => {
                        let capture = match active.take() {
                            Some((stream, buffer, sample_rate)) => {
                                // Dropping the stream stops the callback, so no
                                // sample can land in the buffer after this.
                                drop(stream);
                                Capture {
                                    samples: std::mem::take(&mut *buffer.lock()),
                                    sample_rate,
                                }
                            }
                            None => Capture {
                                samples: Vec::new(),
                                sample_rate: crate::resample::TARGET_SAMPLE_RATE,
                            },
                        };
                        let _ = reply.send(capture);
                    }

                    Cmd::DeviceName(reply) => {
                        let name = cpal::default_host()
                            .default_input_device()
                            .and_then(|d| d.name().ok())
                            .unwrap_or_else(|| "no input device".to_string());
                        let _ = reply.send(name);
                    }
                }
            }
        });

        Self { tx: Mutex::new(tx) }
    }

    // `SendError<Cmd>` is not `Sync`, so it cannot become an `anyhow::Error`
    // via `?`. Map it instead - the only way a send fails is the thread dying.
    fn dispatch(&self, cmd: Cmd) -> Result<()> {
        self.tx
            .lock()
            .send(cmd)
            .map_err(|_| anyhow!("audio thread is not running"))
    }

    pub fn start(&self) -> Result<()> {
        let (tx, rx) = channel();
        self.dispatch(Cmd::Start(tx))?;
        rx.recv()?
    }

    pub fn stop(&self) -> Result<Capture> {
        let (tx, rx) = channel();
        self.dispatch(Cmd::Stop(tx))?;
        Ok(rx.recv()?)
    }

    pub fn device_name(&self) -> String {
        let (tx, rx) = channel();
        if self.dispatch(Cmd::DeviceName(tx)).is_err() {
            return "audio thread stopped".to_string();
        }
        rx.recv().unwrap_or_else(|_| "unknown".to_string())
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

fn open_stream() -> Result<(cpal::Stream, Arc<Mutex<Vec<f32>>>, u32)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no default input device — is a microphone connected?"))?;

    let supported = device.default_input_config()?;
    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    // Pre-allocate for 30s so a long dictation does not reallocate mid-callback.
    let buffer = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
        sample_rate as usize * 30,
    )));

    let err_fn = |e| eprintln!("[audio] stream error: {e}");

    // Each arm mixes the interleaved frame down to mono as it arrives, so the
    // buffer is always single-channel regardless of what the device gave us.
    macro_rules! build_stream {
        ($sample:ty, $to_f32:expr) => {{
            let sink = Arc::clone(&buffer);
            device.build_input_stream(
                &config,
                move |data: &[$sample], _: &cpal::InputCallbackInfo| {
                    let mut sink = sink.lock();
                    for frame in data.chunks(channels) {
                        let sum: f32 = frame.iter().copied().map($to_f32).sum();
                        sink.push(sum / frame.len() as f32);
                    }
                },
                err_fn,
                None,
            )?
        }};
    }

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_stream!(f32, |s: f32| s),
        cpal::SampleFormat::I16 => build_stream!(i16, |s: i16| s as f32 / i16::MAX as f32),
        cpal::SampleFormat::U16 => {
            build_stream!(u16, |s: u16| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
        }
        other => return Err(anyhow!("unsupported sample format: {other:?}")),
    };

    stream.play()?;
    Ok((stream, buffer, sample_rate))
}
