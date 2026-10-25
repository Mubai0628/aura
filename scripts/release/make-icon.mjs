// Aura's application icon from its logo (resources/app-icon/aura-logo.png, black on white): the logo
// is cropped to a square around the mark with an even white margin and scaled down to the sizes
// Windows uses, written as one .ico with PNG images, plus a 256 px PNG to look at. Run with
// Electron:
//   apps\desktop\node_modules\electron\dist\electron.exe scripts\release\make-icon.mjs
import { app, nativeImage } from 'electron';
import { writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const REPOSITORY = resolve(import.meta.dirname, '../..');
const SOURCE = join(REPOSITORY, 'resources/app-icon/aura-logo.png');
const OUT = join(REPOSITORY, 'resources/app-icon/aura.ico');
const SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];
/** White space around the mark, as a share of the square's side. */
const MARGIN = 0.08;

/** The smallest rectangle holding every dark pixel of a BGRA bitmap. */
function markBounds(bitmap, width, height) {
  let left = width;
  let top = height;
  let right = -1;
  let bottom = -1;
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const at = (y * width + x) * 4;
      const luminance = 0.0722 * bitmap[at] + 0.7152 * bitmap[at + 1] + 0.2126 * bitmap[at + 2];
      if (luminance < 128) {
        left = Math.min(left, x);
        right = Math.max(right, x);
        top = Math.min(top, y);
        bottom = Math.max(bottom, y);
      }
    }
  }
  if (right < 0) throw new Error('the logo has no dark mark');
  return { left, top, right, bottom };
}

function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(images.length, 4);
  let offset = 6 + 16 * images.length;
  const entries = images.map(({ size, data }) => {
    const entry = Buffer.alloc(16);
    entry.writeUInt8(size >= 256 ? 0 : size, 0);
    entry.writeUInt8(size >= 256 ? 0 : size, 1);
    entry.writeUInt16LE(1, 4);
    entry.writeUInt16LE(32, 6);
    entry.writeUInt32LE(data.length, 8);
    entry.writeUInt32LE(offset, 12);
    offset += data.length;
    return entry;
  });
  return Buffer.concat([header, ...entries, ...images.map((image) => image.data)]);
}

app.whenReady().then(() => {
  const logo = nativeImage.createFromPath(SOURCE);
  const { width, height } = logo.getSize();
  const bounds = markBounds(logo.toBitmap(), width, height);
  const markSide = Math.max(bounds.right - bounds.left + 1, bounds.bottom - bounds.top + 1);
  const side = Math.min(Math.round(markSide / (1 - 2 * MARGIN)), width, height);
  const centreX = (bounds.left + bounds.right + 1) / 2;
  const centreY = (bounds.top + bounds.bottom + 1) / 2;
  const x = Math.round(Math.min(Math.max(centreX - side / 2, 0), width - side));
  const y = Math.round(Math.min(Math.max(centreY - side / 2, 0), height - side));
  const square = logo.crop({ x, y, width: side, height: side });
  const images = SIZES.map((size) => ({
    size,
    data: square.resize({ width: size, height: size, quality: 'best' }).toPNG(),
  }));
  writeFileSync(OUT, ico(images));
  writeFileSync(OUT.replace(/\.ico$/u, '-256.png'), images[images.length - 1].data);
  console.log(`${OUT}: ${SIZES.join(', ')} px from a ${side} px square at ${x},${y}`);
  app.quit();
});
