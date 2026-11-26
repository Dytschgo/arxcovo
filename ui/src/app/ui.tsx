import { Component, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import type { ActivityOutcome, Arch, ConsoleStatus, OsFamily, Reachability, TerminalStatus, Vm, VmKind, VmState } from '../api/types';
import type { Resource } from './context';
import { OS_LABEL, formatBytes, formatTimeLeft } from './format';
import { Icon, OsGlyph, type IconName } from './icons';
import { href } from './router';

// Shared Night Orbit components. The design language, tokens and rules live in
// docs/design/night-orbit/ (SPEC.md, night-orbit.css): white is the only UI
// accent, status is always an icon plus a word, OS colour is for data only,
// and a null value is drawn hatched with its reason, never as 0.

/** Architecture badge (mono, neutral). The title and hidden prefix name it for every reader. */
export function ArchBadge({ arch }: { arch: Arch }) {
  return (
    <span className="arch" data-arch={arch} title={`Machine architecture: ${arch}`}>
      <span className="visually-hidden">Architecture </span>
      {arch}
    </span>
  );
}

/** OS glyph on a small tile (host columns, host lists). The OS word is read out. */
export function OsBadge({ os }: { os: OsFamily }) {
  return (
    <span className="osb" data-os={os} title={OS_LABEL[os]}>
      <OsGlyph os={os} />
      <span className="visually-hidden">{OS_LABEL[os]} </span>
    </span>
  );
}

export type Tone = 'ok' | 'warn' | 'bad' | 'info' | 'muted';

/** A small neutral tag for secondary states (terminal and console status, guest readiness). */
export function Pill({ tone, children }: { tone: Tone; children: ReactNode }) {
  return <span className="pill" data-tone={tone}>{children}</span>;
}

/**
 * How a status reads: `plain` for normal states, `busy` for transitions (the
 * icon turns slowly when motion is allowed), `bad` for states that need the
 * owner's eye (white, heavier), `off` for inactive states (dimmer ink).
 */
export type StatusTone = 'plain' | 'busy' | 'bad' | 'off';
export interface StatusLook { icon: IconName; word: string; tone: StatusTone }

/** Every `VmState`, as icon + word. Exhaustive by type: a new state fails the build until it is drawn. */
export const VM_STATUS: Record<VmState, StatusLook> = {
  creating: { icon: 'spin', word: 'creating', tone: 'busy' },
  running: { icon: 'run', word: 'running', tone: 'plain' },
  stopped: { icon: 'stop', word: 'stopped', tone: 'off' },
  snapshotting: { icon: 'cam', word: 'snapshotting', tone: 'busy' },
  migrating: { icon: 'swap', word: 'migrating', tone: 'busy' },
  failed: { icon: 'fail', word: 'failed', tone: 'bad' },
  destroying: { icon: 'trash', word: 'destroying', tone: 'off' },
};

/** Every `ActivityOutcome`, as icon + word. */
export const OUTCOME_STATUS: Record<ActivityOutcome, StatusLook> = {
  started: { icon: 'spin', word: 'started', tone: 'busy' },
  succeeded: { icon: 'check', word: 'succeeded', tone: 'plain' },
  failed: { icon: 'fail', word: 'failed', tone: 'bad' },
  rejected: { icon: 'ban', word: 'rejected', tone: 'bad' },
  refused: { icon: 'shield', word: 'refused', tone: 'bad' },
  interrupted: { icon: 'pause', word: 'interrupted', tone: 'bad' },
};

/** Every host `Reachability`, as icon + word. */
export const REACHABILITY_STATUS: Record<Reachability, StatusLook> = {
  online: { icon: 'check', word: 'online', tone: 'plain' },
  degraded: { icon: 'warn', word: 'degraded', tone: 'bad' },
  offline: { icon: 'off', word: 'offline', tone: 'off' },
};

/** Every terminal session `TerminalStatus` (the bounded run_command session), as icon + word. */
export const TERMINAL_STATUS: Record<TerminalStatus, StatusLook> = {
  connecting: { icon: 'spin', word: 'connecting', tone: 'busy' },
  ready: { icon: 'check', word: 'ready', tone: 'plain' },
  running: { icon: 'play', word: 'running', tone: 'plain' },
  closed: { icon: 'stop', word: 'closed', tone: 'off' },
  error: { icon: 'fail', word: 'error', tone: 'bad' },
};

/** Every console `ConsoleStatus` (view-only frames), plus the viewer's own `paused`, as icon + word. */
export const CONSOLE_STATUS: Record<ConsoleStatus | 'paused', StatusLook> = {
  connecting: { icon: 'spin', word: 'connecting', tone: 'busy' },
  streaming: { icon: 'monitor', word: 'streaming', tone: 'plain' },
  paused: { icon: 'pause', word: 'paused', tone: 'off' },
  closed: { icon: 'stop', word: 'closed', tone: 'off' },
  error: { icon: 'fail', word: 'error', tone: 'bad' },
};

/**
 * Status icon + word. `status` is the raw API value and lands in
 * `data-status`, so a view can style one state (for example the activity
 * outcome borders in PR B) without new props. `label`, when given, is read
 * before the word ("Terminal status: ready") but not shown.
 */
export function StatusWord({ look, status, label }: { look: StatusLook; status: string; label?: string }) {
  return (
    <span className="st" data-tone={look.tone} data-status={status}>
      <Icon name={look.icon} />
      {label ? <span className="visually-hidden">{label}: </span> : null}
      <span className="st-word">{look.word}</span>
    </span>
  );
}

export function VmStatus({ state }: { state: VmState }) {
  return <StatusWord look={VM_STATUS[state]} status={state} />;
}

export function OutcomeStatus({ outcome }: { outcome: ActivityOutcome }) {
  return <StatusWord look={OUTCOME_STATUS[outcome]} status={outcome} />;
}

export function HostStatus({ reachability }: { reachability: Reachability }) {
  return <StatusWord look={REACHABILITY_STATUS[reachability]} status={reachability} />;
}

/** Terminal session status, read as "Terminal status: <word>". */
export function TerminalSessionStatus({ status }: { status: TerminalStatus }) {
  return <StatusWord look={TERMINAL_STATUS[status]} status={status} label="Terminal status" />;
}

/** Console stream status (or `paused` by the viewer), read as "Console status: <word>". */
export function ConsoleSessionStatus({ status }: { status: ConsoleStatus | 'paused' }) {
  return <StatusWord look={CONSOLE_STATUS[status]} status={status} label="Console status" />;
}

export const KIND_TITLE: Record<VmKind, string> = { test: 'Test VM, expires', static: 'Static VM, never expires' };

/** VM kind: a ring for test VMs, a diamond for static VMs, with the word unless `word={false}` (then the word is still read out). */
export function KindMarker({ kind, word = true }: { kind: VmKind; word?: boolean }) {
  return (
    <span className="kind-marker" data-kind={kind} title={KIND_TITLE[kind]}>
      <span className="kind" data-kind={kind} aria-hidden="true" />
      <span className={word ? 'kindw' : 'visually-hidden'}>{kind}</span>
    </span>
  );
}

/**
 * A missing value: a hatched swatch and the reason in words. Use it wherever
 * the API sends null so nothing reads as 0.
 */
export function NullValue({ reason }: { reason: string }) {
  return <span className="na" data-null="true">{reason}</span>;
}

/**
 * Renders `children(value)` when the value is present (including 0), and the
 * hatched `NullValue` with `reason` when it is null or undefined.
 */
export function Maybe<T>({ value, reason, children }: { value: T | null | undefined; reason: string; children: (value: T) => ReactNode }) {
  return value === null || value === undefined ? <NullValue reason={reason} /> : <>{children(value)}</>;
}

/** Ticks every `intervalMs`; null pauses (callers that pin a clock pass null). */
export function useNow(intervalMs: number | null): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (intervalMs === null) return;
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);
  return now;
}

const SHOWS_READY: VmState[] = ['running', 'snapshotting', 'migrating'];
const SOON_MS = 60 * 60_000;

/**
 * A VM as a card (hosts board, host detail, anywhere a VM is a card). Shows
 * only `Vm` fields: kind, name (linked), guest OS + version, arch, vCPU and
 * memory (hatched when null), status, expiry and guest readiness.
 *
 * `now` pins the clock for expiry (tests, or a parent that already ticks);
 * without it the card ticks every 30 s. `actions` is a slot in the title row
 * for view-specific controls; the card itself has none.
 */
export function VmCard({ vm, now, actions }: { vm: Vm; now?: number; actions?: ReactNode }) {
  const ticking = useNow(now === undefined ? 30_000 : null);
  const clock = now ?? ticking;
  const look = VM_STATUS[vm.state];
  const soon = vm.expiresAt !== null && Date.parse(vm.expiresAt) - clock < SOON_MS;
  const left = vm.expiresAt === null ? null : formatTimeLeft(vm.expiresAt, clock);
  return (
    <article className="vm-card" data-kind={vm.kind} data-state={vm.state} aria-label={`${vm.name}, ${vm.kind} VM, ${look.word}`}>
      <div className="vm-card-title">
        <KindMarker kind={vm.kind} word={false} />
        <a className="vm-card-name" href={href({ name: 'vm', id: vm.id })}>{vm.name}</a>
        {actions}
      </div>
      <div className="vm-card-guest">
        <OsGlyph os={vm.guestOs} />
        <span className="vm-card-os" title={vm.guestOsVersion}>{vm.guestOsVersion}</span>
        <ArchBadge arch={vm.arch} />
      </div>
      <div className="vm-card-size">
        <span className="kindw">{vm.kind}</span>
        <span aria-hidden="true">·</span>
        <Maybe value={vm.cpus} reason="vCPU unknown">{n => <span>{n} vCPU</span>}</Maybe>
        <span aria-hidden="true">·</span>
        <Maybe value={vm.memoryBytes} reason="memory unknown">{b => <span>{formatBytes(b)}</span>}</Maybe>
      </div>
      <div className="vm-card-status">
        <VmStatus state={vm.state} />
        {left === null
          ? <span className="vm-card-exp">never expires</span>
          : <span className="vm-card-exp" data-soon={soon || undefined}>{left.startsWith('in ') ? <span className="visually-hidden">expires </span> : null}{left}</span>}
      </div>
      {SHOWS_READY.includes(vm.state)
        ? <div className="vm-card-ready" data-ready={vm.ready}><Icon name={vm.ready ? 'check' : 'warn'} />{vm.ready ? 'guest ready' : 'guest not ready'}</div>
        : null}
    </article>
  );
}

/**
 * Sets the document title and moves focus to the page heading on mount.
 * Attach the ref to the view's h1. A view whose h1 moves (loading head, then
 * the loaded hero) passes a `phase` so focus follows the heading that remains.
 */
export function usePageHeading(title: string, phase?: string) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    document.title = `${title} · Arxcovo`;
    heading.current?.focus({ preventScroll: true });
  }, [title, phase]);
  return heading;
}

/**
 * View wrapper with a page head. `hideTitle` keeps the h1 for assistive
 * technology and focus while a view's own hero (the board or VM totals)
 * carries the visible headline.
 */
export function Page({ title, actions, children, eyebrow, lede, hideTitle }: { title: string; eyebrow?: ReactNode; lede?: ReactNode; actions?: ReactNode; hideTitle?: boolean; children: ReactNode }) {
  const heading = usePageHeading(title);
  return (
    <section className="page" aria-labelledby="page-title">
      {hideTitle && !eyebrow && !actions && !lede
        ? <h1 id="page-title" className="visually-hidden" tabIndex={-1} ref={heading}>{title}</h1>
        : (
          <header className="page-head">
            <div className="page-title">
              {eyebrow ? <div className="eyebrow">{eyebrow}</div> : null}
              <h1 id="page-title" className={hideTitle ? 'visually-hidden' : undefined} tabIndex={-1} ref={heading}>{title}</h1>
              {lede ? <p className="lede">{lede}</p> : null}
            </div>
            {actions ? <div className="page-actions">{actions}</div> : null}
          </header>
        )}
      {children}
    </section>
  );
}

/** A back link above a tile title ("‹ Hosts"). */
export function BackLink({ to, children }: { to: string; children: ReactNode }) {
  return <a className="back" href={to}><Icon name="back" />{children}</a>;
}

/** A Night Orbit tile: a titled section with a quiet icon label. */
export function Tile({ id, icon, title, aside, className, children, ...rest }: { id: string; icon?: IconName; title: ReactNode; aside?: ReactNode; className?: string; children: ReactNode; 'data-host-identity'?: boolean }) {
  return (
    <section className={className ? `tile ${className}` : 'tile'} aria-labelledby={id} {...rest}>
      <header className="tile-head">
        <h2 id={id}>{icon ? <Icon name={icon} /> : null}{title}</h2>
        {aside ? <div className="tile-aside">{aside}</div> : null}
      </header>
      {children}
    </section>
  );
}

/**
 * Used against a total, as a thin bar with "used / total" in mono. A null
 * `used` draws the bar hatched with `reason`; a null or zero total has no bar.
 */
export function CapBar({ label, used, total, format, reason, dim }: { label: string; used: number | null; total: number | null; format(n: number): string; reason: string; dim?: boolean }) {
  const pct = used === null || total === null || total <= 0 ? null : Math.min(100, Math.max(0, (used / total) * 100));
  return (
    <div className="cap" data-dim={dim || undefined}>
      <span className="cap-k">{label}</span>
      {used === null
        ? <span className="cap-bar" data-unknown="true" aria-hidden="true" />
        : <span className="cap-bar" aria-hidden="true"><i style={{ inlineSize: `${pct ?? 0}%` }} /></span>}
      {used === null
        ? <NullValue reason={reason} />
        : <span className="cap-v">{format(used)}{total === null ? null : <> / {format(total)}</>}</span>}
    </div>
  );
}

export function Loading({ label }: { label: string }) {
  return (
    <div className="state" role="status" aria-live="polite">
      <span className="spinner" aria-hidden="true" />
      {label}
    </div>
  );
}

export function ErrorState({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <div className="state state-error" role="alert">
      <p>{message}</p>
      {onRetry ? <button type="button" className="button" onClick={onRetry}>Try again</button> : null}
    </div>
  );
}

export function Empty({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="state state-empty">
      <p className="state-title">{title}</p>
      {children ? <p>{children}</p> : null}
    </div>
  );
}

/** Renders loading and error states for a resource, then `children(data)`. */
export function Loaded<T>({ resource, label, children }: { resource: Resource<T> & { reload(): void }; label: string; children: (data: T) => ReactNode }) {
  if (resource.status === 'loading') return <Loading label={`Loading ${label}…`} />;
  if (resource.status === 'error') {
    if (resource.error.code === 'unauthorized') return null;
    return <ErrorState message={`Could not load ${label}. ${resource.error.message}`} onRetry={resource.reload} />;
  }
  return <>{children(resource.data)}</>;
}

export function Facts({ items }: { items: [string, ReactNode][] }) {
  return (
    <dl className="facts">
      {items.map(([k, v]) => (
        <div key={k}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </dl>
  );
}

/** A labelled select. The label is linked by id so its name is just the label text. */
export function SelectField({ label, value, onChange, disabled, children }: { label: string; value: string; onChange(value: string): void; disabled?: boolean; children: ReactNode }) {
  const id = useId();
  return (
    <div className="field field-inline">
      <label htmlFor={id}>{label}</label>
      <select id={id} value={value} onChange={e => onChange(e.target.value)} disabled={disabled}>{children}</select>
    </div>
  );
}

/**
 * Contains render errors to the current view so one bad payload cannot
 * unmount the whole dashboard. Resets when the route changes.
 */
export class ViewBoundary extends Component<{ resetKey: string; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidUpdate(previous: { resetKey: string }): void {
    if (previous.resetKey !== this.props.resetKey && this.state.failed) this.setState({ failed: false });
  }

  render(): ReactNode {
    if (!this.state.failed) return this.props.children;
    return (
      <Page title="This view failed">
        <ErrorState message="Something unexpected happened while showing this view. Other views and your session are unaffected." onRetry={() => this.setState({ failed: false })} />
      </Page>
    );
  }
}
