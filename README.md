# Parrot

Offline push-to-talk dictation. Hold a hotkey, speak, release — the text is
pasted into whatever window has focus. Nothing leaves the machine.

A deliberately small reimplementation of the core of
[Handy](https://github.com/cjpais/Handy), built to be read in one sitting.

## Setup

### 1. Native toolchain

Whisper is C++, so this needs a real compiler even though most of the app is Rust.

**Windows**

```bash
winget install Rustlang.Rustup Kitware.CMake LLVM.LLVM Microsoft.VisualStudio.2022.BuildTools
```

The Build Tools installer must include the **Desktop development with C++**
workload — the default selection does not, and `whisper-rs` will fail to link.
`LLVM` is needed because `whisper-rs` runs `bindgen`, which needs `libclang`. If
the build complains it cannot find libclang, set it explicitly:

```bash
setx LIBCLANG_PATH "C:\Program Files\LLVM\bin"
```

**macOS**

```bash
xcode-select --install && brew install cmake rustup
```

**Linux (Debian/Ubuntu)**

```bash
sudo apt install build-essential cmake clang libclang-dev libasound2-dev libwebkit2gtk-4.1-dev librsvg2-dev libxdo-dev
```

Open a new terminal afterwards so the new tools are on `PATH`, then confirm:

```bash
cargo --version && cmake --version
```

### 2. Project

```bash
npm install && npm run icons && npm run model
```

- `npm run icons` generates a placeholder icon and expands it into every size
  Tauri bundles. Swap `icon-source.png` for real artwork when you have some.
- `npm run model` downloads `ggml-base.en.bin` (142 MB) from Hugging Face into
  the app's data directory. `npm run model -- small.en` gets a better one.

### 3. Run

```bash
npm run tauri dev
```

The first build compiles whisper.cpp and takes 10–20 minutes. Later builds are
seconds. Hold **Ctrl+Alt+Space**, say something, release.

## How it works

```
hotkey down ──> cpal input stream ──> mono f32 buffer
                                          │
hotkey up ────> stop stream ──────────────┤
                                          ▼
                         window-average resample to 16 kHz
                                          │
                                          ▼
                    whisper.cpp (model resident in memory)
                                          │
                                          ▼
                 clipboard swap + synthetic Ctrl+V + restore
```

| File | Responsibility |
| --- | --- |
| [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs) | Wiring: hotkey, state machine, Tauri commands, tray |
| [`src-tauri/src/audio.rs`](src-tauri/src/audio.rs) | Microphone capture on a dedicated thread |
| [`src-tauri/src/resample.rs`](src-tauri/src/resample.rs) | 44.1/48 kHz → 16 kHz |
| [`src-tauri/src/transcribe.rs`](src-tauri/src/transcribe.rs) | Whisper model loading and inference |
| [`src-tauri/src/paste.rs`](src-tauri/src/paste.rs) | Clipboard + synthetic keystrokes |
| [`src-tauri/src/settings.rs`](src-tauri/src/settings.rs) | JSON settings persistence |
| [`src/App.tsx`](src/App.tsx) | Settings window |

Four decisions worth knowing about, because each one is a trap someone else
will otherwise fall into:

**The audio stream gets its own thread.** `cpal::Stream` is `!Send`, so it
cannot live in shared application state that the hotkey handler touches. The
stream is created, held and dropped entirely inside one thread; callers exchange
plain data with it over a channel.

**Transcription never runs on the event loop.** A whisper pass takes hundreds of
milliseconds to seconds. Running it in the shortcut handler freezes the window
and the tray.

**The model is loaded once and kept resident.** Reloading per utterance adds a
second of latency and is the usual reason a dictation app feels broken.

**Key auto-repeat is debounced.** Holding a key emits a stream of `Pressed`
events, not one. An `AtomicBool::swap` makes only the first one count.

## Troubleshooting

**Nothing is transcribed.** Tick *Save each recording* in Diagnostics, record
again, and listen to `last-recording.wav` in the app data directory. If that
file is silent or garbled, the bug is in capture, not the model — check that the
right input device is default in the OS.

**Whisper returns "Thank you." for silence.** Expected; it hallucinates on empty
audio. `SILENCE_THRESHOLD` in `lib.rs` filters most of it. A real VAD (whisper-rs
0.16 ships one, `WhisperVadContext`) is the proper fix.

**The shortcut does not register.** Another application already owns it. Pick a
different one in the settings window.

**Paste does nothing in one specific app.** If the target runs elevated on
Windows and Parrot does not, the OS silently blocks synthetic input. Run both at
the same privilege level.

## What is deliberately missing

Compared to Handy: no GPU acceleration, no Parakeet backend, no voice activity
detection, no recording overlay, no history database, no autostart, no model
picker in the UI. Those are the next things to add, roughly in that order.

## Licence

MIT. Whisper models are MIT; whisper.cpp is MIT.
