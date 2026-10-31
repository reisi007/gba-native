// Browser save library: multiple named save images per ROM digest.
//
// The bytes are raw backup images (the same artifact the native CLI stores).
// They are kept in localStorage as base64; nothing is uploaded. This module is
// deliberately independent of the GPU/input code so Node tests can exercise the
// storage, import/export and default-slot behaviour directly.

export const SAVE_SCHEMA = 1;
export const MAX_SAVE_BYTES = 128 * 1024;
export const MAX_SLOT_NAME = 32;
export const STORAGE_KEY = "gba-native-saves-v1";
export const SYNTHETIC_SAVE_ROM = {
  sha: "f8dbe41f4dcd1e134759a23cce988b4a05f1185f102b3153d69240600e76c4e3",
  name: "SAVETEST",
};

export function validRomDigest(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/i.test(value);
}

export function validSlotName(value) {
  return (
    typeof value === "string" &&
    value.length >= 1 &&
    value.length <= MAX_SLOT_NAME &&
    !value.startsWith(".") &&
    !["__proto__", "prototype", "constructor"].includes(value) &&
    /^[A-Za-z0-9._-]+$/.test(value)
  );
}

export function bytesToBase64(bytes) {
  let binary = "";
  const chunk = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunk) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunk));
  }
  return btoa(binary);
}

export function base64ToBytes(value) {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

function emptyDocument() {
  return { schema: SAVE_SCHEMA, roms: {} };
}

export class SaveStore {
  constructor(storage = globalThis.localStorage, key = STORAGE_KEY) {
    this.storage = storage;
    this.key = key;
  }

  readDocument() {
    const raw = this.storage.getItem(this.key);
    if (!raw) return emptyDocument();
    try {
      const parsed = JSON.parse(raw);
      if (parsed?.schema !== SAVE_SCHEMA || typeof parsed.roms !== "object" || parsed.roms === null) {
        return emptyDocument();
      }
      return parsed;
    } catch {
      return emptyDocument();
    }
  }

  writeDocument(document) {
    this.storage.setItem(this.key, JSON.stringify(document));
  }

  rom(document, digest) {
    if (!validRomDigest(digest)) throw new Error("invalid ROM digest");
    const key = digest.toLowerCase();
    document.roms[key] ??= { default: "", slots: {} };
    return document.roms[key];
  }

  list(digest) {
    const document = this.readDocument();
    if (!validRomDigest(digest)) return [];
    const rom = document.roms[digest.toLowerCase()];
    if (!rom || typeof rom.slots !== "object" || rom.slots === null) return [];
    return Object.entries(rom.slots)
      .filter(([name]) => validSlotName(name))
      .map(([name, slot]) => ({
        name,
        bytes: base64ToBytes(slot.data),
        default: rom.default === name,
      }))
      .sort((left, right) => left.name.localeCompare(right.name));
  }

  defaultSlot(digest) {
    const document = this.readDocument();
    if (!validRomDigest(digest)) return "";
    return document.roms[digest.toLowerCase()]?.default ?? "";
  }

  save(digest, name, bytes) {
    if (!validSlotName(name)) throw new Error("invalid slot name");
    if (!bytes || bytes.length < 1 || bytes.length > MAX_SAVE_BYTES) {
      throw new Error("save image must be 1..=131072 bytes");
    }
    const document = this.readDocument();
    const rom = this.rom(document, digest);
    rom.slots[name] = { data: bytesToBase64(bytes) };
    if (!validSlotName(rom.default) || !Object.hasOwn(rom.slots, rom.default)) {
      rom.default = name;
    }
    this.writeDocument(document);
    return { name, default: rom.default === name };
  }

  remove(digest, name) {
    const document = this.readDocument();
    const rom = this.rom(document, digest);
    if (!Object.hasOwn(rom.slots, name)) throw new Error(`unknown save slot ${name}`);
    delete rom.slots[name];
    if (rom.default === name) {
      rom.default = Object.keys(rom.slots).sort()[0] ?? "";
    }
    this.writeDocument(document);
    return rom.default;
  }

  setDefault(digest, name) {
    const document = this.readDocument();
    const rom = this.rom(document, digest);
    if (!Object.hasOwn(rom.slots, name)) throw new Error(`unknown save slot ${name}`);
    rom.default = name;
    this.writeDocument(document);
    return name;
  }
}
