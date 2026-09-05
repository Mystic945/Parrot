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
    /// The `Option` is the device name the user picked; `None` means "whatever
    /// Windows calls default", which is exactly the assumption that broke here.
    Start(Option<String>, Sender<Result<()>>),
    Stop(Sender<Capture>),
    List(Sender<Vec<String>>),
    Resolve(Option<String>, Sender<String>),
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
                    Cmd::Start(preferred, reply) => {
                        // Starting twice would leak the first stream.
                        if active.is_some() {
                            let _ = reply.send(Ok(()));
                            continue;
                        }
                        match open_stream(preferred.as_deref()) {
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

                    Cmd::List(reply) => {
                        let _ = reply.send(input_device_names());
                    }

                    Cmd::Resolve(preferred, reply) => {
                        let _ = reply.send(resolve_device_name(preferred.as_deref()));
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

    pub fn start(&self, preferred: Option<String>) -> Result<()> {
        let (tx, rx) = channel();
        self.dispatch(Cmd::Start(preferred, tx))?;
        rx.recv()?
    }

    pub fn stop(&self) -> Result<Capture> {
        let (tx, rx) = channel();
        self.dispatch(Cmd::Stop(tx))?;
        Ok(rx.recv()?)
    }

    /// Every input device the host can see, for the settings dropdown.
    pub fn list_devices(&self) -> Vec<String> {
        let (tx, rx) = channel();
        if self.dispatch(Cmd::List(tx)).is_err() {
            return Vec::new();
        }
        rx.recv().unwrap_or_default()
    }

    /// The device that would actually be used right now - which is not the same
    /// as the one that was requested, if the request names something unplugged.
    pub fn device_name(&self, preferred: Option<String>) -> String {
        let (tx, rx) = channel();
        if self.dispatch(Cmd::Resolve(preferred, tx)).is_err() {
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

fn input_device_names() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

fn resolve_device_name(preferred: Option<&str>) -> String {
    match pick_device(preferred).and_then(|d| d.name().map_err(Into::into)) {
        Ok(name) => name,
        Err(e) => format!("unavailable ({e})"),
    }
}

/// A named device that has since been unplugged must not silently fall back to
/// the default - that is how you end up recording silence and not knowing why.
fn pick_device(preferred: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();

    match preferred {
        Some(wanted) => host
            .input_devices()?
            .find(|d| d.name().map(|n| n == wanted).unwrap_or(false))
            .ok_or_else(|| anyhow!("input device not found: {wanted}")),
        None => host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device - is a microphone connected?")),
    }
}

fn open_stream(preferred: Option<&str>) -> Result<(cpal::Stream, Arc<Mutex<Vec<f32>>>, u32)> {
    let device = pick_device(preferred)?;

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
