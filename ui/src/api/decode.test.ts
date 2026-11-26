import { describe, expect, test } from 'vitest';
import { summarizeCpuHistory } from './cpuHistory';
import { decodeActivityPage, decodeCpuHistory, decodeCpuHistoryBatch, decodeHost, decodeHosts, decodeSession, decodeVm, decodeVms, LIMITS } from './decode';
import { FIXTURE_ACTIVITY, FIXTURE_HOSTS, FIXTURE_VMS } from './fixtures';
import { ApiError } from './types';

const host = FIXTURE_HOSTS[0];
const vm = FIXTURE_VMS[0];

function rejects(fn: () => unknown) {
  try {
    fn();
  } catch (e) {
    expect(e).toBeInstanceOf(ApiError);
    expect((e as ApiError).code).toBe('bad_response');
    return;
  }
  throw new Error('expected a bad_response error');
}

describe('fixtures conform to the wire contract', () => {
  test('every stand-in host, VM and activity entry survives the HTTP decoders unchanged', () => {
    expect(decodeHosts(JSON.parse(JSON.stringify({ hosts: FIXTURE_HOSTS })))).toEqual(FIXTURE_HOSTS);
    expect(decodeVms(JSON.parse(JSON.stringify({ vms: FIXTURE_VMS })))).toEqual(FIXTURE_VMS);
    const page = { entries: FIXTURE_ACTIVITY.slice(0, 50), nextBeforeId: 14 };
    expect(decodeActivityPage(JSON.parse(JSON.stringify(page)))).toEqual(page);
  });

  test('fixtures cover both architectures and every reachability state', () => {
    expect(new Set(FIXTURE_HOSTS.map(h => h.arch))).toEqual(new Set(['x86_64', 'arm64']));
    expect(new Set(FIXTURE_VMS.map(v => v.arch))).toEqual(new Set(['x86_64', 'arm64']));
    expect(new Set(FIXTURE_HOSTS.map(h => h.reachability))).toEqual(new Set(['online', 'degraded', 'offline']));
  });
});

describe('decoders reject malformed payloads', () => {
  test('session needs a user, a plausible CSRF token and an ISO expiry', () => {
    const ok = { user: { name: 'owner' }, csrfToken: 'a'.repeat(32), expiresAt: '2026-11-23T12:00:00Z' };
    expect(decodeSession(ok)).toEqual(ok);
    rejects(() => decodeSession({ ...ok, csrfToken: 'short' }));
    rejects(() => decodeSession({ ...ok, expiresAt: 'tomorrow' }));
    rejects(() => decodeSession({ csrfToken: ok.csrfToken, expiresAt: ok.expiresAt }));
  });

  test('host fields are closed enums with bounded strings and safe identifiers', () => {
    rejects(() => decodeHost({ ...host, arch: 'riscv64' }));
    rejects(() => decodeHost({ ...host, os: 'plan9' }));
    rejects(() => decodeHost({ ...host, id: '../etc' }));
    rejects(() => decodeHost({ ...host, name: 'x'.repeat(129) }));
    rejects(() => decodeHost({ ...host, cpu: { ...host.cpu, usedPercent: 140 } }));
    rejects(() => decodeHost({ ...host, tools: Array.from({ length: LIMITS.tools + 1 }, () => ({ name: 'git', version: null })) }));
    rejects(() => decodeHost(null));
  });

  test('a test VM must carry an expiry while a static VM may not', () => {
    rejects(() => decodeVm({ ...vm, kind: 'test', expiresAt: null }));
    expect(decodeVm({ ...vm, kind: 'static', expiresAt: null }).expiresAt).toBeNull();
    rejects(() => decodeVm({ ...vm, state: 'exploded' }));
    rejects(() => decodeVm({ ...vm, ready: 'yes' }));
  });

  test('activity pages are capped at 50 entries and need positive ids', () => {
    rejects(() => decodeActivityPage({ entries: FIXTURE_ACTIVITY.slice(0, 51), nextBeforeId: null }));
    rejects(() => decodeActivityPage({ entries: [{ ...FIXTURE_ACTIVITY[0], id: 0 }], nextBeforeId: null }));
    rejects(() => decodeActivityPage({ entries: [{ ...FIXTURE_ACTIVITY[0], outcome: 'maybe' }], nextBeforeId: null }));
  });
});

describe('CPU history', () => {
  const day = {
    hostId: 'atlas',
    range: '24h',
    bucketSeconds: 600,
    from: '2026-11-25T10:20:00Z',
    to: '2026-11-26T10:20:00Z',
    samples: [0, 38.2, 100, ...Array<null>(141).fill(null)],
  };

  test('preserves coverage gaps and decodes bounded batches', () => {
    expect(decodeCpuHistory(day)).toEqual(day);
    expect(decodeCpuHistoryBatch({ series: [day, { ...day, hostId: 'harbor' }] }))
      .toEqual([day, { ...day, hostId: 'harbor' }]);
  });

  test('requires a known range, matching dimensions and aligned UTC timestamps', () => {
    rejects(() => decodeCpuHistory({ ...day, range: '2h' }));
    rejects(() => decodeCpuHistory({ ...day, bucketSeconds: 300 }));
    rejects(() => decodeCpuHistory({ ...day, from: '2026-11-25T10:21:00Z', to: '2026-11-26T10:21:00Z' }));
    rejects(() => decodeCpuHistory({ ...day, to: '2026-11-26T10:30:00Z' }));
    rejects(() => decodeCpuHistory({ ...day, to: '2026-11-26' }));
    rejects(() => decodeCpuHistory({ ...day, hostId: '../atlas' }));
  });

  test('requires exactly the advertised point count and finite one-decimal percentages or null', () => {
    rejects(() => decodeCpuHistory({ ...day, samples: day.samples.slice(1) }));
    rejects(() => decodeCpuHistory({ ...day, samples: [...day.samples, null] }));
    for (const bad of [-0.1, 101, NaN, Infinity, '37', 38.21, undefined]) {
      rejects(() => decodeCpuHistory({ ...day, samples: [bad, ...day.samples.slice(1)] }));
    }
  });

  test('batch refuses excess series, duplicate hosts and non-24h data', () => {
    const atLimit = Array.from({ length: LIMITS.hosts }, (_, i) => ({ ...day, hostId: `host-${i}` }));
    expect(decodeCpuHistoryBatch({ series: atLimit })).toHaveLength(LIMITS.hosts);
    rejects(() => decodeCpuHistoryBatch({ series: [...atLimit, day] }));
    rejects(() => decodeCpuHistoryBatch({ series: [day, day] }));
    const hour = { ...day, range: '1h', bucketSeconds: 60, from: '2026-11-26T09:20:00Z', samples: day.samples.slice(0, 60) };
    expect(decodeCpuHistory(hour)).toEqual(hour);
    rejects(() => decodeCpuHistoryBatch({ series: [hour] }));
  });

  test('window average and peak use only present buckets and report the number missing', () => {
    expect(summarizeCpuHistory([null, 32, 0, null, 64])).toEqual({ avg: 32, peak: 64, missing: 2 });
    expect(summarizeCpuHistory([25, 75])).toEqual({ avg: 50, peak: 75, missing: 0 });
    expect(summarizeCpuHistory([0, null])).toEqual({ avg: 0, peak: 0, missing: 1 });
    expect(summarizeCpuHistory([null, null])).toEqual({ avg: null, peak: null, missing: 2 });
    expect(summarizeCpuHistory([])).toEqual({ avg: null, peak: null, missing: 0 });
  });
});
