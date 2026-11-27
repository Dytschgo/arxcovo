import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, test, vi } from 'vitest';
import { App } from '../App';
import { summarizeCpuHistory } from '../api/cpuHistory';
import { DEMO_CREDENTIALS, FIXTURE_HOSTS } from '../api/fixtures';
import { StandinApi, standinCpuHistory, type StandinOptions } from '../api/standin';
import { ApiError, CPU_HISTORY_RANGES, type CpuHistory, type CpuHistoryRange } from '../api/types';
import { traceGeometry, traceLabel } from './cpuHistory';

async function signIn(api: StandinApi, hash: string) {
  window.location.hash = hash;
  await api.login(DEMO_CREDENTIALS);
  render(<App api={api} />);
  return api;
}

const standin = (options: StandinOptions = {}) => new StandinApi(options);

function go(hash: string) {
  window.location.hash = hash;
  window.dispatchEvent(new HashChangeEvent('hashchange'));
}

function history(hostId: string, range: CpuHistoryRange, samples: (number | null)[]): CpuHistory {
  const base = standinCpuHistory(hostId, range, () => null);
  return { ...base, samples };
}

const flat = (range: CpuHistoryRange, value: number | null) => Array<number | null>(CPU_HISTORY_RANGES[range].points).fill(value);

/** Every y coordinate the path draws: a null bucket must never put a point at y = 100 (0%). */
const ys = (path: string) => [...path.matchAll(/[ML]([\d.]+) ([\d.]+)/g)].map(m => Number(m[2]));

/** A stand-in whose CPU history calls wait until the test settles them. */
class DeferredHistory extends StandinApi {
  calls: { hostId: string; range: CpuHistoryRange; resolve(h: CpuHistory): void; reject(e: unknown): void }[] = [];
  getCpuHistory(hostId: string, range: CpuHistoryRange): Promise<CpuHistory> {
    return new Promise((resolve, reject) => this.calls.push({ hostId, range, resolve, reject }));
  }
  last(hostId: string, range: CpuHistoryRange) {
    const call = [...this.calls].reverse().find(c => c.hostId === hostId && c.range === range);
    if (!call) throw new Error(`no ${hostId} ${range} request`);
    return call;
  }
}

const cpuTile = async () => (await screen.findByRole('heading', { level: 2, name: 'CPU' })).closest('section') as HTMLElement;
const readout = (tile: HTMLElement, label: 'avg' | 'peak') => within(tile).getByText(label, { selector: 'dt' }).nextElementSibling as HTMLElement;

afterEach(() => {
  vi.useRealTimers();
});

describe('trace geometry', () => {
  test('null buckets break the line into runs and become gaps; no point is drawn for a null', () => {
    const g = traceGeometry([10, null, null, 50, 60, null]);
    expect(g.runs).toEqual([{ start: 0, end: 1 }, { start: 3, end: 5 }]);
    expect(g.gaps).toEqual([{ start: 1, end: 3 }, { start: 5, end: 6 }]);
    expect(g.line.match(/M/g)).toHaveLength(2);
    // A lone sample is held flat across its bucket, so it is still drawn.
    expect(g.line.startsWith('M0 90 L0.5 90 L1 90')).toBe(true);
    expect(ys(g.line).every(y => [90, 50, 40].includes(y))).toBe(true);
    // The fill closes each run on the baseline only at the run's own edges.
    expect(g.area).toContain('M3 100 L3 50');
    expect(g.area).toContain('L5 40 L5 100 Z');
  });

  test('a 0% sample is drawn at the baseline while an all-null series draws nothing', () => {
    expect(ys(traceGeometry([0, 0]).line)).toEqual([100, 100, 100, 100]);
    const none = traceGeometry([null, null, null]);
    expect(none.line).toBe('');
    expect(none.area).toBe('');
    expect(none.gaps).toEqual([{ start: 0, end: 3 }]);
  });

  test('the label names the range, bucket size, avg, peak and the missing count', () => {
    expect(traceLabel(history('atlas', '1h', [...flat('1h', 40).slice(0, 57), null, null, 70]))).toBe(
      'CPU over the last hour, 1-minute buckets: average 41%, peak 70%, 2 of 60 buckets missing');
    expect(traceLabel(history('atlas', '24h', flat('24h', null)))).toBe('CPU over the last 24 hours, 10-minute buckets: no samples');
  });
});

describe('hosts board traces', () => {
  test('each host column draws its 24 h trace with hatched gaps; a host with no samples is hatched, never a 0 line', async () => {
    const api = await signIn(standin(), '#/hosts');
    const columns = within(await screen.findByRole('list', { name: 'Hosts' })).getAllByRole('listitem');
    const batch = new Map((await api.listCpuHistory('24h')).map(h => [h.hostId, h]));
    for (const [i, id] of ['atlas', 'harbor', 'forge'].entries()) {
      const trace = await within(columns[i]).findByRole('img', { name: /^CPU over the last 24 hours/ });
      expect(trace.getAttribute('aria-label')).toBe(traceLabel(batch.get(id)!));
      expect(trace.closest('.col-h')).not.toBeNull();
      expect(trace.getAttribute('data-os')).toBe(FIXTURE_HOSTS[i].os);
    }
    // atlas lost an hour of samples: the gap is hatched, and the line never dips to 0 for it.
    const atlas = within(columns[0]).getByRole('img', { name: /CPU over/ });
    expect(atlas.querySelectorAll('.trace-gap[data-null="true"]').length).toBeGreaterThan(0);
    expect(ys(atlas.querySelector('.trace-line')!.getAttribute('d')!)).not.toContain(100);
    // harbor has no gaps.
    expect(columns[1].querySelectorAll('.trace-gap')).toHaveLength(0);
    // pebble has no buffer: a hatched band that says so, and no trace.
    const pebble = columns[3];
    expect(within(pebble).queryByRole('img', { name: /CPU over/ })).toBeNull();
    const band = pebble.querySelector('.spark-na') as HTMLElement;
    expect(band.getAttribute('data-null')).toBe('true');
    expect(band.textContent).toBe('No CPU samples in the last 24 hours');
  });

  test('the board loads history in one batch request alongside the hosts', async () => {
    const api = standin();
    const batch = vi.spyOn(api, 'listCpuHistory');
    const single = vi.spyOn(api, 'getCpuHistory');
    await signIn(api, '#/hosts');
    await screen.findAllByRole('img', { name: /CPU over the last 24 hours/ });
    expect(batch).toHaveBeenCalledTimes(1);
    expect(batch).toHaveBeenCalledWith('24h');
    expect(single).not.toHaveBeenCalled();
  });

  test('a failed history request leaves the board intact, each column hatched as unavailable', async () => {
    await signIn(standin({ failures: { listCpuHistory: new ApiError('unavailable', 'arxd is busy.', 503) } }), '#/hosts');
    const columns = within(await screen.findByRole('list', { name: 'Hosts' })).getAllByRole('listitem');
    expect(columns).toHaveLength(4);
    await waitFor(() => expect(document.querySelectorAll('.spark-na')).toHaveLength(4));
    for (const column of columns) expect(within(column).getByText('CPU history unavailable')).toBeTruthy();
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryByRole('img', { name: /CPU over/ })).toBeNull();
    // CPU now still shows its own sample.
    expect(within(columns[0]).getByText('38')).toBeTruthy();
    expect(screen.getByText('2 of 4 hosts online')).toBeTruthy();
  });

  test('a slow history request never holds up the board', async () => {
    const api = standin();
    vi.spyOn(api, 'listCpuHistory').mockReturnValue(new Promise(() => undefined));
    await signIn(api, '#/hosts');
    const columns = within(await screen.findByRole('list', { name: 'Hosts' })).getAllByRole('listitem');
    expect(columns).toHaveLength(4);
    expect(document.querySelectorAll('.spark-wait')).toHaveLength(4);
    expect(document.querySelector('.spark-wait')!.getAttribute('aria-hidden')).toBe('true');
  });

  test('a host missing from the batch is hatched as unavailable, and an all-null batch hatches every column', async () => {
    const api = standin();
    vi.spyOn(api, 'listCpuHistory').mockResolvedValue([history('atlas', '24h', flat('24h', null))]);
    await signIn(api, '#/hosts');
    const columns = within(await screen.findByRole('list', { name: 'Hosts' })).getAllByRole('listitem');
    await waitFor(() => expect(within(columns[0]).getByText('No CPU samples in the last 24 hours')).toBeTruthy());
    for (const column of columns.slice(1)) expect(within(column).getByText('CPU history unavailable')).toBeTruthy();
    expect(screen.queryByRole('img', { name: /CPU over/ })).toBeNull();
  });
});

describe('host detail CPU history', () => {
  test('defaults to 24 h with avg and peak over the sampled buckets, and switches between 1h, 6h and 24h', async () => {
    const api = await signIn(standin(), '#/hosts/atlas');
    const tile = await cpuTile();
    const range = within(tile).getByRole('group', { name: 'CPU history range' });
    expect(within(range).getAllByRole('button').map(b => b.textContent)).toEqual(['1h', '6h', '24h']);
    for (const [i, r] of (['24h', '1h', '6h', '24h'] as const).entries()) {
      if (i > 0) fireEvent.click(within(range).getByRole('button', { name: r }));
      expect(tile.querySelector('.cpu-hist')!.getAttribute('data-range')).toBe(r);
      const expected = await api.getCpuHistory('atlas', r);
      const trace = await within(tile).findByRole('img', { name: traceLabel(expected) });
      expect(within(range).getByRole('button', { name: r }).getAttribute('aria-pressed')).toBe('true');
      expect(within(range).getAllByRole('button', { pressed: true })).toHaveLength(1);
      const { avg, peak, missing } = summarizeCpuHistory(expected.samples);
      expect(readout(tile, 'avg').textContent).toBe(`${Math.round(avg!)}%`);
      expect(readout(tile, 'peak').textContent).toBe(`${Math.round(peak!)}%`);
      expect(trace.querySelectorAll('.trace-gap').length).toBe(traceGeometry(expected.samples).gaps.length);
      if (missing > 0) expect(within(tile).getByText(`${missing} of ${expected.samples.length} buckets missing`, { exact: false })).toBeTruthy();
    }
  });

  test('the 1h range shows the six missing minutes as one hatched gap', async () => {
    await signIn(standin(), '#/hosts/atlas');
    const tile = await cpuTile();
    fireEvent.click(within(tile).getByRole('button', { name: '1h' }));
    const trace = await within(tile).findByRole('img', { name: /last hour.*6 of 60 buckets missing/ });
    const gaps = trace.querySelectorAll<HTMLElement>('.trace-gap');
    expect(gaps).toHaveLength(1);
    expect(gaps[0].style.insetInlineStart).toBe('60%');
    expect(gaps[0].style.inlineSize).toBe('10%');
    expect(within(tile).getByText(/6 of 60 buckets missing/)).toBeTruthy();
  });

  test('a late response for an earlier range is dropped', async () => {
    const api = new DeferredHistory();
    await signIn(api, '#/hosts/atlas');
    const tile = await cpuTile();
    await waitFor(() => expect(api.last('atlas', '24h')).toBeTruthy());
    fireEvent.click(within(tile).getByRole('button', { name: '1h' }));
    await waitFor(() => expect(api.last('atlas', '1h')).toBeTruthy());
    await act(async () => api.last('atlas', '1h').resolve(history('atlas', '1h', flat('1h', 64))));
    expect(await within(tile).findByRole('img', { name: /last hour/ })).toBeTruthy();
    await act(async () => api.last('atlas', '24h').resolve(history('atlas', '24h', flat('24h', 11))));
    expect(within(tile).getByRole('img', { name: /last hour/ })).toBeTruthy();
    expect(within(tile).queryByRole('img', { name: /last 24 hours/ })).toBeNull();
    expect(readout(tile, 'avg').textContent).toBe('64%');
    expect(within(tile).getByRole('button', { name: '1h' }).getAttribute('aria-pressed')).toBe('true');
  });

  test('while a new range loads the previous range is not shown under the new label', async () => {
    const api = new DeferredHistory();
    await signIn(api, '#/hosts/atlas');
    const tile = await cpuTile();
    await waitFor(() => expect(api.last('atlas', '24h')).toBeTruthy());
    await act(async () => api.last('atlas', '24h').resolve(history('atlas', '24h', flat('24h', 30))));
    await within(tile).findByRole('img', { name: /last 24 hours/ });
    fireEvent.click(within(tile).getByRole('button', { name: '6h' }));
    expect(within(tile).queryByRole('img', { name: /CPU over/ })).toBeNull();
    expect(within(tile).getByText('Loading CPU history…')).toBeTruthy();
    expect(within(tile).queryByText('avg')).toBeNull();
  });

  test('a late response for the previous host is dropped after switching hosts', async () => {
    const api = new DeferredHistory();
    await signIn(api, '#/hosts/atlas');
    await cpuTile();
    await waitFor(() => expect(api.last('atlas', '24h')).toBeTruthy());
    go('#/hosts/harbor');
    expect(await screen.findByRole('heading', { level: 1, name: 'harbor' })).toBeTruthy();
    await waitFor(() => expect(api.last('harbor', '24h')).toBeTruthy());
    await act(async () => api.last('harbor', '24h').resolve(history('harbor', '24h', flat('24h', 77))));
    const tile = await cpuTile();
    await within(tile).findByRole('img', { name: /average 77%/ });
    await act(async () => api.last('atlas', '24h').resolve(history('atlas', '24h', flat('24h', 11))));
    expect(readout(tile, 'avg').textContent).toBe('77%');
    expect(within(tile).queryByText('11%')).toBeNull();
  });

  test('a response that names another host or range is refused rather than drawn', async () => {
    const api = new DeferredHistory();
    await signIn(api, '#/hosts/atlas');
    const tile = await cpuTile();
    await waitFor(() => expect(api.last('atlas', '24h')).toBeTruthy());
    await act(async () => api.last('atlas', '24h').resolve(history('harbor', '24h', flat('24h', 50))));
    expect(await within(tile).findByText('history unavailable')).toBeTruthy();
    expect(within(tile).getByText('CPU history did not match the requested host and range.')).toBeTruthy();
    expect(within(tile).queryByRole('img', { name: /CPU over/ })).toBeNull();
  });

  test('a failed history request leaves the host page intact and says history is unavailable', async () => {
    await signIn(standin({ failures: { getCpuHistory: new ApiError('unavailable', 'arxd is busy.', 503) } }), '#/hosts/atlas');
    const tile = await cpuTile();
    const na = await within(tile).findByText('history unavailable');
    expect(na.getAttribute('data-null')).toBe('true');
    expect(within(tile).getByText('arxd is busy.')).toBeTruthy();
    expect(within(tile).queryByText('avg')).toBeNull();
    // The current sample, the rest of the bento and the range selector stay.
    expect(screen.getByRole('meter', { name: 'CPU' }).getAttribute('aria-valuenow')).toBe('38');
    expect(screen.getByRole('meter', { name: 'Memory' })).toBeTruthy();
    expect(screen.getByText('qemu-system-x86_64')).toBeTruthy();
    expect(within(tile).getByRole('group', { name: 'CPU history range' })).toBeTruthy();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  test('all-null history hatches avg and peak as usage unavailable and draws no trace, never 0%', async () => {
    await signIn(standin({ cpuMinute: () => null }), '#/hosts/atlas');
    const tile = await cpuTile();
    await within(tile).findByText('no samples');
    for (const label of ['avg', 'peak'] as const) {
      const value = readout(tile, label);
      expect(value.textContent).toBe('usage unavailable');
      expect(value.querySelector('[data-null="true"]')).not.toBeNull();
    }
    expect(within(tile).queryByRole('img', { name: /CPU over/ })).toBeNull();
    expect(tile.querySelector('.trace-line')).toBeNull();
    expect(tile.querySelector('.cpu-hist')!.textContent).not.toMatch(/\d+%/);
    expect(within(tile).getByText(/no CPU samples for atlas in the last 24 hours/)).toBeTruthy();
  });

  test('an offline host without a buffer shows the no-samples state', async () => {
    await signIn(standin(), '#/hosts/pebble');
    const tile = await cpuTile();
    expect(await within(tile).findByText('no samples')).toBeTruthy();
    expect(within(tile).getAllByText('usage unavailable').length).toBeGreaterThanOrEqual(2);
    expect(tile.textContent).not.toMatch(/\b0%/);
  });

  test('the host page refreshes its range every 60 s, and every 30 s on 1h', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const api = standin();
    const calls = vi.spyOn(api, 'getCpuHistory');
    await signIn(api, '#/hosts/atlas');
    const tile = await cpuTile();
    await within(tile).findByRole('img', { name: /last 24 hours/ });
    const count = (r: CpuHistoryRange) => calls.mock.calls.filter(([, range]) => range === r).length;
    expect(count('24h')).toBe(1);
    await act(async () => vi.advanceTimersByTime(60_000));
    expect(count('24h')).toBe(2);
    fireEvent.click(within(tile).getByRole('button', { name: '1h' }));
    await within(tile).findByRole('img', { name: /last hour/ });
    expect(count('1h')).toBe(1);
    await act(async () => vi.advanceTimersByTime(30_000));
    expect(count('1h')).toBe(2);
    expect(count('24h')).toBe(2);
  });
});
