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
// - WebGPU's preferred-format copy warning must be absent when the browser's
//   preferred canvas format is rgba8unorm; other platform preferences are logged.
// - Loss/restart is triggered through the real WebGL context-loss extension.

import { expect, test as base } from '@playwright/test';
import type { Page, TestInfo } from '@playwright/test';
import { readFile, writeFile } from 'node:fs/promises';

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
const DISCLAIMER =
  'Experimental synthetic GPU demo; no playable ROM or cartridge input; synthetic save management only.';

type Fixtures = { failureDiagnostics: void; requests: string[] };

const DIAGNOSTIC_STATUS_FIELDS = [
  'disposition', 'state', 'message', 'requested', 'actual', 'autoFallback', 'action', 'detail',
  'backend', 'adapterName', 'deviceType', 'driver', 'driverInfo', 'vendorId', 'deviceId',
  'surfaceFormat', 'renderFormat', 'presentMode', 'presented', 'viewportX', 'viewportY',
  'viewportWidth', 'viewportHeight', 'viewportScale', 'viewportClipped',
];

type BrowserEvent = {
  kind: string;
  type?: string;
  text?: string;
  url?: string;
  error?: string;
};

const PROBLEM_SAMPLE_LIMIT = 20;
const PROBLEM_TEXT_LIMIT = 1_000;

type ProblemSummary = { total: number; truncated: boolean; sample: string[] };

function createProblemCollector() {
  let total = 0;
  const sample: string[] = [];
  return {
    add(problem: string): void {
      total += 1;
      if (sample.length < PROBLEM_SAMPLE_LIMIT) {
        sample.push(problem.slice(0, PROBLEM_TEXT_LIMIT));
      }
    },
    snapshot(): ProblemSummary {
      return { total, truncated: total > sample.length, sample: [...sample] };
    },
  };
}

function problemFailureMessage(summary: ProblemSummary): string {
  return `console/page errors: ${summary.total}; sample truncated: ${summary.truncated}\n${summary.sample.join('\n')}`;
}

const browserEventTrails = new WeakMap<Page, BrowserEvent[]>();
const webGpuFormatWarnings = new WeakMap<Page, string[]>();

// This preload is deliberately diagnostic-only. In particular it never adds a
// reaction to GPUDevice.lost: doing so could mark an otherwise-unhandled
// rejection as handled. Native unhandledrejection events are observed without
// preventDefault(), while fulfilled loss is available through app status history.
function installLinuxE2EDiagnostics(): void {
  const key = '__gbaLinuxE2EDiagnostics';
  const existing = (window as unknown as Record<string, unknown>)[key];
  if (existing) return;

  const MAX_EVENTS = 48;
  const MAX_EVENT_TEXT = 512;
  const MAX_STACK_TEXT = 1_200;
  const MAX_HOOKS = 8;
  const events: Array<Record<string, unknown>> = [];
  const hooks: Array<{ name: string; installed: boolean; detail: string }> = [];
  const deviceIds = new WeakMap<object, number>();
  const lostPromises = new WeakMap<object, number>();
  let eventCount = 0;
  let droppedEvents = 0;
  let nextDeviceId = 1;

  const clip = (value: unknown, limit: number): string => {
    try {
      if (typeof value === 'string') return value.slice(0, limit);
      if (value instanceof Error) return `${value.name}: ${value.message}`.slice(0, limit);
      if (value === null) return 'null';
      return `[${typeof value}]`;
    } catch {
      return '[unprintable]';
    }
  };
  const deviceId = (device: object): number => {
    const known = deviceIds.get(device);
    if (known !== undefined) return known;
    const assigned = nextDeviceId;
    nextDeviceId = Math.min(nextDeviceId + 1, 1_000_000);
    deviceIds.set(device, assigned);
    return assigned;
  };
  const record = (kind: string, details: Record<string, unknown> = {}): void => {
    try {
      eventCount = Math.min(eventCount + 1, Number.MAX_SAFE_INTEGER);
      const event: Record<string, unknown> = {
        kind: clip(kind, 64),
        atMs: Math.round(performance.now()),
        wallTimeMs: Date.now(),
      };
      for (const field of [
        'deviceId', 'event', 'visibilityState', 'hidden', 'persisted', 'reason', 'stack',
      ]) {
        if (!Object.prototype.hasOwnProperty.call(details, field)) continue;
        const value = details[field];
        if (typeof value === 'number' || typeof value === 'boolean' || value === null) {
          event[field] = value;
        } else if (typeof value === 'string') {
          event[field] = value.slice(0, field === 'stack' ? MAX_STACK_TEXT : MAX_EVENT_TEXT);
        }
      }
      if (events.length === MAX_EVENTS) {
        events.shift();
        droppedEvents = Math.min(droppedEvents + 1, Number.MAX_SAFE_INTEGER);
      }
      events.push(event);
    } catch {
      // Diagnostics must never change browser API or application behavior.
    }
  };
  const hookResult = (name: string, installed: boolean, detail: string): void => {
    if (hooks.length < MAX_HOOKS) {
      hooks.push({ name: clip(name, 64), installed, detail: clip(detail, MAX_EVENT_TEXT) });
    }
  };
  const lifecycle = (event: Event): void => {
    const persisted = (event as Event & { persisted?: boolean }).persisted;
    record('page-lifecycle', {
      event: event.type,
      visibilityState: document.visibilityState,
      hidden: document.hidden,
      ...(typeof persisted === 'boolean' ? { persisted } : {}),
    });
  };

  document.addEventListener('visibilitychange', lifecycle);
  window.addEventListener('pagehide', lifecycle);
  window.addEventListener('pageshow', lifecycle);
  document.addEventListener('freeze', lifecycle);
  document.addEventListener('resume', lifecycle);
  window.addEventListener('unhandledrejection', (event: PromiseRejectionEvent) => {
    const id = lostPromises.get(event.promise);
    if (id === undefined) return;
    record('device-lost-promise-unhandled-rejection', {
      deviceId: id,
      reason: clip(event.reason, MAX_EVENT_TEXT),
    });
    // Intentionally do not call preventDefault(): retain native error reporting.
  });
  window.addEventListener('rejectionhandled', (event: PromiseRejectionEvent) => {
    const id = lostPromises.get(event.promise);
    if (id !== undefined) record('device-lost-promise-rejectionhandled', { deviceId: id });
  });

  const gpuDevice = (globalThis as unknown as {
    GPUDevice?: { prototype?: object };
  }).GPUDevice;
  const prototype = gpuDevice?.prototype;
  if (!prototype) {
    hookResult('GPUDevice.prototype', false, 'GPUDevice was not exposed at preload time');
  } else {
    const findDescriptor = (property: string): { owner: object; descriptor: PropertyDescriptor } | null => {
      let owner: object | null = prototype;
      while (owner) {
        const descriptor = Object.getOwnPropertyDescriptor(owner, property);
        if (descriptor) return { owner, descriptor };
        owner = Object.getPrototypeOf(owner) as object | null;
      }
      return null;
    };

    const destroy = findDescriptor('destroy');
    if (!destroy || typeof destroy.descriptor.value !== 'function') {
      hookResult('GPUDevice.destroy', false, 'method descriptor was unavailable');
    } else if (!destroy.descriptor.configurable && !destroy.descriptor.writable) {
      hookResult('GPUDevice.destroy', false, 'method descriptor was not replaceable');
    } else {
      const original = destroy.descriptor.value as (...args: unknown[]) => unknown;
      const wrapped = function (this: object, ...args: unknown[]): unknown {
        let stack = '';
        try {
          stack = new Error().stack ?? '';
        } catch {
          // Timestamp and device identity remain useful without a stack.
        }
        try {
          record('device-destroy', { deviceId: deviceId(this), stack });
        } catch {
          // A failed diagnostic must not replace the native method's behavior.
        }
        return Reflect.apply(original, this, args);
      };
      try {
        Object.defineProperty(destroy.owner, 'destroy', {
          ...destroy.descriptor,
          value: wrapped,
        });
        hookResult('GPUDevice.destroy', true, 'calls, timestamps, and bounded stacks recorded');
      } catch (error) {
        hookResult('GPUDevice.destroy', false, clip(error, MAX_EVENT_TEXT));
      }
    }

    const lost = findDescriptor('lost');
    if (!lost || typeof lost.descriptor.get !== 'function') {
      hookResult('GPUDevice.lost', false, 'getter descriptor was unavailable');
    } else if (!lost.descriptor.configurable) {
      hookResult('GPUDevice.lost', false, 'getter descriptor was not replaceable');
    } else {
      const original = lost.descriptor.get;
      const wrapped = function (this: object): unknown {
        let promise: unknown;
        try {
          promise = Reflect.apply(original, this, []);
        } catch (error) {
          try {
            record('device-lost-getter-error', {
              deviceId: deviceId(this),
              reason: clip(error, MAX_EVENT_TEXT),
            });
          } catch {
            // Preserve the original getter exception even if diagnostics fail.
          }
          throw error;
        }
        if (promise !== null && (typeof promise === 'object' || typeof promise === 'function')) {
          try {
            lostPromises.set(promise as object, deviceId(this));
          } catch {
            // The original promise is always returned unchanged.
          }
        }
        return promise;
      };
      try {
        Object.defineProperty(lost.owner, 'lost', { ...lost.descriptor, get: wrapped });
        hookResult(
          'GPUDevice.lost',
          true,
          'tracks native unhandled-rejection events only; fulfilled loss is in app status history',
        );
      } catch (error) {
        hookResult('GPUDevice.lost', false, clip(error, MAX_EVENT_TEXT));
      }
    }
  }

  (window as unknown as Record<string, unknown>)[key] = {
    snapshot: () => ({
      enabled: true,
      limits: { maxEvents: MAX_EVENTS, maxEventText: MAX_EVENT_TEXT, maxStackText: MAX_STACK_TEXT },
      eventCount,
      droppedEvents,
      events: events.map((event) => ({ ...event })),
      hooks: hooks.map((hook) => ({ ...hook })),
    }),
  };
}

async function browserDiagnosticSnapshot(page: Page, reason: string): Promise<unknown> {
  return page.evaluate(({ reason, fields }) => {
    const api = (window as unknown as {
      __gbaNative?: {
        state?: () => string;
        status?: () => unknown;
        statusHistory?: () => unknown[];
      };
    }).__gbaNative;
    const linuxDiagnostics = (window as unknown as {
      __gbaLinuxE2EDiagnostics?: { snapshot?: () => unknown };
    }).__gbaLinuxE2EDiagnostics;
    const pick = (value: unknown): unknown => {
      if (value === null || typeof value !== 'object') return value ?? null;
      const record = value as Record<string, unknown>;
      const result: Record<string, unknown> = {};
      for (const key of ['atMs', 'phase', ...fields]) {
        if (!Object.prototype.hasOwnProperty.call(record, key)) continue;
        const field = record[key];
        if (typeof field === 'string') result[key] = field.slice(0, 500);
        else if (typeof field === 'number' || typeof field === 'boolean' || field === null) {
          result[key] = field;
        }
      }
      return result;
    };
    const byId = (id: string) => document.getElementById(id);
    const rect = (element: Element | null) => {
      if (!element) return null;
      const box = element.getBoundingClientRect();
      return {
        x: box.x, y: box.y, top: box.top, right: box.right,
        bottom: box.bottom, left: box.left, width: box.width, height: box.height,
      };
    };
    const canvas = byId('gba-canvas') as HTMLCanvasElement | null;
    const canvasStyle = canvas ? getComputedStyle(canvas) : null;
    const slot = byId('screen-slot');
    const slotStyle = slot ? getComputedStyle(slot) : null;
    let currentStatus: unknown = null;
    let history: unknown[] = [];
    let statusReadError: string | null = null;
    try {
      currentStatus = pick(api?.status?.());
      history = (api?.statusHistory?.() ?? []).slice(-32).map(pick);
    } catch (error) {
      statusReadError = String(error).slice(0, 500);
    }
    return {
      reason,
      capturedAtMs: Math.round(performance.now()),
      appPhase: api?.state?.() ?? null,
      bodyPhase: document.body?.dataset.appState ?? null,
      currentStatus,
      statusHistory: history,
      statusReadError,
      linuxDiagnostics: linuxDiagnostics?.snapshot?.() ?? null,
      ui: {
        statusMessage: byId('status-message')?.textContent?.slice(0, 500) ?? null,
        statusDetail: byId('status-detail')?.textContent?.slice(0, 500) ?? null,
        overlayTitle: byId('overlay-title')?.textContent?.slice(0, 300) ?? null,
      },
      canvas: canvas ? {
        width: canvas.width,
        height: canvas.height,
        hiddenAttribute: canvas.hidden,
        ariaHidden: canvas.getAttribute('aria-hidden'),
        className: canvas.className,
        computed: {
          display: canvasStyle?.display ?? null,
          visibility: canvasStyle?.visibility ?? null,
          opacity: canvasStyle?.opacity ?? null,
        },
        rect: rect(canvas),
        clientRectCount: canvas.getClientRects().length,
      } : null,
      slot: slot ? {
        ariaBusy: slot.getAttribute('aria-busy'),
        computed: {
          display: slotStyle?.display ?? null,
          visibility: slotStyle?.visibility ?? null,
        },
        rect: rect(slot),
      } : null,
    };
  }, { reason, fields: DIAGNOSTIC_STATUS_FIELDS });
}

async function logCanvasVisibilityFailure(page: Page, original: unknown): Promise<string> {
  let snapshot: unknown;
  try {
    snapshot = await browserDiagnosticSnapshot(page, 'canvas visibility assertion failed');
  } catch (error) {
    snapshot = { captureError: String(error).slice(0, 500) };
  }
  const serialized = JSON.stringify({
    snapshot,
    browserEvents: browserEventTrails.get(page) ?? [],
  });
  console.log(`WEB_E2E_CANVAS_VISIBILITY_FAILURE ${serialized}`);
  return `${String(original)}\nBrowser failure diagnostics: ${serialized}`;
}

const test = base.extend<Fixtures>({
  failureDiagnostics: [async ({ page }, use, testInfo) => {
    const events: BrowserEvent[] = [];
    const formatWarnings: string[] = [];
    const problems = createProblemCollector();
    if (
      process.platform === 'linux' &&
      process.env.CI === 'true' &&
      process.env.WEB_E2E_LINUX_DIAGNOSTICS === '1'
    ) {
      await page.addInitScript(installLinuxE2EDiagnostics);
    }
    browserEventTrails.set(page, events);
    webGpuFormatWarnings.set(page, formatWarnings);
    const addEvent = (event: BrowserEvent) => {
      if (events.length === 100) events.shift();
      events.push(event);
    };
    page.on('console', (message) => {
      const text = message.text().slice(0, 1_000);
      addEvent({ kind: 'console', type: message.type(), text });
      if (
        message.type() === 'warning' &&
        text.includes('configured with a different format than is preferred')
      ) {
        formatWarnings.push(text);
      }
      if (message.type() === 'error') problems.add(`console: ${text}`);
    });
    page.on('pageerror', (error) => {
      const text = (error.stack ?? error.message).slice(0, 1_000);
      addEvent({ kind: 'pageerror', error: text });
      problems.add(`pageerror: ${text}`);
    });
    page.on('requestfailed', (request) => {
      addEvent({
        kind: 'requestfailed',
        url: request.url().slice(0, 1_000),
        error: (request.failure()?.errorText ?? 'unknown').slice(0, 500),
      });
    });
    await use();
    const problemSummary = problems.snapshot();
    if (testInfo.status !== testInfo.expectedStatus || problemSummary.total > 0) {
      let snapshot: unknown;
      try {
        snapshot = await browserDiagnosticSnapshot(page, `failed test: ${testInfo.title}`);
      } catch (error) {
        snapshot = { captureError: String(error).slice(0, 500) };
      }
      console.log(`WEB_E2E_FAILURE_DIAGNOSTICS ${JSON.stringify({
        test: testInfo.title,
        problemSummary,
        browserEvents: events,
        snapshot,
      })}`);
    }
    expect(problemSummary.total, problemFailureMessage(problemSummary)).toBe(0);
  }, { auto: true }],
  requests: async ({ page, baseURL }, use) => {
    const requests: string[] = [];
    page.on('request', (request) => requests.push(request.url()));
    await use(requests);
    const foreign = requests.filter((url) => !url.startsWith(baseURL!));
    expect(foreign, `unexpected external request(s): ${foreign.join(', ')}`).toEqual([]);
  },
});

test('synthetic browser-error flood keeps diagnostics bounded and counts every error', async ({ browser }) => {
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    const problems = createProblemCollector();
    page.on('console', (message) => {
      if (message.type() === 'error') problems.add(`console: ${message.text()}`);
    });
    await page.evaluate(async () => {
      for (let index = 0; index < 40; index += 1) {
        console.error(`synthetic diagnostic error ${index}: ${'x'.repeat(1_500)}`);
      }
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await expect.poll(() => problems.snapshot().total).toBe(40);

    const summary = problems.snapshot();
    expect(summary.truncated).toBe(true);
    expect(summary.sample).toHaveLength(PROBLEM_SAMPLE_LIMIT);
    expect(summary.sample.every((problem) => problem.length <= PROBLEM_TEXT_LIMIT)).toBe(true);
    expect(summary.sample[0]).toContain('synthetic diagnostic error 0');
    expect(JSON.stringify(summary).length).toBeLessThanOrEqual(
      PROBLEM_SAMPLE_LIMIT * (PROBLEM_TEXT_LIMIT + 8) + 256,
    );
  } finally {
    await context.close();
  }
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
  action?: string;
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
  try {
    await expect(canvas).toBeVisible({ timeout: CANVAS_VISIBILITY_TIMEOUT_MS });
  } catch (error) {
    throw new Error(await logCanvasVisibilityFailure(page, error));
  }
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
          statusHistory: (window as unknown as {
            __gbaNative: { statusHistory?: () => unknown[] };
          }).__gbaNative.statusHistory?.() ?? [],
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

function expectNoCapabilityProbeContextLoss(page: Page): void {
  const probeLossWarnings = (browserEventTrails.get(page) ?? []).filter(
    (event) =>
      event.kind === 'console' &&
      event.text?.includes('CONTEXT_LOST_WEBGL: loseContext: context lost'),
  );
  expect(
    probeLossWarnings,
    'WebGPU startup must not explicitly lose the detached WebGL2 capability-probe context',
  ).toEqual([]);
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
  expect(status.surfaceFormat).toBe('Rgba8Unorm');
  expect(status.renderFormat).toBe('Rgba8Unorm');
  const preferredCanvasFormat = await page.evaluate(() => {
    const gpu = (navigator as unknown as {
      gpu?: { getPreferredCanvasFormat?: () => string };
    }).gpu;
    return gpu?.getPreferredCanvasFormat?.() ?? null;
  });
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
  const formatWarnings = webGpuFormatWarnings.get(page) ?? [];
  if (preferredCanvasFormat === 'rgba8unorm') {
    expect(formatWarnings, 'WebGPU preferred-format conversion warnings').toEqual([]);
  }
  console.log(
    `observed WebGPU preferred canvas format and conversion warnings: ${JSON.stringify({ preferredCanvasFormat, formatWarnings })}`,
  );
  await expect(page.locator('#probe-gl')).toContainText('not probed here');
  expectNoCapabilityProbeContextLoss(page);
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
  await page.evaluate(() => {
    const lostStatus = {
      state: 'lost',
      message: 'Synthetic WebGPU device loss for diagnostics regression.',
      detail: 'synthetic device-loss reason: hidden canvas diagnostic fixture',
      requested: 'webgpu',
      actual: 'webgpu',
      backend: 'BrowserWebGpu',
      surfaceFormat: 'Rgba8Unorm',
      presented: 1,
      opaqueTestSentinel: 'diagnostic-secret-sentinel',
    };
    (window as unknown as { __gbaNative: unknown }).__gbaNative = {
      state: () => 'lost',
      status: () => lostStatus,
      statusHistory: () => [
        { phase: 'ready', state: 'ready', requested: 'webgpu', actual: 'webgpu', presented: 1 },
        { phase: 'ready', ...lostStatus },
      ],
    };
  });
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
  expect(hiddenFailure).toContain('synthetic device-loss reason: hidden canvas diagnostic fixture');
  expect(hiddenFailure).toContain('"requested":"webgpu"');
  expect(hiddenFailure).toContain('"actual":"webgpu"');
  expect(hiddenFailure).toContain('"presented":1');
  expect(hiddenFailure).toContain('"display":"none"');
  expect(hiddenFailure).not.toContain('diagnostic-secret-sentinel');
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

test('invalid backend history bounds a long query without normalizing the diagnostic', async ({ page }) => {
  const invalidBackend = `invalid-backend-${'x'.repeat(2_048)}`;
  await openApp(page, `autostart=1&backend=${encodeURIComponent(invalidBackend)}`);
  await waitForState(page, 'error');

  const diagnostics = await page.evaluate(() => {
    const api = (window as unknown as {
      __gbaNative: { status: () => Status; statusHistory: () => Array<Record<string, unknown>> };
    }).__gbaNative;
    return { current: api.status(), history: api.statusHistory() };
  });
  expect(diagnostics.current.message).toContain('unknown backend');
  expect(diagnostics.current.requested).toHaveLength(160);
  expect(diagnostics.current.requested).toMatch(/^invalid-backend-/);
  expect(diagnostics.current.requested.endsWith('… [truncated]')).toBe(true);
  expect(diagnostics.history.length).toBeLessThanOrEqual(32);
  expect(diagnostics.history.some((entry) => entry.state === 'error')).toBe(true);
  const invalidHistory = diagnostics.history.filter(
    (entry) => typeof entry.requested === 'string' && entry.requested.startsWith('invalid-backend-'),
  );
  expect(invalidHistory.length).toBeGreaterThan(0);
  for (const entry of invalidHistory) {
    expect((entry.requested as string).length).toBeLessThanOrEqual(160);
    expect(entry.requested).toBe(diagnostics.current.requested);
  }
});

test('status diagnostics return detached snapshots, including caller-added nested values', async ({ page }) => {
  await openApp(page, 'autostart=1&backend=webgl2');
  await expectReady(page);

  const result = await page.evaluate(() => {
    const api = (window as unknown as {
      __gbaNative: { status: () => Status; statusHistory: () => Array<Record<string, unknown>> };
    }).__gbaNative;
    const returned = api.status() as Status & Record<string, unknown>;
    const historyBefore = api.statusHistory();
    const original = {
      state: returned.state,
      message: returned.message,
      requested: returned.requested,
      presented: returned.presented,
    };
    returned.state = 'error';
    returned.message = 'caller mutation';
    returned.requested = 'caller mutation';
    returned.diagnosticMutation = { detail: 'before mutation' };
    (returned.diagnosticMutation as { detail: string }).detail = 'nested mutation';
    const next = api.status() as Status & Record<string, unknown>;
    return {
      original,
      historyBefore,
      historyAfter: api.statusHistory(),
      next: {
        state: next.state,
        message: next.message,
        requested: next.requested,
        presented: next.presented,
      },
      leakedNestedValue: Object.prototype.hasOwnProperty.call(next, 'diagnosticMutation'),
    };
  });
  expect(result.next).toEqual(result.original);
  expect(result.historyAfter).toEqual(result.historyBefore);
  expect(result.leakedNestedValue).toBe(false);
  await expect(page.locator('#status-message')).not.toContainText('caller mutation');
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
  const problems = createProblemCollector();
  page.on('console', (message) => {
    if (message.type() === 'error') problems.add(`console: ${message.text()}`);
  });
  page.on('pageerror', (error) => problems.add(`pageerror: ${error.message}`));
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
  const problemSummary = problems.snapshot();
  expect(problemSummary.total, problemFailureMessage(problemSummary)).toBe(0);
});

test('resize re-negotiates the surface with the same static image', async ({ page }, testInfo) => {
  await openApp(page, 'autostart=1&backend=webgl2');
  await expectReady(page);
  await page.setViewportSize({ width: 900, height: 700 });
  await page.waitForFunction(() => {
    const status = (window as unknown as { __gbaNative: { status: () => Status } }).__gbaNative.status();
    const canvas = document.querySelector<HTMLCanvasElement>('#gba-canvas');
    if (!canvas || !status || status.state !== 'ready') return false;
    // CSS can adopt the new viewport before Winit's resize event updates the
    // physical backing store and status viewport. Do not accept the old, still-
    // internally-consistent geometry during that gap; this remains within the
    // existing bounded wait below and does not retry a failed presentation.
    const rect = canvas.getBoundingClientRect();
    const backingMatchesCss =
      Math.abs(canvas.width - Math.round(rect.width * window.devicePixelRatio)) <= 1 &&
      Math.abs(canvas.height - Math.round(rect.height * window.devicePixelRatio)) <= 1;
    if (!backingMatchesCss) return false;
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

test('save library imports, selects a default, exports and persists', async ({ page }) => {
  await openApp(page, '');
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.locator('#save-status')).toContainText('No save slots yet.');

  const image = (counter: number) => {
    const bytes = new Uint8Array(32 * 1024).fill(0xff);
    bytes.set([0x47, 0x53, 0x41, 0x56, counter], 0);
    return Buffer.from(bytes);
  };

  await page.locator('#save-name').fill('alpha');
  await page.locator('#save-file').setInputFiles({
    name: 'alpha.sav',
    mimeType: 'application/octet-stream',
    buffer: image(1),
  });
  await page.getByRole('button', { name: 'Import' }).click();
  await expect(page.locator('#save-status')).toContainText('imported=alpha');
  await expect(page.locator('#save-list li[data-slot="alpha"]')).toContainText('default');

  await page.locator('#save-name').fill('beta');
  await page.locator('#save-file').setInputFiles({
    name: 'beta.sav',
    mimeType: 'application/octet-stream',
    buffer: image(2),
  });
  await page.getByRole('button', { name: 'Import' }).click();
  await expect(page.locator('#save-list li')).toHaveCount(2);
  await expect(page.locator('#save-status')).toContainText('imported=beta');

  await page.locator('#save-list li[data-slot="beta"]').getByRole('button', { name: 'Use' }).click();
  await expect(page.locator('#save-status')).toContainText('default=beta');
  await expect(page.locator('#save-list li[data-slot="beta"]')).toContainText('default');

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByRole('button', { name: 'Export default' }).click(),
  ]);
  const exported = await readFile((await download.path())!);
  expect(Array.from(exported.subarray(0, 5))).toEqual([0x47, 0x53, 0x41, 0x56, 2]);
  expect(exported.length).toBe(32 * 1024);

  await page.reload();
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.locator('#save-list li')).toHaveCount(2);
  await expect(page.locator('#save-list li[data-slot="beta"]')).toContainText('default');
  const stored = await page.evaluate(() => window.__gbaNative.saves.list());
  expect(stored).toEqual([
    { name: 'alpha', bytes: 32768, default: false },
    { name: 'beta', bytes: 32768, default: true },
  ]);
});

test('audio settings and the WebAudio test tone are wired', async ({ page }) => {
  await openApp(page, '');
  await page.getByRole('button', { name: 'Settings' }).click();
  type AudioDiagnostics = {
    settings: () => { masterVolume: number; muted: boolean; gain: number };
    playTestTone: () => boolean;
    contextState: () => string;
  };
  const settings = () =>
    page.evaluate(() =>
      (window as unknown as { __gbaNative: { audio: AudioDiagnostics } }).__gbaNative.audio.settings(),
    );

  const initial = await settings();
  expect(initial.masterVolume).toBe(80);
  expect(initial.muted).toBe(false);
  expect(initial.gain).toBeCloseTo(0.8, 5);

  const played = await page.evaluate(() =>
    (window as unknown as { __gbaNative: { audio: AudioDiagnostics } }).__gbaNative.audio.playTestTone(),
  );
  expect(played).toBe(true);
  const state = await page.evaluate(() =>
    (window as unknown as { __gbaNative: { audio: AudioDiagnostics } }).__gbaNative.audio.contextState(),
  );
  expect(['running', 'suspended']).toContain(state);

  await page.locator('#mute-toggle').check();
  expect((await settings()).gain).toBe(0);
  await page.locator('#mute-toggle').uncheck();
  await page.evaluate(() => {
    const range = document.querySelector('#volume-range') as HTMLInputElement;
    range.value = '40';
    range.dispatchEvent(new Event('input', { bubbles: true }));
  });
  const quieter = await settings();
  expect(quieter.masterVolume).toBe(40);
  expect(quieter.gain).toBeCloseTo(0.4, 5);
});

test('PCM buffers shaped like the wasm module output play through WebAudio', async ({ page }) => {
  await openApp(page, '');
  await page.getByRole('button', { name: 'Settings' }).click();
  type AudioDiagnostics = {
    playPcm: (samples: Int16Array, sampleRate?: number) => boolean;
    contextState: () => string;
  };
  const result = await page.evaluate(() => {
    const audio = (window as unknown as { __gbaNative: { audio: AudioDiagnostics } }).__gbaNative.audio;
    const samples = new Int16Array(3277);
    for (let index = 0; index < samples.length; index += 1) {
      samples[index] = Math.round(Math.sin((2 * Math.PI * 440 * index) / 32768) * 8000);
    }
    return {
      played: audio.playPcm(samples, 32768),
      empty: audio.playPcm(new Int16Array(0), 32768),
      state: audio.contextState(),
    };
  });
  expect(result.played).toBe(true);
  expect(result.empty).toBe(false);
  expect(['running', 'suspended']).toContain(result.state);
});
