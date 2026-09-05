//! Whisper inference via whisper.cpp.
//!
//! The model is loaded once and kept resident. Loading `base.en` takes on the
//! order of a second, which is fine at startup and completely unacceptable on
//! every keypress — a cold load per utterance is the single most common reason
//! a dictation app "feels broken".

use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct Transcriber {
    ctx: WhisperContext,
    threads: i32,
}

impl Transcriber {
    pub fn load(model_path: &Path) -> Result<Self> {
        if !model_path.exists() {
            bail!(
                "model not found at {} — run `npm run model` to download it",
                model_path.display()
            );
        }

        let ctx = WhisperContext::new_with_params(
            model_path,
            WhisperContextParameters::default(),
        )
        .map_err(|e| anyhow!("failed to load model: {e}"))?;

        // More threads than physical cores makes whisper slower, not faster.
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8) as i32)
            .unwrap_or(4);

        Ok(Self { ctx, threads })
    }

    /// `language` is an ISO code such as "en", or "auto" to let whisper detect it.
    pub fn transcribe(&self, audio_16k_mono: &[f32], language: &str) -> Result<String> {
        // Whisper pads to 30s internally; anything shorter than ~0.2s is noise.
        if audio_16k_mono.len() < 3_200 {
            return Ok(String::new());
        }

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| anyhow!("failed to create whisper state: {e}"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.threads);
        if language != "auto" {
            params.set_language(Some(language));
        }
        params.set_translate(false);
        params.set_suppress_blank(true);
        // Each utterance is independent; carrying context across them makes
        // whisper hallucinate continuations of the previous sentence.
        params.set_no_context(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, audio_16k_mono)
            .map_err(|e| anyhow!("transcription failed: {e}"))?;

        let mut text = String::new();
        for i in 0..state.full_n_segments() {
            if let Some(segment) = state.get_segment(i) {
                let chunk = segment
                    .to_str_lossy()
                    .map_err(|e| anyhow!("bad segment text: {e}"))?;
                text.push_str(&chunk);
            }
        }

        Ok(clean(&text))
    }
}

/// Whisper emits bracketed annotations for non-speech and a leading space on
/// nearly every segment. Neither belongs in the user's text field.
fn clean(raw: &str) -> String {
    const NOISE: [&str; 6] = [
        "[BLANK_AUDIO]",
        "[ Silence ]",
        "[silence]",
        "(silence)",
        "[MUSIC]",
        "[ Pause ]",
    ];

    let mut text = raw.to_string();
    for marker in NOISE {
        text = text.replace(marker, "");
    }

    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Where the model lives. Overridable so you can point at a shared model
/// directory instead of copying multi-hundred-megabyte files around.
pub fn model_path(app_data_dir: &Path, model_file: &str) -> PathBuf {
    if let Ok(dir) = std::env::var("PARROT_MODEL_DIR") {
        return PathBuf::from(dir).join(model_file);
    }
    app_data_dir.join("models").join(model_file)
}

/// Debug aid: dump exactly what was handed to whisper. If transcription looks
/// wrong, listen to this file first — the bug is usually in capture, not the model.
pub fn save_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for s in samples {
        writer.write_sample(*s)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn strips_noise_markers_and_normalises_whitespace() {
        assert_eq!(clean("  [BLANK_AUDIO] hello   there \n"), "hello there");
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        assert_eq!(clean(" Open the pod bay doors."), "Open the pod bay doors.");
    }
}
