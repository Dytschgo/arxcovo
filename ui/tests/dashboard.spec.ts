import { expect, test, type Page } from '@playwright/test';
import { auditText, freezeMotion } from './contrast';
import { nonTextFailures } from './nontext';

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

test('hosts: cards with arch badges and reachability', async ({ page }) => {
  await signIn(page);
  const cards = page.getByRole('list', { name: 'Hosts' }).getByRole('listitem');
  await expect(cards).toHaveCount(4);
  await expect(cards.filter({ hasText: 'harbor' }).getByTitle('Machine architecture: arm64')).toBeVisible();
  await expect(cards.filter({ hasText: 'pebble' })).toContainText('No heartbeat for 2 hours');
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
  await page.getByRole('link', { name: 'Open terminal' }).click();
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
  await page.getByLabel('Host').selectOption('forge');
  await page.getByLabel('Outcome').selectOption('failed');
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

test('console: streams frames of a running VM', async ({ page }) => {
  await signIn(page, '#/vms/ubuntu-build-02');
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

// Contrast (Night Orbit: all text >= 4.5:1 in every frame). With default
// motion the animation timeline is frozen, so each transition an action starts
// is sampled at its start, middle and end; with reduced motion the audit sees
// the finished state the stylesheet promises.
for (const motion of ['no-preference', 'reduce'] as const) {
  test(`contrast (${motion} motion): every painted text, value and placeholder clears 4.5:1 in every frame`, async ({ page }) => {
    test.setTimeout(120_000);
    await page.emulateMedia({ reducedMotion: motion });
    const audit = async (where: string) => {
      const { contrast, opacityGroups } = await auditText(page);
      expect.soft(contrast, where).toEqual([]);
      expect.soft(opacityGroups, `${where}: text inside an opacity group`).toEqual([]);
      expect.soft(await nonTextFailures(page), `${where}: non-text contrast (3:1)`).toEqual([]);
    };
    await page.goto('/');
    if (motion === 'no-preference') await freezeMotion(page);
    await expect(page.getByLabel('Password')).toBeFocused();
    await audit('login');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByRole('alert')).toBeVisible();
    await audit('login error');
    await page.getByLabel('Password').fill('arxcovo-demo');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByRole('list', { name: 'Hosts' })).toBeVisible();
    await audit('hosts');
    // Real navigation through the top bar, so the current-item change is measured as it happens.
    for (const [name, ready] of [['Activity', 'Page 1'], ['VMs', '5 of 5 VMs'], ['Terminal', 'Tools on atlas:'], ['Hosts', '2 of 4 hosts online']] as const) {
      await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name }).click();
      await expect(page.getByText(ready).first()).toBeVisible();
      await audit(`nav to ${name}`);
    }
    for (const [hash, ready] of [
      ['#/hosts/atlas', 'qemu-system-x86_64'],
      ['#/hosts/pebble', 'No heartbeat for 2 hours'],
      ['#/vms/web-static', 'Never (static)'],
      ['#/vms/ubuntu-arm-03', 'No snapshots yet'],
      ['#/nowhere', 'Nothing lives at this address.'],
      ['#/activity', 'Page 1'],
    ] as const) {
      await page.goto(`/${hash}`);
      await expect(page.getByText(ready).first()).toBeVisible();
      await audit(hash);
    }
    await page.getByLabel('Tool').selectOption('list_hosts');
    await expect(page.getByText('No activity matches these filters')).toBeVisible();
    await audit('activity, empty filter');

    await page.goto('/#/terminal/atlas');
    const input = page.getByLabel('Command');
    await expect(input).toBeEnabled();
    await expect(input).toHaveAttribute('placeholder', /\S/);
    await audit('terminal, enabled input showing its placeholder');
    // The connecting state is brief; disable the controls in the DOM to measure that styling and its transition.
    await page.evaluate(() => document.querySelectorAll('.terminal-input input, .terminal-input button').forEach(el => el.setAttribute('disabled', '')));
    await expect(input).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Run' })).toBeDisabled();
    await audit('terminal, disabled input and Run button');
    await page.goto('/#/hosts');
    await page.goto('/#/terminal/atlas');
    await expect(input).toBeEnabled();
    await input.fill('git --version');
    await input.press('Enter');
    await expect(page.getByRole('log')).toContainText('exit 0');
    await audit('terminal with output');

    await page.goto('/#/vms/ubuntu-build-02/console');
    await expect(page.getByText('Console status: streaming')).toBeAttached();
    await audit('console streaming');
    await page.getByRole('button', { name: 'Pause' }).click();
    await audit('console paused');
    await page.goto('/#/vms/win11-test-04/console');
    await expect(page.getByText(/is stopped; start it/)).toBeVisible();
    await audit('console of a stopped VM');
    await page.getByRole('button', { name: 'Sign out' }).click();
    await expect(page.getByText('You are signed out.')).toBeVisible();
    await audit('signed out');
  });
}

// States that last only while a request is in flight, held open with a paused
// clock (the stand-in answers on timers): login submitting and a running command.
test('contrast: in-flight states (signing in, a running command) clear 4.5:1 in every frame', async ({ page }) => {
  test.setTimeout(60_000);
  await page.clock.install();
  await page.goto('/');
  await freezeMotion(page);
  await page.getByLabel('Password').fill('arxcovo-demo');
  await page.clock.pauseAt(Date.now() + 60_000);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('button', { name: 'Signing in…' })).toBeDisabled();
  {
    const { contrast, opacityGroups } = await auditText(page);
    expect.soft(contrast, 'signing in').toEqual([]);
    expect.soft(opacityGroups, 'signing in: text inside an opacity group').toEqual([]);
    expect.soft(await nonTextFailures(page), 'signing in: non-text contrast (3:1)').toEqual([]);
  }
  await page.clock.resume();
  await expect(page.getByRole('list', { name: 'Hosts' })).toBeVisible();

  await page.goto('/#/terminal/atlas');
  const input = page.getByLabel('Command');
  await expect(input).toBeEnabled();
  await input.fill('cargo build');
  await page.clock.pauseAt(Date.now() + 120_000);
  await input.press('Enter');
  await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible();
  await expect(page.getByText('Terminal status: running')).toBeAttached();
  {
    const { contrast, opacityGroups } = await auditText(page);
    expect.soft(contrast, 'command running').toEqual([]);
    expect.soft(opacityGroups, 'command running: text inside an opacity group').toEqual([]);
    expect.soft(await nonTextFailures(page), 'command running: non-text contrast (3:1)').toEqual([]);
  }
  await page.clock.resume();
  await expect(page.getByRole('log')).toContainText('exit 0');
});
