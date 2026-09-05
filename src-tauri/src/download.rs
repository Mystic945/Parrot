//! Fetching whisper models at runtime.
//!
//! Shipping a 141 MB model inside the installer would make every update a
//! 141 MB download, so the app fetches it on first run instead. The installer
//! stays ~20 MB and the model is downloaded once, per machine.

use anyhow::{anyhow, bail, Result};
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

const BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// 64 KiB is large enough that the syscall overhead disappears and small enough
/// that progress still updates smoothly.
const CHUNK: usize = 64 * 1024;

pub struct ModelSpec {
    pub id: &'static str,
    pub file: &'static str,
    pub label: &'static str,
    pub size_mb: u32,
    pub note: &'static str,
}

/// English-only models are meaningfully better than the multilingual ones at
/// the same size, so they are the default. The multilingual `small` is here for
/// anyone who needs a language other than English.
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "tiny.en",
        file: "ggml-tiny.en.bin",
        label: "Tiny (English)",
        size_mb: 75,
        note: "Fastest, noticeably less accurate",
    },
    ModelSpec {
        id: "base.en",
        file: "ggml-base.en.bin",
        label: "Base (English)",
        size_mb: 142,
        note: "Good balance - the default",
    },
    ModelSpec {
        id: "small.en",
        file: "ggml-small.en.bin",
        label: "Small (English)",
        size_mb: 466,
        note: "More accurate, roughly 3x slower",
    },
    ModelSpec {
        id: "small",
        file: "ggml-small.bin",
        label: "Small (multilingual)",
        size_mb: 466,
        note: "Needed for languages other than English",
    },
];

pub fn spec_for(file: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|m| m.file == file)
}

/// Downloads to a `.partial` sibling and renames only on success. A truncated
/// model file is worse than a missing one: whisper.cpp fails on it with an
/// error that says nothing about the download.
pub fn fetch(file: &str, dest: &Path, mut on_progress: impl FnMut(u64, u64)) -> Result<()> {
    if spec_for(file).is_none() {
        bail!("unknown model: {file}");
    }

    let url = format!("{BASE_URL}/{file}");
    let mut response = ureq::get(&url)
        .call()
        .map_err(|e| anyhow!("could not reach Hugging Face: {e}"))?;

    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let partial = partial_path(dest);
    let mut writer = BufWriter::new(File::create(&partial)?);
    let mut reader = response.body_mut().as_reader();
    let mut buffer = vec![0u8; CHUNK];
    let mut received = 0u64;

    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = std::fs::remove_file(&partial);
                return Err(anyhow!("download interrupted: {e}"));
            }
        };

        if let Err(e) = writer.write_all(&buffer[..read]) {
            let _ = std::fs::remove_file(&partial);
            return Err(anyhow!("could not write model: {e}"));
        }

        received += read as u64;
        on_progress(received, total);
    }

    writer.flush()?;
    drop(writer);

    // A server that closes early leaves a plausible-looking short file.
    if total > 0 && received != total {
        let _ = std::fs::remove_file(&partial);
        bail!("download incomplete: got {received} of {total} bytes");
    }

    std::fs::rename(&partial, dest)?;
    Ok(())
}

fn partial_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".to_string());
    dest.with_file_name(format!("{name}.partial"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_appends_rather_than_replacing_extension() {
        let p = partial_path(Path::new("/models/ggml-base.en.bin"));
        assert!(p.ends_with("ggml-base.en.bin.partial"));
    }

    #[test]
    fn catalog_lookup_matches_by_filename() {
        assert_eq!(spec_for("ggml-base.en.bin").unwrap().id, "base.en");
        assert!(spec_for("ggml-nonexistent.bin").is_none());
    }
}
