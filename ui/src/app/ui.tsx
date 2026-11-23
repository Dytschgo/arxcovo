import { useEffect, useId, useRef, type ReactNode } from 'react';
import type { Arch } from '../api/types';
import type { Resource } from './context';

export function ArchBadge({ arch }: { arch: Arch }) {
  return (
    <span className="arch" data-arch={arch} title={`Machine architecture: ${arch}`}>
      <span className="visually-hidden">Architecture </span>
      {arch}
    </span>
  );
}

export type Tone = 'ok' | 'warn' | 'bad' | 'info' | 'muted';

export function Pill({ tone, children }: { tone: Tone; children: ReactNode }) {
  return <span className="pill" data-tone={tone}>{children}</span>;
}

const TONES: Record<string, Tone> = {
  online: 'ok', degraded: 'warn', offline: 'bad',
  running: 'ok', creating: 'info', snapshotting: 'info', migrating: 'info', stopped: 'muted', failed: 'bad', destroying: 'warn',
  succeeded: 'ok', started: 'info', rejected: 'warn', refused: 'warn', interrupted: 'muted',
};

export function StatePill({ value }: { value: string }) {
  return <Pill tone={TONES[value] ?? 'muted'}>{value.replace('_', ' ')}</Pill>;
}

/** View wrapper: sets the document title and moves focus to the heading on mount. */
export function Page({ title, actions, children, eyebrow }: { title: string; eyebrow?: ReactNode; actions?: ReactNode; children: ReactNode }) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    document.title = `${title} · Arxcovo`;
    heading.current?.focus({ preventScroll: true });
  }, [title]);
  return (
    <section className="page" aria-labelledby="page-title">
      <header className="page-head">
        <div className="page-title">
          {eyebrow ? <div className="eyebrow">{eyebrow}</div> : null}
          <h1 id="page-title" tabIndex={-1} ref={heading}>{title}</h1>
        </div>
        {actions ? <div className="page-actions">{actions}</div> : null}
      </header>
      {children}
    </section>
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

export function Meter({ label, value, detail }: { label: string; value: number | null; detail: string }) {
  const tone = value === null ? 'muted' : value >= 90 ? 'bad' : value >= 75 ? 'warn' : 'ok';
  return (
    <div className="meter" data-tone={tone}>
      <div className="meter-label">
        <span>{label}</span>
        <span className="meter-detail">{detail}</span>
      </div>
      <div className="meter-track" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={value ?? undefined} aria-valuetext={value === null ? 'unknown' : `${value}%`}>
        <div className="meter-fill" style={{ inlineSize: `${value ?? 0}%` }} />
      </div>
    </div>
  );
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
