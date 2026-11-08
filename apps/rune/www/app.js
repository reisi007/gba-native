const saveInput = document.querySelector('#rune-save-import');
const saveSlotLabel = / · \d+ bytes(?: · default)?(?: · selected)?$/;

function syncSaveActionAvailability(root) {
  const buttons = [...root.querySelectorAll('button')];
  const selectedSlot = buttons.find((button) => {
    const label = button.textContent?.trim() ?? '';
    return saveSlotLabel.test(label) && label.endsWith(' · selected');
  });
  const selectedLabel = selectedSlot?.textContent?.trim() ?? '';
  const disabledActions = new Map([
    ['Export', !selectedSlot],
    ['Make default', !selectedSlot || selectedLabel.includes(' · default · selected')],
    ['Delete', !selectedSlot],
  ]);

  for (const button of buttons) {
    if (button.textContent?.trim() === 'Import fixture') {
      // Rune handles canvas actions on a later frame. The browser's file picker
      // requires transient user activation, so open it synchronously from the
      // positioned semantic button while the real click is still on the stack.
      button.style.pointerEvents = 'auto';
      if (button.dataset.runeImportBound !== 'true') {
        button.dataset.runeImportBound = 'true';
        button.addEventListener('click', () => {
          let hasPendingFile = false;
          try {
            hasPendingFile = localStorage.getItem('gba-native.rune.save.pending') !== null;
          } catch (_) {
            // Still let the browser picker open when storage is unavailable.
          }
          if (!hasPendingFile) saveInput.click();
        });
      }
    }
    const disabled = disabledActions.get(button.textContent?.trim() ?? '');
    if (disabled === undefined) continue;
    button.disabled = disabled;
    if (disabled) button.setAttribute('aria-disabled', 'true');
    else button.removeAttribute('aria-disabled');
  }
}

function watchSaveActionAvailability() {
  let a11yMirror = document.querySelector('[data-rune-a11y]');
  const watchMirror = (root) => {
    const mirrorObserver = new MutationObserver(() => syncSaveActionAvailability(root));
    mirrorObserver.observe(root, { childList: true, subtree: true, characterData: true });
    syncSaveActionAvailability(root);
  };

  if (a11yMirror) {
    watchMirror(a11yMirror);
    return;
  }

  const documentObserver = new MutationObserver(() => {
    a11yMirror = document.querySelector('[data-rune-a11y]');
    if (!a11yMirror) return;
    documentObserver.disconnect();
    watchMirror(a11yMirror);
  });
  documentObserver.observe(document.documentElement, { childList: true, subtree: true });
}

saveInput.addEventListener('change', async () => {
  const file = saveInput.files?.[0];
  if (!file) return;
  if (file.size > 128 * 1024) {
    saveInput.value = '';
    return;
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  let hex = '';
  for (const byte of bytes) hex += byte.toString(16).padStart(2, '0');
  try {
    localStorage.setItem('gba-native.rune.save.pending', hex);
  } catch (_) {
    // The Rune save panel reports storage errors when the user imports.
  }
  saveInput.value = '';
});

try {
  const { default: init } = await import('./pkg/gba_native_rune.js');
  await init();
  watchSaveActionAvailability();
  document.querySelector('#status').remove();
} catch (error) {
  document.querySelector('#status').textContent = `Unable to start Rune: ${error}`;
  console.error(error);
}
