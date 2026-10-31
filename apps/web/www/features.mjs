// Browser declaration of the shared app feature contract.
//
// `features.test.mjs` compares this list with FEATURE_IDS in
// crates/core/src/frontend/features.rs, so losing a feature fails the web gate.

export const FEATURES = [
  "demo-frame",
  "touch-controls",
  "fullscreen",
  "settings",
  "audio-artifact",
  "audio-playback",
  "save-slots",
  "save-import-export",
];
