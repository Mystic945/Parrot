//! Getting text into whatever application currently has focus.
//!
//! Two options exist: synthesise the keystrokes for every character, or put the
//! text on the clipboard and synthesise one paste. Per-character typing is
//! visibly slow for a paragraph and drops characters in apps that debounce
//! input, so clipboard-and-paste is what real dictation tools do. The cost is
//! that we stomp the user's clipboard, which we undo afterwards.

use anyhow::{anyhow, Result};
use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::time::Duration;

/// Give the target application time to notice the new clipboard contents before
/// we tell it to paste.
const CLIPBOARD_SETTLE: Duration = Duration::from_millis(60);

/// Give it time to actually read the clipboard before we put the old value back.
const RESTORE_DELAY: Duration = Duration::from_millis(400);

#[cfg(target_os = "macos")]
const PASTE_MODIFIER: Key = Key::Meta;
#[cfg(not(target_os = "macos"))]
const PASTE_MODIFIER: Key = Key::Control;

pub fn paste_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let mut clipboard = Clipboard::new().map_err(|e| anyhow!("clipboard unavailable: {e}"))?;
    let previous = clipboard.get_text().ok();

    clipboard
        .set_text(text.to_string())
        .map_err(|e| anyhow!("could not write clipboard: {e}"))?;

    std::thread::sleep(CLIPBOARD_SETTLE);
    send_paste()?;

    // Restore on a detached thread so the caller is not blocked waiting for a
    // courtesy. If the user copies something else in the meantime we lose the
    // race and overwrite them — a real app would watch the clipboard sequence
    // number instead.
    if let Some(previous) = previous {
        std::thread::spawn(move || {
            std::thread::sleep(RESTORE_DELAY);
            if let Ok(mut clipboard) = Clipboard::new() {
                let _ = clipboard.set_text(previous);
            }
        });
    }

    Ok(())
}

fn send_paste() -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow!("could not open input device: {e}"))?;

    enigo
        .key(PASTE_MODIFIER, Direction::Press)
        .map_err(|e| anyhow!("modifier press failed: {e}"))?;

    let result = enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| anyhow!("paste key failed: {e}"));

    // Release the modifier even if the paste failed, or the user is left with a
    // stuck Ctrl key.
    let release = enigo
        .key(PASTE_MODIFIER, Direction::Release)
        .map_err(|e| anyhow!("modifier release failed: {e}"));

    result.and(release)
}
