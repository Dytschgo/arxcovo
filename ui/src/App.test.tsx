import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, test, vi } from 'vitest';
import { App } from './App';
import { DEMO_CREDENTIALS, FIXTURE_HOSTS } from './api/fixtures';
import { StandinApi, type StandinOptions } from './api/standin';
import { ApiError, type ArxApi } from './api/types';
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
  test('cards show OS, arch badges, reachability and reasons', async () => {
    await signIn();
    const list = await screen.findByRole('list', { name: 'Hosts' });
    const cards = within(list).getAllByRole('listitem');
    expect(cards).toHaveLength(FIXTURE_HOSTS.length);
    const harbor = cards[1];
    expect(within(harbor).getByText('arm64')).toBeTruthy();
    expect(within(harbor).getByText('online')).toBeTruthy();
    expect(within(cards[3]).getByText('No heartbeat for 2 hours')).toBeTruthy();
    expect(screen.getByText('2 of 4 hosts online')).toBeTruthy();
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
    expect(screen.getByRole('link', { name: 'Open terminal' }).getAttribute('href')).toBe('#/terminal/atlas');
    go('#/hosts/pebble');
    expect(await screen.findByText('No heartbeat for 2 hours')).toBeTruthy();
    expect(screen.queryByRole('link', { name: 'Open terminal' })).toBeNull();
    expect(screen.getByText('Open terminal').getAttribute('aria-disabled')).toBe('true');
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
    fireEvent.change(screen.getByLabelText('Outcome'), { target: { value: 'refused' } });
    const filtered = await screen.findByRole('list', { name: 'Activity, page 1' });
    const items = within(filtered).getAllByRole('listitem');
    expect(items.every(i => i.getAttribute('data-outcome') === 'refused')).toBe(true);
    expect(screen.getByRole('button', { name: 'Older' })).toHaveProperty('disabled', true);
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
    expect(input).toHaveProperty('value', '');
    fireEvent.keyDown(input, { key: 'ArrowUp' });
    expect(input).toHaveProperty('value', 'git --version');
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
    const select = await screen.findByLabelText('Host');
    const pebble = within(select).getByRole('option', { name: 'pebble (offline)' });
    expect(pebble).toHaveProperty('disabled', true);
  });
});

describe('console', () => {
  test('shows streamed frames with a descriptive alternative text', async () => {
    await signIn({}, '#/vms/ubuntu-build-02/console');
    expect(await screen.findByRole('heading', { level: 1, name: 'Console · ubuntu-build-02' })).toBeTruthy();
    const img = await screen.findByRole('img', { name: /Console of ubuntu-build-02, frame \d+/ });
    expect(img.getAttribute('src')).toMatch(/^blob:/);
    expect(screen.getByText('View only')).toBeTruthy();
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

  test('the theme toggle cycles system, light and dark on the root element', async () => {
    await signIn();
    const toggle = await screen.findByRole('button', { name: /^Theme: system/ });
    fireEvent.click(toggle);
    expect(document.documentElement.dataset.theme).toBe('light');
    fireEvent.click(screen.getByRole('button', { name: /^Theme: light/ }));
    expect(document.documentElement.dataset.theme).toBe('dark');
    fireEvent.click(screen.getByRole('button', { name: /^Theme: dark/ }));
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
