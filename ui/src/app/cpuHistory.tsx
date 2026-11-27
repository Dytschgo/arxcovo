import { useEffect, useState } from 'react';
import { summarizeCpuHistory } from '../api/cpuHistory';
import { ApiError, isApiError, type ArxApi, type CpuHistory, type CpuHistoryRange, type OsFamily } from '../api/types';
import { useApi, useSession } from './context';

// CPU history traces (docs/design/night-orbit/CPU-HISTORY-API.md, ADR 0073).
// A null bucket breaks the line and is drawn as a hatched gap; values are never
// interpolated across a gap and a null is never drawn as 0.

export const RANGE_WORDS: Record<CpuHistoryRange, string> = { '1h': 'last hour', '6h': 'last 6 hours', '24h': 'last 24 hours' };
const BUCKET_WORDS: Record<CpuHistory['bucketSeconds'], string> = { 60: '1-minute', 300: '5-minute', 600: '10-minute' };

/** A drawn run of buckets: `start` and `end` in bucket units, end exclusive. */
export interface Span { start: number; end: number }

export interface TraceGeometry {
  /** SVG path in a `0 0 n 100` box (x in buckets, y = 100 − percent), one subpath per run of samples. */
  line: string;
  /** The same runs closed down to the baseline, for the soft fill. */
  area: string;
  /** Runs of null buckets, drawn hatched. */
  gaps: Span[];
  runs: Span[];
}

const fmt = (n: number) => String(Math.round(n * 100) / 100);

/**
 * Lays out a series: each run of non-null buckets becomes its own subpath
 * through the bucket centres, held flat to the run's outer bucket edges so a
 * lone sample still shows and every gap lines up exactly with its hatch.
 */
export function traceGeometry(samples: readonly (number | null)[]): TraceGeometry {
  const runs: Span[] = [];
  const gaps: Span[] = [];
  let i = 0;
  while (i < samples.length) {
    const isNull = samples[i] === null;
    let j = i;
    while (j < samples.length && (samples[j] === null) === isNull) j += 1;
    (isNull ? gaps : runs).push({ start: i, end: j });
    i = j;
  }
  const y = (k: number) => fmt(100 - Math.min(100, Math.max(0, samples[k] as number)));
  let line = '';
  let area = '';
  for (const { start, end } of runs) {
    const pts = [`${start} ${y(start)}`];
    for (let k = start; k < end; k += 1) pts.push(`${fmt(k + 0.5)} ${y(k)}`);
    pts.push(`${end} ${y(end - 1)}`);
    line += `M${pts.join(' L')}`;
    area += `M${start} 100 L${pts.join(' L')} L${end} 100 Z`;
  }
  return { line, area, gaps, runs };
}

/** The accessible description of a trace: what it covers and its avg, peak and missing buckets. */
export function traceLabel(history: CpuHistory): string {
  const { avg, peak, missing } = summarizeCpuHistory(history.samples);
  const head = `CPU over the ${RANGE_WORDS[history.range]}, ${BUCKET_WORDS[history.bucketSeconds]} buckets`;
  if (avg === null || peak === null) return `${head}: no samples`;
  const gaps = missing > 0 ? `, ${missing} of ${history.samples.length} buckets missing` : '';
  return `${head}: average ${Math.round(avg)}%, peak ${Math.round(peak)}%${gaps}`;
}

/**
 * A CPU trace: an OS-coloured line (data colour only) with hatched gaps for
 * null buckets. The caller shows the all-null state; this draws what exists.
 */
export function CpuTrace({ history, os, className, grid }: { history: CpuHistory; os: OsFamily; className: string; grid?: boolean }) {
  const n = history.samples.length;
  const { line, area, gaps } = traceGeometry(history.samples);
  return (
    <div className={`trace ${className}`} data-os={os} role="img" aria-label={traceLabel(history)}>
      <svg viewBox={`0 0 ${n} 100`} preserveAspectRatio="none" aria-hidden="true" focusable="false">
        {grid ? [25, 50, 75].map(v => <line key={v} className="trace-grid" x1={0} x2={n} y1={v} y2={v} />) : null}
        <path className="trace-area" d={area} />
        <path className="trace-line" d={line} />
      </svg>
      {gaps.map(g => (
        <span key={g.start} className="trace-gap" data-null="true" aria-hidden="true"
          style={{ insetInlineStart: `${(g.start / n) * 100}%`, inlineSize: `${((g.end - g.start) / n) * 100}%` }} />
      ))}
    </div>
  );
}

export type Polled<T> =
  | { status: 'loading' }
  | { status: 'ready'; data: T }
  | { status: 'error'; error: ApiError };

type Keyed<T> = ({ status: 'ready'; data: T } | { status: 'error'; error: ApiError }) & { key: string };

/**
 * Loads `load` now and every `refreshMs`, for history that must never block
 * the view around it. Results are tied to `key` (host and range): a response
 * for an earlier key, or one overtaken by a newer poll, is dropped, so a fast
 * switch never shows another host's or range's trace. An `unauthorized`
 * failure ends the session, like every other request.
 */
export function usePolled<T>(key: string, load: (api: ArxApi) => Promise<T>, refreshMs: number | null): Polled<T> {
  const api = useApi();
  const { expire } = useSession();
  const [state, setState] = useState<Keyed<T> | null>(null);
  const [loader] = useState(() => ({ current: load }));
  loader.current = load;

  useEffect(() => {
    let live = true;
    let issued = 0;
    let applied = 0;
    const run = () => {
      const seq = ++issued;
      loader.current(api).then(
        data => {
          if (!live || seq < applied) return;
          applied = seq;
          setState({ key, status: 'ready', data });
        },
        (error: unknown) => {
          if (!live || seq < applied) return;
          applied = seq;
          const e = isApiError(error) ? error : new ApiError('bad_response', 'Something went wrong while loading.');
          if (e.code === 'unauthorized') expire();
          setState({ key, status: 'error', error: e });
        },
      );
    };
    run();
    const timer = refreshMs === null ? null : setInterval(run, refreshMs);
    return () => {
      live = false;
      if (timer !== null) clearInterval(timer);
    };
  }, [api, expire, key, refreshMs, loader]);

  if (state === null || state.key !== key) return { status: 'loading' };
  return state.status === 'ready' ? { status: 'ready', data: state.data } : { status: 'error', error: state.error };
}

/** Polling cadence from CPU-HISTORY-API.md: the board and the host page every 60 s, the 1h range every 30 s. */
export const refreshFor = (range: CpuHistoryRange) => (range === '1h' ? 30_000 : 60_000);

/** True when the history has at least one sampled bucket. */
export const hasSamples = (history: CpuHistory) => history.samples.some(s => s !== null);
