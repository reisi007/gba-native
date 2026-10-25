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

const byId = (id) => document.getElementById(id);
const params = new URLSearchParams(window.location.search);
const initialRequest = params.get("backend") ?? "auto";
const initialBackend = normalizeBackend(initialRequest);
let appState = "empty";
let lastStatus = null;
let pollTimer = null;
let fallbackNotice = "";
let fallbackActive = false;

window.__gbaNative = {
  state: () => appState,
  status: () => lastStatus,
  fallbackNotice: () => fallbackNotice,
};

function normalizeBackend(value) {
  return value === "webgpu" || value === "webgl2" ? value : "auto";
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
    return;
  }
  status = statusForFallbackPage(status, fallbackActive, fallbackNotice);
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
  window.__gbaNative = { state: () => appState, status: () => lastStatus, fallbackNotice: () => fallbackNotice };
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
  let gl = "unavailable";
  try {
    const probe = document.createElement("canvas");
    const context = probe.getContext("webgl2");
    if (context) {
      gl = "available";
      context.getExtension("WEBGL_lose_context")?.loseContext();
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

main();
