import { expect, test } from '@playwright/test';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { CLEANUP_UNCONFIRMED, CLEANUP_UNINSPECTABLE, waitForStopConfirmation } from './stop-launcher';

// The live teardown confirms launcher cleanup only on positive evidence
// (ENOENT). Node-only; collected by the default smoke config, so it runs in
// CI without the live backend.

const errno = (code: string) => Object.assign(new Error(`injected ${code}`), { code });

async function rejection(promise: Promise<unknown>): Promise<string> {
  try {
    await promise;
  } catch (error) {
    return (error as Error).message;
  }
  return 'resolved';
}

test.describe('live teardown stop confirmation', () => {
  test('ENOENT confirms', async () => {
    await waitForStopConfirmation('stop-file', { lstat: () => Promise.reject(errno('ENOENT')) });
  });

  test('a present stop file is waited on, then confirmed by ENOENT', async () => {
    let calls = 0;
    const lstat = () => (++calls < 3 ? Promise.resolve({}) : Promise.reject(errno('ENOENT')));
    await waitForStopConfirmation('stop-file', { lstat, intervalMs: 5 });
    expect(calls).toBe(3);
  });

  test('an inspection error is a failure, never a confirmation', async () => {
    for (const code of ['EACCES', 'EPERM', 'EBUSY', 'EIO', 'ENOTDIR']) {
      expect(await rejection(waitForStopConfirmation('stop-file', { lstat: () => Promise.reject(errno(code)) }))).toBe(CLEANUP_UNINSPECTABLE);
    }
    // An error without a code, and a non-Error rejection, are failures too.
    expect(await rejection(waitForStopConfirmation('stop-file', { lstat: () => Promise.reject(new Error('no code')) }))).toBe(CLEANUP_UNINSPECTABLE);
    expect(await rejection(waitForStopConfirmation('stop-file', { lstat: () => Promise.reject(null) }))).toBe(CLEANUP_UNINSPECTABLE);
  });

  test('an inspection error after a wait is still a failure', async () => {
    let calls = 0;
    const lstat = () => (++calls < 2 ? Promise.resolve({}) : Promise.reject(errno('EACCES')));
    expect(await rejection(waitForStopConfirmation('stop-file', { lstat, intervalMs: 5 }))).toBe(CLEANUP_UNINSPECTABLE);
  });

  test('a stop file still present at the deadline is unconfirmed', async () => {
    expect(await rejection(waitForStopConfirmation('stop-file', { lstat: () => Promise.resolve({}), timeoutMs: 50, intervalMs: 5 }))).toBe(CLEANUP_UNCONFIRMED);
  });

  test('real filesystem: confirmed only after the file is removed', async () => {
    const dir = mkdtempSync(path.join(tmpdir(), 'arx-stop-'));
    try {
      const stop = path.join(dir, 'stop-test');
      writeFileSync(stop, '');
      expect(await rejection(waitForStopConfirmation(stop, { timeoutMs: 30, intervalMs: 5 }))).toBe(CLEANUP_UNCONFIRMED);
      setTimeout(() => rmSync(stop), 30);
      await waitForStopConfirmation(stop, { timeoutMs: 2_000, intervalMs: 5 });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
