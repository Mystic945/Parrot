// Fetches a ggml whisper model into the same directory the Rust side reads from.
//
//   npm run model                 -> base.en (142 MB, the sane default)
//   npm run model -- small.en     -> better accuracy, ~3x slower
//   npm run model -- tiny.en      -> fastest, noticeably worse
//
// Keep this in sync with `model_path` in src-tauri/src/transcribe.rs.
import { createWriteStream, existsSync, mkdirSync, statSync } from "node:fs";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { homedir, platform } from "node:os";
import { join } from "node:path";

const IDENTIFIER = "com.example.parrot";
const BASE_URL = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

const KNOWN = {
  "tiny.en": 75,
  "base.en": 142,
  "small.en": 466,
  "medium.en": 1500,
  base: 142,
  small: 466,
  "large-v3-turbo": 1600,
};

function appDataDir() {
  if (process.env.PARROT_MODEL_DIR) return process.env.PARROT_MODEL_DIR;

  const home = homedir();
  switch (platform()) {
    case "win32":
      return join(process.env.APPDATA ?? join(home, "AppData", "Roaming"), IDENTIFIER, "models");
    case "darwin":
      return join(home, "Library", "Application Support", IDENTIFIER, "models");
    default:
      return join(
        process.env.XDG_DATA_HOME ?? join(home, ".local", "share"),
        IDENTIFIER,
        "models",
      );
  }
}

const name = process.argv[2] ?? "base.en";
if (!(name in KNOWN)) {
  console.error(`Unknown model "${name}". Options: ${Object.keys(KNOWN).join(", ")}`);
  process.exit(1);
}

const file = `ggml-${name}.bin`;
const dir = appDataDir();
const dest = join(dir, file);

mkdirSync(dir, { recursive: true });

if (existsSync(dest)) {
  const mb = (statSync(dest).size / 1e6).toFixed(0);
  console.log(`${file} already present (${mb} MB) at\n  ${dest}`);
  process.exit(0);
}

console.log(`Downloading ${file} (~${KNOWN[name]} MB) to\n  ${dest}\n`);

const response = await fetch(`${BASE_URL}/${file}`);
if (!response.ok || !response.body) {
  console.error(`Download failed: HTTP ${response.status}`);
  process.exit(1);
}

const total = Number(response.headers.get("content-length")) || 0;
let seen = 0;
let lastPrint = 0;

const source = Readable.fromWeb(response.body);
source.on("data", (buf) => {
  seen += buf.length;
  const now = Date.now();
  if (now - lastPrint < 250) return;
  lastPrint = now;
  const mb = (seen / 1e6).toFixed(1);
  const pct = total ? ` (${((seen / total) * 100).toFixed(0)}%)` : "";
  process.stdout.write(`\r  ${mb} MB${pct}`);
});

// Write to a temp name so an interrupted download is never mistaken for a
// complete model - whisper.cpp fails confusingly on a truncated file.
const temp = `${dest}.partial`;
await pipeline(source, createWriteStream(temp));
const { renameSync } = await import("node:fs");
renameSync(temp, dest);

console.log(`\n\nDone. Restart the app to load it.`);
