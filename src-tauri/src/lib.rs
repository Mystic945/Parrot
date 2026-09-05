mod audio;
mod paste;
mod resample;
mod settings;
mod transcribe;

use audio::Recorder;
use parking_lot::Mutex;
use serde::Serialize;
use settings::Settings;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use transcribe::Transcriber;

/// Ignore taps shorter than this. Without it, every accidental brush of the
/// hotkey costs a whisper pass on a fifth of a second of nothing.
const MIN_RECORDING_MS: u128 = 250;

/// Below this peak amplitude the buffer is treated as silence. Whisper
/// hallucinates confidently on silence — "Thank you." is the classic output.
const SILENCE_THRESHOLD: f32 = 0.008;

pub struct AppState {
    recorder: Recorder,
    transcriber: Arc<Mutex<Option<Transcriber>>>,
    /// True between hotkey press and release. Also debounces key auto-repeat.
    recording: AtomicBool,
    started_at: Mutex<Option<Instant>>,
    settings: Mutex<Settings>,
    data_dir: PathBuf,
}

#[derive(Clone, Serialize)]
struct StatusEvent {
    state: &'static str,
    detail: String,
}

fn emit_status(app: &AppHandle, state: &'static str, detail: impl Into<String>) {
    let _ = app.emit(
        "status",
        StatusEvent {
            state,
            detail: detail.into(),
        },
    );
}

// ---------------------------------------------------------------- recording

fn begin_recording(app: &AppHandle) {
    let state = app.state::<Arc<AppState>>();

    // `swap` is what makes this safe against key auto-repeat: the OS sends a
    // stream of Pressed events while the key is held, and only the first one
    // flips the flag.
    if state.recording.swap(true, Ordering::SeqCst) {
        return;
    }

    match state.recorder.start() {
        Ok(()) => {
            *state.started_at.lock() = Some(Instant::now());
            emit_status(app, "recording", "Listening...");
        }
        Err(e) => {
            state.recording.store(false, Ordering::SeqCst);
            emit_status(app, "error", format!("Microphone error: {e}"));
        }
    }
}

fn end_recording(app: &AppHandle) {
    let state = app.state::<Arc<AppState>>().inner().clone();

    if !state.recording.swap(false, Ordering::SeqCst) {
        return;
    }

    let held_ms = state
        .started_at
        .lock()
        .take()
        .map(|t| t.elapsed().as_millis())
        .unwrap_or(0);

    let app = app.clone();

    // Everything below is slow — audio teardown, resampling and a full whisper
    // pass. Running it inline would block the event loop and freeze the UI.
    std::thread::spawn(move || {
        let capture = match state.recorder.stop() {
            Ok(c) => c,
            Err(e) => {
                emit_status(&app, "error", format!("Could not stop recording: {e}"));
                return;
            }
        };

        if held_ms < MIN_RECORDING_MS {
            emit_status(&app, "idle", "Too short - hold the key while speaking");
            return;
        }

        if resample::peak(&capture.samples) < SILENCE_THRESHOLD {
            emit_status(&app, "idle", "No speech detected");
            return;
        }

        emit_status(&app, "transcribing", "Transcribing...");

        let audio = resample::resample(
            &capture.samples,
            capture.sample_rate,
            resample::TARGET_SAMPLE_RATE,
        );

        if state.settings.lock().save_debug_wav {
            let path = state.data_dir.join("last-recording.wav");
            if let Err(e) = transcribe::save_wav(&path, &audio, resample::TARGET_SAMPLE_RATE) {
                eprintln!("[debug] could not write wav: {e}");
            }
        }

        let language = state.settings.lock().language.clone();
        let started = Instant::now();

        let text = {
            let guard = state.transcriber.lock();
            match guard.as_ref() {
                Some(t) => t.transcribe(&audio, &language),
                None => Err(anyhow::anyhow!("model is still loading")),
            }
        };

        match text {
            Ok(text) if text.is_empty() => {
                emit_status(&app, "idle", "Nothing recognised");
            }
            Ok(text) => {
                let _ = app.emit("transcript", &text);
                let chars = text.chars().count();
                match paste::paste_text(&text) {
                    Ok(()) => emit_status(
                        &app,
                        "idle",
                        format!(
                            "Pasted {} chars in {:.1}s",
                            chars,
                            started.elapsed().as_secs_f32()
                        ),
                    ),
                    Err(e) => emit_status(&app, "error", format!("Paste failed: {e}")),
                }
            }
            Err(e) => emit_status(&app, "error", format!("{e}")),
        }
    });
}

// ----------------------------------------------------------------- commands

#[tauri::command]
fn get_settings(state: State<'_, Arc<AppState>>) -> Settings {
    state.settings.lock().clone()
}

#[tauri::command]
fn get_device_name(state: State<'_, Arc<AppState>>) -> String {
    state.recorder.device_name()
}

#[tauri::command]
fn model_ready(state: State<'_, Arc<AppState>>) -> bool {
    state.transcriber.lock().is_some()
}

#[tauri::command]
fn model_location(state: State<'_, Arc<AppState>>) -> String {
    let model_file = state.settings.lock().model_file.clone();
    transcribe::model_path(&state.data_dir, &model_file)
        .display()
        .to_string()
}

#[tauri::command]
fn set_shortcut(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    shortcut: String,
) -> Result<(), String> {
    let parsed: Shortcut = shortcut
        .parse()
        .map_err(|_| format!("not a valid accelerator: {shortcut} (try Ctrl+Alt+Space)"))?;

    let manager = app.global_shortcut();
    let previous = state.settings.lock().shortcut.clone();

    if let Ok(old) = previous.parse::<Shortcut>() {
        let _ = manager.unregister(old);
    }

    if let Err(e) = manager.register(parsed) {
        // Put the old binding back so the app is not left with no hotkey at all.
        if let Ok(old) = previous.parse::<Shortcut>() {
            let _ = manager.register(old);
        }
        return Err(format!(
            "could not register {shortcut}: {e} (another app may already own it)"
        ));
    }

    let mut settings = state.settings.lock();
    settings.shortcut = shortcut;
    settings.save(&state.data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_language(state: State<'_, Arc<AppState>>, language: String) -> Result<(), String> {
    let mut settings = state.settings.lock();
    settings.language = language;
    settings.save(&state.data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_debug_wav(state: State<'_, Arc<AppState>>, enabled: bool) -> Result<(), String> {
    let mut settings = state.settings.lock();
    settings.save_debug_wav = enabled;
    settings.save(&state.data_dir).map_err(|e| e.to_string())
}

/// Lets the UI drive a recording without the hotkey, which is how you test the
/// pipeline before trusting the global shortcut.
#[tauri::command]
fn toggle_recording(app: AppHandle, state: State<'_, Arc<AppState>>) {
    if state.recording.load(Ordering::SeqCst) {
        end_recording(&app);
    } else {
        begin_recording(&app);
    }
}

// --------------------------------------------------------------- app set-up

fn load_model_in_background(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<Arc<AppState>>().inner().clone();
        let model_file = state.settings.lock().model_file.clone();
        let path = transcribe::model_path(&state.data_dir, &model_file);

        emit_status(&app, "loading", format!("Loading {model_file}..."));

        match Transcriber::load(&path) {
            Ok(t) => {
                *state.transcriber.lock() = Some(t);
                let shortcut = state.settings.lock().shortcut.clone();
                emit_status(&app, "idle", format!("Ready - hold {shortcut} to dictate"));
            }
            Err(e) => emit_status(&app, "error", format!("{e}")),
        }
    });
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::new().tooltip("Parrot").menu(&menu);

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| match event.state {
                    ShortcutState::Pressed => begin_recording(app),
                    ShortcutState::Released => end_recording(app),
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_settings,
            get_device_name,
            model_ready,
            model_location,
            set_shortcut,
            set_language,
            set_debug_wav,
            toggle_recording,
        ])
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let settings = Settings::load(&data_dir);
            let shortcut = settings.shortcut.clone();

            app.manage(Arc::new(AppState {
                recorder: Recorder::new(),
                transcriber: Arc::new(Mutex::new(None)),
                recording: AtomicBool::new(false),
                started_at: Mutex::new(None),
                settings: Mutex::new(settings),
                data_dir,
            }));

            let handle = app.handle().clone();

            match shortcut.parse::<Shortcut>() {
                Ok(parsed) => {
                    if let Err(e) = handle.global_shortcut().register(parsed) {
                        eprintln!("[shortcut] could not register {shortcut}: {e}");
                    }
                }
                Err(_) => eprintln!("[shortcut] invalid accelerator: {shortcut}"),
            }

            build_tray(&handle)?;
            load_model_in_background(handle);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Parrot");
}
