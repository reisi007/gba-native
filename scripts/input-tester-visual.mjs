#!/usr/bin/env node
// Visual comparison for the synthetic input-tester frame.
//
// The native harness writes a binary PPM. This script decodes it, renders the
// expected and actual pixels into two canvases in a real Chromium page, compares
// them pixel by pixel in the browser and writes a screenshot artifact for human
// review. It is a visual check, not a screenshot-baseline claim about fonts or
// GPU scaling: both sides are exact 240x160 RGB buffers.

import { readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');

function parseArgs(argv) {
  const args = { actual: null, expected: null, screenshot: null, png: null };
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index];
    if (!['--actual', '--expected', '--screenshot', '--png'].includes(key)) {
      throw new Error(`unknown argument ${key}`);
    }
    const value = argv[index + 1];
    if (!value) throw new Error(`${key} needs a path`);
    args[key.slice(2)] = value;
    index += 1;
  }
  if (!args.actual || !args.expected) {
    throw new Error('usage: input-tester-visual.mjs --actual A.ppm --expected E.ppm [--screenshot shot.png] [--png actual.png]');
  }
  return args;
}

export function readPpm(bytes) {
  const text = bytes.toString('latin1');
  const match = /^P6\s+(\d+)\s+(\d+)\s+(\d+)\s/.exec(text);
  if (!match) throw new Error('not a binary P6 PPM');
  const width = Number(match[1]);
  const height = Number(match[2]);
  const max = Number(match[3]);
  if (max !== 255) throw new Error(`unsupported PPM maximum ${max}`);
  const offset = Buffer.byteLength(match[0], 'latin1');
  const rgb = bytes.subarray(offset);
  if (rgb.length !== width * height * 3) {
    throw new Error(`PPM payload is ${rgb.length} bytes, expected ${width * height * 3}`);
  }
  return { width, height, rgb };
}

async function loadPlaywright() {
  const require = createRequire(import.meta.url);
  const modulePath = join(root, 'target/web-tools/npm/node_modules/@playwright/test');
  return require(modulePath);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const actual = readPpm(await readFile(args.actual));
  const expected = readPpm(await readFile(args.expected));
  if (actual.width !== expected.width || actual.height !== expected.height) {
    throw new Error(
      `frame size differs: actual ${actual.width}x${actual.height}, expected ${expected.width}x${expected.height}`,
    );
  }

  const { chromium } = await loadPlaywright();
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 560, height: 260 }, deviceScaleFactor: 1 });
    await page.setContent(`<!doctype html>
      <meta charset="utf-8">
      <style>
        body { margin: 0; background: #111; color: #eee; font: 12px monospace; }
        .row { display: flex; gap: 12px; padding: 12px; }
        figure { margin: 0; }
        canvas { display: block; width: 240px; height: 160px; image-rendering: pixelated; border: 1px solid #555; }
        figcaption { padding-top: 4px; }
        #result { padding: 0 12px 12px; }
      </style>
      <div class="row">
        <figure><canvas id="expected" width="240" height="160"></canvas><figcaption>expected (committed)</figcaption></figure>
        <figure><canvas id="actual" width="240" height="160"></canvas><figcaption>actual (native AOT)</figcaption></figure>
      </div>
      <div id="result">comparing…</div>`);

    const result = await page.evaluate(
      ({ actualB64, expectedB64, width, height }) => {
        const decode = (b64) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
        const draw = (id, data) => {
          const canvas = document.getElementById(id);
          const context = canvas.getContext('2d', { willReadFrequently: true });
          const image = context.createImageData(width, height);
          for (let pixel = 0; pixel < width * height; pixel += 1) {
            image.data[pixel * 4] = data[pixel * 3];
            image.data[pixel * 4 + 1] = data[pixel * 3 + 1];
            image.data[pixel * 4 + 2] = data[pixel * 3 + 2];
            image.data[pixel * 4 + 3] = 255;
          }
          context.putImageData(image, 0, 0);
          return context.getImageData(0, 0, width, height).data;
        };
        const a = draw('actual', decode(actualB64));
        const e = draw('expected', decode(expectedB64));
        let differences = 0;
        let first = null;
        for (let pixel = 0; pixel < width * height; pixel += 1) {
          const offset = pixel * 4;
          if (a[offset] !== e[offset] || a[offset + 1] !== e[offset + 1] || a[offset + 2] !== e[offset + 2]) {
            differences += 1;
            if (first === null) {
              first = { x: pixel % width, y: Math.floor(pixel / width) };
            }
          }
        }
        document.getElementById('result').textContent =
          differences === 0
            ? `pixels_match=true; compared=${width * height}`
            : `pixels_match=false; differences=${differences}; first=${JSON.stringify(first)}`;
        return { differences, first, compared: width * height };
      },
      {
        actualB64: Buffer.from(actual.rgb).toString('base64'),
        expectedB64: Buffer.from(expected.rgb).toString('base64'),
        width: actual.width,
        height: actual.height,
      },
    );

    if (args.screenshot) {
      await page.screenshot({ path: args.screenshot, fullPage: true });
    }
    if (args.png) {
      const dataUrl = await page.evaluate(() => document.getElementById('actual').toDataURL('image/png'));
      await writeFile(args.png, Buffer.from(dataUrl.split(',')[1], 'base64'));
    }
    console.log(`visual_compared=${result.compared}`);
    console.log(`visual_differences=${result.differences}`);
    console.log(`visual_pixels_match=${result.differences === 0}`);
    if (result.first) console.log(`visual_first_difference=${JSON.stringify(result.first)}`);
    if (result.differences !== 0) {
      throw new Error(`actual frame differs from the expected frame in ${result.differences} pixels`);
    }
  } finally {
    await browser.close();
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  await main();
}
