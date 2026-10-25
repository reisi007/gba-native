// DOM-free lifecycle rules shared by the page and Node regression tests.

export const RESTART_KEY = "gba-native-web-restarts";
export const FALLBACK_KEY = "gba-native-web-autofallback";
export const FALLBACK_REASON_KEY = "gba-native-web-fallback-reason";
export const RESTART_WINDOW_MS = 30_000;
export const MAX_RESTARTS = 3;

export function readItem(storage, key) {
  if (!storage) return { ok: false, value: null };
  try {
    return { ok: true, value: storage.getItem(key) };
  } catch {
    return { ok: false, value: null };
  }
}

export function writeItem(storage, key, value) {
  if (!storage) return false;
  try {
    storage.setItem(key, value);
    return true;
  } catch {
    return false;
  }
}

export function removeItem(storage, key) {
  if (!storage) return false;
  try {
    storage.removeItem(key);
    return true;
  } catch {
    return false;
  }
}

export function automaticFallbackDecision(status, storage) {
  if (status.action !== "reload-webgl2") return { kind: "none" };
  const used = readItem(storage, FALLBACK_KEY);
  if (!used.ok) return { kind: "blocked", reason: "sessionStorage is unavailable" };
  if (used.value === "used") return { kind: "already-used" };

  const reason = String(status.detail ?? status.message ?? "WebGPU initialization failed");
  if (!writeItem(storage, FALLBACK_KEY, "used")) {
    return { kind: "blocked", reason: "sessionStorage could not record the retry guard" };
  }
  if (!writeItem(storage, FALLBACK_REASON_KEY, reason)) {
    removeItem(storage, FALLBACK_KEY);
    return { kind: "blocked", reason: "sessionStorage could not preserve the WebGPU failure reason" };
  }
  return { kind: "retry", reason };
}

export function restoreFallbackNotice(storage, requested) {
  if (!requested) return { ok: true, active: false, reason: "" };
  const used = readItem(storage, FALLBACK_KEY);
  const saved = readItem(storage, FALLBACK_REASON_KEY);
  if (!used.ok || !saved.ok) return { ok: false, active: false, reason: "" };
  if (used.value !== "used" || !saved.value) {
    return { ok: false, active: false, reason: "" };
  }
  return { ok: true, active: true, reason: saved.value };
}

export function statusForFallbackPage(status, active, reason = "") {
  if (!active || status.requested !== "webgl2") return status;
  return {
    ...status,
    requested: "auto",
    actual: status.actual ?? "webgl2",
    autoFallback: true,
    fallbackReason: reason,
  };
}

export function fallbackNoticeText(reason) {
  return `Automatic WebGL2 compatibility retry after WebGPU failed: ${reason}`;
}

export function compatibilityPathDescription(status) {
  if (!status.autoFallback) return "no";
  return status.fallbackReason
    ? `yes — WebGL2 after WebGPU initialization failed: ${status.fallbackReason}`
    : "yes — WebGL2 after WebGPU was unavailable";
}

export function consumeRestartBudget(storage, now) {
  const stored = readItem(storage, RESTART_KEY);
  if (!stored.ok) return { allowed: false, reason: "sessionStorage is unavailable" };

  let stamps = [];
  if (stored.value !== null) {
    try {
      stamps = JSON.parse(stored.value);
    } catch {
      return { allowed: false, reason: "the saved restart limit is unreadable" };
    }
    if (!Array.isArray(stamps) || stamps.some((stamp) => typeof stamp !== "number" || !Number.isFinite(stamp))) {
      return { allowed: false, reason: "the saved restart limit is invalid" };
    }
  }

  stamps = stamps.filter((stamp) => stamp >= now - RESTART_WINDOW_MS);
  if (stamps.length >= MAX_RESTARTS) return { allowed: false, reason: "restart budget reached" };
  stamps.push(now);
  if (!writeItem(storage, RESTART_KEY, JSON.stringify(stamps))) {
    return { allowed: false, reason: "sessionStorage could not persist the restart limit" };
  }
  return { allowed: true, reason: "" };
}
