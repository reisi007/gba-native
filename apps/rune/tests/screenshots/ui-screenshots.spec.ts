import { expect, test } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { routes } from './ui-review.config';

const OUT = process.env.RUNE_REVIEW_OUT;
if (!OUT) throw new Error('RUNE_REVIEW_OUT is required');

function syntheticSaveHex(): string {
  const bytes = new Uint8Array(512);
  bytes.set(new TextEncoder().encode('GBANSV01'));
  bytes[8] = 0;
  for (let index = 9; index < bytes.length; index += 1) {
    bytes[index] = (index * 31 + bytes[8]) & 0xff;
  }
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

test.describe('Rune UI review captures', () => {
  for (const route of routes) {
    for (const state of route.states) {
      test(`${route.name} — ${state.id}`, { tag: ['@screenshot'] }, async ({ page }, testInfo) => {
        await page.addInitScript(({ slots, fixture }) => {
          for (let index = localStorage.length - 1; index >= 0; index -= 1) {
            const key = localStorage.key(index);
            if (key?.startsWith('gba-native.rune.slot.') || key === 'gba-native.rune.default-slot') {
              localStorage.removeItem(key);
            }
          }
          if (slots === 'filled') {
            localStorage.setItem('gba-native.rune.slot.slot-1', fixture);
            localStorage.setItem('gba-native.rune.default-slot', 'slot-1');
          }
        }, { slots: state.slots, fixture: syntheticSaveHex() });

        await page.goto(route.path, { waitUntil: 'load' });
        await expect(page).toHaveTitle(route.expectedTitle);
        await expect(page.locator('#status')).toHaveCount(0, { timeout: 90_000 });
        await expect(page.locator('canvas').first()).toBeVisible({ timeout: 90_000 });
        await page.waitForTimeout(1_000);

        if (state.page === 'settings') await page.keyboard.press('F1');
        if (state.page === 'saves') await page.keyboard.press('F2');
        if (state.page !== 'demo') await page.waitForTimeout(500);
        if (state.id === 'empty-saves') {
          await expect(page.getByRole('button', { name: 'Export', exact: true })).toBeDisabled();
          await expect(page.getByRole('button', { name: 'Make default', exact: true })).toBeDisabled();
          await expect(page.getByRole('button', { name: 'Delete', exact: true })).toBeDisabled();
          await expect(page.getByRole('button', { name: 'New synthetic slot', exact: true })).toBeEnabled();
          await expect(page.getByRole('button', { name: 'Import fixture', exact: true })).toBeEnabled();
        }
        if (state.id === 'filled-saves') {
          await expect(page.getByRole('button', { name: 'Export', exact: true })).toBeEnabled();
          await expect(page.getByRole('button', { name: 'Make default', exact: true })).toBeDisabled();
          await expect(page.getByRole('button', { name: 'Delete', exact: true })).toBeEnabled();
        }
        const viewport = testInfo.project.name;
        const directory = path.resolve(OUT, state.id, viewport);
        fs.mkdirSync(directory, { recursive: true });
        await page.screenshot({ path: path.join(directory, `${route.name}.png`), fullPage: true });
        await page.screenshot({ path: path.join(directory, `${route.name}-sec0.png`), fullPage: false });
        const viewportHeight = page.viewportSize()?.height ?? 700;
        await page.mouse.move(250, Math.floor(viewportHeight * 0.7));
        await page.mouse.wheel(0, 620);
        await page.waitForTimeout(900);
        await page.screenshot({ path: path.join(directory, `${route.name}-sec1.png`), fullPage: false });
        fs.writeFileSync(path.join(directory, `${route.name}.json`), `${JSON.stringify({
          route: route.name,
          state: state.id,
          note: state.note,
          viewport,
          url: page.url(),
          title: await page.title(),
          canvasCount: await page.locator('canvas').count(),
          sections: 2,
        }, null, 2)}\n`);
      });
    }
  }

  test('desktop settings persist and save controls create the exact synthetic fixture', async ({ page }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'uses stable desktop canvas coordinates');
    await page.addInitScript(() => {
      if (sessionStorage.getItem('rune-functional-clean') === '1') return;
      localStorage.clear();
      sessionStorage.setItem('rune-functional-clean', '1');
    });
    await page.goto('/');
    await expect(page.locator('#status')).toHaveCount(0, { timeout: 90_000 });
    const canvas = page.locator('canvas').first();
    await expect(canvas).toBeVisible({ timeout: 90_000 });
    await page.waitForTimeout(1_000);
    await canvas.click({ position: { x: 1_200, y: 800 } });

    await page.keyboard.press('F1');
    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
    const muteOffControl = page.getByRole('button', { name: 'Mute: Off', exact: true });
    await expect(muteOffControl).toBeVisible();
    const muteBounds = await muteOffControl.boundingBox();
    if (!muteBounds) throw new Error('Rune mute control has no browser geometry');
    const muteCenter = {
      x: muteBounds.x + muteBounds.width / 2,
      y: muteBounds.y + muteBounds.height / 2,
    };
    await page.mouse.move(muteCenter.x, muteCenter.y);
    await page.waitForTimeout(100);
    await page.mouse.click(muteCenter.x, muteCenter.y);
    await page.mouse.move(1240, 880);
    await expect(page.getByRole('button', { name: 'Mute: On', exact: true })).toBeVisible();
    const settingsDocument = await page.evaluate(() => localStorage.getItem('gba-native.rune.settings'));
    expect(settingsDocument).toContain('muted=true\n');

    await page.reload();
    await expect(page.locator('#status')).toHaveCount(0, { timeout: 90_000 });
    await expect(canvas).toBeVisible({ timeout: 90_000 });
    await page.waitForTimeout(1_000);
    await canvas.click({ position: { x: 1_200, y: 800 } });
    await page.keyboard.press('F1');
    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Mute: On', exact: true })).toBeVisible();

    await page.keyboard.press('F2');
    await expect(page.getByRole('heading', { name: 'Save slots' })).toBeVisible();
    await page.waitForTimeout(300);
    const newSlotControl = page.getByRole('button', { name: 'New synthetic slot', exact: true });
    await expect(newSlotControl).toBeVisible();
    const newSlotBounds = await newSlotControl.boundingBox();
    if (!newSlotBounds) throw new Error('Rune new-slot control has no browser geometry');
    const newSlotCenter = {
      x: newSlotBounds.x + newSlotBounds.width / 2,
      y: newSlotBounds.y + newSlotBounds.height / 2,
    };
    await page.mouse.move(newSlotCenter.x, newSlotCenter.y);
    await page.waitForTimeout(100);
    await page.mouse.click(newSlotCenter.x, newSlotCenter.y);
    await page.waitForFunction(() => localStorage.getItem('gba-native.rune.slot.slot1') !== null, { timeout: 8_000 });
    const savedFixture = await page.evaluate(() => localStorage.getItem('gba-native.rune.slot.slot1'));
    expect(savedFixture).toBe(syntheticSaveHex());
    expect(await page.evaluate(() => localStorage.getItem('gba-native.rune.default-slot'))).toBe('slot1');
    await expect(page.getByRole('button', { name: /slot1.*512 bytes/ })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Export', exact: true })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'Make default', exact: true })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Delete', exact: true })).toBeEnabled();
    await page.waitForTimeout(1_000); // Let the Rune canvas publish the relaid-out control bounds.

    const downloadPromise = page.waitForEvent('download', { timeout: 10_000 });
    await page.mouse.move(310, 206);
    await page.waitForTimeout(100);
    await page.mouse.click(310, 206); // Export after the save-slot row has been laid out.
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe('slot1.sav');
    const downloadPath = await download.path();
    expect(downloadPath).not.toBeNull();
    const exportedBytes = await readFile(downloadPath!);
    expect(exportedBytes.toString('hex')).toBe(savedFixture);

    const importFixture = page.getByRole('button', { name: 'Import fixture', exact: true });
    await expect(importFixture).toBeEnabled();
    const [fileChooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      importFixture.click(),
    ]);
    await fileChooser.setFiles({
      name: 'slot-1.sav',
      mimeType: 'application/octet-stream',
      buffer: exportedBytes,
    });
    await page.waitForFunction(() => localStorage.getItem('gba-native.rune.save.pending') !== null);
    await importFixture.click(); // Consume the staged exact fixture.
    await page.waitForFunction(() => localStorage.getItem('gba-native.rune.slot.imported') !== null);
    expect(await page.evaluate(() => localStorage.getItem('gba-native.rune.slot.imported'))).toBe(savedFixture);
    expect(await page.evaluate(() => localStorage.getItem('gba-native.rune.save.pending'))).toBeNull();
  });

  test('browser save actions track selected and default slots in the accessibility mirror', async ({ page }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'uses stable desktop accessibility controls');
    await page.addInitScript((fixture) => {
      localStorage.clear();
      localStorage.setItem('gba-native.rune.slot.slot-1', fixture);
      localStorage.setItem('gba-native.rune.slot.slot-2', fixture);
      localStorage.setItem('gba-native.rune.default-slot', 'slot-1');
    }, syntheticSaveHex());
    await page.goto('/');
    await expect(page.locator('#status')).toHaveCount(0, { timeout: 90_000 });
    const canvas = page.locator('canvas').first();
    await expect(canvas).toBeVisible({ timeout: 90_000 });
    await page.waitForTimeout(1_000);
    await canvas.click({ position: { x: 1_200, y: 800 } });
    await page.keyboard.press('F2');
    await expect(page.getByRole('heading', { name: 'Save slots' })).toBeVisible();

    const slot1 = page.getByRole('button', { name: /slot-1.*default.*selected/ });
    const slot2 = page.getByRole('button', { name: /slot-2.*512 bytes/ });
    const makeDefault = page.getByRole('button', { name: 'Make default', exact: true });
    await expect(slot1).toBeVisible();
    await expect(makeDefault).toBeDisabled();

    await slot2.evaluate((button) => button.click());
    await expect(page.getByRole('button', { name: /slot-2.*selected/ })).toBeVisible();
    await expect(makeDefault).toBeEnabled();
    await makeDefault.evaluate((button) => button.click());
    await expect.poll(() => page.evaluate(() => localStorage.getItem('gba-native.rune.default-slot')))
      .toBe('slot-2');
    await expect(makeDefault).toBeDisabled();

    const unselectedSlot1 = page.getByRole('button', { name: /slot-1.*512 bytes/ });
    await unselectedSlot1.evaluate((button) => button.click());
    await expect(makeDefault).toBeEnabled();
  });
});
