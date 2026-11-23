import { expect, test, type Page } from '@playwright/test';
import path from 'node:path';

// Runs against a real arxd serving the production ui/dist (see
// playwright.live.config.ts). Nothing here uses the stand-in adapter: the
// production bundle has it compiled out.
const SHOTS = process.env.ARX_LIVE_SHOTS!;
const PASSWORD = process.env.ARX_LIVE_PASSWORD!;
// Seeded by crates/arxd/tests/dashboard_live.rs.
const VM_RUNNING = 'a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1';
const VM_STOPPED = 'b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2';
const SNAPSHOT = 'e5'.repeat(16);
const UNAVAILABLE = 'The dashboard cannot serve this request now.';

test.describe.configure({ mode: 'serial' });

const shot = (page: Page, name: string) => page.screenshot({ path: path.join(SHOTS, `${name}.png`), fullPage: true });

async function signIn(page: Page, hash = '#/hosts') {
  await page.goto(`/${hash}`);
  await page.getByLabel('Password').fill(PASSWORD);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
}

async function sessionStatus(page: Page) {
  return page.evaluate(async () => (await fetch('/api/v1/session', { credentials: 'same-origin' })).status);
}

/** The single local host's id, read from the UI rather than assumed. */
async function openLocalHost(page: Page) {
  await page.getByRole('list', { name: 'Hosts' }).getByRole('link').first().click();
  await expect(page).toHaveURL(/#\/hosts\/[A-Za-z0-9][A-Za-z0-9._-]*$/);
  return decodeURIComponent(new URL(page.url()).hash.split('/')[2]);
}

test('login: production bundle, server-side refusal, then sign-in', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle('Sign in · Arxcovo');
  await expect(page.getByRole('note', { name: 'Demo data' })).toHaveCount(0);
  await expect(page.getByText('The session is kept in a secure, HTTP-only cookie on this host.')).toBeVisible();
  expect(await sessionStatus(page)).toBe(401);
  await shot(page, '01-login');
  await page.getByLabel('Password').fill('not-the-owner-password');
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('alert')).toHaveText('Invalid username or password.');
  await page.getByLabel('Password').fill(PASSWORD);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Hosts' })).toBeFocused();
  expect(await sessionStatus(page)).toBe(200);
  const cookies = await page.context().cookies();
  const session = cookies.find(c => c.name === '__Host-arx_session');
  expect(session).toMatchObject({ httpOnly: true, secure: true, sameSite: 'Strict', path: '/' });
});

test('hosts: the local host list and detail come from arxd', async ({ page }) => {
  await signIn(page);
  const cards = page.getByRole('list', { name: 'Hosts' }).getByRole('listitem');
  await expect(cards).toHaveCount(1);
  await expect(cards.first()).toHaveAttribute('data-reachability', 'online');
  await expect(page.getByText('1 of 1 hosts online')).toBeVisible();
  await shot(page, '02-hosts');
  await openLocalHost(page);
  await expect(page.getByRole('heading', { level: 2, name: /VMs on this host/ })).toContainText('2');
  await expect(page.getByRole('list', { name: 'Recent activity on this host' }).getByRole('listitem').first()).toBeVisible();
  await expect(page.getByRole('link', { name: 'Open terminal' })).toBeVisible();
  await shot(page, '03-host-detail');
});

test('VMs: catalog rows and detail with a snapshot', async ({ page }) => {
  await signIn(page, '#/vms');
  const table = page.getByRole('table');
  await expect(table.getByRole('row')).toHaveCount(3);
  await expect(page.getByText('2 of 2 VMs')).toBeVisible();
  await expect(table.getByRole('row').filter({ hasText: VM_RUNNING })).toContainText('running');
  await expect(table.getByRole('row').filter({ hasText: VM_STOPPED })).toContainText('stopped');
  await shot(page, '04-vms');
  await table.getByRole('link', { name: VM_RUNNING }).click();
  await expect(page).toHaveURL(new RegExp(`#/vms/${VM_RUNNING}$`));
  await expect(page.getByRole('heading', { level: 1, name: VM_RUNNING })).toBeVisible();
  await expect(page.getByRole('heading', { level: 2, name: /Snapshots/ })).toContainText('1');
  await expect(page.getByText(SNAPSHOT)).toBeVisible();
  await shot(page, '05-vm-detail');
});

test('activity: newest first with server cursor paging', async ({ page }) => {
  await signIn(page, '#/activity');
  const first = page.getByRole('list', { name: 'Activity, page 1' }).getByRole('listitem');
  await expect(first).toHaveCount(20);
  await shot(page, '06-activity');
  await page.getByRole('button', { name: 'Older' }).click();
  const second = page.getByRole('list', { name: 'Activity, page 2' }).getByRole('listitem');
  // 27 seeded run_command rows; reads are excluded by default.
  await expect(second).toHaveCount(7);
  await expect(page.getByRole('button', { name: 'Older' })).toBeDisabled();
  await shot(page, '07-activity-page-2');
  await page.getByRole('button', { name: 'Newer' }).click();
  await expect(first).toHaveCount(20);
});

test('terminal: documented unavailable state without an owner executor', async ({ page }) => {
  await signIn(page);
  const host = await openLocalHost(page);
  await page.getByRole('link', { name: 'Open terminal' }).click();
  await expect(page).toHaveURL(new RegExp(`#/terminal/${host}$`));
  await expect(page.getByRole('alert')).toHaveText(UNAVAILABLE);
  await expect(page.getByText('Terminal status: error')).toBeAttached();
  await expect(page.getByLabel('Command')).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Reconnect' })).toBeVisible();
  await shot(page, '08-terminal-unavailable');
});

test('console: documented unavailable state without a capture source', async ({ page }) => {
  await signIn(page, `#/vms/${VM_RUNNING}`);
  await page.getByRole('link', { name: 'Open console' }).click();
  await expect(page.getByRole('heading', { level: 1, name: `Console · ${VM_RUNNING}` })).toBeVisible();
  await expect(page.getByRole('alert')).toHaveText(UNAVAILABLE);
  await expect(page.getByText('Console status: error')).toBeAttached();
  await expect(page.getByRole('img', { name: /Console of/ })).toHaveCount(0);
  await shot(page, '09-console-unavailable');
});

test('sign out revokes the server session', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Sign out' }).click();
  await expect(page.getByRole('status')).toHaveText('You are signed out.');
  await expect(page.getByLabel('Password')).toBeFocused();
  expect(await sessionStatus(page)).toBe(401);
  await shot(page, '10-signed-out');
});

test('a session ended elsewhere returns the open dashboard to login', async ({ page, context }) => {
  await signIn(page, '#/hosts');
  // A second tab shares the cookie, recovers the CSRF token from
  // GET /api/v1/session and signs out, revoking the session server-side.
  const other = await context.newPage();
  await other.goto('/#/hosts');
  await expect(other.getByRole('navigation', { name: 'Main' })).toBeVisible();
  await other.getByRole('button', { name: 'Sign out' }).click();
  await expect(other.getByRole('status')).toHaveText('You are signed out.');
  await other.close();
  // The first tab's next API call gets 401 and ends only its local session.
  await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Activity' }).click();
  await expect(page.getByRole('status')).toHaveText('Your session has ended. Sign in again.');
  await expect(page.getByLabel('Password')).toBeVisible();
  await shot(page, '11-session-ended');
});
