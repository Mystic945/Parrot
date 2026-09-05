// Wraps the artifact-flavoured page into a standalone HTML document and stages
// the installer beside it.
//
// website/index.html is authored for the Artifact tool, which supplies its own
// <!doctype>, <html> and <head> at publish time — so the source file starts at
// <title> and would be served without a viewport meta tag if uploaded as-is,
// rendering at desktop width on phones. This produces website/dist/, which is
// what you hand to Netlify, GitHub Pages or any other static host.
//
//   npm run site
import {
  readFileSync,
  writeFileSync,
  mkdirSync,
  copyFileSync,
  existsSync,
  readdirSync,
  statSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const SRC = join(root, "website", "index.html");
const OUT_DIR = join(root, "website", "dist");
const OUT = join(OUT_DIR, "index.html");
const BUNDLE = join(root, "src-tauri", "target", "release", "bundle", "nsis");

const DESCRIPTION =
  "Offline push-to-talk dictation for Windows. Hold a key, speak, and the text " +
  "appears wherever your cursor is. Nothing leaves your machine.";

const source = readFileSync(SRC, "utf8");

// Everything up to the end of the stylesheet is head material; the markup that
// follows is the body.
const split = source.indexOf("</style>");
if (split === -1) {
  console.error("Could not find </style> in website/index.html — has the file changed shape?");
  process.exit(1);
}

const head = source.slice(0, split + "</style>".length).trim();
const body = source.slice(split + "</style>".length).trim();
const title = (source.match(/<title>([^<]*)<\/title>/) ?? [, "Parrot"])[1];

const page = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="description" content="${DESCRIPTION}">
<meta name="color-scheme" content="light dark">
<meta property="og:type" content="website">
<meta property="og:title" content="${title}">
<meta property="og:description" content="${DESCRIPTION}">
<meta name="twitter:card" content="summary_large_image">
<link rel="icon" href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><text y='.9em' font-size='90'>🦜</text></svg>">
${head}
</head>
<body>
${body}
</body>
</html>
`;

mkdirSync(OUT_DIR, { recursive: true });
writeFileSync(OUT, page, "utf8");

console.log(`wrote ${OUT}`);
console.log(`  page: ${(page.length / 1024).toFixed(1)} KB`);

// The installer is small enough to serve from the site itself, which saves
// setting up a separate release host just to hand over one 2 MB file.
const installers = existsSync(BUNDLE)
  ? readdirSync(BUNDLE).filter((f) => f.endsWith("-setup.exe"))
  : [];

if (installers.length === 0) {
  console.log("");
  console.log("  No installer found. Run `npm run tauri build` first,");
  console.log("  otherwise the download button will 404.");
} else {
  for (const file of installers) {
    copyFileSync(join(BUNDLE, file), join(OUT_DIR, file));
    const mb = (statSync(join(BUNDLE, file)).size / 1e6).toFixed(1);
    console.log(`  staged: ${file} (${mb} MB)`);
  }

  // Warn rather than quietly shipping a page whose button leads nowhere.
  const linked = installers.some((f) => page.includes(f));
  if (!linked) {
    console.log("");
    console.log(`  WARNING: the download button does not link to ${installers[0]}`);
    console.log("  Update the href in website/index.html.");
  }
}

console.log("");
console.log("Drag the website/dist folder onto your host to publish.");
