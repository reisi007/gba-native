//! IndexedDB persistence for browser save profiles.

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r##"
const databaseName = "gba-native.rune.saves";
const profileStore = "profiles";
const pendingKey = "gba-native.rune.save.pending";
const maxHexLength = 2 * 128 * 1024;
const completed = [];
let opening = null;
let nextRequestId = 1;

function validName(name) {
  return typeof name === "string" && /^[A-Za-z0-9_-]{1,32}$/.test(name);
}
function validProfile(id) {
  return typeof id === "string" && /^[A-Za-z0-9._-]{1,32}$/.test(id);
}
function validRawHex(value) {
  return typeof value === "string" && value.length >= 2 && value.length <= maxHexLength &&
    value.length % 2 === 0 && /^[\da-f]+$/i.test(value);
}
function validSyntheticHex(value) {
  if (!validRawHex(value) || value.length !== 1024) return false;
  const bytes = Uint8Array.from(value.match(/../g), (byte) => Number.parseInt(byte, 16));
  if (String.fromCharCode(...bytes.slice(0, 8)) !== "GBANSV01") return false;
  const seed = bytes[8];
  for (let index = 9; index < bytes.length; index += 1) {
    if (bytes[index] !== ((index * 31 + seed) & 0xff)) return false;
  }
  return true;
}
function safeStorage() {
  try { return globalThis.localStorage; } catch (_) { return null; }
}
function openDatabase() {
  if (!globalThis.indexedDB) return Promise.reject(new Error("IndexedDB is unavailable in this browser context"));
  if (opening) return opening;
  opening = new Promise((resolve, reject) => {
    let request;
    let settled = false;
    const fail = (error) => {
      if (settled) return;
      settled = true;
      opening = null;
      reject(error instanceof Error ? error : new Error(String(error)));
    };
    try { request = globalThis.indexedDB.open(databaseName, 1); }
    catch (error) { fail(error); return; }
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(profileStore)) db.createObjectStore(profileStore, { keyPath: "id" });
    };
    request.onblocked = () => fail(new Error("IndexedDB upgrade is blocked by another open tab"));
    request.onerror = () => fail(request.error ?? new Error("IndexedDB could not be opened"));
    request.onsuccess = () => {
      if (settled) { request.result.close(); return; }
      settled = true;
      request.result.onversionchange = () => request.result.close();
      resolve(request.result);
    };
  });
  return opening;
}
function transactionFailure(tx, fallback) {
  return tx.error ?? fallback ?? new Error("IndexedDB transaction was aborted");
}
async function database() { return openDatabase(); }
function recordFor(id, stored) {
  if (!stored) return { id, slots: {}, defaultSlot: null };
  if (stored.id !== id || !stored.slots || typeof stored.slots !== "object" || Array.isArray(stored.slots)) {
    throw new Error("stored profile record is invalid; no data was changed");
  }
  return stored;
}
function updateProfile(db, id, mutate) {
  return new Promise((resolve, reject) => {
    let tx;
    try { tx = db.transaction(profileStore, "readwrite"); }
    catch (error) { reject(error); return; }
    const store = tx.objectStore(profileStore);
    let result;
    let failure = null;
    tx.oncomplete = () => resolve(result);
    tx.onerror = () => {};
    tx.onabort = () => reject(transactionFailure(tx, failure));
    const request = store.get(id);
    request.onerror = () => { failure = request.error; try { tx.abort(); } catch (_) {} };
    request.onsuccess = () => {
      try {
        const record = recordFor(id, request.result);
        const outcome = mutate(record);
        result = outcome.value;
        if (outcome.write) store.put(record);
      } catch (error) {
        failure = error;
        try { tx.abort(); } catch (_) {}
      }
    };
  });
}
function readProfile(db, id, read) {
  return new Promise((resolve, reject) => {
    let tx;
    try { tx = db.transaction(profileStore, "readonly"); }
    catch (error) { reject(error); return; }
    let result;
    let failure = null;
    tx.oncomplete = () => resolve(result);
    tx.onerror = () => {};
    tx.onabort = () => reject(transactionFailure(tx, failure));
    const request = tx.objectStore(profileStore).get(id);
    request.onerror = () => { failure = request.error; try { tx.abort(); } catch (_) {} };
    request.onsuccess = () => {
      try { result = read(recordFor(id, request.result)); }
      catch (error) { failure = error; try { tx.abort(); } catch (_) {} }
    };
  });
}
function syntheticImageHex(seed) {
  const bytes = new Uint8Array(512);
  bytes.set([0x47, 0x42, 0x41, 0x4e, 0x53, 0x56, 0x30, 0x31]);
  bytes[8] = seed & 0xff;
  for (let index = 9; index < bytes.length; index += 1) bytes[index] = (index * 31 + seed) & 0xff;
  let encoded = "";
  for (const byte of bytes) encoded += byte.toString(16).padStart(2, "0");
  return encoded;
}
function selectNextName(slots) {
  let number = Object.keys(slots).length + 1;
  for (;;) {
    const name = `slot${number}`;
    if (!validName(name)) throw new Error("no unused valid synthetic save slot name is available");
    if (!Object.hasOwn(slots, name)) return name;
    number += 1;
  }
}
function encodeSlots(record) {
  return Object.keys(record.slots).sort().flatMap((name) => {
    const hex = record.slots[name];
    if (!validName(name) || !validRawHex(hex)) return [];
    return [`${name}\t${hex.length / 2}\t${record.defaultSlot === name ? "1" : "0"}`];
  }).join("\n");
}
function requestProfiles(db) {
  return new Promise((resolve, reject) => {
    let tx;
    try { tx = db.transaction(profileStore, "readonly"); }
    catch (error) { reject(error); return; }
    let names = ["default"];
    let failure = null;
    tx.oncomplete = () => resolve([...new Set(names)].sort((a, b) => a === "default" ? -1 : b === "default" ? 1 : a.localeCompare(b)).join("\n"));
    tx.onerror = () => {};
    tx.onabort = () => reject(transactionFailure(tx, failure));
    const request = tx.objectStore(profileStore).getAllKeys();
    request.onerror = () => { failure = request.error; try { tx.abort(); } catch (_) {} };
    request.onsuccess = () => { names = names.concat(request.result.filter((id) => validProfile(id))); };
  });
}
async function createProfile(db) {
  return new Promise((resolve, reject) => {
    let tx;
    try { tx = db.transaction(profileStore, "readwrite"); }
    catch (error) { reject(error); return; }
    const store = tx.objectStore(profileStore);
    let created;
    let failure = null;
    tx.oncomplete = () => resolve(created);
    tx.onerror = () => {};
    tx.onabort = () => reject(transactionFailure(tx, failure));
    const request = store.getAllKeys();
    request.onerror = () => { failure = request.error; try { tx.abort(); } catch (_) {} };
    request.onsuccess = () => {
      try {
        const existing = new Set(request.result);
        let number = 1;
        while (existing.has(`profile-${number}`)) number += 1;
        created = `profile-${number}`;
        store.add({ id: created, slots: {}, defaultSlot: null });
      } catch (error) { failure = error; try { tx.abort(); } catch (_) {} }
    };
  });
}
function importHex() {
  const storage = safeStorage();
  let hex;
  try { hex = storage?.getItem(pendingKey) ?? null; }
  catch (_) { throw new Error("staged import is unavailable because browser storage is blocked"); }
  if (!validSyntheticHex(hex)) throw new Error("only exact generated GBANSV01 synthetic save fixtures are accepted");
  return hex;
}
async function store(db, profile, name, hex, consumeStagedImport) {
  if (!validName(name) || !validSyntheticHex(hex)) throw new Error("only exact generated GBANSV01 synthetic save fixtures are accepted");
  const slot = await updateProfile(db, profile, (record) => {
    record.slots[name] = hex;
    if (!record.defaultSlot) record.defaultSlot = name;
    return { value: name, write: true };
  });
  if (consumeStagedImport) {
    try { safeStorage()?.removeItem(pendingKey); } catch (_) {}
  }
  return slot;
}
function download(name, hex) {
  if (!validName(name) || !validRawHex(hex)) throw new Error("save image is invalid");
  try {
    const bytes = Uint8Array.from(hex.match(/../g), (byte) => Number.parseInt(byte, 16));
    const blob = new Blob([bytes], { type: "application/octet-stream" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `${name}.sav`;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  } catch (_) { throw new Error("browser download is unavailable"); }
}
async function execute(operation, profile, name, value) {
  if (!validProfile(profile)) throw new Error("invalid save profile id");
  const db = await database();
  if (operation === "profiles") return requestProfiles(db);
  if (operation === "create-profile") return createProfile(db);
  if (operation === "slots") return readProfile(db, profile, encodeSlots);
  if (operation === "create") {
    return updateProfile(db, profile, (record) => {
      const slot = selectNextName(record.slots);
      record.slots[slot] = syntheticImageHex(Object.keys(record.slots).length & 0xff);
      if (!record.defaultSlot) record.defaultSlot = slot;
      return { value: slot, write: true };
    });
  }
  if (operation === "store") {
    return store(db, profile, name, value, false);
  }
  if (operation === "import") {
    return store(db, profile, "imported", importHex(), true);
  }
  if (operation === "delete") {
    if (!validName(name)) throw new Error("invalid save slot name");
    return updateProfile(db, profile, (record) => {
      if (!validRawHex(record.slots[name])) throw new Error(`save slot ${name} is unavailable`);
      delete record.slots[name];
      if (record.defaultSlot === name) record.defaultSlot = Object.keys(record.slots).sort()[0] ?? null;
      return { value: name, write: true };
    });
  }
  if (operation === "default") {
    if (!validName(name)) throw new Error("invalid save slot name");
    return updateProfile(db, profile, (record) => {
      if (!validRawHex(record.slots[name])) throw new Error(`save slot ${name} is unavailable`);
      record.defaultSlot = name;
      return { value: name, write: true };
    });
  }
  if (operation === "export") {
    if (!validName(name)) throw new Error("invalid save slot name");
    const hex = await readProfile(db, profile, (record) => record.slots[name]);
    if (!validRawHex(hex)) throw new Error(`save slot ${name} is unavailable`);
    download(name, hex);
    return `downloaded ${name}.sav`;
  }
  throw new Error("unsupported browser save operation");
}
function errorMessage(error) {
  const name = error?.name && error.name !== "Error" ? `${error.name}: ` : "";
  const message = error?.message ?? String(error);
  return `${name}${message}`.replace(/[\t\r\n]/g, " ");
}
export function runeSaveStart(operation, profile, name, value) {
  const id = nextRequestId++;
  Promise.resolve().then(() => execute(operation, profile, name, value)).then(
    (result) => completed.push(`${id}\t1\t${result ?? ""}`),
    (error) => completed.push(`${id}\t0\t${errorMessage(error)}`),
  );
  return id;
}
export function runeSaveTakeResult() { return completed.shift() ?? null; }
"##)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveStart)]
    fn browser_start(operation: &str, profile: &str, name: &str, value: &str) -> u32;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveTakeResult)]
    fn browser_take_result() -> Option<String>;
}

pub(crate) fn start(operation: &str, profile: &str, name: &str, value: &str) -> u32 {
    browser_start(operation, profile, name, value)
}

pub(crate) fn take_result() -> Option<String> {
    browser_take_result()
}
