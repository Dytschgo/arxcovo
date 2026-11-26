import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, test, vi } from 'vitest';
import { App } from './App';
import { DEMO_CREDENTIALS, FIXTURE_ACTIVITY, FIXTURE_HOSTS, FIXTURE_VMS } from './api/fixtures';
import { StandinApi, type StandinOptions } from './api/standin';
import { ApiError, type ArxApi, type Host } from './api/types';
import { formatDate } from './app/format';
import { ViewBoundary } from './app/ui';

async function signIn(options: StandinOptions = {}, hash = '#/hosts') {
  window.location.hash = hash;
  const api = new StandinApi(options);
  await api.login(DEMO_CREDENTIALS);
  render(<App api={api} />);
  return api;
}

function go(hash: string) {
  window.location.hash = hash;
  window.dispatchEvent(new HashChangeEvent('hashchange'));
}

describe('login and session', () => {
  test('the stand-in shows the demo banner and a labelled login form; wrong passwords keep you out', async () => {
    render(<App api={new StandinApi()} />);
    expect(screen.getByRole('note', { name: 'Demo data' })).toBeTruthy();
    const password = await screen.findByLabelText('Password');
    expect(screen.getByLabelText('Username')).toHaveProperty('value', 'owner');
    fireEvent.change(password, { target: { value: 'wrong' } });
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Wrong username or password.');
    expect(screen.getByLabelText('Password')).toHaveProperty('value', '');
  });

  test('signing in shows the hosts and signing out returns to the login form', async () => {
    render(<App api={new StandinApi()} />);
    fireEvent.change(await screen.findByLabelText('Password'), { target: { value: DEMO_CREDENTIALS.password } });
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(await screen.findByRole('heading', { level: 1, name: 'Hosts' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(await screen.findByText('You are signed out.')).toBeTruthy();
  });

  test('the HTTP adapter never shows the demo banner', async () => {
    const api = { kind: 'http', getSession: async () => null } as unknown as ArxApi;
    render(<App api={api} />);
    await screen.findByLabelText('Password');
    expect(screen.queryByRole('note', { name: 'Demo data' })).toBeNull();
    expect(screen.queryByText(/arxcovo-demo/)).toBeNull();
  });

  test('a request that finds the session gone sends you to login with a notice', async () => {
    await signIn({ failures: { listHosts: new ApiError('unauthorized', 'gone', 401) } });
    expect(await screen.findByText('Your session has ended. Sign in again.')).toBeTruthy();
    expect(screen.getByLabelText('Password')).toBeTruthy();
  });
});

describe('hosts', () => {
  test('the board has one column per host with OS, arch badge, reachability, reasons and its VM cards', async () => {
    await signIn();
    const list = await screen.findByRole('list', { name: 'Hosts' });
    const columns = within(list).getAllByRole('listitem');
    expect(columns).toHaveLength(FIXTURE_HOSTS.length);
    const harbor = columns[1];
    const head = harbor.querySelector('.col-h') as HTMLElement;
    expect(within(head).getByText('arm64')).toBeTruthy();
    expect(within(head).getByText('online')).toBeTruthy();
    expect(within(head).getByRole('link', { name: 'harbor' }).getAttribute('href')).toBe('#/hosts/harbor');
    expect(within(harbor).getByRole('article', { name: /ubuntu-arm-03/ })).toBeTruthy();
    expect(harbor.getAttribute('data-reachability')).toBe('online');
    expect(within(columns[3]).getByText('No heartbeat for 2 hours')).toBeTruthy();
    expect(within(columns[3]).getByText('No VMs')).toBeTruthy();
    // atlas holds three VMs; every card sits in its host's column.
    expect(within(columns[0]).getAllByRole('article')).toHaveLength(3);
    expect(screen.getByText('2 of 4 hosts online')).toBeTruthy();
  });

  test('the board header totals count every host and VM, and the kind filter only narrows the cards', async () => {
    await signIn();
    const list = await screen.findByRole('list', { name: 'Hosts' });
    const totals = screen.getByText('Hosts not online').closest('dl')!;
    expect(totals.textContent).toBe('Hosts not online1 degraded1 offline');
    const vms = screen.getByText('VMs on all hosts, incl. offline').closest('dl')!;
    expect(vms.textContent).toBe('VMs on all hosts, incl. offline5 total3 running4 test1 static');
    fireEvent.click(within(screen.getByRole('group', { name: 'Show VMs' })).getByRole('button', { name: 'Static' }));
    expect(within(list).getAllByRole('article').map(a => a.getAttribute('aria-label'))).toEqual(['web-static, static VM, running']);
    expect(within(within(list).getAllByRole('listitem')[1]).getByText('No static VMs')).toBeTruthy();
    expect(vms.textContent).toBe('VMs on all hosts, incl. offline5 total3 running4 test1 static');
  });

  test('a host without a CPU sample shows usage unavailable on its column, never 0', async () => {
    await signIn();
    const list = await screen.findByRole('list', { name: 'Hosts' });
    const pebble = within(list).getAllByRole('listitem')[3];
    const cpu = pebble.querySelector('.cpu-now') as HTMLElement;
    expect(within(cpu).getByText('usage unavailable').getAttribute('data-null')).toBe('true');
    expect(cpu.textContent).not.toMatch(/\d/);
  });

  test('an error offers a retry and an empty cluster explains how to join', async () => {
    await signIn({ failures: { listHosts: new ApiError('unavailable', 'arxd is unavailable right now.', 503) } });
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('Could not load hosts. arxd is unavailable right now.');
    expect(within(alert).getByRole('button', { name: 'Try again' })).toBeTruthy();
  });

  test('an empty cluster explains how to add a host', async () => {
    await signIn({ hosts: [] });
    expect(await screen.findByText('No hosts yet')).toBeTruthy();
  });

  test('host detail lists tools and VMs, and an offline host cannot open a terminal', async () => {
    await signIn({}, '#/hosts/atlas');
    expect(await screen.findByText('qemu-system-x86_64')).toBeTruthy();
    expect(screen.getByRole('heading', { level: 1, name: 'atlas' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'win11-ci-01' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Run command' }).getAttribute('href')).toBe('#/terminal/atlas');
    go('#/hosts/pebble');
    expect(await screen.findByText('No heartbeat for 2 hours')).toBeTruthy();
    expect(screen.queryByRole('link', { name: 'Run command' })).toBeNull();
    expect(screen.getByText('Run command').closest('[aria-disabled]')!.getAttribute('aria-disabled')).toBe('true');
  });
});

describe('host detail polish', () => {
  const atlas = FIXTURE_HOSTS[0];

  test('availability is labelled as availability, never as a dedicated account', async () => {
    await signIn({}, '#/hosts/atlas');
    const facts = (await screen.findByText('Operating system')).closest('dl')!;
    expect(within(facts).getByText('Availability').closest('div')!.textContent).toBe('AvailabilityAlways on');
    expect(within(facts).queryByText('Mode')).toBeNull();
    expect(facts.textContent).not.toMatch(/dedicated/i);
    go('#/hosts/harbor');
    expect((await screen.findByText('Opportunistic (used day to day)'))).toBeTruthy();
  });

  test('a CPU reading shows a bar with its percentage', async () => {
    await signIn({}, '#/hosts/atlas');
    const cpu = await screen.findByRole('meter', { name: 'CPU' });
    expect(cpu.getAttribute('aria-valuenow')).toBe('38');
    expect(cpu.getAttribute('aria-valuetext')).toBe('38%');
    expect(cpu.getAttribute('aria-hidden')).toBeNull();
    expect(cpu.hasAttribute('data-unknown')).toBe(false);
    expect(within(cpu.closest('section') as HTMLElement).queryByText(/usage unavailable/)).toBeNull();
    expect(within(cpu.closest('section') as HTMLElement).getByText('38').textContent).toBe('38');
  });

  test('when the host reports no CPU usage the row says so instead of faking a bar', async () => {
    const host: Host = { ...atlas, cpu: { ...atlas.cpu, usedPercent: null } };
    await signIn({ hosts: [host] }, '#/hosts/atlas');
    const tile = (await screen.findByRole('heading', { level: 2, name: 'CPU' })).closest('section') as HTMLElement;
    expect(within(tile).getByText(/16 cores/)).toBeTruthy();
    // No false 0% for assistive technology: there is no meter and no number at all.
    expect(screen.queryByRole('meter', { name: 'CPU' })).toBeNull();
    expect(tile.querySelector('.meter-track')).toBeNull();
    expect(tile.querySelector('[aria-valuenow]')).toBeNull();
    expect(tile.querySelector('.big')).toBeNull();
    // The state is conveyed by visible text that stays in the accessibility tree.
    const text = within(tile).getByText('usage unavailable');
    expect(text.getAttribute('data-null')).toBe('true');
    expect(text.closest('[aria-hidden="true"]')).toBeNull();
    // Memory and disk keep real meters.
    expect(screen.getByRole('meter', { name: 'Memory' }).getAttribute('aria-valuenow')).toBe('55');
  });
});

describe('virtual machines', () => {
  test('the table shows an arch badge per VM and filters by architecture', async () => {
    await signIn({}, '#/vms');
    const table = await screen.findByRole('table');
    expect(within(table).getAllByRole('row')).toHaveLength(6);
    expect(within(table).getAllByTitle('Machine architecture: arm64')).toHaveLength(1);
    fireEvent.change(screen.getByLabelText('Architecture'), { target: { value: 'arm64' } });
    expect(within(screen.getByRole('table')).getAllByRole('row')).toHaveLength(2);
    expect(screen.getByText('1 of 5 VMs')).toBeTruthy();
  });

  test('VM detail shows snapshots, expiry and greys out incompatible hosts with a reason', async () => {
    await signIn({}, '#/vms/web-static');
    expect(await screen.findByText('Never (static)')).toBeTruthy();
    expect(screen.getByRole('heading', { level: 1, name: 'web-static' })).toBeTruthy();
    expect(screen.getByText('nightly 2026-11-22')).toBeTruthy();
    const hosts = screen.getByRole('list', { name: 'Compatible hosts' });
    const harbor = within(hosts).getByText('harbor').closest('li')!;
    expect(harbor.getAttribute('aria-disabled')).toBe('true');
    expect(within(harbor).getByText('arm64 host cannot run x86_64 guests')).toBeTruthy();
    const forge = within(hosts).getByText('forge').closest('li')!;
    expect(within(forge).getByText('Compatible')).toBeTruthy();
  });

  test('a VM without snapshots says so', async () => {
    await signIn({}, '#/vms/ubuntu-arm-03');
    expect(await screen.findByText('No snapshots yet')).toBeTruthy();
  });
});

describe('activity', () => {
  test('pages forward and back and filters reset to the first page', async () => {
    await signIn({}, '#/activity');
    const first = await screen.findByRole('list', { name: 'Activity, page 1' });
    expect(within(first).getAllByRole('listitem')).toHaveLength(20);
    expect(screen.getByRole('button', { name: 'Newer' })).toHaveProperty('disabled', true);
    fireEvent.click(screen.getByRole('button', { name: 'Older' }));
    expect(await screen.findByRole('list', { name: 'Activity, page 2' })).toBeTruthy();
    fireEvent.click(within(screen.getByRole('group', { name: 'Outcome' })).getByRole('button', { name: 'refused' }));
    const filtered = await screen.findByRole('list', { name: 'Activity, page 1' });
    const items = within(filtered).getAllByRole('listitem');
    expect(items.every(i => i.getAttribute('data-outcome') === 'refused')).toBe(true);
    expect(screen.getByRole('button', { name: 'Older' })).toHaveProperty('disabled', true);
  });

  test('a row whose summary only repeats the tool name shows the tool once', async () => {
    const base = FIXTURE_ACTIVITY[0];
    const entries = [
      { ...base, id: 2, tool: 'run_command', summary: 'run_command', hostId: 'atlas', exitCode: 0 },
      { ...base, id: 1, tool: 'run_command', summary: 'cargo test --workspace', hostId: 'atlas', exitCode: 0 },
    ];
    await signIn({ activity: entries }, '#/activity');
    const list = await screen.findByRole('list', { name: /Activity/ });
    const [repeat, distinct] = within(list).getAllByRole('listitem');
    expect(within(repeat).getAllByText('run_command')).toHaveLength(1);
    expect(repeat.textContent).toContain('exit 0');
    expect(within(repeat).getByRole('link', { name: 'atlas' })).toBeTruthy();
    expect(within(distinct).getByText('run_command')).toBeTruthy();
    expect(within(distinct).getByText('cargo test --workspace')).toBeTruthy();
  });

  test('filters with no matches show an empty state with a way out', async () => {
    await signIn({}, '#/activity');
    await screen.findByRole('list', { name: 'Activity, page 1' });
    fireEvent.change(screen.getByLabelText('Tool'), { target: { value: 'list_hosts' } });
    expect(await screen.findByText('No activity matches these filters')).toBeTruthy();
    fireEvent.click(screen.getByLabelText('Include reads'));
    expect(await screen.findByRole('list', { name: 'Activity, page 1' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }));
    await waitFor(() => expect(screen.getByLabelText('Tool')).toHaveProperty('value', ''));
  });
});

describe('terminal', () => {
  test('runs a command, streams output into the log and reports the exit', async () => {
    await signIn({}, '#/terminal/atlas');
    const input = await screen.findByLabelText('Command');
    await waitFor(() => expect(input).toHaveProperty('disabled', false));
    fireEvent.change(input, { target: { value: 'git --version' } });
    fireEvent.click(screen.getByRole('button', { name: 'Run' }));
    const log = screen.getByRole('log', { name: 'Terminal output for atlas' });
    await waitFor(() => expect(log.textContent).toContain('exit 0 · succeeded'));
    expect(log.textContent).toContain('git version 2.43.0');
    const status = document.querySelector('.term-head .st')!;
    expect(status.textContent).toBe('Terminal status: ready');
    expect(status.querySelector('svg[data-icon]')).toBeTruthy();
    expect(status.querySelector('.st-word')!.textContent).toBe('ready');
    expect(input).toHaveProperty('value', '');
    fireEvent.keyDown(input, { key: 'ArrowUp' });
    expect(input).toHaveProperty('value', 'git --version');
  });

  test('the placeholder suggests a tool the host actually has, or a neutral hint', async () => {
    const atlas = FIXTURE_HOSTS[0];
    const noGit: Host = { ...atlas, tools: [{ name: 'cargo', version: '1.91.0' }, { name: 'node', version: null }] };
    await signIn({ hosts: [noGit] }, '#/terminal/atlas');
    expect((await screen.findByLabelText('Command')).getAttribute('placeholder')).toBe('cargo --version');
  });

  test('a host with no discovered tools gets a neutral placeholder, not a git example', async () => {
    const bare: Host = { ...FIXTURE_HOSTS[0], tools: [] };
    await signIn({ hosts: [bare] }, '#/terminal/atlas');
    const placeholder = (await screen.findByLabelText('Command')).getAttribute('placeholder');
    expect(placeholder).toBe('tool name and arguments');
    expect(placeholder).not.toContain('git');
  });

  test('a malformed command line is explained without being sent', async () => {
    await signIn({}, '#/terminal/atlas');
    const input = await screen.findByLabelText('Command');
    await waitFor(() => expect(input).toHaveProperty('disabled', false));
    fireEvent.change(input, { target: { value: 'git "unterminated' } });
    fireEvent.click(screen.getByRole('button', { name: 'Run' }));
    expect((await screen.findByRole('alert')).textContent).toBe('Close the open quote.');
    expect(screen.getByRole('log').textContent).not.toContain('git');
  });

  test('offline hosts cannot be chosen', async () => {
    await signIn({}, '#/terminal');
    const hosts = await screen.findByRole('list', { name: 'Host' });
    const pebble = within(hosts).getByText('pebble').closest('li')!;
    expect(within(pebble).queryByRole('link')).toBeNull();
    expect(pebble.querySelector('[aria-disabled="true"]')!.textContent).toContain('No heartbeat for 2 hours');
    const atlas = within(hosts).getByRole('link', { name: /atlas/ });
    expect(atlas.getAttribute('href')).toBe('#/terminal/atlas');
    expect(atlas.getAttribute('aria-current')).toBe('true');
  });
});

describe('console', () => {
  test('shows streamed frames with a descriptive alternative text', async () => {
    await signIn({}, '#/vms/ubuntu-build-02/console');
    expect(await screen.findByRole('heading', { level: 1, name: 'Console · ubuntu-build-02' })).toBeTruthy();
    const img = await screen.findByRole('img', { name: /Console of ubuntu-build-02, frame \d+/ });
    expect(img.getAttribute('src')).toMatch(/^blob:/);
    expect(screen.getByText('View only')).toBeTruthy();
    const status = document.querySelector('.console-bar .st[data-status="streaming"]')!;
    expect(status.textContent).toBe('Console status: streaming');
    expect(status.getAttribute('data-status')).toBe('streaming');
    expect(status.querySelector('svg[data-icon="monitor"]')).toBeTruthy();
  });

  test('a stopped VM explains why there is no picture', async () => {
    await signIn({}, '#/vms/win11-test-04/console');
    expect((await screen.findByText(/is stopped; start it/)).textContent).toContain('win11-test-04');
    expect(screen.getByRole('button', { name: 'Reconnect' })).toBeTruthy();
  });
});

describe('shell', () => {
  test('navigation marks the current section and unknown routes get a not-found page', async () => {
    await signIn({}, '#/vms/win11-ci-01');
    await screen.findByText('clean-install');
    const nav = screen.getByRole('navigation', { name: 'Main' });
    expect(within(nav).getByRole('link', { name: 'VMs' }).getAttribute('aria-current')).toBe('page');
    go('#/nowhere');
    expect(await screen.findByRole('heading', { level: 1, name: 'Page not found' })).toBeTruthy();
  });

  test('the top bar shows the logo, labels every section and only the current one shows its word; there is no theme control', async () => {
    await signIn({}, '#/activity');
    await screen.findByRole('list', { name: 'Activity, page 1' });
    const home = screen.getByRole('link', { name: 'Arxcovo home' });
    expect(home.querySelector('svg.logo')).toBeTruthy();
    const nav = screen.getByRole('navigation', { name: 'Main' });
    for (const name of ['Hosts', 'VMs', 'Activity', 'Terminal']) {
      const link = within(nav).getByRole('link', { name });
      const word = within(link).getByText(name);
      expect(link.querySelector('svg.i')).toBeTruthy();
      expect(word.className).toBe(name === 'Activity' ? 'nav-word' : 'visually-hidden');
    }
    expect(screen.getByText('Signed in as').parentElement!.textContent).toBe('Signed in as owner');
    expect(screen.queryByRole('button', { name: /theme/i })).toBeNull();
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });
});

describe('round 2: sign-out, stream close and render errors', () => {
  test('a failed sign-out keeps you signed in, says so, and a retry signs out', async () => {
    await signIn({ failures: { logout: new ApiError('unavailable', 'arxd is restarting.', 503) } });
    await screen.findByRole('list', { name: 'Hosts' });
    fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('Sign-out failed.');
    expect(alert.textContent).toContain('arxd is restarting.');
    expect(screen.getByRole('navigation', { name: 'Main' })).toBeTruthy();
    expect(screen.queryByText('You are signed out.')).toBeNull();
    fireEvent.click(within(alert).getByRole('button', { name: 'Try sign-out again' }));
    expect(await screen.findByText('You are signed out.')).toBeTruthy();
  });

  test('when the server ends the session, an open terminal closes and the login form explains why', async () => {
    const api = await signIn({}, '#/terminal/atlas');
    const input = await screen.findByLabelText('Command');
    await waitFor(() => expect(input).toHaveProperty('disabled', false));
    act(() => api.endSession());
    expect(await screen.findByText('Your session has ended. Sign in again.')).toBeTruthy();
    expect(screen.getByLabelText('Password')).toBeTruthy();
  });

  test('when the server ends the session, an open console closes and the login form explains why', async () => {
    const api = await signIn({}, '#/vms/ubuntu-build-02/console');
    await screen.findByRole('img', { name: /Console of ubuntu-build-02/ });
    act(() => api.endSession());
    expect(await screen.findByText('Your session has ended. Sign in again.')).toBeTruthy();
  });

  test('a render error is contained to the view and can be retried', () => {
    let explode = true;
    function Fragile() {
      if (explode) throw new Error('bad payload');
      return <p>recovered</p>;
    }
    const spy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    try {
      render(<ViewBoundary resetKey="#/vms/x/console"><Fragile /></ViewBoundary>);
      const alert = screen.getByRole('alert');
      expect(alert.textContent).toContain('Something unexpected happened');
      explode = false;
      fireEvent.click(within(alert).getByRole('button', { name: 'Try again' }));
      expect(screen.getByText('recovered')).toBeTruthy();
    } finally {
      spy.mockRestore();
    }
  });

  test('an unparseable timestamp formats as a dash instead of throwing', () => {
    expect(formatDate('not a date')).toBe('—');
    expect(formatDate('2026-11-23T12:00:00Z')).not.toBe('—');
  });
});

describe('Night Orbit views: search, console closed state, not found', () => {
  test('the search jump list filters loaded hosts, VMs and tools, moves with the arrows and jumps on Enter', async () => {
    const api = await signIn();
    await screen.findByRole('list', { name: 'Hosts' });
    const listHosts = vi.spyOn(api, 'listHosts');
    const search = screen.getByRole('combobox', { name: 'Search hosts, VMs and tools' });
    fireEvent.keyDown(document.body, { key: '/' });
    expect(document.activeElement).toBe(search);
    fireEvent.focus(search);
    fireEvent.change(search, { target: { value: 'ubuntu' } });
    const results = await screen.findByRole('listbox', { name: 'Search results' });
    const options = await within(results).findAllByRole('option');
    expect(options.map(o => o.textContent)).toEqual(['ubuntu-build-02test VM', 'ubuntu-arm-03test VM']);
    expect(search.getAttribute('aria-expanded')).toBe('true');
    // Typing filters on the client: one load on focus, none per keystroke.
    fireEvent.change(search, { target: { value: 'ubuntu-a' } });
    expect(within(results).getAllByRole('option')).toHaveLength(1);
    expect(listHosts).toHaveBeenCalledTimes(1);
    fireEvent.change(search, { target: { value: 'ubuntu' } });
    fireEvent.keyDown(search, { key: 'ArrowDown' });
    fireEvent.keyDown(search, { key: 'ArrowDown' });
    const active = search.getAttribute('aria-activedescendant')!;
    expect(document.getElementById(active)!.textContent).toBe('ubuntu-arm-03test VM');
    expect(document.getElementById(active)!.getAttribute('aria-selected')).toBe('true');
    fireEvent.keyDown(search, { key: 'Enter' });
    expect(window.location.hash).toBe('#/vms/ubuntu-arm-03');
    expect(search).toHaveProperty('value', '');
  });

  test('search lists a tool on reachable hosts only and says when nothing matches', async () => {
    await signIn();
    await screen.findByRole('list', { name: 'Hosts' });
    const search = screen.getByRole('combobox', { name: 'Search hosts, VMs and tools' });
    fireEvent.focus(search);
    fireEvent.change(search, { target: { value: 'python3' } });
    const options = await within(screen.getByRole('listbox', { name: 'Search results' })).findAllByRole('option');
    expect(options.map(o => o.getAttribute('href'))).toEqual(['#/terminal/atlas', '#/terminal/harbor']);
    fireEvent.change(search, { target: { value: 'nothing-like-this' } });
    expect(screen.getByRole('status').textContent).toBe('No host, VM or tool matches “nothing-like-this”');
    fireEvent.keyDown(search, { key: 'Escape' });
    expect(search.getAttribute('aria-expanded')).toBe('false');
  });

  test('a failed VM opens its console closed with the reason and never asks for a stream', async () => {
    const failed = { ...FIXTURE_VMS[1], state: 'failed' as const, ready: false };
    const api = await signIn({ vms: [failed] }, '#/vms/ubuntu-build-02/console');
    const open = vi.spyOn(api, 'openConsole');
    expect((await screen.findByText(/failed, so there is no picture/)).textContent).toContain('ubuntu-build-02');
    expect(screen.getByText('Console status:').parentElement!.textContent).toBe('Console status: closed');
    expect(screen.getByRole('heading', { level: 1, name: 'Console · ubuntu-build-02' })).toBeTruthy();
    expect(open).not.toHaveBeenCalled();
  });

  test('a running VM on an offline host opens its console closed with the host reason', async () => {
    const onPebble = { ...FIXTURE_VMS[3], hostId: 'pebble', state: 'running' as const };
    const api = await signIn({ vms: [onPebble] }, '#/vms/ubuntu-arm-03/console');
    const open = vi.spyOn(api, 'openConsole');
    expect(await screen.findByText('pebble is offline (No heartbeat for 2 hours), so its VMs have no console.')).toBeTruthy();
    expect(open).not.toHaveBeenCalled();
    expect(screen.queryByRole('img')).toBeNull();
  });

  test('the VM page waits for Open console before it opens a stream', async () => {
    const api = await signIn({}, '#/vms/ubuntu-build-02');
    const open = vi.spyOn(api, 'openConsole');
    const link = await screen.findByRole('link', { name: 'Open console' });
    expect(link.getAttribute('href')).toBe('#/vms/ubuntu-build-02/console');
    expect(open).not.toHaveBeenCalled();
    go('#/vms/ubuntu-build-02/console');
    await screen.findByRole('img', { name: /Console of ubuntu-build-02/ });
    expect(open).toHaveBeenCalledTimes(1);
  });

  test('VM sizes the API leaves out are hatched, never 0', async () => {
    const unsized = { ...FIXTURE_VMS[0], cpus: null, memoryBytes: null };
    await signIn({ vms: [unsized] }, '#/vms/win11-ci-01');
    const size = (await screen.findByRole('heading', { level: 2, name: 'Resources' })).closest('section')!;
    const dd = (label: string) => within(size).getByText(label).closest('div')!.querySelector('dd')!;
    expect(dd('vCPUs').textContent).toBe('unknown');
    expect(dd('vCPUs').querySelector('[data-null="true"]')).toBeTruthy();
    expect(dd('Memory').textContent).toBe('unknown');
    expect(dd('Disk').textContent).toBe('64 GiB');
  });

  test('the not-found page explains itself and links back to the hosts', async () => {
    await signIn({}, '#/nowhere');
    expect(await screen.findByRole('heading', { level: 1, name: 'Page not found' })).toBeTruthy();
    expect(screen.getByText('Nothing lives at this address.')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Go to hosts' }).getAttribute('href')).toBe('#/hosts');
  });
});
