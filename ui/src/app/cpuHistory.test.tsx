import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, test, vi } from 'vitest';
import { App } from '../App';
import { summarizeCpuHistory } from '../api/cpuHistory';
import { DEMO_CREDENTIALS, FIXTURE_HOSTS } from '../api/fixtures';
import { StandinApi, standinCpuHistory, type StandinOptions } from '../api/standin';
import { ApiError, CPU_HISTORY_RANGES, type CpuHistory, type CpuHistoryRange } from '../api/types';
import { HISTORY_TIMEOUT_MESSAGE, HISTORY_TIMEOUT_MS, agoText, moveHandle, traceGeometry, traceLabel, usePolled, windowText } from './cpuHistory';
import { AppProviders } from './context';

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

/**
 * A stand-in whose CPU history calls wait until the test settles them. It
 * records each call's signal but ignores it, like a server that never answers,
 * so the view's own guards are what the tests exercise.
 */
class DeferredHistory extends StandinApi {
  calls: { hostId: string; range: CpuHistoryRange; signal?: AbortSignal; resolve(h: CpuHistory): void; reject(e: unknown): void }[] = [];
  getCpuHistory(hostId: string, range: CpuHistoryRange, options: { signal?: AbortSignal } = {}): Promise<CpuHistory> {
    return new Promise((resolve, reject) => this.calls.push({ hostId, range, signal: options.signal, resolve, reject }));
  }
  /** Calls whose signal has not been aborted. */
  open() {
    return this.calls.filter(c => !c.signal?.aborted);
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
    expect(batch).toHaveBeenCalledWith('24h', { signal: expect.any(AbortSignal) });
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

  for (const [what, wrong] of [
    ['another host', () => history('harbor', '24h', flat('24h', 50))],
    ['another range', () => history('atlas', '6h', flat('6h', 50))],
  ] as const) {
    test(`a response that names ${what} is refused rather than drawn`, async () => {
      const api = new DeferredHistory();
      await signIn(api, '#/hosts/atlas');
      const tile = await cpuTile();
      await waitFor(() => expect(api.last('atlas', '24h')).toBeTruthy());
      await act(async () => api.last('atlas', '24h').resolve(wrong()));
      expect(await within(tile).findByText('history unavailable')).toBeTruthy();
      expect(within(tile).getByText('CPU history did not match the requested host and range.')).toBeTruthy();
      expect(within(tile).queryByRole('img', { name: /CPU over/ })).toBeNull();
      expect(within(tile).queryByText('50%')).toBeNull();
    });
  }

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

describe('history request lifetime', () => {
  test('returning 24h -> 1h -> 24h never shows the old 24h series before the new 24h response', async () => {
    const api = new DeferredHistory();
    await signIn(api, '#/hosts/atlas');
    const tile = await cpuTile();
    await waitFor(() => expect(api.last('atlas', '24h')).toBeTruthy());
    await act(async () => api.last('atlas', '24h').resolve(history('atlas', '24h', flat('24h', 31))));
    await within(tile).findByRole('img', { name: /last 24 hours.*average 31%/ });
    const first24h = api.last('atlas', '24h');
    fireEvent.click(within(tile).getByRole('button', { name: '1h' }));
    await waitFor(() => expect(api.last('atlas', '1h')).toBeTruthy());
    fireEvent.click(within(tile).getByRole('button', { name: '24h' }));
    await waitFor(() => expect(api.last('atlas', '24h')).not.toBe(first24h));
    // The old 31% chart and readouts stay gone while the fresh 24h request is open.
    expect(within(tile).queryByRole('img', { name: /CPU over/ })).toBeNull();
    expect(within(tile).queryByText('31%')).toBeNull();
    expect(within(tile).getByText('Loading CPU history…')).toBeTruthy();
    // A late answer to the abandoned 1h request is dropped too.
    await act(async () => api.last('atlas', '1h').resolve(history('atlas', '1h', flat('1h', 64))));
    expect(within(tile).queryByText('64%')).toBeNull();
    expect(api.last('atlas', '1h').signal!.aborted).toBe(true);
    await act(async () => api.last('atlas', '24h').resolve(history('atlas', '24h', flat('24h', 45))));
    await within(tile).findByRole('img', { name: /last 24 hours.*average 45%/ });
    expect(readout(tile, 'avg').textContent).toBe('45%');
  });

  test('a stalled request times out to unavailable, polls never pile up, and the next healthy poll recovers', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const api = new DeferredHistory();
    await signIn(api, '#/hosts/atlas');
    const tile = await cpuTile();
    await waitFor(() => expect(api.calls).toHaveLength(1));
    const stalled = api.calls[0];
    expect(stalled.signal!.aborted).toBe(false);
    await act(async () => vi.advanceTimersByTime(HISTORY_TIMEOUT_MS));
    expect(stalled.signal!.aborted).toBe(true);
    expect(stalled.signal!.reason).toMatchObject({ code: 'unavailable', message: HISTORY_TIMEOUT_MESSAGE });
    expect(await within(tile).findByText('history unavailable')).toBeTruthy();
    expect(within(tile).getByText(HISTORY_TIMEOUT_MESSAGE)).toBeTruthy();
    // Three more minutes of stalled polls: never more than one request open at a time.
    for (let i = 0; i < 6; i += 1) {
      await act(async () => vi.advanceTimersByTime(30_000));
      expect(api.open().length).toBeLessThanOrEqual(1);
    }
    expect(api.calls.length).toBeGreaterThan(1);
    // Up to the next poll (at 240 s), which is still open.
    await act(async () => vi.advanceTimersByTime(48_000));
    // A late answer to a timed-out request is ignored.
    await act(async () => stalled.resolve(history('atlas', '24h', flat('24h', 12))));
    expect(within(tile).queryByText('12%')).toBeNull();
    // The open poll answers: the trace comes back.
    const open = api.open();
    expect(open).toHaveLength(1);
    await act(async () => open[0].resolve(history('atlas', '24h', flat('24h', 52))));
    await within(tile).findByRole('img', { name: /average 52%/ });
    expect(within(tile).queryByText('history unavailable')).toBeNull();
  });

  test('a poll that fires while the previous request is open aborts the previous one first', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const signals: AbortSignal[] = [];
    function Probe() {
      // A deadline longer than the poll interval, so a poll can overtake an open request.
      const state = usePolled('probe', (_api, signal) => {
        signals.push(signal);
        return new Promise<number>(() => undefined);
      }, 1_000, 5_000);
      return <p>probe {state.status}</p>;
    }
    const api = standin();
    render(<AppProviders api={api}><Probe /></AppProviders>);
    expect(await screen.findByText('probe loading')).toBeTruthy();
    expect(signals).toHaveLength(1);
    await act(async () => vi.advanceTimersByTime(1_000));
    expect(signals).toHaveLength(2);
    expect(signals[0].aborted).toBe(true);
    expect(signals[0].reason).toMatchObject({ code: 'network' });
    expect(signals[1].aborted).toBe(false);
    await act(async () => vi.advanceTimersByTime(3_000));
    expect(signals).toHaveLength(5);
    expect(signals.filter(s => !s.aborted)).toHaveLength(1);
    // Superseded requests never surface as an error.
    expect(screen.getByText('probe loading')).toBeTruthy();
  });

  test('changing range aborts the open request, and leaving the page aborts the current one', async () => {
    const api = new DeferredHistory();
    await signIn(api, '#/hosts/atlas');
    const tile = await cpuTile();
    await waitFor(() => expect(api.last('atlas', '24h')).toBeTruthy());
    const day = api.last('atlas', '24h');
    fireEvent.click(within(tile).getByRole('button', { name: '6h' }));
    await waitFor(() => expect(api.last('atlas', '6h')).toBeTruthy());
    expect(day.signal!.aborted).toBe(true);
    const six = api.last('atlas', '6h');
    expect(six.signal!.aborted).toBe(false);
    go('#/vms');
    await screen.findByRole('heading', { level: 1, name: 'Virtual machines' });
    expect(six.signal!.aborted).toBe(true);
    expect(api.open()).toHaveLength(0);
  });

  test('the board aborts its open batch request when the view goes away', async () => {
    const api = standin();
    const signals: AbortSignal[] = [];
    vi.spyOn(api, 'listCpuHistory').mockImplementation((_range, options) => {
      signals.push(options!.signal!);
      return new Promise(() => undefined);
    });
    await signIn(api, '#/hosts');
    await screen.findByRole('list', { name: 'Hosts' });
    expect(signals).toHaveLength(1);
    expect(signals[0].aborted).toBe(false);
    go('#/activity');
    await screen.findByRole('heading', { level: 1, name: 'Activity' });
    expect(signals[0].aborted).toBe(true);
  });

  test('the stand-in rejects with the abort reason and never answers an aborted request', async () => {
    const api = standin({ latencyMs: 50 });
    await api.login(DEMO_CREDENTIALS);
    const controller = new AbortController();
    const pending = api.getCpuHistory('atlas', '1h', { signal: controller.signal });
    const reason = new ApiError('unavailable', 'gone');
    controller.abort(reason);
    await expect(pending).rejects.toBe(reason);
    await expect(api.listCpuHistory('24h', { signal: controller.signal })).rejects.toBe(reason);
  });
});

describe('bucket window', () => {
  const span = (start: number, end: number) => ({ start, end });

  test('handles move by whole buckets and are clamped so start <= end within the range', () => {
    expect(moveHandle(span(0, 59), 'start', 'ArrowRight', 60)).toEqual(span(1, 59));
    expect(moveHandle(span(0, 59), 'start', 'ArrowLeft', 60)).toEqual(span(0, 59));
    expect(moveHandle(span(10, 20), 'start', 'End', 60)).toEqual(span(20, 20));
    expect(moveHandle(span(10, 20), 'start', 'Home', 60)).toEqual(span(0, 20));
    expect(moveHandle(span(10, 20), 'end', 'Home', 60)).toEqual(span(10, 10));
    expect(moveHandle(span(10, 20), 'end', 'End', 60)).toEqual(span(10, 59));
    expect(moveHandle(span(10, 20), 'end', 'ArrowDown', 60)).toEqual(span(10, 19));
    expect(moveHandle(span(10, 20), 'end', 'PageUp', 60)).toEqual(span(10, 25));
    expect(moveHandle(span(20, 20), 'end', 'ArrowLeft', 60)).toEqual(span(20, 20));
    expect(moveHandle(span(0, 143), 'end', 'PageUp', 144)).toEqual(span(0, 143));
    expect(moveHandle(span(0, 59), 'start', 'a', 60)).toBeNull();
  });

  test('the window reads as times before the end of the range', () => {
    expect(agoText(0)).toBe('now');
    expect(agoText(45 * 60)).toBe('45 min ago');
    expect(agoText(4 * 3600)).toBe('4 h ago');
    expect(agoText(250 * 60)).toBe('4 h 10 min ago');
    expect(windowText(span(0, 59), 60, 60)).toBe('Window: 1 h ago → now · 60 buckets');
    expect(windowText(span(36, 36), 60, 60)).toBe('Window: 24 min ago → 23 min ago · 1 bucket');
    expect(windowText(span(0, 143), 144, 600)).toBe('Window: 24 h ago → now · 144 buckets');
  });

  const slider = (tile: HTMLElement, name: 'Window start' | 'Window end') => within(tile).getByRole('slider', { name });

  test('the keyboard moves both handles; avg, peak and missing follow the selected buckets', async () => {
    // 1h series: 10 for the first 30 buckets, 50 for the next 20, 90 for the last 10, with buckets 40-44 missing.
    const samples: (number | null)[] = Array.from({ length: 60 }, (_, i) => (i >= 40 && i < 45 ? null : i < 30 ? 10 : i < 50 ? 50 : 90));
    const api = new DeferredHistory();
    await signIn(api, '#/hosts/atlas');
    const tile = await cpuTile();
    fireEvent.click(within(tile).getByRole('button', { name: '1h' }));
    await waitFor(() => expect(api.last('atlas', '1h')).toBeTruthy());
    await act(async () => api.last('atlas', '1h').resolve(history('atlas', '1h', samples)));
    await within(tile).findByRole('img', { name: /last hour/ });
    const start = slider(tile, 'Window start');
    const end = slider(tile, 'Window end');
    expect([start.getAttribute('aria-valuenow'), end.getAttribute('aria-valuenow')]).toEqual(['0', '59']);
    expect(start.getAttribute('aria-valuetext')).toBe('1 h ago');
    expect(end.getAttribute('aria-valuetext')).toBe('now');
    expect(start.tabIndex).toBe(0);
    expect(within(tile).getByText(/^Window: 1 h ago → now · 60 buckets$/)).toBeTruthy();
    const full = summarizeCpuHistory(samples);
    expect(readout(tile, 'avg').textContent).toBe(`${Math.round(full.avg!)}%`);
    expect(within(tile).getByText(/5 of 60 buckets missing ·/)).toBeTruthy();
    expect(tile.querySelector('.trace-dim')).toBeNull();

    // End handle down to bucket 29: only the 10% buckets.
    for (let i = 0; i < 30; i += 1) fireEvent.keyDown(end, { key: 'ArrowLeft' });
    expect(end.getAttribute('aria-valuenow')).toBe('29');
    expect(readout(tile, 'avg').textContent).toBe('10%');
    expect(readout(tile, 'peak').textContent).toBe('10%');
    expect(within(tile).queryByText(/buckets missing/)).toBeNull();
    expect(start.getAttribute('aria-valuemax')).toBe('29');
    expect(tile.querySelectorAll('.trace-dim')).toHaveLength(1);

    // Window 30-49 via End on the end handle, then the start handle.
    fireEvent.keyDown(end, { key: 'End' });
    for (let i = 0; i < 30; i += 1) fireEvent.keyDown(start, { key: 'ArrowRight' });
    for (let i = 0; i < 10; i += 1) fireEvent.keyDown(end, { key: 'ArrowLeft' });
    expect([start.getAttribute('aria-valuenow'), end.getAttribute('aria-valuenow')]).toEqual(['30', '49']);
    expect(readout(tile, 'avg').textContent).toBe('50%');
    expect(readout(tile, 'peak').textContent).toBe('50%');
    expect(within(tile).getByText(/5 of 20 buckets missing in the window/)).toBeTruthy();
    expect(within(tile).getByText('Window: 30 min ago → 10 min ago · 20 buckets')).toBeTruthy();
    expect(tile.querySelectorAll('.trace-dim')).toHaveLength(2);

    // Start cannot pass end: End on the start handle stops at the end handle.
    fireEvent.keyDown(start, { key: 'End' });
    expect([start.getAttribute('aria-valuenow'), end.getAttribute('aria-valuenow')]).toEqual(['49', '49']);
    expect(readout(tile, 'peak').textContent).toBe('50%');
  });

  test('a window of only missing buckets hatches avg and peak as usage unavailable, never 0%', async () => {
    const samples: (number | null)[] = Array.from({ length: 144 }, (_, i) => (i >= 140 ? null : 30));
    const api = new DeferredHistory();
    await signIn(api, '#/hosts/atlas');
    const tile = await cpuTile();
    await waitFor(() => expect(api.last('atlas', '24h')).toBeTruthy());
    await act(async () => api.last('atlas', '24h').resolve(history('atlas', '24h', samples)));
    await within(tile).findByRole('img', { name: /last 24 hours/ });
    fireEvent.keyDown(slider(tile, 'Window start'), { key: 'End' });
    for (let i = 0; i < 3; i += 1) fireEvent.keyDown(slider(tile, 'Window start'), { key: 'ArrowLeft' });
    expect(slider(tile, 'Window start').getAttribute('aria-valuenow')).toBe('140');
    for (const label of ['avg', 'peak'] as const) {
      expect(readout(tile, label).textContent).toBe('usage unavailable');
      expect(readout(tile, label).querySelector('[data-null="true"]')).not.toBeNull();
    }
    expect(within(tile).getByText(/4 of 4 buckets missing in the window/)).toBeTruthy();
    expect(tile.querySelector('.cpu-hist')!.textContent).not.toMatch(/\b0%/);
  });

  test('the window resets to the whole range when the range changes', async () => {
    await signIn(standin(), '#/hosts/atlas');
    const tile = await cpuTile();
    await within(tile).findByRole('img', { name: /last 24 hours/ });
    fireEvent.keyDown(slider(tile, 'Window start'), { key: 'PageUp' });
    fireEvent.keyDown(slider(tile, 'Window end'), { key: 'PageDown' });
    expect(slider(tile, 'Window start').getAttribute('aria-valuenow')).toBe('12');
    expect(slider(tile, 'Window end').getAttribute('aria-valuenow')).toBe('131');
    fireEvent.click(within(tile).getByRole('button', { name: '6h' }));
    await within(tile).findByRole('img', { name: /last 6 hours/ });
    expect(slider(tile, 'Window start').getAttribute('aria-valuenow')).toBe('0');
    expect(slider(tile, 'Window end').getAttribute('aria-valuenow')).toBe('71');
    fireEvent.click(within(tile).getByRole('button', { name: '24h' }));
    await within(tile).findByRole('img', { name: /last 24 hours/ });
    expect(slider(tile, 'Window start').getAttribute('aria-valuenow')).toBe('0');
    expect(slider(tile, 'Window end').getAttribute('aria-valuenow')).toBe('143');
  });
});
