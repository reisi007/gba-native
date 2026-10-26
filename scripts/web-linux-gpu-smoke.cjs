#!/usr/bin/env node
// Linux CI WebGPU diagnostics. Modes canvas/offscreen are standalone probes:
// each gets a CSP-restricted document via page.route() and never loads the app
// bundle. Mode app observes the real served app on its public route, forcing
// WebGPU startup through the ordinary query parameters and changing nothing.
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const zlib = require('node:zlib');

const OUTPUT_DIR = process.env.WEBGPU_SMOKE_OUTPUT_DIR;
const MODE = process.argv[2];
const ORIGIN = process.env.WEB_APP_URL;
const CHANNEL = process.env.WEB_E2E_CHANNEL || 'chromium';
const LAUNCH_ARGS_TEXT = process.env.WEB_E2E_LAUNCH_ARGS || '';
const CHROMIUM_REVISION = process.env.WEB_PLAYWRIGHT_CHROMIUM_REVISION || 'unknown';
const POST_WORK_WAIT_MS = 7_000;
const OPERATION_TIMEOUT_MS = 10_000;
const TEARDOWN_LOSS_WAIT_MS = 1_000;
const RAF_TIMEOUT_MS = 1_500;
const PLAYWRIGHT_TIMEOUTS_MS = {
  browserLaunch: 35_000,
  contextCreate: 8_000,
  pageCreate: 8_000,
  route: 5_000,
  navigation: 15_000,
  evaluation: 50_000,
  visibility: 5_000,
  screenshot: 12_000,
  teardown: 3_000,
  contextClose: 3_000,
  browserClose: 5_000,
};
const HARNESS_HARD_DEADLINE_MS = 150_000;
const PROBE_PATH = '/__linux_webgpu_smoke__';
// Real app entry for mode app. The query parameters are the documented public
// ones the functional suite already uses; the harness installs no fallback.
const APP_PATH = '/?autostart=1&backend=webgpu';
let activeResult = null;
let hardDeadlineTimer = null;

function writeJson(name, value) {
  fs.writeFileSync(path.join(OUTPUT_DIR, name), `${JSON.stringify(value, null, 2)}\n`, {
    flag: 'w',
  });
}

function withDeadline(operation, label, durationMs, onLateSuccess = null) {
  let timer;
  let expired = false;
  const pending = Promise.resolve().then(operation);
  // A command that outlives its deadline is abandoned, but its eventual
  // rejection is still observed. The owned harness has a final hard exit.
  pending.then(
    (value) => {
      if (expired && onLateSuccess) {
        Promise.resolve(onLateSuccess(value)).catch((error) => {
          if (activeResult) {
            activeResult.lateCommandErrors ??= [];
            if (activeResult.lateCommandErrors.length < 20) {
              activeResult.lateCommandErrors.push({ operation: label, error: safeText(error) });
            }
          }
        });
      }
    },
    (error) => {
      if (expired && activeResult) {
        activeResult.lateCommandErrors ??= [];
        if (activeResult.lateCommandErrors.length < 20) {
          activeResult.lateCommandErrors.push({ operation: label, error: safeText(error) });
        }
      }
    },
  ).catch(() => {});
  const deadline = new Promise((_, reject) => {
    timer = setTimeout(() => {
      expired = true;
      const error = new Error(`${label} exceeded ${durationMs}ms`);
      error.name = 'DeadlineExceededError';
      error.operation = label;
      error.deadlineMs = durationMs;
      reject(error);
    }, durationMs);
  });
  return Promise.race([pending, deadline]).catch((error) => {
    if (error?.name === 'DeadlineExceededError' && activeResult) {
      activeResult.timeouts.push({ operation: label, deadlineMs: durationMs });
    }
    throw error;
  }).finally(() => clearTimeout(timer));
}

function persistHardDeadlineFailure() {
  const result = activeResult || {
    schemaVersion: 1,
    mode: MODE,
    entry: MODE === 'app' ? APP_PATH : PROBE_PATH,
    launch: {
      channel: CHANNEL,
      headless: true,
      args: LAUNCH_ARGS_TEXT.split(/\s+/).filter(Boolean),
      chromiumRevision: CHROMIUM_REVISION,
    },
    timeouts: [],
  };
  result.ok = false;
  result.processExitStatus = 1;
  result.harnessError ||= `smoke harness exceeded its ${HARNESS_HARD_DEADLINE_MS}ms hard deadline`;
  result.timeouts ??= [];
  result.timeouts.push({ operation: 'smoke-harness-hard-deadline', deadlineMs: HARNESS_HARD_DEADLINE_MS });
  result.browserTerminationFallback =
    'Playwright does not expose the launched browser process; exiting this owned harness closes its control pipe. No broad process kill is used.';
  try {
    writeJson('result.json', result);
  } catch (error) {
    try { fs.writeSync(2, `could not persist hard-deadline result: ${safeText(error)}\n`); } catch {}
  }
  try { fs.writeSync(2, `WEBGPU_SMOKE_HARD_DEADLINE ${JSON.stringify(result)}\n`); } catch {}
  process.exit(1);
}

hardDeadlineTimer = setTimeout(persistHardDeadlineFailure, HARNESS_HARD_DEADLINE_MS);

function safeText(value) {
  if (value instanceof Error) return `${value.name}: ${value.message}`;
  try {
    return String(value);
  } catch {
    return '[unprintable]';
  }
}

function validateEnvironment() {
  if (!OUTPUT_DIR || !fs.statSync(OUTPUT_DIR).isDirectory()) {
    throw new Error('WEBGPU_SMOKE_OUTPUT_DIR must name the fresh probe directory');
  }
  if (!['canvas', 'offscreen', 'app'].includes(MODE)) throw new Error(`unknown probe mode: ${MODE}`);
  const origin = new URL(ORIGIN);
  if (
    origin.protocol !== 'http:' ||
    !['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname) ||
    origin.username || origin.password
  ) {
    throw new Error(`probe requires the existing loopback HTTP origin, got ${ORIGIN}`);
  }
  return origin.origin;
}

function pngPixel(png, x, y) {
  const signature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  if (!png.subarray(0, 8).equals(signature)) throw new Error('canvas screenshot was not a PNG');
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  let interlace = 0;
  const compressed = [];
  for (let offset = 8; offset + 12 <= png.length;) {
    const length = png.readUInt32BE(offset);
    const type = png.toString('ascii', offset + 4, offset + 8);
    const data = png.subarray(offset + 8, offset + 8 + length);
    if (type === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      bitDepth = data[8];
      colorType = data[9];
      interlace = data[12];
    } else if (type === 'IDAT') {
      compressed.push(data);
    } else if (type === 'IEND') {
      break;
    }
    offset += length + 12;
  }
  const channels = colorType === 6 ? 4 : colorType === 2 ? 3 : 0;
  if (bitDepth !== 8 || !channels || interlace !== 0 || x < 0 || y < 0 || x >= width || y >= height) {
    throw new Error(`unsupported screenshot PNG layout: ${width}x${height}, depth=${bitDepth}, type=${colorType}, interlace=${interlace}`);
  }
  const rowBytes = width * channels;
  const decoded = zlib.inflateSync(Buffer.concat(compressed));
  const rows = Buffer.alloc(height * rowBytes);
  const paeth = (a, b, c) => {
    const p = a + b - c;
    const pa = Math.abs(p - a);
    const pb = Math.abs(p - b);
    const pc = Math.abs(p - c);
    return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
  };
  for (let row = 0; row < height; row += 1) {
    const filter = decoded[row * (rowBytes + 1)];
    const source = row * (rowBytes + 1) + 1;
    const destination = row * rowBytes;
    for (let column = 0; column < rowBytes; column += 1) {
      const raw = decoded[source + column];
      const left = column >= channels ? rows[destination + column - channels] : 0;
      const above = row > 0 ? rows[destination + column - rowBytes] : 0;
      const upperLeft = row > 0 && column >= channels
        ? rows[destination + column - rowBytes - channels]
        : 0;
      let predictor = 0;
      if (filter === 1) predictor = left;
      else if (filter === 2) predictor = above;
      else if (filter === 3) predictor = Math.floor((left + above) / 2);
      else if (filter === 4) predictor = paeth(left, above, upperLeft);
      else if (filter !== 0) throw new Error(`unsupported PNG filter ${filter}`);
      rows[destination + column] = (raw + predictor) & 0xff;
    }
  }
  const index = y * rowBytes + x * channels;
  return {
    width,
    height,
    rgba: [rows[index], rows[index + 1], rows[index + 2], channels === 4 ? rows[index + 3] : 255],
  };
}

async function inPageProbe(mode) {
  const POST_WORK_WAIT_MS = 7_000;
  const OPERATION_TIMEOUT_MS = 10_000;
  const TEARDOWN_LOSS_WAIT_MS = 1_000;
  const RAF_TIMEOUT_MS = 1_500;
  const safeText = (value) => {
    if (value instanceof Error) return `${value.name}: ${value.message}`;
    try {
      return String(value);
    } catch {
      return '[unprintable]';
    }
  };
  const started = performance.now();
  const report = {
    mode,
    secureContext: window.isSecureContext,
    origin: location.origin,
    adapter: null,
    preferredCanvasFormat: null,
    canvas: null,
    offscreen: null,
    workError: null,
    uncapturedErrors: [],
    deviceLoss: null,
    boundedWaitMs: 0,
    boundedWaitLimitMs: 7_000,
    expectedDeviceTeardown: null,
  };
  let device = null;
  let phase = 'initialization';
  let lossResolve;
  const lossSignal = new Promise((resolve) => { lossResolve = resolve; });
  let lossEvent = null;
  const addError = (event) => {
    const error = event.error;
    report.uncapturedErrors.push({
      name: error?.name || 'GPUUncapturedError',
      message: error?.message || safeText(error),
      phase,
      atMs: Math.round(performance.now() - started),
    });
  };
  let deviceLostPromise = null;

  const timeout = (promise, duration, label) => {
    let timer;
    return Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label} exceeded ${duration}ms`)), duration);
      }),
    ]).finally(() => clearTimeout(timer));
  };
  const nextFrame = () => timeout(
    new Promise((resolve) => requestAnimationFrame(() => resolve())),
    RAF_TIMEOUT_MS,
    'requestAnimationFrame',
  );
  const observeDevice = (createdDevice) => {
    device = createdDevice;
    device.addEventListener('uncapturederror', addError);
    deviceLostPromise = device.lost.then((info) => {
      lossEvent = {
        reason: info.reason,
        message: info.message || '',
        phase,
        atMs: Math.round(performance.now() - started),
      };
      report.deviceLoss = lossEvent;
      lossResolve(lossEvent);
      return lossEvent;
    });
  };

  try {
    if (!report.secureContext) throw new Error('the loopback page is not a secure context');
    if (!navigator.gpu) throw new Error('navigator.gpu is unavailable');
    const adapter = await timeout(navigator.gpu.requestAdapter(), OPERATION_TIMEOUT_MS, 'requestAdapter');
    if (!adapter) throw new Error('requestAdapter() returned null');
    const info = adapter.info || {};
    report.adapter = {
      vendor: info.vendor || null,
      architecture: info.architecture || null,
      device: info.device || null,
      description: info.description || null,
      backend: info.backend || null,
      isFallbackAdapter: typeof adapter.isFallbackAdapter === 'boolean'
        ? adapter.isFallbackAdapter
        : null,
    };
    const requestedDevice = await timeout(adapter.requestDevice(), OPERATION_TIMEOUT_MS, 'requestDevice');
    observeDevice(requestedDevice);
    phase = 'work';

    if (mode === 'canvas') {
      const canvas = document.querySelector('canvas');
      if (!canvas) throw new Error('routed canvas document has no HTMLCanvasElement');
      const rect = canvas.getBoundingClientRect();
      if (!canvas.isConnected || rect.width <= 0 || rect.height <= 0) {
        throw new Error('HTMLCanvasElement is not visible with a nonzero layout box');
      }
      const context = canvas.getContext('webgpu');
      if (!context) throw new Error('canvas.getContext("webgpu") returned null');
      const preferred = navigator.gpu.getPreferredCanvasFormat();
      report.preferredCanvasFormat = preferred;
      context.configure({ device, format: preferred, alphaMode: 'opaque' });
      let submissions = 0;
      for (let frame = 0; frame < 2; frame += 1) {
        const encoder = device.createCommandEncoder({ label: `smoke-clear-${frame}` });
        const pass = encoder.beginRenderPass({
          colorAttachments: [{
            view: context.getCurrentTexture().createView(),
            clearValue: { r: 1, g: 0, b: 1, a: 1 },
            loadOp: 'clear',
            storeOp: 'store',
          }],
        });
        pass.end();
        device.queue.submit([encoder.finish()]);
        submissions += 1;
        await nextFrame();
      }
      await timeout(device.queue.onSubmittedWorkDone(), OPERATION_TIMEOUT_MS, 'canvas queue completion');
      await nextFrame();
      report.canvas = {
        configured: true,
        visible: true,
        width: canvas.width,
        height: canvas.height,
        submissions,
        presentationAttempts: submissions,
        presentationAcknowledgement: 'not exposed by WebGPU; screenshot verifies composited pixels',
      };
    } else {
      if (document.querySelector('canvas')) throw new Error('offscreen probe unexpectedly has a canvas');
      const shader = device.createShaderModule({
        code: `
          @group(0) @binding(0) var<storage, read_write> result: array<u32>;
          @compute @workgroup_size(4)
          fn main(@builtin(global_invocation_id) id: vec3<u32>) {
            result[id.x] = id.x + 41u;
          }
        `,
      });
      const output = device.createBuffer({
        size: 16,
        usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC,
      });
      const readback = device.createBuffer({ size: 16, usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST });
      const pipeline = device.createComputePipeline({
        layout: 'auto',
        compute: { module: shader, entryPoint: 'main' },
      });
      const bindGroup = device.createBindGroup({
        layout: pipeline.getBindGroupLayout(0),
        entries: [{ binding: 0, resource: { buffer: output } }],
      });
      const encoder = device.createCommandEncoder({ label: 'smoke-offscreen-compute' });
      const pass = encoder.beginComputePass();
      pass.setPipeline(pipeline);
      pass.setBindGroup(0, bindGroup);
      pass.dispatchWorkgroups(1);
      pass.end();
      encoder.copyBufferToBuffer(output, 0, readback, 0, 16);
      device.queue.submit([encoder.finish()]);
      await timeout(readback.mapAsync(GPUMapMode.READ), OPERATION_TIMEOUT_MS, 'offscreen readback');
      const values = Array.from(new Uint32Array(readback.getMappedRange().slice(0)));
      readback.unmap();
      await timeout(device.queue.onSubmittedWorkDone(), OPERATION_TIMEOUT_MS, 'offscreen queue completion');
      report.offscreen = {
        canvasCreated: false,
        commandSubmissions: 1,
        readbackValues: values,
        expectedValues: [41, 42, 43, 44],
        outputMatches: values.join(',') === '41,42,43,44',
      };
      output.destroy();
      readback.destroy();
    }
  } catch (error) {
    report.workError = safeText(error);
  }

  phase = 'bounded-post-work-wait';
  if (device && !lossEvent) {
    const waitStarted = performance.now();
    await Promise.race([
      lossSignal,
      new Promise((resolve) => setTimeout(resolve, POST_WORK_WAIT_MS)),
    ]);
    report.boundedWaitMs = Math.round(performance.now() - waitStarted);
  }

  const preTeardownLoss = lossEvent;
  report.deviceLossBeforeTeardown = preTeardownLoss;
  window.__webGpuSmokeTeardown = async () => {
    if (!device) return null;
    const lossBeforeTeardown = lossEvent;
    phase = 'expected-device-teardown';
    const teardownStarted = performance.now();
    try {
      device.destroy();
      const teardownLoss = await timeout(
        deviceLostPromise,
        TEARDOWN_LOSS_WAIT_MS,
        'expected device teardown loss observation',
      ).catch(() => null);
      return {
        destroyRequested: true,
        lostReason: teardownLoss?.reason ?? null,
        lostMessage: teardownLoss?.message ?? null,
        elapsedMs: Math.round(performance.now() - teardownStarted),
        preTeardownLoss: lossBeforeTeardown,
      };
    } catch (error) {
      return { destroyRequested: true, error: safeText(error), preTeardownLoss: lossBeforeTeardown };
    }
  };
  return report;
}

async function inPageAppProbe() {
  const APP_STARTUP_WAIT_MS = 30_000;
  const APP_POST_WORK_WAIT_MS = 7_000;
  const POLL_INTERVAL_MS = 100;
  const MAX_ENTRIES = 32;
  const STATUS_TEXT_LIMIT = 500;
  const STATUS_FIELDS = [
    'state', 'message', 'requested', 'actual', 'autoFallback', 'action', 'detail', 'presented',
    'sourceSha256', 'guestBudget', 'imageWidth', 'imageHeight', 'surfaceCapEdge', 'surfaceCapPixels',
    'viewportX', 'viewportY', 'viewportWidth', 'viewportHeight', 'viewportScale', 'viewportClipped',
    'adapterName', 'backend', 'deviceType', 'driver', 'driverInfo', 'vendorId', 'deviceId',
    'surfaceFormat', 'renderFormat', 'presentMode', 'maxTextureDimension2d', 'maxBufferSize',
  ];
  const started = performance.now();
  const elapsed = () => Math.round(performance.now() - started);
  const safeText = (value) => {
    if (value instanceof Error) return `${value.name}: ${value.message}`;
    try {
      return String(value);
    } catch {
      return '[unprintable]';
    }
  };
  const clip = (value) => (typeof value === 'string' ? value.slice(0, STATUS_TEXT_LIMIT) : value ?? null);
  const pickStatus = (status) => {
    if (status === null || typeof status !== 'object') return null;
    const snapshot = {};
    for (const field of STATUS_FIELDS) {
      if (!Object.prototype.hasOwnProperty.call(status, field)) continue;
      const value = status[field];
      if (typeof value === 'string') snapshot[field] = clip(value);
      else if (
        value === null ||
        typeof value === 'boolean' ||
        (typeof value === 'number' && Number.isFinite(value))
      ) {
        snapshot[field] = value;
      }
    }
    return snapshot;
  };
  const transitions = [];
  let lastState = null;
  // Reading the public state attribute never writes to the app.
  const noteState = () => {
    const state = document.body?.dataset?.appState ?? null;
    if (state !== lastState && transitions.length < MAX_ENTRIES) {
      transitions.push({ state, atMs: elapsed() });
      lastState = state;
    }
    return state;
  };
  try {
    const snapshot = (phase) => {
      const api = window.__gbaNative;
      let apiState = null;
      let status = null;
      let statusHistory = [];
      let fallbackNotice = null;
      let statusReadError = null;
      try {
        apiState = typeof api?.state === 'function' ? clip(api.state()) : null;
        status = pickStatus(typeof api?.status === 'function' ? api.status() : null);
        statusHistory = (typeof api?.statusHistory === 'function' ? api.statusHistory() : [])
          .slice(-MAX_ENTRIES)
          .map(pickStatus);
        fallbackNotice = typeof api?.fallbackNotice === 'function' ? clip(api.fallbackNotice()) : null;
      } catch (error) {
        statusReadError = safeText(error);
      }
      const canvas = document.getElementById('gba-canvas') || document.querySelector('canvas');
      const style = canvas ? getComputedStyle(canvas) : null;
      const box = canvas ? canvas.getBoundingClientRect() : null;
      const visible = Boolean(
        canvas &&
        canvas.isConnected &&
        canvas.getClientRects().length > 0 &&
        box.width > 0 &&
        box.height > 0 &&
        !canvas.hidden &&
        style.display !== 'none' &&
        style.visibility !== 'hidden' &&
        Number(style.opacity) > 0,
      );
      return {
        phase,
        atMs: elapsed(),
        bodyState: noteState(),
        apiState,
        status,
        statusHistory,
        statusReadError,
        fallbackNotice,
        canvas: canvas ? {
          width: canvas.width,
          height: canvas.height,
          isConnected: canvas.isConnected,
          hiddenAttribute: canvas.hidden,
          ariaHidden: canvas.getAttribute('aria-hidden'),
          clientRectCount: canvas.getClientRects().length,
          rect: { x: box.x, y: box.y, width: box.width, height: box.height },
          display: style.display,
          visibility: style.visibility,
          opacity: style.opacity,
          visible,
        } : null,
      };
    };

    const startupStarted = performance.now();
    let state = noteState();
    while (
      performance.now() - startupStarted < APP_STARTUP_WAIT_MS &&
      (state === null || state === 'empty' || state === 'loading')
    ) {
      await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
      state = noteState();
    }
    const settled = ['ready', 'error', 'lost'].includes(state);
    const startup = {
      waitLimitMs: APP_STARTUP_WAIT_MS,
      waitMs: Math.round(performance.now() - startupStarted),
      settled,
      state,
    };
    const initial = snapshot('initial');
    const postWorkStarted = performance.now();
    await new Promise((resolve) => setTimeout(resolve, APP_POST_WORK_WAIT_MS));
    const postWork = snapshot('post-work');
    postWork.boundedWaitMs = Math.round(performance.now() - postWorkStarted);

    const lossPattern = /device lost|device was destroyed/i;
    const known = [postWork.status, ...postWork.statusHistory].filter(Boolean);
    const deviceLossSignals = known.filter(
      (entry) => entry.state === 'lost' || lossPattern.test(`${entry.message ?? ''} ${entry.detail ?? ''}`),
    ).length;
    const bodyStates = transitions.map((entry) => entry.state);
    return {
      mode: 'app',
      secureContext: window.isSecureContext,
      origin: location.origin,
      route: `${location.pathname}${location.search}`,
      requestedBackend: new URLSearchParams(location.search).get('backend'),
      startup,
      initial,
      postWork,
      stateTransitions: transitions,
      observed: {
        deviceLoss: bodyStates.includes('lost') || deviceLossSignals > 0,
        errorState: bodyStates.includes('error') || known.some((entry) => entry.state === 'error'),
        readyState: bodyStates.includes('ready'),
        stillLoading: postWork.bodyState === 'loading',
        deviceLossSignals,
      },
    };
  } catch (error) {
    return {
      mode: 'app',
      secureContext: window.isSecureContext,
      origin: location.origin,
      route: `${location.pathname}${location.search}`,
      probeError: safeText(error),
      stateTransitions: transitions,
    };
  }
}

async function main() {
  const origin = validateEnvironment();
  const { chromium } = require(process.env.PLAYWRIGHT_PACKAGE_PATH || 'playwright');
  const args = LAUNCH_ARGS_TEXT.split(/\s+/).filter(Boolean);
  const result = {
    schemaVersion: 1,
    mode: MODE,
    ok: false,
    origin,
    entry: MODE === 'app' ? APP_PATH : PROBE_PATH,
    secureOrigin: true,
    launch: { channel: CHANNEL, headless: true, args, chromiumRevision: CHROMIUM_REVISION },
    browserVersion: null,
    console: [],
    pageErrors: [],
    requestFailures: [],
    timeouts: [],
    browserProcessExitStatus: 'not-exposed-by-Playwright-Browser-API',
    browserTerminationFallback:
      'On a close timeout the harness exits; this closes its Playwright control pipe. No broad process kill is used.',
    browserDisconnected: false,
    earlyBrowserDisconnect: false,
    probe: null,
    harnessError: null,
    processExitStatus: null,
  };
  const html = MODE === 'canvas'
    ? '<!doctype html><meta charset="utf-8"><canvas width="64" height="64"></canvas>'
    : MODE === 'offscreen'
      ? '<!doctype html><meta charset="utf-8"><title>offscreen WebGPU probe</title>'
      : null;
  let browser = null;
  let context = null;
  let page = null;
  let expectedBrowserClose = false;
  const launchedAt = Date.now();
  activeResult = result;

  try {
    browser = await withDeadline(
      () => chromium.launch({
        channel: CHANNEL,
        headless: true,
        args,
        timeout: 30_000,
      }),
      'browser.launch',
      PLAYWRIGHT_TIMEOUTS_MS.browserLaunch,
      (lateBrowser) => withDeadline(
        () => lateBrowser.close(),
        'late browser.close',
        PLAYWRIGHT_TIMEOUTS_MS.browserClose,
      ),
    );
    result.browserVersion = browser.version();
    browser.on('disconnected', () => {
      result.browserDisconnected = true;
      result.browserDisconnectedAtMs = Date.now() - launchedAt;
      result.earlyBrowserDisconnect = !expectedBrowserClose;
    });
    context = await withDeadline(
      () => browser.newContext({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1 }),
      'browser.newContext', PLAYWRIGHT_TIMEOUTS_MS.contextCreate,
    );
    page = await withDeadline(() => context.newPage(), 'context.newPage', PLAYWRIGHT_TIMEOUTS_MS.pageCreate);
    page.on('console', (message) => {
      const entry = { type: message.type(), text: message.text().slice(0, 2_000) };
      if (result.console.length < 100) result.console.push(entry);
    });
    page.on('pageerror', (error) => {
      if (result.pageErrors.length < 100) result.pageErrors.push(safeText(error).slice(0, 2_000));
    });
    page.on('requestfailed', (request) => {
      if (result.requestFailures.length < 100) {
        result.requestFailures.push({ url: request.url(), error: request.failure()?.errorText || 'unknown' });
      }
    });
    const probeUrl = new URL(PROBE_PATH, `${origin}/`).toString();
    if (html !== null) {
      await withDeadline(() => page.route(probeUrl, async (route) => {
        await withDeadline(() => route.fulfill({
          status: 200,
          contentType: 'text/html; charset=utf-8',
          headers: {
            'Content-Security-Policy': "default-src 'none'; script-src 'none'; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'",
            'Cache-Control': 'no-store',
            'X-Content-Type-Options': 'nosniff',
          },
          body: html,
        }), 'route.fulfill', PLAYWRIGHT_TIMEOUTS_MS.route);
      }), 'page.route', PLAYWRIGHT_TIMEOUTS_MS.route);
    }
    // App mode navigates the real served bundle; the standalone modes stay on
    // the routed CSP-restricted document.
    const entryUrl = MODE === 'app' ? new URL(APP_PATH, `${origin}/`).toString() : probeUrl;
    await withDeadline(
      () => page.goto(entryUrl, { waitUntil: 'load', timeout: PLAYWRIGHT_TIMEOUTS_MS.navigation }),
      'page.goto', PLAYWRIGHT_TIMEOUTS_MS.navigation,
    );
    result.probe = MODE === 'app'
      ? await withDeadline(
        () => page.evaluate(inPageAppProbe),
        'page.evaluate(inPageAppProbe)', PLAYWRIGHT_TIMEOUTS_MS.evaluation,
      )
      : await withDeadline(
        () => page.evaluate(inPageProbe, MODE),
        'page.evaluate(inPageProbe)', PLAYWRIGHT_TIMEOUTS_MS.evaluation,
      );

    if (MODE === 'canvas' && result.probe.canvas?.configured) {
      const locator = page.locator('canvas');
      const visible = await withDeadline(
        () => locator.isVisible(), 'canvas.isVisible', PLAYWRIGHT_TIMEOUTS_MS.visibility,
      );
      if (!visible) throw new Error('Playwright did not observe the routed HTMLCanvasElement as visible');
      const screenshot = await withDeadline(
        () => locator.screenshot({ timeout: PLAYWRIGHT_TIMEOUTS_MS.screenshot }),
        'canvas.screenshot', PLAYWRIGHT_TIMEOUTS_MS.screenshot,
      );
      const filename = path.join(OUTPUT_DIR, 'canvas-present.png');
      fs.writeFileSync(filename, screenshot, { flag: 'wx' });
      const sampled = pngPixel(screenshot, 32, 32);
      const pixel = sampled.rgba;
      result.probe.canvas.screenshot = {
        path: filename,
        width: sampled.width,
        height: sampled.height,
        centerPixelRGBA: pixel,
        expectedClearPixelRGB: [255, 0, 255],
        pixelVerified: Math.abs(pixel[0] - 255) <= 16 && pixel[1] <= 16 && Math.abs(pixel[2] - 255) <= 16,
      };
    }
  } catch (error) {
    result.harnessError = safeText(error);
  } finally {
    if (page && MODE !== 'app') {
      try {
        const teardown = await withDeadline(
          () => page.evaluate(async () => {
            const finish = window.__webGpuSmokeTeardown;
            return typeof finish === 'function' ? finish() : null;
          }),
          'page.evaluate(device teardown)', PLAYWRIGHT_TIMEOUTS_MS.teardown,
        );
        if (result.probe) result.probe.expectedDeviceTeardown = teardown;
      } catch (error) {
        result.deviceTeardownError = safeText(error);
      }
    }
    if (context) {
      try {
        await withDeadline(() => context.close(), 'context.close', PLAYWRIGHT_TIMEOUTS_MS.contextClose);
      } catch (error) {
        result.contextCloseError = safeText(error);
      }
    }
    if (browser) {
      expectedBrowserClose = true;
      try {
        await withDeadline(() => browser.close(), 'browser.close', PLAYWRIGHT_TIMEOUTS_MS.browserClose);
      } catch (error) {
        result.browserCloseError = safeText(error);
      }
    }
  }

  const probe = result.probe;
  const appOutcomeObserved = [probe?.initial, probe?.postWork].some((snapshot) =>
    ['ready', 'error', 'lost'].includes(snapshot?.apiState) ||
    ['ready', 'error', 'lost'].includes(snapshot?.status?.state),
  );
  const appDiagnosticsCaptured = Boolean(
    probe?.startup?.settled &&
    ['ready', 'error', 'lost'].includes(probe.startup.state) &&
    appOutcomeObserved &&
    probe?.initial && probe?.postWork &&
    !probe?.probeError,
  );
  const probeWorkSucceeded = MODE === 'app'
    ? appDiagnosticsCaptured
    : MODE === 'canvas'
      ? Boolean(probe?.canvas?.configured && probe.canvas.submissions === 2 && probe.canvas.screenshot?.pixelVerified)
      : Boolean(probe?.offscreen?.outputMatches && probe.offscreen.commandSubmissions === 1 && !probe.offscreen.canvasCreated);
  // In app mode a device loss or an app error state is the observation itself,
  // not a harness failure. Console, page and request failures stay recorded;
  // only harness integrity fails closed there.
  const derivedProblemsClear = MODE === 'app' || (
    (probe?.uncapturedErrors?.length || 0) === 0 &&
    result.console.every((entry) => entry.type !== 'error') &&
    result.pageErrors.length === 0 &&
    result.requestFailures.length === 0
  );
  const earlyDeviceLoss = MODE !== 'app' && Boolean(
    probe?.deviceLossBeforeTeardown || probe?.expectedDeviceTeardown?.preTeardownLoss,
  );
  result.ok = Boolean(
    probeWorkSucceeded &&
    !probe?.workError &&
    !probe?.probeError &&
    !earlyDeviceLoss &&
    derivedProblemsClear &&
    result.timeouts.length === 0 &&
    !result.harnessError &&
    !result.deviceTeardownError &&
    !result.earlyBrowserDisconnect &&
    !result.contextCloseError &&
    !result.browserCloseError
  );
  if (MODE === 'app') {
    result.app = {
      route: APP_PATH,
      startup: probe?.startup ?? null,
      initialBodyState: probe?.initial?.bodyState ?? null,
      postWorkBodyState: probe?.postWork?.bodyState ?? null,
      postWorkStatusState: probe?.postWork?.status?.state ?? null,
      deviceLossObserved: Boolean(probe?.observed?.deviceLoss),
      errorStateObserved: Boolean(probe?.observed?.errorState),
      diagnosticsCaptured: appDiagnosticsCaptured,
    };
  }
  result.processExitStatus = result.ok ? 0 : 1;
  result.elapsedMs = Date.now() - launchedAt;
  clearTimeout(hardDeadlineTimer);
  writeJson('result.json', result);
  fs.writeSync(1, `WEBGPU_SMOKE_RESULT ${JSON.stringify(result)}\n`);
  process.exit(result.processExitStatus);
}

main().catch((error) => {
  clearTimeout(hardDeadlineTimer);
  const result = activeResult || {
    schemaVersion: 1,
    mode: MODE,
    entry: MODE === 'app' ? APP_PATH : PROBE_PATH,
    launch: {
      channel: CHANNEL,
      headless: true,
      args: LAUNCH_ARGS_TEXT.split(/\s+/).filter(Boolean),
      chromiumRevision: CHROMIUM_REVISION,
    },
    timeouts: [],
  };
  result.ok = false;
  result.harnessError ||= safeText(error);
  result.processExitStatus = 1;
  try {
    writeJson('result.json', result);
  } catch (writeError) {
    try { fs.writeSync(2, `could not persist result.json: ${safeText(writeError)}\n`); } catch {}
  }
  try { fs.writeSync(2, `WEBGPU_SMOKE_RESULT ${JSON.stringify(result)}\n`); } catch {}
  process.exit(1);
});
