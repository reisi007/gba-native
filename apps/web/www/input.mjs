// Portable browser input/settings model for the synthetic GBA shell.
//
// The key codes and default bindings match crates/core/src/input. Settings are
// stored as a small JSON document in localStorage; no remote storage, account
// or cartridge data is involved. The touch rects mirror the core landscape
// layout closely enough for pointer hit testing and are pure functions so they
// can be unit-tested under Node.

export const BUTTONS = ["A", "B", "Select", "Start", "Right", "Left", "Up", "Down", "R", "L"];

export const DEFAULT_BINDINGS = {
  A: ["KeyZ", "KeyJ"],
  B: ["KeyX", "KeyK"],
  Select: ["Backspace"],
  Start: ["Enter"],
  Right: ["ArrowRight", "KeyD"],
  Left: ["ArrowLeft", "KeyA"],
  Up: ["ArrowUp", "KeyW"],
  Down: ["ArrowDown", "KeyS"],
  R: ["KeyE"],
  L: ["KeyQ"],
};

export const DEFAULT_SETTINGS = {
  schema: 1,
  fullscreen: false,
  touchControls: true,
  integerScaling: true,
  masterVolume: 80,
  muted: false,
  bindings: DEFAULT_BINDINGS,
};

export const STORAGE_KEY = "gba-native.web.settings.v1";

export function normaliseSettings(value) {
  const source = value && typeof value === "object" ? value : {};
  const bindings = {};
  for (const button of BUTTONS) {
    const keys = Array.isArray(source.bindings?.[button])
      ? source.bindings[button].filter((key) => typeof key === "string" && key.length > 0)
      : [];
    bindings[button] = keys.length > 0 ? [...new Set(keys)] : [...DEFAULT_BINDINGS[button]];
  }
  const volume = Number(source.masterVolume);
  return {
    schema: 1,
    fullscreen: source.fullscreen === true,
    touchControls: source.touchControls !== false,
    integerScaling: source.integerScaling !== false,
    masterVolume: Number.isFinite(volume) ? Math.min(100, Math.max(0, Math.round(volume))) : 80,
    muted: source.muted === true,
    bindings,
  };
}

export function parseSettings(text) {
  try {
    return normaliseSettings(JSON.parse(text));
  } catch {
    return normaliseSettings(DEFAULT_SETTINGS);
  }
}

export function serialiseSettings(settings) {
  return JSON.stringify(normaliseSettings(settings));
}

export function loadSettings(storage) {
  try {
    const text = storage?.getItem(STORAGE_KEY);
    return text ? parseSettings(text) : normaliseSettings(DEFAULT_SETTINGS);
  } catch {
    return normaliseSettings(DEFAULT_SETTINGS);
  }
}

export function saveSettings(storage, settings) {
  try {
    storage?.setItem(STORAGE_KEY, serialiseSettings(settings));
    return true;
  } catch {
    return false;
  }
}

export function buttonForCode(code, bindings) {
  for (const button of BUTTONS) {
    if ((bindings[button] ?? []).includes(code)) return button;
  }
  return null;
}

export function maskForButtons(buttons) {
  let mask = 0;
  for (const button of buttons) {
    const index = BUTTONS.indexOf(button);
    if (index >= 0) mask |= 1 << index;
  }
  return mask;
}

export function bindingLabel(button, bindings) {
  return (bindings[button] ?? []).map(humanKey).join(" / ");
}

export function humanKey(code) {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code === "ArrowUp") return "Up";
  if (code === "ArrowDown") return "Down";
  if (code === "ArrowLeft") return "Left";
  if (code === "ArrowRight") return "Right";
  if (code === "Backspace") return "Backspace";
  if (code === "Escape") return "Esc";
  return code;
}

// Normalised landscape shell rects, mirroring the core GbaLayout.
export const LANDSCAPE = {
  screen: { x: 0.2, y: 0.0667, width: 0.6, height: 0.5333 },
  controls: {
    L: { x: 0.02, y: 0.04, width: 0.14, height: 0.07 },
    R: { x: 0.84, y: 0.04, width: 0.14, height: 0.07 },
    Up: { x: 0.1, y: 0.64, width: 0.08, height: 0.12 },
    Down: { x: 0.1, y: 0.8, width: 0.08, height: 0.12 },
    Left: { x: 0.02, y: 0.72, width: 0.08, height: 0.12 },
    Right: { x: 0.18, y: 0.72, width: 0.08, height: 0.12 },
    B: { x: 0.76, y: 0.72, width: 0.1, height: 0.14 },
    A: { x: 0.88, y: 0.66, width: 0.1, height: 0.14 },
    Select: { x: 0.36, y: 0.86, width: 0.09, height: 0.1 },
    Start: { x: 0.52, y: 0.86, width: 0.09, height: 0.1 },
  },
};

export function hitTest(x, y, layout = LANDSCAPE) {
  if (!(x >= 0 && x <= 1 && y >= 0 && y <= 1)) return null;
  for (const [button, rect] of Object.entries(layout.controls)) {
    if (x >= rect.x && y >= rect.y && x <= rect.x + rect.width && y <= rect.y + rect.height) {
      return button;
    }
  }
  return null;
}
