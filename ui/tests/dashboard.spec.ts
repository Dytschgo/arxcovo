import { expect, test, type Page } from '@playwright/test';
import { auditText, freezeMotion } from './contrast';
import { nonTextFailures } from './nontext';
import { viewPartFailures } from './nontext-views';

// Runs against `vite dev`, so the deterministic stand-in adapter is active.
// Screenshots land in test-results/screenshots (gitignored) for the PR.
const SHOTS = 'test-results/screenshots';

async function signIn(page: Page, hash = '#/hosts') {
  await page.goto(`/${hash}`);
  await page.getByLabel('Password').fill('arxcovo-demo');
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
}

// Finite entrances (rise, trace reveal) are settled so each shot shows the finished view.
const shot = (page: Page, name: string) => page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: true, animations: 'disabled' });

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

test('hosts: each column head draws a 24 h CPU trace; gaps and a host without samples are hatched', async ({ page }) => {
  await signIn(page);
  const cards = page.getByRole('list', { name: 'Hosts' }).getByRole('listitem');
  for (const name of ['atlas', 'harbor', 'forge']) {
    const trace = cards.filter({ hasText: name }).locator('.col-h').getByRole('img', { name: /^CPU over the last 24 hours/ });
    await expect(trace).toBeVisible();
    expect((await trace.boundingBox())!.height).toBeGreaterThan(20);
  }
  // atlas lost samples: hatched gap bands sit inside its trace, painted with the hatch.
  const gap = cards.filter({ hasText: 'atlas' }).locator('.trace-gap').first();
  await expect(gap).toBeVisible();
  expect(await gap.evaluate(el => getComputedStyle(el).backgroundImage)).toContain('repeating-linear-gradient');
  // The windows host's line uses the windows data colour.
  expect(await cards.filter({ hasText: 'forge' }).locator('.trace-line').evaluate(el => getComputedStyle(el).stroke)).toBe('rgb(76, 194, 255)');
  // pebble has no samples: a hatched band, no trace.
  const pebble = cards.filter({ hasText: 'pebble' });
  await expect(pebble.getByRole('img', { name: /CPU over/ })).toHaveCount(0);
  await expect(pebble.locator('.spark-na')).toBeVisible();
  await expect(pebble.getByText('No CPU samples in the last 24 hours')).toBeAttached();
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
});

test('host detail: CPU history switches between 1h, 6h and 24h with avg and peak, without page overflow', async ({ page }) => {
  await signIn(page, '#/hosts/atlas');
  const tile = page.locator('section.tile', { has: page.getByRole('heading', { level: 2, name: 'CPU' }) });
  const range = tile.getByRole('group', { name: 'CPU history range' });
  await expect(tile.getByRole('img', { name: /^CPU over the last 24 hours, 10-minute buckets: average \d+%, peak \d+%/ })).toBeVisible();
  await expect(range.getByRole('button', { name: '24h' })).toHaveAttribute('aria-pressed', 'true');
  await expect(tile.locator('dt', { hasText: 'avg' }).locator('+ dd')).toHaveText(/^\d+%$/);
  await expect(tile.locator('dt', { hasText: 'peak' }).locator('+ dd')).toHaveText(/^\d+%$/);
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
  await shot(page, '13-host-cpu-24h');
  await range.getByRole('button', { name: '1h' }).click();
  await expect(tile.getByRole('img', { name: /last hour, 1-minute buckets: .*6 of 60 buckets missing/ })).toBeVisible();
  await expect(tile.locator('.trace-gap')).toHaveCount(1);
  await expect(tile.getByText(/6 of 60 buckets missing/)).toBeVisible();
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
  await shot(page, '14-host-cpu-1h');

  // The two-handle window selects whole buckets from the keyboard; avg, peak and missing follow it.
  const start = tile.getByRole('slider', { name: 'Window start' });
  const end = tile.getByRole('slider', { name: 'Window end' });
  await expect(start).toHaveAttribute('aria-valuenow', '0');
  await expect(end).toHaveAttribute('aria-valuenow', '59');
  await start.focus();
  for (let i = 0; i < 30; i += 1) await page.keyboard.press('ArrowRight');
  await expect(start).toHaveAttribute('aria-valuenow', '30');
  await expect(start).toHaveAttribute('aria-valuetext', '30 min ago');
  await expect(tile.getByText('Window: 30 min ago → now · 30 buckets')).toBeVisible();
  await expect(tile.getByText(/6 of 30 buckets missing in the window/)).toBeVisible();
  await expect(tile.locator('.trace-dim')).toHaveCount(1);
  await end.focus();
  await page.keyboard.press('Home');
  await expect(end).toHaveAttribute('aria-valuenow', '30');
  await expect(tile.getByText('Window: 30 min ago → 29 min ago · 1 bucket')).toBeVisible();
  await page.keyboard.press('End');
  await expect(end).toHaveAttribute('aria-valuenow', '59');
  // Dragging the end handle to the middle of the track lands on whole buckets.
  const track = (await tile.locator('.brush').boundingBox())!;
  const handle = (await end.boundingBox())!;
  await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
  await page.mouse.down();
  await page.mouse.move(track.x + track.width * 0.75, handle.y + handle.height / 2, { steps: 4 });
  await page.mouse.up();
  await expect(end).toHaveAttribute('aria-valuenow', '44');
  await expect(tile.locator('dt', { hasText: 'avg' }).locator('+ dd')).toHaveText(/^\d+%$/);
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
  await shot(page, '16-host-cpu-window');
  await range.getByRole('button', { name: '6h' }).click();
  await expect(tile.getByRole('img', { name: /last 6 hours, 5-minute buckets/ })).toBeVisible();
  await expect(range.getByRole('button', { pressed: true })).toHaveText('6h');
  expect(await overflowX(page)).toBeLessThanOrEqual(0);

  // An offline host with no buffer: avg and peak are hatched, and nothing reads 0%.
  await page.goto('/#/hosts/pebble');
  const pebbleTile = page.locator('section.tile', { has: page.getByRole('heading', { level: 2, name: 'CPU' }) });
  await expect(pebbleTile.getByText('no samples')).toBeVisible();
  await expect(pebbleTile.locator('dt', { hasText: 'avg' }).locator('+ dd')).toHaveText('usage unavailable');
  await expect(pebbleTile.getByRole('img', { name: /CPU over/ })).toHaveCount(0);
  expect(await pebbleTile.textContent()).not.toMatch(/\b0%/);
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
  await shot(page, '15-host-cpu-no-samples');
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
  const list = page.getByRole('list', { name: 'Activity, page 1' });
  const items = list.getByRole('listitem');
  // The host-only page (11 forge rows) can still be on screen while the
  // host+outcome query loads, so wait for the exact filtered count (6 failed
  // forge rows in the fixtures) and check every row with retrying locators
  // instead of a one-time items.all() snapshot.
  await expect(items).toHaveCount(6);
  await expect(items.first()).toBeVisible();
  await expect(list.locator('li:not([data-outcome="failed"])')).toHaveCount(0);
  // String form is case-sensitive, like the per-row toContainText it replaces.
  await expect(items).toContainText(Array<string>(6).fill('forge'));
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
  const search = page.getByRole('combobox', { name: 'Search hosts, VMs and tools' });
  // A bare "/" is an ordinary key (WCAG 2.2 SC 2.1.4); Ctrl+K focuses the search.
  await page.keyboard.press('/');
  await expect(search).not.toBeFocused();
  await expect(search).toHaveAttribute('aria-keyshortcuts', 'Control+K Meta+K');
  await page.keyboard.press('Control+K');
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

test('search: Tab leaves the combobox and closes the list; at 390 px the open list stays on screen', async ({ page }) => {
  await signIn(page);
  await expect(page.getByRole('list', { name: 'Hosts' })).toBeVisible();
  const search = page.getByRole('combobox', { name: 'Search hosts, VMs and tools' });
  const options = page.getByRole('listbox', { name: 'Search results' }).getByRole('option');
  await search.fill('ubuntu');
  await expect(options).toHaveCount(2);
  // Tab leaves the combobox for the next control (Sign out) and closes the list.
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'Sign out' })).toBeFocused();
  await expect(search).toHaveAttribute('aria-expanded', 'false');
  expect(await page.evaluate(() => document.activeElement?.getAttribute('role') ?? null)).not.toBe('option');

  // Enter on a closed list does nothing: open it, Escape, then Enter on the field itself.
  await search.focus();
  await expect(options).toHaveCount(2);
  await search.press('Escape');
  await expect(search).toBeFocused();
  await expect(search).toHaveAttribute('aria-expanded', 'false');
  await search.press('Enter');
  await expect(page).toHaveURL(/#\/hosts$/);
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
  await expect(page.getByRole('list', { name: 'Hosts' })).toBeVisible();

  // At 390 px every option of the open list is on screen (bounds read in one evaluation).
  await page.setViewportSize({ width: 390, height: 844 });
  await search.fill('a');
  await expect(options).toHaveCount(8);
  const bounds = await options.evaluateAll(rows => rows.map(row => {
    const box = row.getBoundingClientRect();
    return [box.left, box.right, box.width];
  }));
  expect(bounds).toHaveLength(8);
  expect(bounds.every(([left, right, width]) => width > 0 && left >= 0 && right <= 390), JSON.stringify(bounds)).toBe(true);
  await expect(search).toHaveAttribute('aria-expanded', 'true');
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
  await shot(page, '12-search-390');
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
  // CPU traces are drawn in full at once: no reveal clip, no running animation.
  const trace = column.locator('.trace svg');
  await expect(trace).toBeVisible();
  expect(await trace.evaluate(el => [getComputedStyle(el).animationName, getComputedStyle(el).clipPath])).toEqual(['none', 'none']);
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
      expect.soft(await viewPartFailures(page), `${where}: PR B view parts (3:1)`).toEqual([]);
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
    await page.getByLabel('Tool', { exact: true }).selectOption('list_hosts');
    await expect(page.getByText('No activity matches these filters')).toBeVisible();
    await audit('activity, empty filter');
    await page.getByText('Include reads').click();
    await page.getByRole('group', { name: 'Outcome' }).getByRole('button', { name: 'succeeded' }).click();
    await expect(page.getByRole('list', { name: 'Activity, page 1' }).getByRole('listitem').first()).toBeVisible();
    await audit('activity, outcome chip pressed and reads switch on');

    // PR B views: the board filter, the search jump list, VM table filters and the idle console tile.
    await page.goto('/#/hosts');
    await expect(page.getByRole('list', { name: 'Hosts' })).toBeVisible();
    await expect(page.getByRole('img', { name: /^CPU over the last 24 hours/ }).first()).toBeVisible();
    await audit('hosts board with CPU traces');
    await page.getByRole('group', { name: 'Show VMs' }).getByRole('button', { name: 'Static' }).click();
    await expect(page.getByText('No static VMs').first()).toBeVisible();
    await audit('hosts board, static filter pressed');
    const search = page.getByRole('combobox', { name: 'Search hosts, VMs and tools' });
    await search.fill('a');
    const options = page.getByRole('listbox', { name: 'Search results' }).getByRole('option');
    await expect(options.first()).toBeVisible();
    await search.press('ArrowDown');
    await expect(options.first()).toHaveAttribute('aria-selected', 'true');
    await audit('search listbox, first option active');
    await search.fill('zzz-nothing');
    await expect(page.getByText('No host, VM or tool matches “zzz-nothing”')).toBeVisible();
    await audit('search, no match');
    await search.press('Escape');

    // Host CPU history: the 1h range (a hatched gap and the missing count) and the no-samples state.
    await page.goto('/#/hosts/atlas');
    await page.getByRole('group', { name: 'CPU history range' }).getByRole('button', { name: '1h' }).click();
    await expect(page.getByText(/6 of 60 buckets missing/)).toBeVisible();
    await audit('host CPU history, 1h range pressed');
    const windowStart = page.getByRole('slider', { name: 'Window start' });
    await windowStart.focus();
    for (let i = 0; i < 20; i += 1) await page.keyboard.press('ArrowRight');
    await page.getByRole('slider', { name: 'Window end' }).press('PageDown');
    await expect(page.getByText(/^Window: 40 min ago → 5 min ago/)).toBeVisible();
    await audit('host CPU history, window narrowed');
    await page.goto('/#/hosts/pebble');
    await expect(page.getByText('no samples', { exact: true })).toBeVisible();
    await audit('host CPU history, no samples');

    await page.goto('/#/vms');
    await expect(page.getByText('5 of 5 VMs')).toBeVisible();
    await page.getByRole('group', { name: 'State' }).getByRole('button', { name: /running/ }).click();
    await page.getByRole('group', { name: 'Kind' }).getByRole('button', { name: /Test/ }).click();
    await expect(page.getByText('2 of 5 VMs')).toBeVisible();
    await audit('VMs, state chip and kind pressed');
    await page.getByLabel('Filter by name').fill('zzz');
    await expect(page.getByText('No VMs match these filters')).toBeVisible();
    await audit('VMs, name filter with no match');

    await page.goto('/#/vms/ubuntu-build-02');
    await expect(page.getByRole('link', { name: 'Open console' })).toBeVisible();
    await audit('VM page, idle console tile');

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
