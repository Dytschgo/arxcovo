import { useId, type ReactNode } from 'react';
import type { OsFamily } from '../api/types';

// Night Orbit icon set and O3 logo. Source: docs/design/night-orbit/_shell.html
// (the owner-approved design package, 2026-11-26). Icons are drawn inline so a
// component never depends on a page-level sprite, and every icon is decorative:
// the visible word next to it carries the meaning.

const PATHS = {
  hosts: <><rect x="3" y="4" width="18" height="7" rx="2" /><rect x="3" y="13" width="18" height="7" rx="2" /><path d="M7 7.5h.01M7 16.5h.01" /></>,
  vm: <><path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z" /><path d="M12 12l8-4.5M12 12v9M12 12L4 7.5" /></>,
  term: <><rect x="3" y="4" width="18" height="16" rx="3" /><path d="M7 9l3 3-3 3M13 15h4" /></>,
  act: <path d="M3 12h4l3-8 4 16 3-8h4" />,
  run: <><circle cx="12" cy="12" r="8" /><circle cx="12" cy="12" r="2.5" fill="currentColor" /></>,
  stop: <rect x="6" y="6" width="12" height="12" rx="2" />,
  spin: <><path d="M20 12a8 8 0 1 1-3-6.2" /><path d="M20 4v4h-4" /></>,
  fail: <><circle cx="12" cy="12" r="9" /><path d="M9 9l6 6M15 9l-6 6" /></>,
  trash: <path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" />,
  swap: <path d="M5 12h14M13 6l6 6-6 6" />,
  check: <path d="M5 12l5 5 9-10" />,
  warn: <><path d="M12 3l10 18H2z" /><path d="M12 10v5M12 18h.01" /></>,
  off: <path d="M3 3l18 18M8.5 8.5A5 5 0 0 0 12 17h4M17 7h-3" />,
  ban: <><circle cx="12" cy="12" r="9" /><path d="M5.6 5.6l12.8 12.8" /></>,
  shield: <path d="M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z" />,
  pause: <path d="M9 6v12M15 6v12" />,
  cam: <><path d="M4 8h3l2-3h6l2 3h3v11H4z" /><circle cx="12" cy="13" r="3.5" /></>,
  signout: <path d="M14 4h5v16h-5M10 16l-4-4 4-4M6 12h10" />,
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof PATHS;

/** A 24-unit stroke icon from the Night Orbit set. Always decorative. */
export function Icon({ name, className = 'i' }: { name: IconName; className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" aria-hidden="true" focusable="false" data-icon={name}>
      {PATHS[name]}
    </svg>
  );
}

/** OS glyph for data (guest or host OS). Colour comes from `--win` / `--lin`. */
export function OsGlyph({ os }: { os: OsFamily }) {
  return (
    <svg className="os-glyph" data-os={os} viewBox="0 0 16 16" aria-hidden="true" focusable="false">
      {os === 'windows'
        ? <path d="M2 2h5v5H2zM9 2h5v5H9zM2 9h5v5H2zM9 9h5v5H9z" fill="currentColor" />
        : <path d="M5 3v10h7" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />}
    </svg>
  );
}

/**
 * The chosen O3 "Night Orbit" mark: a white A, its orbit and moon, with a
 * short light in the OS colours sweeping the orbit now and then. The motion is
 * CSS only (`.logo .lg-*` in styles.css) and is skipped under reduced motion.
 */
export function Logo({ className = 'logo' }: { className?: string }) {
  const gradient = `lgo${useId().replace(/[^A-Za-z0-9_-]/g, '')}`;
  return (
    <svg className={className} viewBox="0 0 32 32" aria-hidden="true" focusable="false">
      <defs>
        <linearGradient id={gradient} gradientUnits="userSpaceOnUse" x1="3" y1="0" x2="29" y2="0">
          <stop offset="0" stopColor="#4cc2ff" />
          <stop offset=".5" stopColor="#b9b0e8" />
          <stop offset="1" stopColor="#f5a524" />
        </linearGradient>
      </defs>
      <path className="lg-a" pathLength={1} d="M7.5 27 L16 7 L24.5 27" />
      <g transform="rotate(-14 16 19.5)">
        <ellipse className="lg-o" pathLength={1} cx="16" cy="19.5" rx="13" ry="4.4" />
        <ellipse className="lg-c" pathLength={1} cx="16" cy="19.5" rx="13" ry="4.4" stroke={`url(#${gradient})`} />
      </g>
      <circle className="lg-n" cx="16" cy="7" r="2.9" />
      <circle className="lg-m" cx="28.2" cy="16.3" r="1.9" />
    </svg>
  );
}
