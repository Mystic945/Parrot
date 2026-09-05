import { useEffect, useState } from "react";
import type { KeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Settings = {
  shortcut: string;
  language: string;
  input_device: string | null;
  model_file: string;
  save_debug_wav: boolean;
};

type StatusEvent = {
  state: "loading" | "idle" | "recording" | "transcribing" | "error" | "no-model";
  detail: string;
};

type ModelEntry = {
  id: string;
  file: string;
  label: string;
  note: string;
  size_mb: number;
  installed: boolean;
  active: boolean;
};

type DownloadProgress = {
  file: string;
  received: number;
  total: number;
  percent: number;
};

const LANGUAGES = [
  ["en", "English"],
  ["auto", "Auto-detect"],
  ["es", "Spanish"],
  ["fr", "French"],
  ["de", "German"],
  ["hi", "Hindi"],
  ["it", "Italian"],
  ["pt", "Portuguese"],
  ["ja", "Japanese"],
  ["zh", "Chinese"],
];

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [status, setStatus] = useState<StatusEvent>({
    state: "loading",
    detail: "Starting...",
  });
  const [transcripts, setTranscripts] = useState<string[]>([]);
  const [device, setDevice] = useState("");
  const [devices, setDevices] = useState<string[]>([]);
  const [modelPath, setModelPath] = useState("");
  const [shortcutDraft, setShortcutDraft] = useState("");
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    invoke<Settings>("get_settings").then((s) => {
      setSettings(s);
      setShortcutDraft(s.shortcut);
    });
    invoke<string>("get_device_name").then(setDevice);
    invoke<string[]>("list_input_devices").then(setDevices);
    invoke<ModelEntry[]>("list_models").then(setModels);
    invoke<string>("model_location").then(setModelPath);

    const unlistenStatus = listen<StatusEvent>("status", (e) =>
      setStatus(e.payload),
    );
    const unlistenText = listen<string>("transcript", (e) =>
      setTranscripts((prev) => [e.payload, ...prev].slice(0, 20)),
    );
    const unlistenProgress = listen<DownloadProgress>("download-progress", (e) => {
      setProgress(e.payload);
      // The backend stops emitting at 100%; clear the bar and re-read which
      // models are on disk now.
      if (e.payload.percent >= 100) {
        setProgress(null);
        invoke<ModelEntry[]>("list_models").then(setModels);
      }
    });

    return () => {
      unlistenStatus.then((f) => f());
      unlistenText.then((f) => f());
      unlistenProgress.then((f) => f());
    };
  }, []);

  // Capture a real key combination rather than making the user type
  // "Ctrl+Alt+Space" by hand and guess the spelling.
  function captureShortcut(e: KeyboardEvent<HTMLInputElement>) {
    e.preventDefault();
    const parts: string[] = [];
    if (e.ctrlKey) parts.push("Ctrl");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");
    if (e.metaKey) parts.push("Super");

    const key = e.key;
    const isModifier = ["Control", "Alt", "Shift", "Meta"].includes(key);
    if (isModifier) {
      setShortcutDraft(parts.join("+") + "+");
      return;
    }

    const named = key === " " ? "Space" : key.length === 1 ? key.toUpperCase() : key;
    parts.push(named);
    setShortcutDraft(parts.join("+"));
  }

  async function saveShortcut() {
    setError("");
    try {
      await invoke("set_shortcut", { shortcut: shortcutDraft });
      setSettings((s) => (s ? { ...s, shortcut: shortcutDraft } : s));
    } catch (e) {
      setError(String(e));
      if (settings) setShortcutDraft(settings.shortcut);
    }
  }

  async function changeLanguage(language: string) {
    await invoke("set_language", { language });
    setSettings((s) => (s ? { ...s, language } : s));
  }

  async function changeDevice(name: string) {
    // The backend answers with the device it actually resolved to, which
    // differs from the request if that microphone has been unplugged.
    const resolved = await invoke<string>("set_input_device", { device: name });
    setDevice(resolved);
    setSettings((s) => (s ? { ...s, input_device: name || null } : s));
  }

  async function downloadModel(file: string) {
    setError("");
    try {
      await invoke("download_model", { file });
    } catch (e) {
      setError(String(e));
    }
  }

  async function selectModel(file: string) {
    setError("");
    try {
      await invoke("select_model", { file });
      setModels((prev) => prev.map((m) => ({ ...m, active: m.file === file })));
      setSettings((s) => (s ? { ...s, model_file: file } : s));
    } catch (e) {
      setError(String(e));
    }
  }

  async function toggleDebug(enabled: boolean) {
    await invoke("set_debug_wav", { enabled });
    setSettings((s) => (s ? { ...s, save_debug_wav: enabled } : s));
  }

  const busy = status.state === "recording" || status.state === "transcribing";

  return (
    <main>
      <header>
        <h1>Parrot</h1>
        <span className={`pill pill-${status.state}`}>{status.state}</span>
      </header>

      <p className="detail">{status.detail}</p>

      <button
        className={busy ? "record recording" : "record"}
        onClick={() => invoke("toggle_recording")}
      >
        {status.state === "recording" ? "Stop and transcribe" : "Test recording"}
      </button>
      <p className="hint">
        Or hold <kbd>{settings?.shortcut ?? "..."}</kbd> anywhere and speak.
      </p>

      <section>
        <h2>Shortcut</h2>
        <div className="row">
          <input
            value={shortcutDraft}
            onKeyDown={captureShortcut}
            onChange={() => {}}
            placeholder="Click, then press keys"
            spellCheck={false}
          />
          <button onClick={saveShortcut} disabled={shortcutDraft === settings?.shortcut}>
            Save
          </button>
        </div>
        {error && <p className="error">{error}</p>}
      </section>

      <section>
        <h2>Language</h2>
        <select
          value={settings?.language ?? "en"}
          onChange={(e) => changeLanguage(e.target.value)}
        >
          {LANGUAGES.map(([code, label]) => (
            <option key={code} value={code}>
              {label}
            </option>
          ))}
        </select>
        <p className="hint">
          An <code>.en</code> model only speaks English; switch models to use
          another language.
        </p>
      </section>

      <section>
        <h2>Speech model</h2>
        <ul className="models">
          {models.map((m) => (
            <li key={m.file} className={m.active ? "model active" : "model"}>
              <div className="model-text">
                <strong>{m.label}</strong>
                {m.active && <span className="tag">in use</span>}
                <span className="model-note">
                  {m.note} &middot; {m.size_mb} MB
                </span>
              </div>
              {!m.installed ? (
                <button
                  onClick={() => downloadModel(m.file)}
                  disabled={progress !== null}
                >
                  Download
                </button>
              ) : m.active ? (
                <button disabled>Active</button>
              ) : (
                <button onClick={() => selectModel(m.file)}>Use</button>
              )}
            </li>
          ))}
        </ul>

        {progress && (
          <div className="progress">
            <div className="progress-bar">
              <div
                className="progress-fill"
                style={{ width: `${progress.percent}%` }}
              />
            </div>
            <span className="progress-label">
              {progress.file} &middot; {(progress.received / 1e6).toFixed(0)} /{" "}
              {(progress.total / 1e6).toFixed(0)} MB ({progress.percent}%)
            </span>
          </div>
        )}
      </section>

      <section>
        <h2>History</h2>
        {transcripts.length === 0 ? (
          <p className="hint">Nothing transcribed yet this session.</p>
        ) : (
          <ul className="history">
            {transcripts.map((t, i) => (
              <li key={i}>{t}</li>
            ))}
          </ul>
        )}
      </section>

      <section>
        <h2>Diagnostics</h2>
        <label className="field">
          <span>Microphone</span>
          <select
            value={settings?.input_device ?? ""}
            onChange={(e) => changeDevice(e.target.value)}
          >
            <option value="">System default</option>
            {devices.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <p className="hint">
          Currently using <strong>{device || "checking..."}</strong>. If
          recordings come back silent, the default device is the first thing to
          change.
        </p>

        <dl>
          <dt>Model file</dt>
          <dd className="path">{modelPath || "checking..."}</dd>
        </dl>
        <label className="check">
          <input
            type="checkbox"
            checked={settings?.save_debug_wav ?? false}
            onChange={(e) => toggleDebug(e.target.checked)}
          />
          Save each recording as <code>last-recording.wav</code>
        </label>
      </section>
    </main>
  );
}
