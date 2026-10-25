// Functional E2E: real WASM execution and real browser GPU presentation.
//
// Evidence rules:
// - The source SHA-256 and 230,411-instruction budget must equal the recorded
//   native values; the image is sampled from a real Playwright PNG of the real
//   canvas, not from a status counter.
// - Adapter/backend values are observed and printed; a missing WebGPU backend
//   fails the forced-WebGPU test instead of skipping it.
// - Every request must stay on the local test origin; console errors and page
//   errors fail the test.
// - Loss/restart is triggered through the real WebGL context-loss extension.

import { expect, test as base } from '@playwright/test';
import type { Page, TestInfo } from '@playwright/test';
import { writeFile } from 'node:fs/promises';

const SOURCE_SHA256 = 'b02968d620ee66d7175c29bbde12bc9ff075290f3e8086b26e9d4829b9ccb154';
const GUEST_BUDGET = 230_411;
const GOLDEN = [
  { guest: [120, 80], rgb: [0xc6, 0xde, 0x94] },
  { guest: [60, 40], rgb: [0xe7, 0x6b, 0x4a] },
  { guest: [180, 120], rgb: [0xa5, 0x4a, 0xe7] },
  // Independently expanded RGB555 for guest pixel (200,100), index 24200.
  { guest: [200, 100], rgb: [0x42, 0xa5, 0xbd] },
  { guest: [239, 159], rgb: [0xff, 0x7b, 0x29] },
];
// Chromium colour management can shift a value slightly; the observed local
// difference was zero. Keep a small, documented tolerance instead of exactness.
const COLOR_TOLERANCE = 6;
const VISIBLE_PIXEL_POLL_WINDOW_MS = 2_500;
const VISIBLE_PIXEL_POLL_MS = 50;
// Applies to the complete retry/capture/PNG-decode/artifact-write step. This is
// Playwright's timeout budget; timeout scheduling and teardown can add overhead.
const VISIBLE_PIXEL_SAMPLE_BUDGET_MS = 10_000;
const CANVAS_VISIBILITY_TIMEOUT_MS = 500;
const SCREENSHOT_TIMEOUT_MS = 5_000;
const WEBGPU_CONFIRMATION_TIMEOUT_MS = 1_000;
const WEBGPU_STABILITY_STEP_TIMEOUT_MS = 3_000;
const WEBGPU_STABILITY_FRAMES = 8;
const DISCLAIMER = 'Experimental synthetic GPU demo; no playable ROM, saves or game input yet.';

type Fixtures = {
  problems: string[];
  requests: string[];
};

const test = base.extend<Fixtures>({
  problems: async ({ page }, use) => {
    const problems: string[] = [];
    page.on('console', (message) => {
      if (message.type() === 'error') problems.push(`console: ${message.text()}`);
    });
    page.on('pageerror', (error) => problems.push(`pageerror: ${error.message}`));
    await use(problems);
    expect(problems, `console/page errors: ${problems.join('\n')}`).toEqual([]);
  },
  requests: async ({ page, baseURL }, use) => {
    const requests: string[] = [];
    page.on('request', (request) => requests.push(request.url()));
    await use(requests);
    const foreign = requests.filter((url) => !url.startsWith(baseURL!));
    expect(foreign, `unexpected external request(s): ${foreign.join(', ')}`).toEqual([]);
  },
});

type Status = {
  state: string;
  message: string;
  requested: string;
  actual?: string;
  autoFallback: boolean;
  sourceSha256?: string;
  guestBudget?: number;
  imageWidth?: number;
  imageHeight?: number;
  adapterName?: string;
  backend?: string;
  deviceType?: string;
  surfaceFormat?: string;
  renderFormat?: string;
  viewportX?: number;
  viewportY?: number;
  viewportWidth?: number;
  viewportHeight?: number;
  viewportScale?: number;
  viewportClipped?: boolean;
  presented?: number;
  detail?: string;
};

async function waitForState(page: Page, state: string, timeout = 90_000): Promise<void> {
  await page.waitForFunction(
    (wanted) => document.body.dataset.appState === wanted,
    state,
    { timeout },
  );
}

async function statusOf(page: Page): Promise<Status> {
  return page.evaluate(() => (window as unknown as { __gbaNative: { status: () => Status } }).__gbaNative.status());
}

async function openApp(page: Page, query: string): Promise<void> {
  await page.goto(`/?${query}`, { waitUntil: 'load' });
}

async function expectReady(page: Page): Promise<Status> {
  await waitForState(page, 'ready');
  await page.waitForFunction(
    () => {
      const status = (window as unknown as { __gbaNative: { status: () => Status } }).__gbaNative.status();
      return status?.state === 'ready' && (status.presented ?? 0) >= 1;
    },
    undefined,
    { timeout: 5_000 },
  );
  const status = await statusOf(page);
  expect(status.state).toBe('ready');
  expect(status.sourceSha256).toBe(SOURCE_SHA256);
  expect(status.guestBudget).toBe(GUEST_BUDGET);
  expect(status.imageWidth).toBe(240);
  expect(status.imageHeight).toBe(160);
  expect(status.presented ?? 0).toBeGreaterThanOrEqual(1);
  return status;
}

function assertViewport(status: Status, canvasWidth: number, canvasHeight: number): void {
  const scale = Math.max(1, Math.min(Math.floor(canvasWidth / 240), Math.floor(canvasHeight / 160)));
  expect(status.viewportScale).toBe(scale);
  expect(status.viewportWidth).toBe(Math.min(240 * scale, canvasWidth));
  expect(status.viewportHeight).toBe(Math.min(160 * scale, canvasHeight));
  expect(status.viewportX).toBe(Math.floor((canvasWidth - Math.min(240 * scale, canvasWidth)) / 2));
  expect(status.viewportY).toBe(Math.floor((canvasHeight - Math.min(160 * scale, canvasHeight)) / 2));
  expect(status.viewportClipped).toBe(false);
}

function expectPhysicalBacking(
  width: number,
  height: number,
  cssWidth: number,
  cssHeight: number,
  dpr: number,
): void {
  // CSS geometry can be fractional; the backing store rounds to the nearest
  // device pixel, with at most one-pixel observer/event readback tolerance.
  expect(Math.abs(width - Math.round(cssWidth * dpr))).toBeLessThanOrEqual(1);
  expect(Math.abs(height - Math.round(cssHeight * dpr))).toBeLessThanOrEqual(1);
}

async function badgeContrast(page: Page, selector: string): Promise<number> {
  return page.locator(selector).evaluate((element) => {
    const style = getComputedStyle(element);
    const canvas = document.createElement('canvas');
    canvas.width = 1;
    canvas.height = 1;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('2D context unavailable for contrast measurement');
    const rgba = (value: string): number[] => {
      context.clearRect(0, 0, 1, 1);
      context.fillStyle = value;
      context.fillRect(0, 0, 1, 1);
      return [...context.getImageData(0, 0, 1, 1).data];
    };
    const foreground = rgba(style.color);
    const ownBackground = rgba(style.backgroundColor);
    const themeSurface = getComputedStyle(document.documentElement)
      .getPropertyValue('--color-base-100')
      .trim();
    const background = ownBackground[3] === 0 ? rgba(themeSurface) : ownBackground;
    const alpha = foreground[3] / 255;
    const composited = foreground.slice(0, 3).map((value, index) =>
      value * alpha + background[index] * (1 - alpha),
    );
    const luminance = (channels: number[]): number => {
      const [red, green, blue] = channels.map((value) => {
        const channel = value / 255;
        return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    };
    const first = luminance(composited);
    const second = luminance(background.slice(0, 3));
    return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
  });
}

type Samples = {
  png: [number, number];
  canvas: [number, number];
  capture: Uint8Array;
  corner: [number, number, number];
  guest: Array<[number, number, number]>;
  guestNeighborhoods: Array<Array<{ dx: number; dy: number; rgb: [number, number, number] }>>;
  nearestGolden: { x: number; y: number; rgb: [number, number, number]; maxError: number };
};

async function sampleRenderedCanvas(page: Page, status: Status): Promise<Samples> {
  const canvas = page.locator('#gba-canvas');
  await expect(canvas).toBeVisible({ timeout: CANVAS_VISIBILITY_TIMEOUT_MS });
  const png = await canvas.screenshot({ timeout: SCREENSHOT_TIMEOUT_MS });
  const metrics = await canvas.evaluate((element: HTMLCanvasElement) => ({
    width: element.width,
    height: element.height,
  }));
  const samples = await page.evaluate(
    async ({ base64, status, canvasWidth, canvasHeight, guestPoints, golden }) => {
      const bytes = Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));
      const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
      const offscreen = new OffscreenCanvas(bitmap.width, bitmap.height);
      const context = offscreen.getContext('2d');
      if (!context) throw new Error('2D context unavailable for screenshot decoding');
      context.drawImage(bitmap, 0, 0);
      const data = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
      const at = (x: number, y: number): [number, number, number] => {
        const index = (Math.floor(y) * bitmap.width + Math.floor(x)) * 4;
        return [data[index], data[index + 1], data[index + 2]];
      };
      const scaleX = bitmap.width / canvasWidth;
      const scaleY = bitmap.height / canvasHeight;
      const guestLocation = (guestX: number, guestY: number): [number, number] => [
        (status.viewportX! + (guestX + 0.5) * status.viewportScale!) * scaleX,
        (status.viewportY! + (guestY + 0.5) * status.viewportScale!) * scaleY,
      ];
      const guestPixel = (guestX: number, guestY: number): [number, number, number] => {
        const [x, y] = guestLocation(guestX, guestY);
        return at(x, y);
      };
      const guestNeighborhood = (guestX: number, guestY: number) => {
        const [x, y] = guestLocation(guestX, guestY);
        const centerX = Math.floor(x);
        const centerY = Math.floor(y);
        return [-1, 0, 1].flatMap((dy) =>
          [-1, 0, 1].map((dx) => ({ dx, dy, rgb: at(centerX + dx, centerY + dy) })),
        );
      };
      let nearestGolden = { x: 0, y: 0, rgb: [0, 0, 0] as [number, number, number], maxError: 255 };
      for (let y = 0; y < bitmap.height; y += 1) {
        for (let x = 0; x < bitmap.width; x += 1) {
          const rgb = at(x, y);
          const maxError = Math.max(...rgb.map((value, index) => Math.abs(value - golden[index])));
          if (maxError < nearestGolden.maxError) nearestGolden = { x, y, rgb, maxError };
        }
      }
      return {
        png: [bitmap.width, bitmap.height] as [number, number],
        canvas: [canvasWidth, canvasHeight] as [number, number],
        // 4 CSS px inset: the rounded slot clips the true outermost corner and
        // an element screenshot can include that antialiased clip edge.
        corner: at(8, 8),
        guest: guestPoints.map(([x, y]) => guestPixel(x, y)),
        guestNeighborhoods: guestPoints.map(([x, y]) => guestNeighborhood(x, y)),
        nearestGolden,
      };
    },
    {
      base64: png.toString('base64'),
      status,
      canvasWidth: metrics.width,
      canvasHeight: metrics.height,
      guestPoints: GOLDEN.map(({ guest }) => guest),
      golden: GOLDEN[0].rgb,
    },
  );
  return { ...samples, capture: png };
}

function close(a: [number, number, number], b: number[]): boolean {
  return a.every((value, index) => Math.abs(value - b[index]) <= COLOR_TOLERANCE);
}

async function waitForVisibleCanvas(
  page: Page,
  status: Status,
  testInfo: TestInfo,
  description: string,
  matches: (samples: Samples) => boolean,
): Promise<Samples> {
  return test.step(
    `sample real canvas pixels: ${description}`,
    async () => {
      const started = Date.now();
      const deadline = started + VISIBLE_PIXEL_POLL_WINDOW_MS;
      let attempts = 0;
      let latest: Samples | undefined;
      while (Date.now() <= deadline) {
        attempts += 1;
        latest = await sampleRenderedCanvas(page, status);
        if (matches(latest)) {
          const elapsed = Date.now() - started;
          await writeFile(testInfo.outputPath('presented-canvas.png'), latest.capture);
          console.log(
            `observed real canvas pixels (${description}) after ${elapsed}ms/${attempts} screenshot(s): ${JSON.stringify({ actual: status.actual, backend: status.backend, surface: status.surfaceFormat, guest: latest.guest })}`,
          );
          return latest;
        }
        if (Date.now() >= deadline) break;
        await page.waitForTimeout(Math.min(VISIBLE_PIXEL_POLL_MS, deadline - Date.now()));
      }

      if (latest) {
        await writeFile(testInfo.outputPath('last-canvas-observation.png'), latest.capture);
      }
      const elapsed = Date.now() - started;
      throw new Error(
        `real canvas pixels were not observed in the ${VISIBLE_PIXEL_POLL_WINDOW_MS}ms ` +
          `polling window (${attempts} screenshot(s)); the complete visibility, screenshot, PNG decode, ` +
          `pixel evaluation, retry, and artifact-write operation has a ` +
          `${VISIBLE_PIXEL_SAMPLE_BUDGET_MS}ms Playwright step timeout budget ` +
          `(plus timeout/teardown scheduling overhead): ` +
          JSON.stringify({
            description,
            elapsed,
            requested: status.requested,
            actual: status.actual,
            backend: status.backend,
            surface: status.surfaceFormat,
            sourceSha256: status.sourceSha256,
            guestBudget: status.guestBudget,
            lastCanvas: latest?.canvas,
            lastPng: latest?.png,
            lastCorner: latest?.corner,
            lastGuest: latest?.guest,
            nearestGolden: latest?.nearestGolden,
          }),
      );
    },
    { timeout: VISIBLE_PIXEL_SAMPLE_BUDGET_MS },
  );
}

function allGoldenPixelsVisible(samples: Samples): boolean {
  return GOLDEN.every((entry, index) => close(samples.guest[index], entry.rgb));
}

async function expectWebGpuConfirmationRedraw(page: Page, status: Status): Promise<void> {
  if (status.actual !== 'webgpu') return;
  await test.step(
    'WebGPU confirmation redraw settles without a redraw loop or resize',
    async () => {
      await page.waitForFunction(
        () => {
          const current = (window as unknown as { __gbaNative: { status: () => Status } }).__gbaNative.status();
          return current?.actual === 'webgpu' && (current.presented ?? 0) >= 2;
        },
        undefined,
        { timeout: WEBGPU_CONFIRMATION_TIMEOUT_MS },
      );
      const stability = await page.evaluate(async (stableFrames) => {
        const canvas = document.querySelector<HTMLCanvasElement>('#gba-canvas');
        if (!canvas) throw new Error('WebGPU canvas disappeared during redraw stability check');
        let windowResizeEvents = 0;
        let canvasResizeEvents = 0;
        const onWindowResize = () => { windowResizeEvents += 1; };
        const resizeObserver = new ResizeObserver(() => { canvasResizeEvents += 1; });
        window.addEventListener('resize', onWindowResize);
        resizeObserver.observe(canvas);
        const geometry = () => {
          const rect = canvas.getBoundingClientRect();
          return [rect.x, rect.y, rect.width, rect.height, canvas.width, canvas.height];
        };
        const equalGeometry = (left: number[], right: number[]) =>
          left.every((value, index) => value === right[index]);
        const nextFrame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
        let previous = geometry();
        let stableLayoutFrames = 0;
        for (let frame = 0; frame < 30 && stableLayoutFrames < 3; frame += 1) {
          await nextFrame();
          const current = geometry();
          stableLayoutFrames = equalGeometry(current, previous) ? stableLayoutFrames + 1 : 0;
          previous = current;
        }
        // Let any ResizeObserver notification for the settled layout run before
        // taking the no-resize baseline for the following animation frames.
        await nextFrame();
        const baselineGeometry = geometry();
        const baselineStatus = (window as unknown as { __gbaNative: { status: () => Status } }).__gbaNative.status();
        const baselinePresented = baselineStatus.presented ?? 0;
        const baselineWindowResizeEvents = windowResizeEvents;
        const baselineCanvasResizeEvents = canvasResizeEvents;
        let stablePresentationFrames = 0;
        for (let frame = 0; frame < stableFrames; frame += 1) {
          await nextFrame();
          const currentStatus = (window as unknown as { __gbaNative: { status: () => Status } }).__gbaNative.status();
          const noResize =
            equalGeometry(geometry(), baselineGeometry) &&
            windowResizeEvents === baselineWindowResizeEvents &&
            canvasResizeEvents === baselineCanvasResizeEvents;
          if ((currentStatus.presented ?? 0) === baselinePresented && noResize) {
            stablePresentationFrames += 1;
          }
        }
        const finalStatus = (window as unknown as { __gbaNative: { status: () => Status } }).__gbaNative.status();
        resizeObserver.disconnect();
        window.removeEventListener('resize', onWindowResize);
        return {
          layoutStable: stableLayoutFrames >= 3,
          baselinePresented,
          finalPresented: finalStatus.presented ?? 0,
          stablePresentationFrames,
          windowResizeEvents: windowResizeEvents - baselineWindowResizeEvents,
          canvasResizeEvents: canvasResizeEvents - baselineCanvasResizeEvents,
        };
      }, WEBGPU_STABILITY_FRAMES);
      expect(stability.layoutStable, 'canvas geometry must settle before stability sampling').toBe(true);
      expect(stability.baselinePresented, 'the one confirmation redraw must be acknowledged').toBeGreaterThanOrEqual(2);
      expect(stability.finalPresented, 'presentation count must stop after confirmation').toBe(stability.baselinePresented);
      expect(stability.stablePresentationFrames, 'all observed animation frames must stay at the settled count').toBe(
        WEBGPU_STABILITY_FRAMES,
      );
      expect(stability.windowResizeEvents, 'window resize events during the stability window').toBe(0);
      expect(stability.canvasResizeEvents, 'canvas resize observations during the stability window').toBe(0);
      console.log(`WebGPU redraw settled across ${WEBGPU_STABILITY_FRAMES} animation frames: ${JSON.stringify(stability)}`);
    },
    { timeout: WEBGPU_STABILITY_STEP_TIMEOUT_MS },
  );
}

test('empty state is explicit, not blank', async ({ page }) => {
  await openApp(page, '');
  await waitForState(page, 'empty');
  await expect(page.getByRole('heading', { name: 'gba-native · browser GPU foundation' })).toBeVisible();
  await expect(page.getByText(DISCLAIMER)).toBeVisible();
  const start = page.getByRole('button', { name: 'Start synthetic demo' });
  await expect(start).toBeVisible();
  await expect(start).toBeEnabled();
  await expect(page.locator('#overlay-title')).toHaveText('No frame displayed yet');
  await expect(page.locator('#gba-canvas')).toBeHidden();
  const status = await statusOf(page);
  expect(status).toBeNull();
  const report = page.locator('#report');
  await expect(report).toBeEmpty();
});

test('semantic badges meet WCAG AA text contrast on desktop and portrait states', async ({ browser }) => {
  for (const viewport of [
    { width: 1280, height: 900, deviceScaleFactor: 1, isMobile: false },
    { width: 390, height: 844, deviceScaleFactor: 2, isMobile: true },
  ]) {
    const context = await browser.newContext({ ...viewport, hasTouch: viewport.isMobile });
    const page = await context.newPage();
    for (const state of [
      { query: '', wanted: 'empty' },
      { query: 'autostart=1&backend=webgl2', wanted: 'ready' },
      { query: 'autostart=1&backend=canvas2d', wanted: 'error' },
    ]) {
      await openApp(page, state.query);
      await waitForState(page, state.wanted);
      const experimental = await badgeContrast(page, 'header .badge-warning');
      const current = await badgeContrast(page, '#state-badge');
      console.log(
        `WCAG badge contrast ${viewport.width}x${viewport.height} ${state.wanted}: experimental=${experimental.toFixed(2)}:1 state=${current.toFixed(2)}:1`,
      );
      expect(experimental, `${state.wanted} experimental badge contrast`).toBeGreaterThanOrEqual(4.5);
      expect(current, `${state.wanted} state badge contrast`).toBeGreaterThanOrEqual(4.5);
    }
    await context.close();
  }
});

test('auto start executes the fixture once and presents real canvas pixels', async ({ page }, testInfo) => {
  await openApp(page, 'autostart=1&backend=auto');
  const status = await expectReady(page);
  expect(['webgpu', 'webgl2']).toContain(status.actual);
  expect(['BrowserWebGpu', 'Gl']).toContain(status.backend);
  expect(status.autoFallback).toBe(status.actual === 'webgl2');
  console.log(`observed auto backend: ${JSON.stringify({ actual: status.actual, backend: status.backend, adapter: status.adapterName, deviceType: status.deviceType })}`);

  const metrics = await page.locator('#gba-canvas').evaluate((element: HTMLCanvasElement) => ({
    width: element.width,
    height: element.height,
    cssWidth: element.getBoundingClientRect().width,
    cssHeight: element.getBoundingClientRect().height,
    dpr: window.devicePixelRatio,
  }));
  expect(metrics.width).toBeGreaterThan(0);
  expectPhysicalBacking(metrics.width, metrics.height, metrics.cssWidth, metrics.cssHeight, metrics.dpr);
  assertViewport(status, metrics.width, metrics.height);

  const samples = await waitForVisibleCanvas(
    page,
    status,
    testInfo,
    'auto backend golden samples',
    allGoldenPixelsVisible,
  );
  await expectWebGpuConfirmationRedraw(page, status);
  expect(samples.corner).toEqual([0, 0, 0]);
  GOLDEN.forEach((entry, index) => {
    expect(close(samples.guest[index], entry.rgb), `guest ${entry.guest.join(',')} got ${samples.guest[index]}`).toBe(true);
  });
  await expect(page.locator('#present-badge')).toContainText('render acknowledgement');
  await expect(page.locator('#source-line')).toContainText(SOURCE_SHA256);
  await expect(page.locator('#source-line')).toContainText(String(GUEST_BUDGET));
});

test('forced WebGL2 reports the GL adapter and presents pixels', async ({ page }, testInfo) => {
  await openApp(page, 'autostart=1&backend=webgl2');
  const status = await expectReady(page);
  expect(status.actual).toBe('webgl2');
  expect(status.backend).toBe('Gl');
  expect(status.autoFallback).toBe(false);
  console.log(`observed forced WebGL2: ${JSON.stringify({ backend: status.backend, adapter: status.adapterName, deviceType: status.deviceType, surface: status.surfaceFormat })}`);
  const samples = await waitForVisibleCanvas(
    page,
    status,
    testInfo,
    'forced WebGL2 golden samples',
    allGoldenPixelsVisible,
  );
  GOLDEN.forEach((entry, index) => expect(close(samples.guest[index], entry.rgb)).toBe(true));
});

test('forced WebGPU reports the browser WebGPU adapter and presents pixels', async ({ page }, testInfo) => {
  await openApp(page, 'autostart=1&backend=webgpu');
  const status = await expectReady(page);
  expect(status.actual).toBe('webgpu');
  expect(status.backend).toBe('BrowserWebGpu');
  expect(status.autoFallback).toBe(false);
  console.log(`observed forced WebGPU: ${JSON.stringify({ backend: status.backend, adapter: status.adapterName, deviceType: status.deviceType, surface: status.surfaceFormat })}`);
  const samples = await waitForVisibleCanvas(
    page,
    status,
    testInfo,
    'forced WebGPU golden samples',
    allGoldenPixelsVisible,
  );
  await expectWebGpuConfirmationRedraw(page, status);
  GOLDEN.forEach((entry, index) => expect(close(samples.guest[index], entry.rgb)).toBe(true));
});

test('permanently blank or hidden canvas fails within the sampler bounds', async ({ page }, testInfo) => {
  const status: Status = {
    state: 'ready',
    message: 'synthetic blank-canvas regression fixture',
    requested: 'webgl2',
    actual: 'webgl2',
    autoFallback: false,
    viewportX: 0,
    viewportY: 0,
    viewportWidth: 240,
    viewportHeight: 160,
    viewportScale: 1,
  };
  const description = 'blank-canvas timeout regression';

  await page.setContent('<canvas id="gba-canvas" width="240" height="160"></canvas>');
  const blankStarted = Date.now();
  let blankFailure = '';
  try {
    await waitForVisibleCanvas(page, status, testInfo, description, (samples) =>
      close(samples.guest[0], GOLDEN[0].rgb),
    );
  } catch (error) {
    blankFailure = String(error);
  }
  const blankElapsed = Date.now() - blankStarted;
  if (!blankFailure.includes('real canvas pixels were not observed')) {
    throw new Error(`blank canvas did not reach the expected bounded poll failure: ${blankFailure}`);
  }
  expect(blankElapsed, 'a permanently black canvas must fail promptly').toBeLessThan(
    VISIBLE_PIXEL_POLL_WINDOW_MS + 2_500,
  );

  await page.setContent('<canvas id="gba-canvas" width="240" height="160" hidden></canvas>');
  const hiddenStarted = Date.now();
  let hiddenFailure = '';
  try {
    await waitForVisibleCanvas(page, status, testInfo, description, () => false);
  } catch (error) {
    hiddenFailure = String(error);
  }
  const hiddenElapsed = Date.now() - hiddenStarted;
  if (!hiddenFailure.includes(`with timeout ${CANVAS_VISIBILITY_TIMEOUT_MS}ms`)) {
    throw new Error(`hidden canvas did not reach the expected visibility timeout: ${hiddenFailure}`);
  }
  expect(hiddenElapsed, 'a permanently hidden canvas must not inherit the global expect timeout').toBeLessThan(2_000);
  console.log(`bounded blank-canvas failures: visible=${blankElapsed}ms hidden=${hiddenElapsed}ms`);
});

test('unsupported-WebGPU browser uses the explicit WebGL2 path and refuses forced WebGPU', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, 'gpu', { configurable: true, get: () => undefined });
  });
  await openApp(page, 'autostart=1&backend=auto');
  const status = await expectReady(page);
  expect(status.actual).toBe('webgl2');
  expect(status.autoFallback).toBe(true);
  await expect(page.locator('#report')).toContainText('Auto compatibility path');

  await openApp(page, 'autostart=1&backend=webgpu');
  await waitForState(page, 'error');
  const errorStatus = await statusOf(page);
  expect(errorStatus.message).toContain('WebGPU was requested explicitly');
  expect(errorStatus.detail).toContain('webgpu=false');
  expect(errorStatus.autoFallback).toBe(false);
  expect(page.url()).toContain('backend=webgpu');
  expect(page.url()).not.toContain('fallback=');
  await expect(page.locator('#screen-overlay')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Restart with selected backend' })).toBeEnabled();
});

test('invalid backend is a real configuration error, not a fake frame', async ({ page }) => {
  await openApp(page, 'autostart=1&backend=canvas2d');
  await waitForState(page, 'error');
  const status = await statusOf(page);
  expect(status.message).toContain('unknown backend');
  await expect(page.locator('#screen-overlay')).toBeVisible();
  await expect(page.locator('#gba-canvas')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Start synthetic demo' })).toBeHidden();
  const restart = page.getByRole('button', { name: 'Restart with selected backend' });
  await expect(restart).toBeVisible();
  // Restart from the error state is the real recovery control: it reloads with
  // the (normalised-Auto) selection and must reach a presented canvas.
  await restart.click();
  await page.waitForLoadState('load');
  const recovered = await expectReady(page);
  expect(recovered.requested).toBe('auto');
});

test('WebGL context loss halts rendering and restart creates a fresh ready canvas', async ({ page }) => {
  await openApp(page, 'autostart=1&backend=webgl2&scene=lose-context');
  await waitForState(page, 'lost');
  const lost = await statusOf(page);
  expect(lost.detail?.toLowerCase()).toContain('context lost');
  await expect(page.locator('#gba-canvas')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Restart with selected backend' })).toBeEnabled();

  await page.getByRole('button', { name: 'Restart with selected backend' }).click();
  await page.waitForLoadState('load');
  const restarted = await expectReady(page);
  expect(restarted.actual).toBe('webgl2');
  const stamps = await page.evaluate(() => JSON.parse(sessionStorage.getItem('gba-native-web-restarts') ?? '[]'));
  expect(stamps.length).toBe(1);
});

test('keyboard-only backend selection and start work', async ({ page }) => {
  await openApp(page, '');
  await waitForState(page, 'empty');
  const select = page.getByLabel('GPU backend');
  await select.focus();
  await page.keyboard.press('Alt+ArrowDown');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(select).toHaveValue('webgpu');
  await expect(page.locator('#backend-hint')).toContainText('applies when you press Start or Restart');
  const start = page.getByRole('button', { name: 'Start synthetic demo' });
  await start.focus();
  await page.keyboard.press('Enter');
  await waitForState(page, 'loading');
  const status = await expectReady(page);
  expect(status.requested).toBe('webgpu');
});

test('mobile portrait device-pixel-ratio letterbox stays integer and centred', async ({ browser }, testInfo) => {
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    deviceScaleFactor: 2,
    isMobile: true,
    hasTouch: true,
  });
  const page = await context.newPage();
  const problems: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') problems.push(message.text());
  });
  page.on('pageerror', (error) => problems.push(error.message));
  await openApp(page, 'autostart=1&backend=webgl2');
  const status = await expectReady(page);
  const metrics = await page.locator('#gba-canvas').evaluate(async (element: HTMLCanvasElement) => {
    // Some browser/emulation combinations report this box in CSS pixels while
    // others report physical pixels. It is diagnostic only; the CSS rect × DPR
    // assertions below are authoritative.
    const reported = await new Promise<{ inline: number; block: number }>((resolve) => {
      const observer = new ResizeObserver((entries) => {
        observer.disconnect();
        const sizes = entries[0].devicePixelContentBoxSize;
        resolve({ inline: sizes[0].inlineSize, block: sizes[0].blockSize });
      });
      observer.observe(element, { box: 'device-pixel-content-box' });
      setTimeout(() => resolve({ inline: element.clientWidth, block: element.clientHeight }), 1_000);
    });
    return {
      width: element.width,
      height: element.height,
      cssWidth: element.getBoundingClientRect().width,
      cssHeight: element.getBoundingClientRect().height,
      dpr: window.devicePixelRatio,
      reported,
      imageRendering: getComputedStyle(element).imageRendering,
    };
  });
  expect(metrics.dpr).toBe(2);
  expectPhysicalBacking(metrics.width, metrics.height, metrics.cssWidth, metrics.cssHeight, metrics.dpr);
  expect([Math.round(metrics.cssWidth), Math.round(metrics.cssWidth * metrics.dpr)]).toContain(metrics.reported.inline);
  expect([Math.round(metrics.cssHeight), Math.round(metrics.cssHeight * metrics.dpr)]).toContain(metrics.reported.block);
  expect(metrics.imageRendering).toBe('pixelated');
  assertViewport(status, metrics.width, metrics.height);
  const samples = await waitForVisibleCanvas(
    page,
    status,
    testInfo,
    'mobile WebGL2 source sample',
    (samples) => close(samples.guest[0], GOLDEN[0].rgb),
  );
  expect(samples.corner).toEqual([0, 0, 0]);
  expect(close(samples.guest[0], GOLDEN[0].rgb)).toBe(true);
  await context.close();
  expect(problems).toEqual([]);
});

test('resize re-negotiates the surface with the same static image', async ({ page }, testInfo) => {
  await openApp(page, 'autostart=1&backend=webgl2');
  await expectReady(page);
  await page.setViewportSize({ width: 900, height: 700 });
  await page.waitForFunction(() => {
    const status = (window as unknown as { __gbaNative: { status: () => Status } }).__gbaNative.status();
    const canvas = document.querySelector<HTMLCanvasElement>('#gba-canvas');
    if (!canvas || !status || status.state !== 'ready') return false;
    const scale = Math.max(1, Math.min(Math.floor(canvas.width / 240), Math.floor(canvas.height / 160)));
    return status.viewportWidth === Math.min(240 * scale, canvas.width) &&
      status.viewportHeight === Math.min(160 * scale, canvas.height);
  }, undefined, { timeout: 5_000 });
  const status = await statusOf(page);
  const metrics = await page.locator('#gba-canvas').evaluate((element: HTMLCanvasElement) => ({
    width: element.width,
    height: element.height,
    cssWidth: element.getBoundingClientRect().width,
    cssHeight: element.getBoundingClientRect().height,
    dpr: window.devicePixelRatio,
  }));
  expectPhysicalBacking(metrics.width, metrics.height, metrics.cssWidth, metrics.cssHeight, metrics.dpr);
  assertViewport(status, metrics.width, metrics.height);
  const samples = await waitForVisibleCanvas(
    page,
    status,
    testInfo,
    'resized WebGL2 source sample',
    (samples) => close(samples.guest[0], GOLDEN[0].rgb),
  );
  expect(samples.corner).toEqual([0, 0, 0]);
  expect(close(samples.guest[0], GOLDEN[0].rgb)).toBe(true);
});

test('fractional DPR uses nearest physical backing pixels and preserves source samples', async ({ browser }, testInfo) => {
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    deviceScaleFactor: 1.25,
    isMobile: true,
    hasTouch: true,
  });
  const page = await context.newPage();
  await openApp(page, 'autostart=1&backend=webgl2');
  const status = await expectReady(page);
  const metrics = await page.locator('#gba-canvas').evaluate((element: HTMLCanvasElement) => {
    const rect = element.getBoundingClientRect();
    return {
      width: element.width,
      height: element.height,
      cssWidth: rect.width,
      cssHeight: rect.height,
      dpr: window.devicePixelRatio,
      imageRendering: getComputedStyle(element).imageRendering,
    };
  });
  expect(metrics.dpr).toBe(1.25);
  expectPhysicalBacking(metrics.width, metrics.height, metrics.cssWidth, metrics.cssHeight, metrics.dpr);
  expect(metrics.imageRendering).toBe('pixelated');
  assertViewport(status, metrics.width, metrics.height);
  const samples = await waitForVisibleCanvas(
    page,
    status,
    testInfo,
    'fractional-DPR neighborhood samples',
    (sample) => GOLDEN.slice(0, -1).every((entry, index) =>
      sample.guestNeighborhoods[index].some(({ rgb }) => close(rgb, entry.rgb)),
    ),
  );
  GOLDEN.slice(0, -1).forEach((entry, index) => {
    // At fractional DPR, the screenshot clip can put a backing-pixel center on
    // a PNG pixel boundary. Allow only that immediate rasterization ambiguity.
    // Keep the final edge pixel in the exact-DPR forced-backend oracles above;
    // these four interior source pixels independently prove fractional output.
    const neighborhood = samples.guestNeighborhoods[index];
    const nearest = neighborhood.reduce((best, candidate) => {
      const maxError = Math.max(
        ...candidate.rgb.map((value, channel) => Math.abs(value - entry.rgb[channel])),
      );
      return maxError < best.maxError ? { ...candidate, maxError } : best;
    }, { dx: 0, dy: 0, rgb: [0, 0, 0] as [number, number, number], maxError: 256 });
    expect(
      neighborhood.some(({ rgb }) => close(rgb, entry.rgb)),
      `fractional-DPR source sample ${entry.guest.join(',')} centered at ${samples.guest[index]}: nearest within one pixel is ${nearest.rgb} at (${nearest.dx},${nearest.dy}) with max error ${nearest.maxError}; global first-golden match at (${samples.nearestGolden.x},${samples.nearestGolden.y}) is ${samples.nearestGolden.rgb} (max error ${samples.nearestGolden.maxError}); PNG ${samples.png}, backing ${samples.canvas}`,
    ).toBe(true);
  });
  await context.close();
});

test('initial oversized canvas errors immediately and a fresh-canvas restart recovers', async ({ page }) => {
  await openApp(page, 'backend=webgl2');
  await waitForState(page, 'empty');
  await page.evaluate(() => {
    const sheet = Array.from(document.styleSheets).find((item) => item.href?.endsWith('/app.css'));
    if (!sheet) throw new Error('same-origin application stylesheet was not found');
    sheet.insertRule(
      '#screen-slot{width:6000px!important;height:4500px!important;max-width:none!important;aspect-ratio:auto!important}',
      sheet.cssRules.length,
    );
  });
  await page.getByRole('button', { name: 'Start synthetic demo' }).click();
  await waitForState(page, 'error');
  const status = await statusOf(page);
  expect(status.message).toContain('surface allocation cap');
  expect(status.detail).toContain('design cap');
  await expect(page.locator('#gba-canvas')).toBeHidden();

  await page.getByRole('button', { name: 'Restart with selected backend' }).click();
  await page.waitForLoadState('load');
  const recovered = await expectReady(page);
  expect(recovered.actual).toBe('webgl2');
});

test('storage denial allows initial in-page start but blocks unbounded manual reloads', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, 'sessionStorage', {
      configurable: true,
      get() { throw new DOMException('storage denied', 'SecurityError'); },
    });
  });
  await openApp(page, '');
  await waitForState(page, 'empty');
  await page.getByRole('button', { name: 'Start synthetic demo' }).click();
  await expectReady(page);

  const url = page.url();
  await page.getByRole('button', { name: 'Restart with selected backend' }).click();
  await expect(page.locator('#status-message')).toContainText('Restart blocked');
  await expect(page.locator('#status-detail')).toContainText('sessionStorage');
  expect(page.url()).toBe(url);
  await expect(page.locator('#gba-canvas')).toBeVisible();
});

test('restart budget stops an unbounded reload storm', async ({ page }) => {
  await openApp(page, 'autostart=1&backend=webgl2');
  await expectReady(page);
  // Seed three recent restarts (the real rolling 30 s budget). This exercises
  // the guard deterministically without three slow page reloads.
  await page.evaluate(() => {
    const now = Date.now();
    sessionStorage.setItem('gba-native-web-restarts', JSON.stringify([now - 2_000, now - 1_000, now]));
  });
  const settledUrl = page.url();
  await page.getByRole('button', { name: 'Restart with selected backend' }).click();
  await expect(page.locator('#status-message')).toContainText('Restart budget reached');
  expect(page.url()).toBe(settledUrl);
});
