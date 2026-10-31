// Generic manifest-driven UI-review capture spec.
//
// It only captures pixels: no functional assertions about rendering, GPU APIs
// or game behaviour. The user's review reads these PNGs; a broken capture set
// means the harness or the page is broken.
//
// Sampling follows the ui-review harness: for short pages (<= 8 viewport
// screens) every section plus a full-page PNG is written; for longer pages only
// the leading 4 and trailing 2 sections are written and no full-page image is
// produced (a downscaled long image is not reviewable). The per-combination JSON
// records exactly which sections were captured.

import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { routes } from './ui-review.config';
import type { UiReviewViewport } from './ui-review.config';

const OUT = process.env.WEB_REVIEW_OUT;
if (!OUT) throw new Error('WEB_REVIEW_OUT is required');
const FULL_PAGE_MAX_SECTIONS = 8;
const DEFAULT_LEADING = 4;
const DEFAULT_TRAILING = 2;

const projectViewport = (projectName: string): UiReviewViewport =>
  projectName as UiReviewViewport;

const outPath = (state: string, viewport: string, file: string): string =>
  path.resolve(process.cwd(), OUT, state, viewport, file);

async function settleState(page: Page, wanted: string): Promise<void> {
  await page.waitForFunction((state) => document.body.dataset.appState === state, wanted, {
    timeout: 90_000,
  });
  // Let layout, fonts and the single presented frame settle.
  await page.waitForTimeout(wanted === 'ready' ? 1_000 : 400);
  if (wanted === 'ready') {
    await expect(page.locator('#gba-canvas')).toBeVisible();
  }
}

type Scroller = { kind: 'window' | 'inner'; max: number; step: number; viewport: number };

async function detectScroller(page: Page): Promise<Scroller> {
  return page.evaluate(() => {
    const doc = document.scrollingElement;
    const winHeight = window.innerHeight;
    if (doc && doc.scrollHeight > winHeight + 4) {
      return {
        kind: 'window' as const,
        max: doc.scrollHeight - winHeight,
        step: Math.round(winHeight * 0.8),
        viewport: winHeight,
      };
    }
    const inner = Array.from(document.querySelectorAll('main, [data-scroll-container]')).find(
      (element) => element.scrollHeight > element.clientHeight + 4,
    );
    if (inner) {
      return {
        kind: 'inner' as const,
        max: inner.scrollHeight - inner.clientHeight,
        step: Math.round(inner.clientHeight * 0.8),
        viewport: inner.clientHeight,
      };
    }
    return { kind: 'window' as const, max: 0, step: Math.round(winHeight * 0.8), viewport: winHeight };
  });
}

async function scrollTo(page: Page, kind: 'window' | 'inner', y: number): Promise<void> {
  await page.evaluate(
    ({ kind, y }) => {
      if (kind === 'inner') {
        const inner = Array.from(document.querySelectorAll('main, [data-scroll-container]')).find(
          (element) => element.scrollHeight > element.clientHeight + 4,
        );
        if (inner) inner.scrollTop = y;
      } else {
        window.scrollTo(0, y);
      }
    },
    { kind, y },
  );
}

test.describe('ui-review captures', () => {
  for (const route of routes) {
    for (const viewport of route.viewports ?? ['desktop', 'mobile-portrait', 'mobile-landscape']) {
      test.describe(`${route.name} @ ${viewport}`, () => {
        for (const state of route.states) {
          test(`capture ${route.name} (${state.id}, ${viewport})`, { tag: ['@screenshot'] }, async ({ page }, testInfo) => {
            const actualViewport = projectViewport(testInfo.project.name);
            test.skip(actualViewport !== viewport, `project ${testInfo.project.name} renders ${actualViewport}`);
            if (state.prepare === 'seed-fallback-notice') {
              await page.addInitScript(() => {
                sessionStorage.setItem('gba-native-web-autofallback', 'used');
                sessionStorage.setItem(
                  'gba-native-web-fallback-reason',
                  'Capture fixture: transient WebGPU adapter initialization failed.',
                );
              });
            } else if (state.prepare === 'deny-restart-storage') {
              await page.addInitScript(() => {
                Object.defineProperty(window, 'sessionStorage', {
                  configurable: true,
                  get() { throw new DOMException('storage denied for capture', 'SecurityError'); },
                });
              });
            }
            await page.goto(`${route.path}?${state.query}`, { waitUntil: 'load' });
            await expect(page).toHaveTitle(route.expectedTitle);
            if (state.prepare === 'oversize-initial') {
              await page.waitForFunction(() => document.body.dataset.appState === 'empty');
              await page.evaluate(() => {
                const sheet = Array.from(document.styleSheets).find((item) => /\/app\.[0-9a-f]{12}\.css$/.test(item.href ?? ''));
                if (!sheet) throw new Error('same-origin application stylesheet was not found');
                sheet.insertRule(
                  '#gba-canvas{width:6000px!important;height:4500px!important}',
                  sheet.cssRules.length,
                );
              });
              await page.getByRole('button', { name: 'Start synthetic demo' }).click();
            }
            await settleState(page, state.expectedState);
            if (state.prepare === 'deny-restart-storage') {
              await page.getByRole('button', { name: 'Restart with selected backend' }).click();
              await expect(page.locator('#status-message')).toContainText('Restart blocked');
            }

            const dir = path.dirname(outPath(state.id, actualViewport, 'x.png'));
            fs.mkdirSync(dir, { recursive: true });

            const scroller = await detectScroller(page);
            const sections = Math.max(
              1,
              scroller.max === 0 ? 1 : Math.ceil((scroller.max + scroller.viewport) / scroller.viewport),
            );
            const shortPage = sections <= FULL_PAGE_MAX_SECTIONS;
            const leading = route.leadingSections ?? DEFAULT_LEADING;
            const trailing = route.trailingSections ?? DEFAULT_TRAILING;
            let indexes: number[] = [];
            if (shortPage) {
              indexes = Array.from({ length: sections }, (_, index) => index);
            } else {
              const head = Array.from({ length: Math.min(leading, sections) }, (_, index) => index);
              const tail = Array.from(
                { length: Math.min(trailing, sections - head.length) },
                (_, index) => sections - Math.min(trailing, sections - head.length) + index,
              );
              indexes = [...new Set([...head, ...tail])];
            }

            let fullPage = false;
            if (shortPage) {
              await page.screenshot({ path: outPath(state.id, actualViewport, `${route.name}.png`), fullPage: true });
              fullPage = true;
            }
            const captured: number[] = [];
            for (const index of indexes) {
              const y = Math.min(scroller.max, index * scroller.step);
              await scrollTo(page, scroller.kind, y);
              await page.waitForTimeout(150);
              await page.screenshot({
                path: outPath(state.id, actualViewport, `${route.name}-sec${index}.png`),
                fullPage: false,
              });
              captured.push(index);
            }
            await scrollTo(page, scroller.kind, 0);

            const record = {
              route: route.name,
              state: state.id,
              stateNote: state.note ?? null,
              viewport: actualViewport,
              project: testInfo.project.name,
              url: page.url(),
              title: await page.title(),
              bodyState: await page.evaluate(() => document.body.dataset.appState),
              sections,
              capturedSectionIndexes: captured,
              fullPageOverview: fullPage,
              viewportHeight: scroller.viewport,
              scrollKind: scroller.kind,
            };
            fs.writeFileSync(
              outPath(state.id, actualViewport, `${route.name}.json`),
              `${JSON.stringify(record, null, 2)}\n`,
            );
            testInfo.annotations.push({ type: 'capture', description: `${state.id}/${actualViewport}` });
          });
        }
      });
    }
  }
});
