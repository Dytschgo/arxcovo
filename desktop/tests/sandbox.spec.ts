import { _electron as electron, expect, test } from '@playwright/test';
import { resolve } from 'node:path';
import { env } from 'node:process';

test('desktop renders the UI with isolation and denies navigation and popups', async () => {
  const launchEnv = { ...env };
  delete launchEnv.ELECTRON_RUN_AS_NODE;
  const app = await electron.launch({ args: [resolve('desktop/main.cjs')], env: launchEnv });
  try {
    const page = await app.firstWindow();
    await expect(page.getByRole('heading', { name: 'Arxcovo' })).toBeVisible();
    const initialUrl = page.url();
    const preferences = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].webContents.getLastWebPreferences());
    expect(preferences?.nodeIntegration).toBe(false);
    expect(preferences?.contextIsolation).toBe(true);
    expect(preferences?.sandbox).toBe(true);
    const prevented = await app.evaluate(({ BrowserWindow }) => {
      const contents = BrowserWindow.getAllWindows()[0].webContents;
      return new Promise<boolean>((resolveNavigation, reject) => {
        contents.once('will-navigate', event => resolveNavigation(event.defaultPrevented));
        void contents.executeJavaScript("window.location.href = 'https://example.invalid/'").catch(reject);
      });
    });
    expect(prevented).toBe(true);
    expect(page.url()).toBe(initialUrl);
    expect(await page.evaluate(() => window.open('https://example.invalid/'))).toBeNull();
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(1);
  } finally {
    await app.close();
  }
});
