// UI-review screenshot config. Deliberately separate from the functional
// Playwright config: this set only captures pixels, asserts nothing about game
// behaviour and never runs in CI.
//
// Output paths come from WEB_REVIEW_OUT (set by scripts/web-screenshots.sh to
// an ignored, uniquely named output/web-review-* directory).

import { defineConfig } from '@playwright/test';

const baseURL = process.env.WEB_APP_URL;
if (!baseURL) throw new Error('WEB_APP_URL is required (start scripts/web-screenshots.sh)');

const outputDir = process.env.WEB_REVIEW_OUT;
if (!outputDir) throw new Error('WEB_REVIEW_OUT is required');

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

export default defineConfig({
  testDir: './screenshots',
  testMatch: '**/*.spec.ts',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: false,
  timeout: 120_000,
  expect: { timeout: 30_000 },
  reporter: [['list']],
  // Playwright wipes outputDir at the start of every run. Keep it a dedicated
  // subdirectory so the PNG state/viewport tree is never touched by that wipe.
  outputDir: `${outputDir}/.playwright-artifacts`,
  use: {
    baseURL,
    headless: process.env.WEB_E2E_HEADLESS !== '0',
    channel,
    launchOptions: { args: launchArgs },
    trace: 'off',
    video: 'off',
    screenshot: 'off',
  },
  projects: [
    {
      name: 'desktop',
      use: { viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 },
    },
    {
      name: 'mobile-portrait',
      use: {
        viewport: { width: 390, height: 844 },
        deviceScaleFactor: 2,
        isMobile: true,
        hasTouch: true,
      },
    },
    {
      name: 'mobile-landscape',
      use: {
        viewport: { width: 844, height: 390 },
        deviceScaleFactor: 2,
        isMobile: true,
        hasTouch: true,
      },
    },
  ],
});
