// Emits a 512x512 placeholder icon so `tauri icon` has something to chew on.
// Replace icon-source.png with real artwork whenever you have some; this exists
// only so a fresh clone can build without hunting for an image.
import { deflateSync, crc32 } from "node:zlib";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const SIZE = 512;
const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "icon-source.png");

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

// Rounded square with a soft vertical gradient and a microphone-ish glyph.
function pixel(x, y) {
  const r = 96; // corner radius
  const inCorner = (cx, cy) => (x - cx) ** 2 + (y - cy) ** 2 > r * r;
  const outside =
    (x < r && y < r && inCorner(r, r)) ||
    (x > SIZE - r && y < r && inCorner(SIZE - r, r)) ||
    (x < r && y > SIZE - r && inCorner(r, SIZE - r)) ||
    (x > SIZE - r && y > SIZE - r && inCorner(SIZE - r, SIZE - r));

  if (outside) return [0, 0, 0, 0];

  const t = y / SIZE;
  const bg = [
    Math.round(217 - 40 * t),
    Math.round(85 + 20 * t),
    Math.round(20 + 10 * t),
    255,
  ];

  // Capsule body of the mic.
  const cx = SIZE / 2;
  const capsuleTop = 150;
  const capsuleBottom = 300;
  const capsuleR = 52;
  const withinCapsule =
    Math.abs(x - cx) <= capsuleR &&
    y >= capsuleTop &&
    y <= capsuleBottom &&
    (y >= capsuleTop + capsuleR ||
      (x - cx) ** 2 + (y - capsuleTop - capsuleR) ** 2 <= capsuleR ** 2) &&
    (y <= capsuleBottom - capsuleR ||
      (x - cx) ** 2 + (y - capsuleBottom + capsuleR) ** 2 <= capsuleR ** 2);

  // Arc cradling the capsule, plus the stand.
  const d = Math.hypot(x - cx, y - 300);
  const withinArc = d > 92 && d < 112 && y >= 300;
  const withinStem = Math.abs(x - cx) <= 12 && y > 392 && y < 432;
  const withinBase = Math.abs(x - cx) <= 78 && y >= 432 && y <= 452;

  if (withinCapsule || withinArc || withinStem || withinBase) {
    return [255, 250, 245, 255];
  }
  return bg;
}

const raw = Buffer.alloc(SIZE * (SIZE * 4 + 1));
let offset = 0;
for (let y = 0; y < SIZE; y++) {
  raw[offset++] = 0; // filter type: none
  for (let x = 0; x < SIZE; x++) {
    const [r, g, b, a] = pixel(x, y);
    raw[offset++] = r;
    raw[offset++] = g;
    raw[offset++] = b;
    raw[offset++] = a;
  }
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // colour type: RGBA
// bytes 10-12: compression, filter, interlace — all zero

writeFileSync(
  OUT,
  Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]),
);

console.log(`wrote ${OUT}`);
