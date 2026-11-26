import { expect, test, type BrowserContext, type Page, type Response } from '@playwright/test';
import path from 'node:path';
import { contrastFailures } from '../contrast';
import { replaySessionStatus } from './replay';

// Runs against a real arxd serving the production ui/dist (see
// playwright.live.config.ts). Nothing here uses the stand-in adapter: the
// production bundle has it compiled out.
const SHOTS = process.env.ARX_LIVE_SHOTS!;
const PASSWORD = process.env.ARX_LIVE_PASSWORD!;
const COOKIE = '__Host-arx_session';
// Seeded by crates/arxd/tests/dashboard_live.rs.
const VM_RUNNING = 'a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1';
const VM_STOPPED = 'b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2';
const SNAPSHOT = 'e5'.repeat(16);
const ACTIVITY_ROWS = 27;
const UNAVAILABLE = 'The dashboard cannot serve this request now.';

test.describe.configure({ mode: 'serial' });

/** Identity of the real local host, used only to mask it in screenshots. */
interface HostIdentity { id: string; name: string }

async function localHost(page: Page): Promise<HostIdentity> {
  return page.evaluate(async () => {
    const { hosts } = await (await fetch('/api/v1/hosts')).json();
    return { id: hosts[0].id, name: hosts[0].name };
  });
}

/**
 * Screenshots are committed as M6 evidence, so every host-identifying region
 * (host name and id, OS, CPU, memory and disk) is painted over by Playwright's
 * deterministic mask before the PNG is written. The Night Orbit views mark
 * those regions with `data-host-identity` (host bento tiles, VM resources);
 * a board column's head (`.col-h`) and the terminal's host list carry the
 * same facts. Animations are disabled so the logo is never caught half drawn.
 */
async function shot(page: Page, name: string, host?: HostIdentity) {
  const mask = host
    ? [
      page.getByText(host.name),
      page.getByText(host.id),
      page.locator('select', { hasText: host.name }),
      page.locator('[data-host-identity], .col-h, .host-pick, .term-head'),
    ]
    : [];
  await page.screenshot({ path: path.join(SHOTS, `${name}.png`), fullPage: true, mask, maskColor: '#7c8794', animations: 'disabled' });
}

/** Night Orbit contrast audit of the real page (see ../contrast.ts); soft, so every state is reported. */
async function audit(page: Page, where: string) {
  expect.soft(await contrastFailures(page), `contrast: ${where}`).toEqual([]);
}

async function signIn(page: Page, hash = '#/hosts') {
  await page.goto(`/${hash}`);
  await page.getByLabel('Password').fill(PASSWORD);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
}

async function sessionStatus(page: Page) {
  return page.evaluate(async () => (await fetch('/api/v1/session', { credentials: 'same-origin' })).status);
}

/** The session cookie's value, kept out of every assertion message. */
async function sessionValue(context: BrowserContext) {
  const value = (await context.cookies()).find(c => c.name === COOKIE)?.value;
  expect(value === undefined ? 'missing' : 'present').toBe('present');
  return value!;
}

/**
 * Status (200 or 401) of GET /api/v1/session replayed with an explicit,
 * possibly stale, session cookie and no cookie jar. Every failure surfaces as
 * a fixed, redacted ReplayError; see replay.ts and replay.spec.ts.
 */
const replayStatus = (value: string) =>
  replaySessionStatus({ port: process.env.ARX_LIVE_PORT, cookieName: COOKIE, value });

const isApi = (method: string, pathname: string) => (r: Response) =>
  r.request().method() === method && new URL(r.url()).pathname === pathname;

async function expectUnavailable(response: Response) {
  expect(response.status()).toBe(503);
  expect(await response.json()).toEqual({ error: { code: 'unavailable', message: UNAVAILABLE } });
}

test('login: production bundle, server-side refusal, then sign-in', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle('Sign in · Arxcovo');
  await expect(page.getByRole('note', { name: 'Demo data' })).toHaveCount(0);
  await expect(page.getByText('The session is kept in a secure, HTTP-only cookie on this host.')).toBeVisible();
  expect(await sessionStatus(page)).toBe(401);
  await shot(page, '01-login');
  await audit(page, '01-login');
  await page.getByLabel('Password').fill('not-the-owner-password');
  const refused = page.waitForResponse(isApi('POST', '/api/v1/session'));
  await page.getByRole('button', { name: 'Sign in' }).click();
  expect((await refused).status()).toBe(401);
  await expect(page.getByRole('alert')).toHaveText('Invalid username or password.');
  await audit(page, 'login refused');
  await page.getByLabel('Password').fill(PASSWORD);
  const accepted = page.waitForResponse(isApi('POST', '/api/v1/session'));
  await page.getByRole('button', { name: 'Sign in' }).click();
  expect((await accepted).status()).toBe(200);
  await expect(page.getByRole('heading', { level: 1, name: 'Hosts' })).toBeFocused();
  expect(await sessionStatus(page)).toBe(200);
  // Project the metadata so a failure can never print the session value.
  const cookie = (await page.context().cookies()).find(c => c.name === COOKIE);
  const flags = cookie && { name: cookie.name, httpOnly: cookie.httpOnly, secure: cookie.secure, sameSite: cookie.sameSite, path: cookie.path };
  expect(flags).toEqual({ name: COOKIE, httpOnly: true, secure: true, sameSite: 'Strict', path: '/' });
});

test('hosts: the local host list and detail come from arxd', async ({ page }) => {
  const list = page.waitForResponse(isApi('GET', '/api/v1/hosts'));
  await signIn(page);
  const body = await (await list).json();
  expect(body.hosts).toHaveLength(1);
  expect(body.hosts[0]).toMatchObject({ reachability: 'online', mode: 'always_on' });
  const host = { id: body.hosts[0].id as string, name: body.hosts[0].name as string };
  // The board: one column per host, holding that host's VM cards.
  const cards = page.getByRole('list', { name: 'Hosts' }).getByRole('listitem');
  await expect(cards).toHaveCount(1);
  await expect(cards.first()).toHaveAttribute('data-reachability', 'online');
  await expect(cards.first().locator('.col-h').getByTitle(`Machine architecture: ${body.hosts[0].arch}`)).toBeVisible();
  await expect(cards.first().getByRole('article')).toHaveCount(2);
  await expect(page.getByText('1 of 1 hosts online')).toBeVisible();
  await shot(page, '02-hosts', host);
  await audit(page, '02-hosts');
  const detail = page.waitForResponse(isApi('GET', `/api/v1/hosts/${host.id}`));
  await cards.first().getByRole('link', { name: host.name, exact: true }).click();
  expect((await detail).status()).toBe(200);
  await expect(page).toHaveURL(new RegExp(`#/hosts/${host.id}$`));
  await expect(page.getByRole('heading', { level: 2, name: /VMs on this host/ })).toContainText('2');
  await expect(page.getByRole('list', { name: 'Recent activity on this host' }).getByRole('listitem').first()).toBeVisible();
  await expect(page.getByRole('link', { name: 'Run command' })).toBeVisible();
  await shot(page, '03-host-detail', host);
  await audit(page, '03-host-detail');
});

test('VMs: catalog rows, architecture badges and detail with size and snapshot', async ({ page }) => {
  const list = page.waitForResponse(isApi('GET', '/api/v1/vms'));
  await signIn(page, '#/vms');
  const { vms } = await (await list).json();
  expect(vms.map((vm: { id: string; state: string }) => [vm.id, vm.state])).toEqual([[VM_RUNNING, 'running'], [VM_STOPPED, 'stopped']]);
  const host = await localHost(page);
  const table = page.getByRole('table');
  await expect(table.getByRole('row')).toHaveCount(3);
  await expect(page.getByText('2 of 2 VMs')).toBeVisible();
  await expect(table.getByTitle('Machine architecture: x86_64')).toHaveCount(2);
  await expect(table.getByRole('row').filter({ hasText: VM_RUNNING })).toContainText('running');
  await expect(table.getByRole('row').filter({ hasText: VM_STOPPED })).toContainText('stopped');
  await shot(page, '04-vms', host);
  await audit(page, '04-vms');
  const detail = page.waitForResponse(isApi('GET', `/api/v1/vms/${VM_RUNNING}`));
  await table.getByRole('link', { name: VM_RUNNING }).click();
  expect(await (await detail).json()).toMatchObject({
    id: VM_RUNNING, kind: 'test', guestOs: 'linux', arch: 'x86_64', state: 'running',
    cpus: 4, memoryBytes: 8589934592, diskBytes: 21474836480,
    snapshots: [{ id: SNAPSHOT, sizeBytes: 1073741824 }],
  });
  await expect(page).toHaveURL(new RegExp(`#/vms/${VM_RUNNING}$`));
  await expect(page.getByRole('heading', { level: 1, name: VM_RUNNING })).toBeVisible();
  const size = page.locator('section[aria-labelledby="vm-size"]');
  await expect(size.locator('.facts > div', { hasText: 'vCPUs' }).locator('dd')).toHaveText('4');
  await expect(size.locator('.facts > div', { hasText: 'Memory' }).locator('dd')).toHaveText('8.0 GiB');
  await expect(size.locator('.facts > div', { hasText: 'Disk' }).locator('dd')).toHaveText('20 GiB');
  await expect(page.locator('section[aria-labelledby="vm-facts"]').getByTitle('Machine architecture: x86_64')).toBeVisible();
  await expect(page.getByRole('heading', { level: 2, name: /Snapshots/ })).toContainText('1');
  const snapshot = page.locator('section[aria-labelledby="vm-snapshots"] li', { hasText: SNAPSHOT });
  await expect(snapshot).toHaveCount(1);
  await expect(snapshot.locator('span').last()).toHaveText('1.0 GiB');
  await shot(page, '05-vm-detail', host);
  await audit(page, '05-vm-detail');
});

test('activity: newest first with server cursor paging', async ({ page }) => {
  const firstPage = page.waitForResponse(r => {
    const url = new URL(r.url());
    return url.pathname === '/api/v1/activity' && !url.searchParams.has('before_id') && !url.searchParams.has('host_id');
  });
  await signIn(page, '#/activity');
  const one = await (await firstPage).json();
  const ids1: number[] = one.entries.map((e: { id: number }) => e.id);
  expect(ids1).toHaveLength(20);
  expect(ids1).toEqual([...ids1].sort((a, b) => b - a));
  expect(new Set(ids1).size).toBe(20);
  expect(one.nextBeforeId).toBe(ids1.at(-1));
  const host = await localHost(page);
  const first = page.getByRole('list', { name: 'Activity, page 1' }).getByRole('listitem');
  await expect(first).toHaveCount(20);
  await shot(page, '06-activity', host);
  await audit(page, '06-activity');
  const secondPage = page.waitForResponse(r => new URL(r.url()).pathname === '/api/v1/activity' && new URL(r.url()).searchParams.get('before_id') === String(one.nextBeforeId));
  await page.getByRole('button', { name: 'Older' }).click();
  const two = await (await secondPage).json();
  const ids2: number[] = two.entries.map((e: { id: number }) => e.id);
  expect(ids2).toHaveLength(ACTIVITY_ROWS - 20);
  expect(ids2).toEqual([...ids2].sort((a, b) => b - a));
  expect(new Set(ids2).size).toBe(ids2.length);
  expect(ids2.every(id => id < one.nextBeforeId && !ids1.includes(id))).toBe(true);
  expect(two.nextBeforeId).toBeNull();
  await expect(page.getByRole('list', { name: 'Activity, page 2' }).getByRole('listitem')).toHaveCount(ACTIVITY_ROWS - 20);
  await expect(page.getByRole('button', { name: 'Older' })).toBeDisabled();
  await shot(page, '07-activity-page-2', host);
  await audit(page, '07-activity-page-2');
  await page.getByRole('button', { name: 'Newer' }).click();
  await expect(first).toHaveCount(20);
});

test('terminal: 503 unavailable without an owner executor, shown as documented', async ({ page }) => {
  await signIn(page);
  const host = await localHost(page);
  await page.goto(`/#/hosts/${host.id}`);
  const ticket = page.waitForResponse(isApi('POST', `/api/v1/hosts/${host.id}/terminal`));
  await page.getByRole('link', { name: 'Run command' }).click();
  await expectUnavailable(await ticket);
  await expect(page).toHaveURL(new RegExp(`#/terminal/${host.id}$`));
  await expect(page.getByRole('alert')).toHaveText(UNAVAILABLE);
  await expect(page.getByText('Terminal status: error')).toBeAttached();
  await expect(page.getByLabel('Command')).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Reconnect' })).toBeVisible();
  await shot(page, '08-terminal-unavailable', host);
  await audit(page, '08-terminal-unavailable');
});

test('console: 503 unavailable without a capture source, shown as documented', async ({ page }) => {
  await signIn(page, `#/vms/${VM_RUNNING}`);
  const host = await localHost(page);
  // The VM page itself opens no stream; the console tile asks only on "Open console".
  await expect(page.getByRole('heading', { level: 1, name: VM_RUNNING })).toBeVisible();
  const ticket = page.waitForResponse(isApi('POST', `/api/v1/vms/${VM_RUNNING}/console`));
  await page.getByRole('link', { name: 'Open console' }).click();
  await expectUnavailable(await ticket);
  await expect(page.getByRole('heading', { level: 1, name: `Console · ${VM_RUNNING}` })).toBeVisible();
  await expect(page.getByRole('alert')).toHaveText(UNAVAILABLE);
  await expect(page.getByText('Console status: error')).toBeAttached();
  await expect(page.getByRole('img', { name: /Console of/ })).toHaveCount(0);
  await shot(page, '09-console-unavailable', host);
  await audit(page, '09-console-unavailable');
});

test('sign out revokes the server session', async ({ page, context }) => {
  await signIn(page);
  const value = await sessionValue(context);
  // Control: the replayed cookie is accepted while the session is live.
  expect(await replayStatus(value)).toBe(200);
  const revoked = page.waitForResponse(isApi('DELETE', '/api/v1/session'));
  await page.getByRole('button', { name: 'Sign out' }).click();
  expect((await revoked).status()).toBe(204);
  await expect(page.getByRole('status')).toHaveText('You are signed out.');
  await expect(page.getByLabel('Password')).toBeFocused();
  // The old cookie, sent explicitly, no longer authenticates.
  expect(await replayStatus(value)).toBe(401);
  await shot(page, '10-signed-out');
  await audit(page, '10-signed-out');
});

test('a session ended elsewhere returns the open dashboard to login', async ({ page, context }) => {
  await signIn(page, '#/hosts');
  const value = await sessionValue(context);
  // A second tab shares the cookie, recovers the CSRF token from
  // GET /api/v1/session and signs out, revoking the session server-side.
  const other = await context.newPage();
  await other.goto('/#/hosts');
  await expect(other.getByRole('navigation', { name: 'Main' })).toBeVisible();
  const revoked = other.waitForResponse(isApi('DELETE', '/api/v1/session'));
  await other.getByRole('button', { name: 'Sign out' }).click();
  expect((await revoked).status()).toBe(204);
  await expect(other.getByRole('status')).toHaveText('You are signed out.');
  await other.close();
  expect(await replayStatus(value)).toBe(401);
  // The first tab's next API call gets 401 and ends its local session.
  const refused = page.waitForResponse(isApi('GET', '/api/v1/activity'));
  await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Activity' }).click();
  expect((await refused).status()).toBe(401);
  await expect(page.getByRole('status')).toHaveText('Your session has ended. Sign in again.');
  await expect(page.getByLabel('Password')).toBeVisible();
  await shot(page, '11-session-ended');
  await audit(page, '11-session-ended');
});
