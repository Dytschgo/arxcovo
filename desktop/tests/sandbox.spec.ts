import { _electron as electron, expect, test } from '@playwright/test';
import { readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { env } from 'node:process';

const DIST = resolve('ui/dist');

test('the production bundle contains no stand-in fixtures or demo credentials', () => {
  const assets = readdirSync(join(DIST, 'assets')).filter(f => f.endsWith('.js'));
  expect(assets.length).toBeGreaterThan(0);
  for (const file of assets) {
    const js = readFileSync(join(DIST, 'assets', file), 'utf8');
    for (const marker of ['arx-standin-fixture-v1', 'arxcovo-demo', 'claude-code-1']) expect(js, `${file} contains ${marker}`).not.toContain(marker);
  }
});

test('desktop serves the UI from app://arxcovo with isolation, CSP and locked navigation', async () => {
  const launchEnv = { ...env };
  delete launchEnv.ELECTRON_RUN_AS_NODE;
  const app = await electron.launch({ args: [resolve('desktop/main.cjs')], env: launchEnv });
  try {
    const page = await app.firstWindow();
    await expect(page.getByLabel('Password')).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Arxcovo' })).toBeAttached();
    // Production adapter: no demo banner, and the unconnected API says so honestly.
    await expect(page.getByRole('note', { name: 'Demo data' })).toHaveCount(0);
    await expect(page.getByRole('alert')).toContainText('not connected to an arxd host');
    // Signing in (POST) and signing out (DELETE) get the same documented 503, not a 405.
    const methods = await page.evaluate(async () => {
      const post = await fetch('/api/v1/session', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
      const del = await fetch('/api/v1/session', { method: 'DELETE' });
      return { post: post.status, postBody: await post.json(), del: del.status };
    });
    expect(methods).toEqual({ post: 503, postBody: { error: { code: 'unavailable', message: 'This desktop build is not connected to an arxd host yet.' } }, del: 503 });
    await page.getByLabel('Password').fill('anything');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByRole('alert')).toHaveText('This desktop build is not connected to an arxd host yet.');
    const initialUrl = page.url();
    expect(initialUrl).toBe('app://arxcovo/index.html');

    const preferences = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].webContents.getLastWebPreferences());
    expect(preferences?.nodeIntegration).toBe(false);
    expect(preferences?.contextIsolation).toBe(true);
    expect(preferences?.sandbox).toBe(true);
    expect(preferences?.webviewTag).toBe(false);
    expect(await page.evaluate(() => typeof (globalThis as { require?: unknown }).require)).toBe('undefined');

    const probe = await page.evaluate(async () => {
      const index = await fetch('/index.html');
      const traversal = await fetch('/%2e%2e/%2e%2e/package.json');
      const unknownType = await fetch('/assets/../index.html.map');
      return { csp: index.headers.get('content-security-policy'), nosniff: index.headers.get('x-content-type-options'), traversal: traversal.status, unknownType: unknownType.status };
    });
    expect(probe.csp).toContain("default-src 'none'");
    expect(probe.csp).toContain("script-src 'self'");
    expect(probe.csp).not.toContain('unsafe');
    expect(probe.nosniff).toBe('nosniff');
    expect(probe.traversal).toBe(404);
    expect(probe.unknownType).toBe(404);

    // Remote fetches are blocked by CSP and by the request filter.
    expect(await page.evaluate(() => fetch('https://example.invalid/').then(() => 'loaded', () => 'blocked'))).toBe('blocked');

    // In-app hash navigation works; leaving the origin does not.
    await page.evaluate(() => { window.location.hash = '#/vms'; });
    expect(page.url()).toBe('app://arxcovo/index.html#/vms');
    const prevented = await app.evaluate(({ BrowserWindow }) => {
      const contents = BrowserWindow.getAllWindows()[0].webContents;
      return new Promise<boolean>((resolveNavigation, reject) => {
        contents.once('will-navigate', event => resolveNavigation(event.defaultPrevented));
        void contents.executeJavaScript("window.location.href = 'https://example.invalid/'").catch(reject);
      });
    });
    expect(prevented).toBe(true);
    expect(page.url()).toBe('app://arxcovo/index.html#/vms');
    expect(await page.evaluate(() => window.open('https://example.invalid/'))).toBeNull();
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(1);
  } finally {
    await app.close();
  }
});
