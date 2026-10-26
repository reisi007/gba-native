// Functional browser E2E for the synthetic gba-native web foundation.
//
// This config is only for behaviour/evidence checks. The visual screenshot set
// lives in playwright.screenshots.config.ts and never runs here.

import { defineConfig } from '@playwright/test';

const baseURL = process.env.WEB_APP_URL;
if (!baseURL) throw new Error('WEB_APP_URL is required (start scripts/web-e2e.sh)');

const channel = process.env.WEB_E2E_CHANNEL || 'chromium';
const launchArgs = (
  process.env.WEB_E2E_LAUNCH_ARGS ??
  '--enable-unsafe-webgpu --use-webgpu-adapter=swiftshader ' +
    '--enable-dawn-features=allow_unsafe_apis --disable-dawn-features=use_dxc ' +
    '--enable-webgpu-developer-features --use-gpu-in-tests ' +
    '--enable-accelerated-2d-canvas'
)
  .split(/\s+/)
  .filter(Boolean);
const outputDir = process.env.WEB_E2E_OUTPUT_DIR || './test-results/functional';

export default defineConfig({
  testDir: '.',
  testMatch: 'functional.spec.ts',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: !!process.env.CI,
  timeout: 120_000,
  expect: { timeout: 30_000 },
  reporter: [['list']],
  outputDir,
  use: {
    baseURL,
    headless: process.env.WEB_E2E_HEADLESS !== '0',
    channel,
    launchOptions: { args: launchArgs },
    trace: 'off',
    video: 'off',
    screenshot: 'off',
  },
});
