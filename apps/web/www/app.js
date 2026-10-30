// Browser shell for the synthetic gba-native GPU foundation.
//
// The Rust/WASM module owns execution, GPU initialization and the facts it
// observes. This file owns DOM text and the bounded page lifecycle:
//   - Start runs the synthetic fixture once.
//   - Restart reloads the page with the selected backend (a new canvas is
//     required because a canvas context type cannot change).
//   - One automatic WebGPU -> WebGL2 compatibility retry, recorded once.
//   - Restart budget: at most three explicit restarts per 30 seconds.
// No ROM, save, upload, remote asset or game-input path exists here.

import init, { poll_gpu_status, set_status_sink, start } from "./pkg/gba_native_web.js";
import {
  automaticFallbackDecision,
  compatibilityPathDescription,
  consumeRestartBudget,
  fallbackNoticeText,
  FALLBACK_KEY,
  FALLBACK_REASON_KEY,
  removeItem,
  restoreFallbackNotice,
  statusForFallbackPage,
} from "./lifecycle.mjs";
import {
  BUTTONS,
  bindingLabel,
  buttonForCode,
  loadSettings,
  maskForButtons,
  normaliseSettings,
  saveSettings,
} from "./input.mjs";
const POLL_MS = 500;

const STATE_LABEL = {
  empty: "Not started",
  loading: "Initializing",
  ready: "Presented",
  error: "Error",
  lost: "Context lost",
};
const STATE_BADGE = {
  empty: "badge-ghost",
  loading: "badge-info",
  ready: "badge-success",
  error: "badge-error",
  lost: "badge-warning",
};
const ACTIVE_STATES = new Set(["loading", "ready"]);
const STARTED_STATES = new Set(["loading", "ready", "error", "lost"]);
const STATUS_HISTORY_LIMIT = 32;
const STATUS_TEXT_LIMIT = 500;
const REQUESTED_BACKEND_LIMIT = 160;
const TRUNCATION_MARKER = "… [truncated]";
const STATUS_SNAPSHOT_FIELDS = [
  "state", "message", "requested", "actual", "autoFallback", "action", "detail", "presented",
  "sourceSha256", "guestBudget", "imageWidth", "imageHeight", "surfaceCapEdge", "surfaceCapPixels",
  "viewportX", "viewportY", "viewportWidth", "viewportHeight", "viewportScale", "viewportClipped",
  "adapterName", "backend", "deviceType", "driver", "driverInfo", "vendorId", "deviceId",
  "surfaceFormat", "renderFormat", "presentMode", "maxTextureDimension2d", "maxBufferSize",
];
const statusHistory = [];

const byId = (id) => document.getElementById(id);
const params = new URLSearchParams(window.location.search);
const initialRequest = params.get("backend") ?? "auto";
const initialBackend = normalizeBackend(initialRequest);
let appState = "empty";
let lastStatus = null;
let pollTimer = null;
let fallbackNotice = "";
let fallbackActive = false;

function nativeDiagnostics() {
  return {
    state: () => appState,
    // Export only a detached, field-limited snapshot. The native payload is
    // flat, and arbitrary caller-added/nested data must never alias lastStatus.
    status: () => diagnosticStatusSnapshot(lastStatus),
    fallbackNotice: () => fallbackNotice,
    statusHistory: () => statusHistory.map((entry) => ({ ...entry })),
  };
}

window.__gbaNative = nativeDiagnostics();

function normalizeBackend(value) {
  return value === "webgpu" || value === "webgl2" ? value : "auto";
}

function boundedText(value, limit) {
  const text = String(value);
  if (text.length <= limit) return text;
  return `${text.slice(0, limit - TRUNCATION_MARKER.length)}${TRUNCATION_MARKER}`;
}

function boundedRequestedBackend(value) {
  if (value == null) return null;
  const requested = String(value);
  // Preserve useful invalid input while making truncation explicit in the
  // exported diagnostic snapshot; supported backend IDs stay exact.
  if (requested === "auto" || requested === "webgpu" || requested === "webgl2") return requested;
  return boundedText(requested, REQUESTED_BACKEND_LIMIT);
}

function diagnosticStatusSnapshot(status) {
  if (status === null || typeof status !== "object") return null;
  const snapshot = {};
  for (const field of STATUS_SNAPSHOT_FIELDS) {
    if (!Object.prototype.hasOwnProperty.call(status, field)) continue;
    const value = status[field];
    if (field === "requested") {
      snapshot[field] = boundedRequestedBackend(value);
    } else if (typeof value === "string") {
      snapshot[field] = boundedText(value, STATUS_TEXT_LIMIT);
    } else if (
      value === null ||
      typeof value === "boolean" ||
      (typeof value === "number" && Number.isFinite(value))
    ) {
      snapshot[field] = value;
    }
  }
  return snapshot;
}

function boundedOptionalText(value, limit = STATUS_TEXT_LIMIT) {
  return value == null ? null : boundedText(value, limit);
}

function finiteNumber(value, fallback = null) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function rememberStatus(status, disposition) {
  statusHistory.push({
    atMs: Math.round(performance.now()),
    phase: appState,
    disposition,
    state: boundedText(status.state ?? "", 80),
    message: boundedOptionalText(status.message) ?? "",
    requested: boundedRequestedBackend(status.requested),
    actual: boundedOptionalText(status.actual, 120),
    action: boundedOptionalText(status.action, 120),
    detail: boundedOptionalText(status.detail),
    backend: boundedOptionalText(status.backend, 120),
    adapterName: boundedOptionalText(status.adapterName, 200),
    deviceType: boundedOptionalText(status.deviceType, 120),
    surfaceFormat: boundedOptionalText(status.surfaceFormat, 120),
    renderFormat: boundedOptionalText(status.renderFormat, 120),
    presented: finiteNumber(status.presented ?? 0, 0),
    viewportX: finiteNumber(status.viewportX),
    viewportY: finiteNumber(status.viewportY),
    viewportWidth: finiteNumber(status.viewportWidth),
    viewportHeight: finiteNumber(status.viewportHeight),
    viewportScale: finiteNumber(status.viewportScale),
  });
  if (statusHistory.length > STATUS_HISTORY_LIMIT) statusHistory.shift();
}

function sessionStorageOrNull() {
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
}

function setState(next) {
  appState = next;
  document.body.dataset.appState = next;
  const badge = byId("state-badge");
  badge.textContent = STATE_LABEL[next] ?? next;
  badge.dataset.state = next;
  badge.classList.remove(...Object.values(STATE_BADGE));
  badge.classList.add(STATE_BADGE[next] ?? "badge-ghost");

  byId("start-button").hidden = next !== "empty";
  byId("start-button").disabled = next !== "empty";
  const restart = byId("restart-button");
  restart.hidden = !STARTED_STATES.has(next);
  restart.disabled = next === "loading";
  byId("screen-slot").setAttribute("aria-busy", next === "loading" ? "true" : "false");

  renderOverlay(next);
}

function renderOverlay(state) {
  const spinner = byId("overlay-spinner");
  const title = byId("overlay-title");
  const hint = byId("overlay-hint");
  spinner.classList.toggle("hidden", state !== "loading");
  if (state === "empty") {
    title.textContent = "No frame displayed yet";
    hint.textContent = "Press “Start synthetic demo”. Nothing is executed or drawn before that.";
  } else if (state === "loading") {
    title.textContent = "Initializing the synthetic demo…";
    hint.textContent =
      "Executing the fixture once, then creating a real GPU surface. Initialization is deferred until the canvas has a non-zero physical size.";
  } else if (state === "error") {
    title.textContent = "No frame was presented";
    hint.textContent = "Read the status panel. A forced backend never pretends to succeed.";
  } else if (state === "lost") {
    title.textContent = "Rendering halted after GPU context/device loss";
    hint.textContent = "Use Restart to create a fresh canvas. No fake frame is kept on screen.";
  }
}

function reportRow(label, value) {
  if (value === undefined || value === null || value === "") return;
  const term = document.createElement("dt");
  term.className = "font-medium opacity-70";
  term.textContent = label;
  const detail = document.createElement("dd");
  detail.className = "break-words";
  detail.textContent = String(value);
  byId("report").append(term, detail);
}

function describeViewport(status) {
  if (status.viewportWidth === undefined) return null;
  const suffix = status.viewportClipped ? " (source clipped below 1×)" : "";
  return `${status.viewportWidth}×${status.viewportHeight} at (${status.viewportX}, ${status.viewportY}) · integer scale ${status.viewportScale}${suffix}`;
}

function renderReport(status) {
  byId("report").replaceChildren();
  reportRow("Requested backend", status.requested);
  reportRow("Actual backend", status.actual);
  reportRow("Auto compatibility path", compatibilityPathDescription(status));
  reportRow("Adapter", status.adapterName || (status.actual ? "(not exposed by this browser)" : null));
  reportRow("WGPU backend", status.backend);
  reportRow("Device type", status.deviceType || (status.actual ? "(not exposed by this browser)" : null));
  reportRow("Driver", [status.driver, status.driverInfo].filter(Boolean).join(" · "));
  reportRow(
    "Vendor / device",
    status.vendorId === undefined ? null : `0x${status.vendorId.toString(16).padStart(4, "0")} / 0x${status.deviceId.toString(16).padStart(4, "0")}`,
  );
  reportRow("Surface format", status.surfaceFormat);
  reportRow("Pixel/render format", status.renderFormat);
  reportRow("Present mode", status.presentMode);
  reportRow("Adapter max 2D texture", status.maxTextureDimension2d);
  reportRow("Adapter max buffer", status.maxBufferSize);
  reportRow("Viewport", describeViewport(status));
  reportRow(
    "Render acknowledgements",
    status.presented === undefined
      ? null
      : `${status.presented} Pixel::render acknowledgement(s) — not guest frames or FPS`,
  );
  reportRow("Source SHA-256 (RGB)", status.sourceSha256);
  reportRow(
    "Guest instruction budget",
    status.guestBudget === undefined ? null : `${status.guestBudget} executed once — not frames/FPS`,
  );
}

function renderStatus(status) {
  // Lost/error are terminal for this page instance: a late ready/viewport status
  // from the Rust redraw path must not re-show a dead canvas or claim success.
  if ((appState === 'lost' || appState === 'error') && (status.state === 'ready' || status.state === 'loading')) {
    rememberStatus(status, "ignored-after-terminal");
    return;
  }
  status = statusForFallbackPage(status, fallbackActive, fallbackNotice);
  rememberStatus(status, "applied");
  lastStatus = status;
  byId("status-message").textContent = status.message ?? "";
  renderReport(status);

  const alert = byId("status-alert");
  const detail = byId("status-detail");
  if (status.state === "error" || status.state === "lost") {
    alert.classList.remove("hidden", "alert-error", "alert-warning");
    alert.classList.add(status.state === "lost" ? "alert-warning" : "alert-error");
    detail.textContent = status.detail ?? status.message ?? "Unspecified GPU error.";
  } else if (fallbackNotice) {
    alert.classList.remove("hidden", "alert-error");
    alert.classList.add("alert-warning");
    detail.textContent = fallbackNoticeText(fallbackNotice);
  } else {
    alert.classList.add("hidden");
    detail.textContent = "";
  }

  const presented = Number(status.presented ?? 0);
  byId("present-badge").textContent = `${presented} render acknowledgement${presented === 1 ? "" : "s"}`;

  if (status.sourceSha256) {
    byId("source-line").textContent = `Source: synthetic fixture 240×160, sha256(RGB)=${status.sourceSha256}, ${status.guestBudget} guest instructions executed once (not frames or FPS).`;
  }

  if (status.state === "ready") {
    setState("ready");
    startPolling();
  } else if (status.state === "error") {
    startPollingStop();
    maybeCompatibilityRetry(status);
  } else if (status.state === "lost") {
    startPollingStop();
    setState("lost");
  } else if (status.state === "loading") {
    setState("loading");
  }
  window.__gbaNative = nativeDiagnostics();
}

function startPolling() {
  startPollingStop();
  pollTimer = window.setInterval(() => {
    let reason = null;
    try {
      reason = poll_gpu_status();
    } catch (error) {
      reason = `status poll failed: ${String(error)}`;
    }
    if (reason) {
      startPollingStop();
      renderStatus({
        ...(lastStatus ?? {}),
        state: "lost",
        message: "The GPU device was lost; rendering is halted. Restart to create a fresh canvas.",
        detail: reason,
      });
    }
  }, POLL_MS);
}

function startPollingStop() {
  if (pollTimer !== null) {
    window.clearInterval(pollTimer);
    pollTimer = null;
  }
}

function maybeCompatibilityRetry(status) {
  const decision = automaticFallbackDecision(status, sessionStorageOrNull());
  if (decision.kind === "none") {
    setState("error");
    return;
  }
  if (decision.kind === "already-used") {
    setState("error");
    byId("status-message").textContent =
      "WebGPU initialization failed and the single automatic WebGL2 compatibility retry was already used. Choose WebGL2 explicitly or Restart.";
    return;
  }
  if (decision.kind === "blocked") {
    setState("error");
    byId("status-message").textContent =
      "Automatic WebGL2 retry was blocked because sessionStorage could not persist its one-retry guard. Enable sessionStorage and reload manually with WebGL2; no automatic reload was performed.";
    const alert = byId("status-alert");
    alert.classList.remove("hidden", "alert-error");
    alert.classList.add("alert-warning");
    byId("status-detail").textContent =
      `${decision.reason}; WebGPU failure: ${status.detail ?? status.message ?? "initialization failed"}`;
    return;
  }
  fallbackNotice = decision.reason;
  fallbackActive = true;
  setState("loading");
  byId("status-message").textContent =
    "WebGPU initialization failed. Reloading once with the WebGL2 compatibility path; the recorded reason is shown after the reload.";
  window.location.assign(buildUrl("webgl2", { autostart: "1", fallback: "webgpu-init" }));
}

function buildUrl(backend, extra = {}) {
  const url = new URL(window.location.href);
  const search = new URLSearchParams();
  search.set("backend", backend);
  for (const [key, value] of Object.entries(extra)) search.set(key, value);
  url.search = search.toString();
  url.hash = "";
  return url.toString();
}

function restart() {
  if (appState === "loading") return;
  const budget = consumeRestartBudget(sessionStorageOrNull(), Date.now());
  if (!budget.allowed) {
    const message = budget.reason === "restart budget reached"
      ? "Restart budget reached: at most three restarts per 30 seconds. Wait for the window to pass, then try again."
      : `Restart blocked because its reload limit could not be enforced (${budget.reason}). Enable sessionStorage, then use Restart again; no reload was performed.`;
    byId("status-message").textContent = message;
    const alert = byId("status-alert");
    alert.classList.remove("hidden", "alert-error");
    alert.classList.add("alert-warning");
    byId("status-detail").textContent = message;
    return;
  }
  removeItem(sessionStorageOrNull(), FALLBACK_KEY);
  removeItem(sessionStorageOrNull(), FALLBACK_REASON_KEY);
  fallbackNotice = "";
  fallbackActive = false;
  window.location.assign(buildUrl(normalizeBackend(byId("backend-select").value), { autostart: "1" }));
}

async function beginStart(choice) {
  if (ACTIVE_STATES.has(appState) || appState === "lost" || appState === "error") return;
  setState("loading");
  try {
    await start(choice);
  } catch (error) {
    renderStatus({
      state: "error",
      message: "The WASM entry point failed before GPU initialization.",
      detail: String(error),
      requested: normalizeBackend(choice),
    });
  }
}

function probeCapabilities() {
  const gpu = "gpu" in navigator ? "present" : "absent";
  byId("probe-gpu").textContent = gpu;
  if (initialBackend !== "webgl2") {
    byId("probe-gl").textContent =
      "not probed here; auto checks WebGL2 only if WebGPU is unusable, and forced WebGPU never falls back";
    return;
  }
  let gl = "unavailable";
  try {
    const probe = document.createElement("canvas");
    const context = probe.getContext("webgl2");
    if (context) {
      gl = "available";
    }
  } catch {
    gl = "unavailable";
  }
  byId("probe-gl").textContent = gl;
}

function wireUi() {
  byId("backend-select").value = initialBackend;
  byId("start-button").addEventListener("click", () => {
    void beginStart(byId("backend-select").value);
  });
  byId("restart-button").addEventListener("click", restart);
  byId("backend-select").addEventListener("change", () => {
    byId("backend-hint").textContent =
      "Backend selected. It applies when you press Start or Restart; a reload creates a fresh canvas because the context type cannot change.";
  });
  window.addEventListener("pagehide", startPollingStop);
}

async function main() {
  probeCapabilities();
  wireUi();
  setState("empty");

  const restored = restoreFallbackNotice(
    sessionStorageOrNull(),
    params.get("fallback") === "webgpu-init",
  );
  if (restored.active) {
    fallbackNotice = restored.reason;
    fallbackActive = true;
    const alert = byId("status-alert");
    alert.classList.remove("hidden", "alert-error");
    alert.classList.add("alert-warning");
    byId("status-detail").textContent = fallbackNoticeText(fallbackNotice);
  } else if (params.get("fallback") === "webgpu-init" && !restored.ok) {
    const alert = byId("status-alert");
    alert.classList.remove("hidden", "alert-error");
    alert.classList.add("alert-warning");
    byId("status-detail").textContent =
      "The automatic fallback reason or retry guard could not be read from sessionStorage. No fallback success is claimed.";
  }

  try {
    await init();
    set_status_sink(renderStatus);
  } catch (error) {
    renderStatus({
      state: "error",
      message: "The WebAssembly module could not be initialized.",
      detail: String(error),
      requested: initialBackend,
    });
    return;
  }

  if (params.get("autostart") === "1") {
    // Preserve an invalid query value: Rust validates it and reports a real
    // configuration error instead of silently treating it as auto.
    await beginStart(initialRequest);
    if (params.get("scene") === "lose-context") await loseContextWhenReady();
  }
}

async function loseContextWhenReady() {
  for (let attempt = 0; attempt < 80; attempt += 1) {
    if (appState === "ready") {
      const canvas = byId("gba-canvas");
      const context = canvas.getContext("webgl2");
      if (context) {
        context.getExtension("WEBGL_lose_context")?.loseContext();
        return;
      }
      renderStatus({
        ...(lastStatus ?? {}),
        state: "error",
        message: "The context-loss scenario is only meaningful on the WebGL2 path.",
        detail: "No WebGL2 context was available on the canvas.",
        requested: lastStatus?.requested ?? "webgl2",
      });
      return;
    }
    if (appState === "error" || appState === "lost") return;
    await new Promise((resolve) => window.setTimeout(resolve, 100));
  }
}

// ---------------------------------------------------------------------------
// Windowed GBA shell: touch/pointer buttons, key bindings, fullscreen and the
// persisted settings page. This layer maps documented KEYINPUT bits only; it
// never uploads or reads cartridge data.

const inputState = {
  settings: loadSettings(window.localStorage),
  pressed: new Set(),
};

function persistInputSettings() {
  saveSettings(window.localStorage, inputState.settings);
}

function inputMask() {
  return maskForButtons(inputState.pressed);
}

function renderInputStatus() {
  const status = byId("input-status");
  if (!status) return;
  const mask = inputMask();
  status.dataset.mask = String(mask);
  status.textContent =
    inputState.pressed.size === 0
      ? "input: released"
      : `input: ${[...inputState.pressed].join(" + ")} (0x${mask.toString(16).padStart(4, "0")})`;
}

function setButtonVisual(button, active) {
  for (const element of document.querySelectorAll(`[data-gba-button="${button}"]`)) {
    element.classList.toggle("is-active", active);
    element.setAttribute("aria-pressed", active ? "true" : "false");
  }
}

function pressButton(button) {
  if (!button) return;
  inputState.pressed.add(button);
  setButtonVisual(button, true);
  renderInputStatus();
}

function releaseButton(button) {
  if (!button) return;
  inputState.pressed.delete(button);
  setButtonVisual(button, false);
  renderInputStatus();
}

function releaseAllButtons() {
  for (const button of [...inputState.pressed]) releaseButton(button);
}

function renderSettings() {
  const settings = inputState.settings;
  const list = byId("binding-list");
  if (list) {
    list.replaceChildren();
    for (const button of BUTTONS) {
      const term = document.createElement("dt");
      term.className = "font-medium";
      term.textContent = button;
      const value = document.createElement("dd");
      value.className = "opacity-80 break-words";
      value.textContent = bindingLabel(button, settings.bindings);
      list.append(term, value);
    }
  }
  byId("touch-toggle").checked = settings.touchControls;
  byId("integer-toggle").checked = settings.integerScaling;
  byId("fullscreen-toggle").checked = settings.fullscreen;
  byId("volume-range").value = String(settings.masterVolume);
  byId("volume-value").textContent = String(settings.masterVolume);
  byId("mute-toggle").checked = settings.muted;
  document.body.classList.toggle("touch-controls", settings.touchControls);
  document.body.classList.toggle("integer-scaling", settings.integerScaling);
  byId("settings-button").setAttribute(
    "aria-expanded",
    byId("settings-panel").hidden ? "false" : "true",
  );
}

function showSettings(visible) {
  const panel = byId("settings-panel");
  panel.hidden = !visible;
  byId("settings-button").setAttribute("aria-expanded", visible ? "true" : "false");
  if (visible) panel.scrollIntoView({ block: "nearest" });
}

async function toggleFullscreen() {
  const screen = byId("screen-slot");
  try {
    if (document.fullscreenElement) {
      await document.exitFullscreen();
    } else {
      await screen.requestFullscreen();
    }
  } catch {
    // Headless browsers and embedding contexts can deny the Fullscreen API.
    // Keep a CSS-only windowed/fullscreen switch so the layout is testable.
    document.body.classList.toggle("input-fullscreen");
    inputState.settings.fullscreen = document.body.classList.contains("input-fullscreen");
    persistInputSettings();
    renderSettings();
  }
}

let audioContext = null;

function audioGain() {
  const settings = inputState.settings;
  return settings.muted ? 0 : settings.masterVolume / 100;
}

function playTestTone() {
  const Context = window.AudioContext ?? window.webkitAudioContext;
  if (!Context) return false;
  audioContext ??= new Context();
  const oscillator = audioContext.createOscillator();
  const gain = audioContext.createGain();
  oscillator.type = "square";
  oscillator.frequency.value = 440;
  gain.gain.value = 0.2 * audioGain();
  oscillator.connect(gain);
  gain.connect(audioContext.destination);
  oscillator.start();
  oscillator.stop(audioContext.currentTime + 0.4);
  return true;
}

function setupInputUi() {
  renderSettings();
  renderInputStatus();

  byId("settings-button").addEventListener("click", () => {
    showSettings(byId("settings-panel").hidden);
  });
  byId("close-settings").addEventListener("click", () => showSettings(false));
  byId("fullscreen-button").addEventListener("click", () => {
    void toggleFullscreen();
  });
  byId("apply-fullscreen").addEventListener("click", () => {
    void toggleFullscreen();
  });
  byId("fullscreen-toggle").addEventListener("change", (event) => {
    inputState.settings.fullscreen = event.target.checked;
    persistInputSettings();
    void toggleFullscreen();
  });
  byId("touch-toggle").addEventListener("change", (event) => {
    inputState.settings.touchControls = event.target.checked;
    persistInputSettings();
    if (!event.target.checked) releaseAllButtons();
    renderSettings();
  });
  byId("integer-toggle").addEventListener("change", (event) => {
    inputState.settings.integerScaling = event.target.checked;
    persistInputSettings();
    renderSettings();
  });
  byId("volume-range").addEventListener("input", (event) => {
    inputState.settings.masterVolume = Number(event.target.value);
    persistInputSettings();
    renderSettings();
  });
  byId("mute-toggle").addEventListener("change", (event) => {
    inputState.settings.muted = event.target.checked;
    persistInputSettings();
    renderSettings();
  });
  byId("test-tone").addEventListener("click", () => {
    void playTestTone();
  });
  byId("reset-settings").addEventListener("click", () => {
    inputState.settings = normaliseSettings();
    persistInputSettings();
    renderSettings();
  });

  document.addEventListener("fullscreenchange", () => {
    const active = document.fullscreenElement !== null;
    document.body.classList.toggle("input-fullscreen", active);
    inputState.settings.fullscreen = active;
    persistInputSettings();
    byId("fullscreen-button").textContent = active ? "Windowed" : "Fullscreen";
    renderSettings();
  });

  const isFormControl = (target) =>
    ["SELECT", "INPUT", "TEXTAREA", "BUTTON", "A"].includes(target?.tagName ?? "");

  const formControlHasFocus = () => isFormControl(document.activeElement);

  // Keep keyboard-only backend selection deterministic across browser
  // platforms. Native select popups differ (some open on Alt+ArrowDown and
  // leave the value unchanged in headless mode), so a focused select steps its
  // own value on ArrowUp/ArrowDown and commits immediately. Enter is swallowed
  // because the value is already committed; other native keys still work.
  const backendSelect = byId("backend-select");
  backendSelect.addEventListener("keydown", (event) => {
    if (event.altKey) {
      event.preventDefault();
      return;
    }
    if (event.code === "ArrowDown" || event.code === "ArrowUp") {
      event.preventDefault();
      const delta = event.code === "ArrowDown" ? 1 : -1;
      const next = Math.min(
        backendSelect.options.length - 1,
        Math.max(0, backendSelect.selectedIndex + delta),
      );
      backendSelect.selectedIndex = next;
      backendSelect.dispatchEvent(new Event("change", { bubbles: true }));
      return;
    }
    if (event.code === "Enter") {
      event.preventDefault();
    }
  });

  document.addEventListener("keydown", (event) => {
    if (isFormControl(event.target) || formControlHasFocus()) return;
    if (event.code === "F1") {
      event.preventDefault();
      showSettings(byId("settings-panel").hidden);
      return;
    }
    if (event.code === "F11" || (event.code === "Enter" && event.altKey)) {
      event.preventDefault();
      void toggleFullscreen();
      return;
    }
    if (event.code === "KeyM") {
      event.preventDefault();
      inputState.settings.muted = !inputState.settings.muted;
      persistInputSettings();
      renderSettings();
      return;
    }
    if (event.code === "Minus" || event.code === "Equal") {
      event.preventDefault();
      const delta = event.code === "Minus" ? -10 : 10;
      inputState.settings.masterVolume = Math.min(
        100,
        Math.max(0, inputState.settings.masterVolume + delta),
      );
      persistInputSettings();
      renderSettings();
      return;
    }
    const button = buttonForCode(event.code, inputState.settings.bindings);
    if (button) {
      event.preventDefault();
      pressButton(button);
    }
  });
  document.addEventListener("keyup", (event) => {
    if (isFormControl(event.target) || formControlHasFocus()) return;
    const button = buttonForCode(event.code, inputState.settings.bindings);
    if (button) {
      event.preventDefault();
      releaseButton(button);
    }
  });
  window.addEventListener("blur", releaseAllButtons);

  for (const element of document.querySelectorAll("[data-gba-button]")) {
    const button = element.dataset.gbaButton;
    element.setAttribute("aria-pressed", "false");
    element.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      element.setPointerCapture?.(event.pointerId);
      pressButton(button);
    });
    for (const name of ["pointerup", "pointercancel", "lostpointercapture"]) {
      element.addEventListener(name, (event) => {
        event.preventDefault?.();
        releaseButton(button);
      });
    }
    element.addEventListener("pointerleave", () => releaseButton(button));
    element.addEventListener("contextmenu", (event) => event.preventDefault());
  }

  window.__gbaNative.input = {
    mask: inputMask,
    pressed: () => [...inputState.pressed],
    settings: () => JSON.parse(JSON.stringify(inputState.settings)),
    press: pressButton,
    release: releaseButton,
    showSettings,
    fullscreen: () => document.fullscreenElement !== null || document.body.classList.contains("input-fullscreen"),
  };
  window.__gbaNative.audio = {
    settings: () => ({
      masterVolume: inputState.settings.masterVolume,
      muted: inputState.settings.muted,
      gain: audioGain(),
    }),
    playTestTone,
    contextState: () => audioContext?.state ?? "closed",
  };
}

setupInputUi();
main();
