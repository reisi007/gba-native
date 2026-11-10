// Tests the bootstrap's status handling with a stub, not a browser or GPU.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('../www/app.js', import.meta.url), 'utf8');

async function start(init, importFailure = null, options = {}) {
  const status = { removed: false, textContent: '', remove() { this.removed = true; } };
  const saveInput = {
    files: [], value: '', clicks: 0, listeners: {},
    addEventListener(name, fn) { this.listeners[name] = fn; },
    click() { this.clicks += 1; },
  };
  const windowState = {};
  if (options.pendingRom) windowState.__gbaNativeRunePendingRom = options.pendingRom;
  const values = new Map();
  const errors = [];
  const observers = [];
  const requested = [];
  const documentElement = {};
  let a11yMirror = null;
  class MutationObserverStub {
    constructor(callback) { this.callback = callback; observers.push(this); }
    observe(target) { this.target = target; }
    disconnect() { this.disconnected = true; }
    trigger() { this.callback(); }
  }
  const context = vm.createContext({
    document: { querySelector(selector) {
      if (selector === '#status') return status;
      if (selector === '#rune-save-import') return saveInput;
      if (selector === '[data-rune-a11y]') return a11yMirror;
      assert.fail(`unexpected selector: ${selector}`);
    }, documentElement },
    localStorage: {
      getItem(key) { return values.get(key) ?? null; },
      setItem(key, value) { values.set(key, value); },
    },
    window: windowState,
    location: { href: 'http://localhost/' },
    fetch: async (url) => {
      requested.push(String(url));
      return {
        ok: true,
        arrayBuffer: async () => Uint8Array.of(1, 2).buffer,
      };
    },
    MutationObserver: MutationObserverStub,
    Uint8Array,
    URL,
    console: { error(error) { errors.push(error); } },
  });
  const app = new vm.SourceTextModule(source, {
    context,
    async importModuleDynamically(specifier) {
      assert.equal(specifier, './pkg/gba_native_rune.js');
      if (importFailure) throw importFailure;
      const exports = { default: init, ...(options.exports ?? {}) };
      const glue = new vm.SyntheticModule(Object.keys(exports), function () {
        for (const [name, value] of Object.entries(exports)) this.setExport(name, value);
      }, { context });
      await glue.link(() => {});
      await glue.evaluate();
      return glue;
    },
  });
  await app.link(() => {});
  await app.evaluate();
  return {
    status,
    errors,
    saveInput,
    windowState,
    values,
    requested,
    observers,
    documentElement,
    setA11yMirror(root) { a11yMirror = root; },
  };
}

function action(label) {
  const attributes = new Map();
  const listeners = new Map();
  return {
    textContent: label,
    disabled: false,
    style: {},
    dataset: {},
    addEventListener(name, fn) { listeners.set(name, fn); },
    click() { listeners.get('click')?.(); },
    setAttribute(name, value) { attributes.set(name, value); },
    removeAttribute(name) { attributes.delete(name); },
    getAttribute(name) { return attributes.get(name) ?? null; },
  };
}

function mirror(buttons) {
  return { querySelectorAll(selector) { assert.equal(selector, 'button'); return buttons; } };
}

test('successful initialization removes the loading status', async () => {
  const { status, errors } = await start(async () => {});
  assert.equal(status.removed, true);
  assert.deepEqual(errors, []);
});

test('failed initialization keeps a readable error', async () => {
  const failure = new Error('unavailable');
  const { status, errors } = await start(async () => { throw failure; });
  assert.equal(status.removed, false);
  assert.equal(status.textContent, 'Unable to start Rune: Error: unavailable');
  assert.deepEqual(errors, [failure]);
});

test('a failed glue import also reports an error', async () => {
  const failure = new Error('missing glue');
  const { status, errors } = await start(async () => {}, failure);
  assert.equal(status.removed, false);
  assert.equal(status.textContent, 'Unable to start Rune: Error: missing glue');
  assert.deepEqual(errors, [failure]);
});

test('a chosen file is staged for the Rune synthetic-save importer', async () => {
  const { saveInput, values } = await start(async () => {});
  saveInput.files = [{ size: 2, async arrayBuffer() { return Uint8Array.from([0x47, 0x42]).buffer; } }];
  await saveInput.listeners.change();
  assert.equal(values.get('gba-native.rune.save.pending'), '4742');
});

test('oversized save files are rejected before reading their payload', async () => {
  const { saveInput, values } = await start(async () => {});
  saveInput.files = [{
    size: 128 * 1024 + 1,
    async arrayBuffer() { assert.fail('oversized payload must not be read'); },
  }];
  await saveInput.listeners.change();
  assert.equal(saveInput.value, '');
  assert.equal(values.has('gba-native.rune.save.pending'), false);
});

test('save action availability follows delayed Rune mirror and selected-slot updates', async () => {
  const app = await start(async () => {});
  const exportAction = action('Export');
  const defaultAction = action('Make default');
  const deleteAction = action('Delete');
  const alwaysAvailable = action('New synthetic slot');
  const buttons = [exportAction, defaultAction, deleteAction, alwaysAvailable];
  app.setA11yMirror(mirror(buttons));

  const documentObserver = app.observers[0];
  assert.equal(documentObserver.target, app.documentElement);
  documentObserver.trigger();
  assert.equal(exportAction.disabled, true);
  assert.equal(exportAction.getAttribute('aria-disabled'), 'true');
  assert.equal(defaultAction.disabled, true);
  assert.equal(deleteAction.disabled, true);
  assert.equal(alwaysAvailable.disabled, false);

  buttons.push(action('slot-1 · 512 bytes · default · selected'));
  app.observers[1].trigger();
  assert.equal(exportAction.disabled, false);
  assert.equal(defaultAction.disabled, true);
  assert.equal(deleteAction.disabled, false);

  buttons[4].textContent = 'slot-1 · 512 bytes · selected';
  app.observers[1].trigger();
  assert.equal(defaultAction.disabled, false);
  assert.equal(defaultAction.getAttribute('aria-disabled'), null);
});

test('Import fixture opens the native file picker synchronously only when no file is staged', async () => {
  const app = await start(async () => {});
  const importAction = action('Import fixture');
  app.setA11yMirror(mirror([importAction]));
  app.observers[0].trigger();
  app.observers[1].trigger();

  assert.equal(importAction.style.pointerEvents, 'auto');
  importAction.click();
  assert.equal(app.saveInput.clicks, 1);

  app.values.set('gba-native.rune.save.pending', '4742');
  importAction.click();
  assert.equal(app.saveInput.clicks, 1, 'the second click consumes the staged file instead');
});

test('a bound ROM request uses the same-origin URL only without a browser preload', async () => {
  const bound = { bound_rom_url: () => './emerald.gba' };
  const app = await start(async () => {}, null, { exports: bound });
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.deepEqual(app.requested, ['http://localhost/emerald.gba']);
  assert.deepEqual([...app.windowState.__gbaNativeRunePendingRom], [1, 2]);

  const preloaded = await start(async () => {}, null, {
    exports: bound,
    pendingRom: Uint8Array.of(3),
  });
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.deepEqual(preloaded.requested, []);
  assert.deepEqual([...preloaded.windowState.__gbaNativeRunePendingRom], [3]);
});
