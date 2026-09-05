import { useEffect, useState } from "react";
import type { KeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Settings = {
  shortcut: string;
  language: string;
  model_file: string;
  save_debug_wav: boolean;
};

type StatusEvent = {
  state: "loading" | "idle" | "recording" | "transcribing" | "error";
  detail: string;
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
  const [modelPath, setModelPath] = useState("");
  const [shortcutDraft, setShortcutDraft] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    invoke<Settings>("get_settings").then((s) => {
      setSettings(s);
      setShortcutDraft(s.shortcut);
    });
    invoke<string>("get_device_name").then(setDevice);
    invoke<string>("model_location").then(setModelPath);

    const unlistenStatus = listen<StatusEvent>("status", (e) =>
      setStatus(e.payload),
    );
    const unlistenText = listen<string>("transcript", (e) =>
      setTranscripts((prev) => [e.payload, ...prev].slice(0, 20)),
    );

    return () => {
      unlistenStatus.then((f) => f());
      unlistenText.then((f) => f());
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
        <dl>
          <dt>Microphone</dt>
          <dd>{device || "checking..."}</dd>
          <dt>Model</dt>
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
