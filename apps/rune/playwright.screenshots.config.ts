import { defineConfig } from '@playwright/test';

const baseURL = process.env.RUNE_APP_URL;
if (!baseURL) throw new Error('RUNE_APP_URL is required');
const outputDir = process.env.RUNE_REVIEW_OUT;
if (!outputDir) throw new Error('RUNE_REVIEW_OUT is required');

const launchArgs = (process.env.RUNE_E2E_LAUNCH_ARGS ?? '')
  .split(/\s+/)
  .filter(Boolean);

export default defineConfig({
  testDir: './screenshots',
  testMatch: '**/*.spec.ts',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 120_000,
  expect: { timeout: 30_000 },
  reporter: [['list']],
  outputDir: `${outputDir}/.playwright-artifacts`,
  use: {
    baseURL,
    headless: process.env.RUNE_E2E_HEADLESS !== '0',
    channel: process.env.RUNE_E2E_CHANNEL || 'chromium',
    launchOptions: { args: launchArgs },
    acceptDownloads: true,
    trace: 'off',
    video: 'off',
    screenshot: 'off',
  },
  projects: [
    { name: 'desktop', use: { viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 } },
    {
      name: 'mobile-portrait',
      use: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
    },
    {
      name: 'mobile-landscape',
      use: { viewport: { width: 844, height: 390 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
    },
  ],
});
