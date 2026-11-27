import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import { summarizeCpuHistory } from '../api/cpuHistory';
import { ApiError, isApiError, type ArxApi, type CpuHistory, type CpuHistoryRange, type OsFamily } from '../api/types';
import { useApi, useSession } from './context';

// CPU history traces (docs/design/night-orbit/CPU-HISTORY-API.md, ADR 0073).
// A null bucket breaks the line and is drawn as a hatched gap; values are never
// interpolated across a gap and a null is never drawn as 0.

export const RANGE_WORDS: Record<CpuHistoryRange, string> = { '1h': 'last hour', '6h': 'last 6 hours', '24h': 'last 24 hours' };
const BUCKET_WORDS: Record<CpuHistory['bucketSeconds'], string> = { 60: '1-minute', 300: '5-minute', 600: '10-minute' };

/**
 * A range of buckets. In trace geometry `end` is exclusive; a selected window
 * (`BucketWindow`) holds whole buckets with `end` inclusive.
 */
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
export function CpuTrace({ history, os, className, grid, window: win }: { history: CpuHistory; os: OsFamily; className: string; grid?: boolean; window?: Span }) {
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
      {win ? (
        <>
          {/* Buckets outside the selected window are dimmed; the window itself stays at full strength. */}
          {win.start > 0 ? <span className="trace-dim trace-dim-l" aria-hidden="true" style={{ insetInlineStart: 0, inlineSize: `${(win.start / n) * 100}%` }} /> : null}
          {win.end < n - 1 ? <span className="trace-dim trace-dim-r" aria-hidden="true" style={{ insetInlineStart: `${((win.end + 1) / n) * 100}%`, insetInlineEnd: 0 }} /> : null}
        </>
      ) : null}
    </div>
  );
}

/** How long ago a moment `seconds` before the end of the range was: "now", "45 min ago", "4 h 10 min ago". */
export function agoText(seconds: number): string {
  if (seconds <= 0) return 'now';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m === 0 ? `${h} h ago` : `${h} h ${m} min ago`;
}

/** The start handle sits at its bucket's start, the end handle at its bucket's end. */
const startAgo = (span: Span, n: number, bucketSeconds: number) => (n - span.start) * bucketSeconds;
const endAgo = (span: Span, n: number, bucketSeconds: number) => (n - 1 - span.end) * bucketSeconds;

/** The window as words: "Window: 60 min ago → now · 60 buckets". */
export function windowText(span: Span, n: number, bucketSeconds: number): string {
  const count = span.end - span.start + 1;
  return `Window: ${agoText(startAgo(span, n, bucketSeconds))} → ${agoText(endAgo(span, n, bucketSeconds))} · ${count} ${count === 1 ? 'bucket' : 'buckets'}`;
}

/**
 * Moves one handle of a whole-bucket window for a key press, clamped so that
 * 0 <= start <= end <= n - 1. Returns null for a key that does not move it.
 */
export function moveHandle(span: Span, handle: 'start' | 'end', key: string, n: number): Span | null {
  const page = Math.max(1, Math.round(n / 12));
  const steps: Record<string, number> = { ArrowLeft: -1, ArrowDown: -1, ArrowRight: 1, ArrowUp: 1, PageDown: -page, PageUp: page };
  const [min, max] = handle === 'start' ? [0, span.end] : [span.start, n - 1];
  const now = span[handle];
  let next: number;
  if (key in steps) next = now + steps[key];
  else if (key === 'Home') next = min;
  else if (key === 'End') next = max;
  else return null;
  next = Math.min(max, Math.max(min, next));
  return { ...span, [handle]: next };
}

/** Which bucket edge a dragged handle edge lands on: the nearest boundary, clamped for that handle. */
export function dragTarget(span: Span, handle: 'start' | 'end', edgeFraction: number, n: number): Span {
  const boundary = Math.round(edgeFraction * n);
  return handle === 'start'
    ? { ...span, start: Math.min(span.end, Math.max(0, boundary)) }
    : { ...span, end: Math.max(span.start, Math.min(n - 1, boundary - 1)) };
}

/**
 * Two handles that select whole buckets of the trace above them (ARIA
 * sliders: arrows move one bucket, Page Up/Down a twelfth of the range, Home
 * and End go to the limit). Dragging works too. The start handle never passes
 * the end handle.
 *
 * The handles are bookends: the start handle sits wholly left of the window's
 * left edge and the end handle wholly right of its right edge, so even a
 * one-bucket window on 24 h (under 5 px wide) keeps two separate pointer
 * targets and a focus outline that never lands on the other handle. A drag
 * moves the window edge by the pointer's travel, wherever the handle was grabbed.
 */
export function BucketWindow({ n, bucketSeconds, value, onChange }: { n: number; bucketSeconds: number; value: Span; onChange(span: Span): void }) {
  const track = useRef<HTMLDivElement>(null);
  /** Pointer x minus the dragged window edge's x at grab time. */
  const grab = useRef<{ handle: 'start' | 'end'; offset: number } | null>(null);
  const edgeX = (handle: 'start' | 'end', box: DOMRect) => box.left + ((handle === 'start' ? value.start : value.end + 1) / n) * box.width;
  const onKey = (handle: 'start' | 'end') => (event: KeyboardEvent) => {
    const next = moveHandle(value, handle, event.key, n);
    if (!next) return;
    event.preventDefault();
    if (next.start !== value.start || next.end !== value.end) onChange(next);
  };
  const onDrag = (handle: 'start' | 'end') => (event: PointerEvent<HTMLDivElement>) => {
    if (event.buttons !== 1 || !track.current || grab.current?.handle !== handle) return;
    const box = track.current.getBoundingClientRect();
    if (box.width <= 0) return;
    const next = dragTarget(value, handle, (event.clientX - grab.current.offset - box.left) / box.width, n);
    if (next.start !== value.start || next.end !== value.end) onChange(next);
  };
  const handleProps = (handle: 'start' | 'end') => {
    const [min, max] = handle === 'start' ? [0, value.end] : [value.start, n - 1];
    const ago = handle === 'start' ? startAgo(value, n, bucketSeconds) : endAgo(value, n, bucketSeconds);
    const left = handle === 'start' ? value.start / n : (value.end + 1) / n;
    return {
      className: 'brush-h',
      role: 'slider',
      tabIndex: 0,
      'aria-label': handle === 'start' ? 'Window start' : 'Window end',
      'aria-orientation': 'horizontal' as const,
      'aria-valuemin': min,
      'aria-valuemax': max,
      'aria-valuenow': value[handle],
      'aria-valuetext': agoText(ago),
      'data-handle': handle,
      style: { insetInlineStart: `${left * 100}%` },
      onKeyDown: onKey(handle),
      onPointerDown: (event: PointerEvent<HTMLDivElement>) => {
        const box = track.current?.getBoundingClientRect();
        grab.current = box ? { handle, offset: event.clientX - edgeX(handle, box) } : null;
        event.currentTarget.setPointerCapture?.(event.pointerId);
        event.currentTarget.focus();
      },
      onPointerMove: onDrag(handle),
      onPointerUp: () => {
        grab.current = null;
      },
      onPointerCancel: () => {
        grab.current = null;
      },
    };
  };
  return (
    <div className="brush" ref={track}>
      <span className="brush-band" aria-hidden="true" style={{ insetInlineStart: `${(value.start / n) * 100}%`, inlineSize: `${((value.end - value.start + 1) / n) * 100}%` }} />
      <div {...handleProps('start')} />
      <div {...handleProps('end')} />
    </div>
  );
}

export type Polled<T> =
  | { status: 'loading' }
  | { status: 'ready'; data: T }
  | { status: 'error'; error: ApiError };

type Held<T> = ({ status: 'ready'; data: T } | { status: 'error'; error: ApiError }) & { key: string; generation: number };

/** How long one history request may take before the view shows history as unavailable; well under the 30 s poll. */
export const HISTORY_TIMEOUT_MS = 12_000;
export const HISTORY_TIMEOUT_MESSAGE = 'CPU history did not arrive in time.';

/**
 * Loads `load` now and every `refreshMs`, for history that must never block
 * the view around it.
 *
 * - At most one request is in flight: each poll aborts the one before it, and
 *   each request has a deadline (`timeoutMs`), after which it is aborted and
 *   the view shows history as unavailable until a later poll succeeds.
 * - A change of `key` (host and range) starts a new generation: what was
 *   shown is cleared at once, the in-flight request is aborted, and only a
 *   response from the current generation's current request is accepted, so
 *   returning to an earlier range never shows its old data.
 * - Unmounting aborts the in-flight request. An `unauthorized` failure ends
 *   the session, like every other request.
 */
export function usePolled<T>(
  key: string,
  load: (api: ArxApi, signal: AbortSignal) => Promise<T>,
  refreshMs: number | null,
  timeoutMs = HISTORY_TIMEOUT_MS,
): Polled<T> {
  const api = useApi();
  const { expire } = useSession();
  const [state, setState] = useState<Held<T> | null>(null);
  const [current, setCurrent] = useState({ key, generation: 0 });
  const [loader] = useState(() => ({ current: load }));
  loader.current = load;
  let generation = current.generation;
  if (current.key !== key) {
    // A new host or range: a new generation, so nothing held from before shows.
    generation += 1;
    setCurrent({ key, generation });
  }

  useEffect(() => {
    let live = true;
    let active: { controller: AbortController; timer: ReturnType<typeof setTimeout> } | null = null;
    const accept = (controller: AbortController, next: Held<T>) => {
      if (!live || active?.controller !== controller) return;
      clearTimeout(active.timer);
      active = null;
      if (next.status === 'error' && next.error.code === 'unauthorized') expire();
      setState(next);
    };
    const run = () => {
      if (active) {
        clearTimeout(active.timer);
        active.controller.abort(new ApiError('network', 'Replaced by a newer CPU history request.'));
      }
      const controller = new AbortController();
      const timer = setTimeout(() => {
        const error = new ApiError('unavailable', HISTORY_TIMEOUT_MESSAGE);
        controller.abort(error);
        // Settle here too, in case the request ignores its signal.
        accept(controller, { key, generation, status: 'error', error });
      }, timeoutMs);
      active = { controller, timer };
      loader.current(api, controller.signal).then(
        data => accept(controller, { key, generation, status: 'ready', data }),
        (error: unknown) => accept(controller, {
          key, generation, status: 'error',
          error: isApiError(error) ? error : new ApiError('bad_response', 'Something went wrong while loading.'),
        }),
      );
    };
    run();
    const interval = refreshMs === null ? null : setInterval(run, refreshMs);
    return () => {
      live = false;
      if (interval !== null) clearInterval(interval);
      if (active) {
        clearTimeout(active.timer);
        active.controller.abort(new ApiError('network', 'The CPU history request was cancelled.'));
        active = null;
      }
    };
  }, [api, expire, key, generation, refreshMs, timeoutMs, loader]);

  if (state === null || state.key !== key || state.generation !== generation) return { status: 'loading' };
  return state.status === 'ready' ? { status: 'ready', data: state.data } : { status: 'error', error: state.error };
}

/** Polling cadence from CPU-HISTORY-API.md: the board and the host page every 60 s, the 1h range every 30 s. */
export const refreshFor = (range: CpuHistoryRange) => (range === '1h' ? 30_000 : 60_000);

/** True when the history has at least one sampled bucket. */
export const hasSamples = (history: CpuHistory) => history.samples.some(s => s !== null);
