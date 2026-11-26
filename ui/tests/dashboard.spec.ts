import { expect, test, type Page } from '@playwright/test';

// Runs against `vite dev`, so the deterministic stand-in adapter is active.
// Screenshots land in test-results/screenshots (gitignored) for the PR.
const SHOTS = 'test-results/screenshots';

async function signIn(page: Page, hash = '#/hosts') {
  await page.goto(`/${hash}`);
  await page.getByLabel('Password').fill('arxcovo-demo');
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
}

const shot = (page: Page, name: string) => page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: true });

// Night Orbit is dark only (no light scheme, no theme switch).
test.use({ viewport: { width: 1280, height: 800 } });

const overflowX = (page: Page) => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);

test('login: demo banner, wrong password, keyboard sign-in', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle('Sign in · Arxcovo');
  await expect(page.getByRole('note', { name: 'Demo data' })).toBeVisible();
  await expect(page.getByLabel('Password')).toBeFocused();
  await shot(page, '01-login');
  await page.keyboard.type('wrong');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('alert')).toHaveText('Wrong username or password.');
  await page.keyboard.type('arxcovo-demo');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { level: 1, name: 'Hosts' })).toBeFocused();
});

test('hosts: board columns with arch badges, reachability and VM cards', async ({ page }) => {
  await signIn(page);
  const cards = page.getByRole('list', { name: 'Hosts' }).getByRole('listitem');
  await expect(cards).toHaveCount(4);
  await expect(cards.filter({ hasText: 'harbor' }).locator('.col-h').getByTitle('Machine architecture: arm64')).toBeVisible();
  await expect(cards.filter({ hasText: 'pebble' })).toContainText('No heartbeat for 2 hours');
  await expect(cards.filter({ hasText: 'atlas' }).getByRole('article')).toHaveCount(3);
  await expect(page.getByText('2 of 4 hosts online')).toBeAttached();
  await expect(page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Hosts' })).toHaveAttribute('aria-current', 'page');
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
  await shot(page, '02-hosts');
});

test('host detail: tools, VMs, activity and terminal entry', async ({ page }) => {
  await signIn(page);
  await page.getByRole('link', { name: 'atlas', exact: true }).click();
  await expect(page).toHaveURL(/#\/hosts\/atlas$/);
  await expect(page.getByText('qemu-system-x86_64')).toBeVisible();
  await expect(page.getByRole('list', { name: 'Recent activity on this host' }).getByRole('listitem').first()).toBeVisible();
  await shot(page, '03-host-detail');
  await page.getByRole('link', { name: 'Run command' }).click();
  await expect(page).toHaveURL(/#\/terminal\/atlas$/);
});

test('VMs: table with arch badges and an architecture filter', async ({ page }) => {
  await signIn(page, '#/vms');
  const rows = page.getByRole('table').getByRole('row');
  await expect(rows).toHaveCount(6);
  await expect(page.getByRole('table').getByTitle('Machine architecture: x86_64')).toHaveCount(4);
  await shot(page, '04-vms');
  await page.getByLabel('Architecture').selectOption('arm64');
  await expect(rows).toHaveCount(2);
  await expect(page.getByText('1 of 5 VMs')).toBeVisible();
});

test('VM detail: snapshots, expiry and greyed-out incompatible hosts', async ({ page }) => {
  await signIn(page, '#/vms');
  await page.getByRole('link', { name: 'ubuntu-build-02' }).click();
  await expect(page.getByText('before-integration-tests')).toBeVisible();
  const pebble = page.getByRole('list', { name: 'Compatible hosts' }).getByRole('listitem').filter({ hasText: 'pebble' });
  await expect(pebble).toHaveAttribute('aria-disabled', 'true');
  await expect(pebble).toContainText('arm64 host cannot run x86_64 guests');
  await shot(page, '05-vm-detail');
});

test('activity: filters and paging', async ({ page }) => {
  await signIn(page, '#/activity');
  await expect(page.getByRole('list', { name: 'Activity, page 1' }).getByRole('listitem')).toHaveCount(20);
  await shot(page, '06-activity');
  await page.getByRole('button', { name: 'Older' }).click();
  await expect(page.getByRole('list', { name: 'Activity, page 2' })).toBeVisible();
  await page.getByLabel('Host', { exact: true }).selectOption('forge');
  await page.getByRole('group', { name: 'Outcome' }).getByRole('button', { name: 'failed' }).click();
  const items = page.getByRole('list', { name: 'Activity, page 1' }).getByRole('listitem');
  await expect(items.first()).toBeVisible();
  for (const item of await items.all()) {
    await expect(item).toHaveAttribute('data-outcome', 'failed');
    await expect(item).toContainText('forge');
  }
});

test('terminal: run, stream, exit, history and a refused tool', async ({ page }) => {
  await signIn(page, '#/terminal/atlas');
  const input = page.getByLabel('Command');
  await expect(input).toBeEnabled();
  await input.fill('cargo build');
  await input.press('Enter');
  const log = page.getByRole('log', { name: 'Terminal output for atlas' });
  await expect(log).toContainText('Finished `dev` profile');
  await expect(log).toContainText('exit 0 · succeeded');
  await input.fill('docker ps');
  await input.press('Enter');
  await expect(log).toContainText('docker: not an available tool on atlas');
  await expect(log).toContainText('exit — · refused');
  await input.press('ArrowUp');
  await expect(input).toHaveValue('docker ps');
  await shot(page, '07-terminal');
});

test('console: a stopped VM opens closed with the reason, without a stream', async ({ page }) => {
  await signIn(page, '#/vms/win11-test-04/console');
  await expect(page.getByText('win11-test-04 is stopped; start it to see its console.')).toBeVisible();
  await expect(page.getByText('Console status: closed')).toBeAttached();
  await expect(page.getByRole('img', { name: /Console of/ })).toHaveCount(0);
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
});

test('search: the jump list filters hosts, VMs and tools and jumps with the keyboard', async ({ page }) => {
  await signIn(page);
  await expect(page.getByRole('list', { name: 'Hosts' })).toBeVisible();
  await page.keyboard.press('/');
  const search = page.getByRole('combobox', { name: 'Search hosts, VMs and tools' });
  await expect(search).toBeFocused();
  await search.fill('cargo');
  const options = page.getByRole('listbox', { name: 'Search results' }).getByRole('option');
  await expect(options).toHaveText([/cargo on atlas/, /cargo on harbor/, /cargo on forge/]);
  await shot(page, '11-search');
  await search.fill('web');
  await expect(options).toHaveText([/web-static/]);
  await search.press('Enter');
  await expect(page).toHaveURL(/#\/vms\/web-static$/);
  await expect(page.getByRole('heading', { level: 1, name: 'web-static' })).toBeFocused();
});

test('layout: no view overflows the page at 1280 px', async ({ page }) => {
  await signIn(page);
  for (const hash of ['#/hosts', '#/hosts/atlas', '#/hosts/pebble', '#/vms', '#/vms/ubuntu-build-02', '#/vms/web-static/console', '#/terminal/atlas', '#/activity', '#/nowhere']) {
    await page.goto(`/${hash}`);
    await expect(page.getByRole('heading', { level: 1 })).toBeAttached();
    await page.waitForLoadState('networkidle');
    expect(await overflowX(page), hash).toBeLessThanOrEqual(0);
  }
});

test('reduced motion: everything shows its finished state', async ({ browser }) => {
  const context = await browser.newContext({ reducedMotion: 'reduce', viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  await signIn(page);
  const column = page.getByRole('list', { name: 'Hosts' }).getByRole('listitem').first();
  await expect(column).toBeVisible();
  // No wait: the entrance and the logo are already at their end state.
  expect(await column.evaluate(el => getComputedStyle(el).opacity)).toBe('1');
  expect(await page.locator('.top .logo .lg-c').evaluate(el => getComputedStyle(el).display)).toBe('none');
  expect(await page.locator('.top .logo .lg-a').evaluate(el => Number(getComputedStyle(el).strokeDashoffset.replace('px', '')))).toBe(0);
  await context.close();
});

test('console: streams frames of a running VM', async ({ page }) => {
  await signIn(page, '#/vms/ubuntu-build-02');
  await expect(page.getByRole('img', { name: /Console of/ })).toHaveCount(0);
  await page.getByRole('link', { name: 'Open console' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Console · ubuntu-build-02' })).toBeVisible();
  await expect(page.getByRole('img', { name: /Console of ubuntu-build-02, frame \d+/ })).toBeVisible();
  await expect(page.getByText('Console status: streaming')).toBeVisible();
  await shot(page, '08-console');
  await page.getByRole('button', { name: 'Pause' }).click();
  await expect(page.getByText('Console status: paused')).toBeVisible();
});

test('responsive: the narrow layout stays usable without page overflow', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  await expect(page.getByRole('list', { name: 'Hosts' }).getByRole('listitem')).toHaveCount(4);
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
  await shot(page, '09-hosts-mobile-dark');
  await page.getByRole('link', { name: 'VMs' }).click();
  await expect(page.getByRole('table')).toBeVisible();
  await shot(page, '10-vms-mobile-dark');
});

test('keyboard: skip link and navigation reach every section', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Skip to content' }).focus();
  await expect(page.getByRole('button', { name: 'Skip to content' })).toBeInViewport();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('main')).toBeFocused();
  for (const [name, heading] of [['VMs', 'Virtual machines'], ['Activity', 'Activity'], ['Terminal', 'Terminal'], ['Hosts', 'Hosts']] as const) {
    await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name }).focus();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: heading })).toBeFocused();
  }
});
