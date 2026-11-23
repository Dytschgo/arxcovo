import type { Arch, OsFamily } from '../api/types';

export function formatBytes(bytes: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 || value >= 10 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' });

/** Never throws: an unparseable timestamp renders as a dash. */
export function formatDate(iso: string | null): string {
  if (!iso) return '—';
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? '—' : dateFormat.format(date);
}

export const OS_LABEL: Record<OsFamily, string> = { windows: 'Windows', macos: 'macOS', linux: 'Linux' };

export function percent(used: number | null, total: number): number | null {
  if (used === null || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.round((used / total) * 100)));
}

/** Why a guest cannot run on a host, or null when it can. */
export function incompatibility(guest: { arch: Arch; guestOs: OsFamily }, host: { arch: Arch; os: OsFamily; reachability: string; name: string }): string | null {
  if (guest.guestOs === 'macos' && host.os !== 'macos') return 'macOS guests need a Mac host';
  if (guest.arch !== host.arch) return `${host.arch} host cannot run ${guest.arch} guests`;
  if (guest.guestOs === 'windows' && host.os === 'macos') return 'Windows guests are not supported on Mac hosts';
  if (host.reachability === 'offline') return `${host.name} is offline`;
  return null;
}
