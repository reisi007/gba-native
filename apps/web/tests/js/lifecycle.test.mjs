import assert from "node:assert/strict";
import test from "node:test";
import {
  automaticFallbackDecision,
  compatibilityPathDescription,
  consumeRestartBudget,
  FALLBACK_KEY,
  FALLBACK_REASON_KEY,
  fallbackNoticeText,
  MAX_RESTARTS,
  RESTART_KEY,
  restoreFallbackNotice,
  statusForFallbackPage,
} from "../../www/lifecycle.mjs";

function memoryStorage() {
  const values = new Map();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, String(value)),
    removeItem: (key) => values.delete(key),
    values,
  };
}

test("runtime auto fallback keeps its original reason through loading and ready", () => {
  const storage = memoryStorage();
  const failure = {
    state: "error",
    action: "reload-webgl2",
    requested: "auto",
    detail: "WebGPU device creation failed: transient adapter error",
  };
  assert.deepEqual(automaticFallbackDecision(failure, storage), {
    kind: "retry",
    reason: failure.detail,
  });
  assert.equal(storage.getItem(FALLBACK_KEY), "used");
  assert.equal(storage.getItem(FALLBACK_REASON_KEY), failure.detail);

  const restored = restoreFallbackNotice(storage, true);
  assert.deepEqual(restored, { ok: true, active: true, reason: failure.detail });
  const loading = statusForFallbackPage(
    { state: "loading", requested: "webgl2" },
    restored.active,
    restored.reason,
  );
  const ready = statusForFallbackPage({
    state: "ready",
    requested: "webgl2",
    actual: "webgl2",
    autoFallback: false,
  }, restored.active, restored.reason);
  assert.equal(loading.requested, "auto");
  assert.equal(ready.requested, "auto");
  assert.equal(ready.actual, "webgl2");
  assert.equal(ready.autoFallback, true);
  assert.match(
    compatibilityPathDescription(ready),
    /initialization failed: WebGPU device creation failed: transient adapter error/,
  );
  assert.match(fallbackNoticeText(restored.reason), /transient adapter error/);
  assert.equal(storage.getItem(FALLBACK_REASON_KEY), failure.detail);
});

test("forced backend failures never enter automatic fallback", () => {
  const storage = memoryStorage();
  assert.deepEqual(automaticFallbackDecision({
    state: "error",
    requested: "webgpu",
    message: "WebGPU was requested explicitly but is unavailable",
  }, storage), { kind: "none" });
  assert.equal(storage.getItem(FALLBACK_KEY), null);
});

test("automatic fallback fails closed when storage cannot persist its guard", () => {
  const denied = {
    getItem() { throw new Error("denied"); },
    setItem() { throw new Error("denied"); },
    removeItem() { throw new Error("denied"); },
  };
  assert.deepEqual(automaticFallbackDecision({ action: "reload-webgl2", detail: "adapter failed" }, denied), {
    kind: "blocked",
    reason: "sessionStorage is unavailable",
  });
});

test("manual restart budget requires reliable storage and allows exactly three in 30 seconds", () => {
  assert.equal(consumeRestartBudget(null, 10_000).allowed, false);
  let failWrites = true;
  const deniedWrite = {
    getItem: () => null,
    setItem() { if (failWrites) throw new Error("denied"); },
    removeItem() {},
  };
  assert.equal(consumeRestartBudget(deniedWrite, 10_000).allowed, false);
  failWrites = false;
  const storage = memoryStorage();
  for (let index = 0; index < MAX_RESTARTS; index += 1) {
    assert.equal(consumeRestartBudget(storage, 10_000 + index).allowed, true);
  }
  assert.equal(consumeRestartBudget(storage, 10_003).allowed, false);
  assert.equal(JSON.parse(storage.getItem(RESTART_KEY)).length, MAX_RESTARTS);
  assert.equal(consumeRestartBudget(storage, 41_000).allowed, true);
});

test("corrupt restart history fails closed instead of resetting the bound", () => {
  const storage = memoryStorage();
  storage.setItem(RESTART_KEY, "not-json");
  assert.equal(consumeRestartBudget(storage, 50_000).allowed, false);
  assert.equal(storage.getItem(RESTART_KEY), "not-json");
});
