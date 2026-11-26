import { act, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, test, vi } from 'vitest';
import type { ActivityOutcome, Reachability, Vm, VmState } from '../api/types';
import { formatTimeLeft } from './format';
import { Logo } from './icons';
import {
  ArchBadge, HostStatus, KindMarker, Maybe, NullValue, OUTCOME_STATUS, OutcomeStatus, REACHABILITY_STATUS,
  VM_STATUS, VmCard, VmStatus,
} from './ui';

// Every value of the API unions, spelled out so a new member must be added here too.
const VM_STATES: VmState[] = ['creating', 'running', 'stopped', 'snapshotting', 'migrating', 'failed', 'destroying'];
const OUTCOMES: ActivityOutcome[] = ['started', 'succeeded', 'failed', 'rejected', 'refused', 'interrupted'];
const REACHABILITY: Reachability[] = ['online', 'degraded', 'offline'];

const NOW = Date.parse('2026-11-26T12:00:00Z');
const at = (ms: number) => new Date(NOW + ms).toISOString();

const VM: Vm = {
  id: 'vm-1', name: 't-ubuntu-77c1', kind: 'test', template: { id: 'ubuntu-base', version: 4 },
  guestOs: 'linux', guestOsVersion: 'Ubuntu 24.04', arch: 'x86_64', hostId: 'atlas', state: 'running', ready: true,
  expiresAt: at(2 * 3600e3 + 5 * 60e3), cpus: 2, memoryBytes: 4 * 1024 ** 3, diskBytes: 20 * 1024 ** 3, snapshots: [],
};

describe('status icon + word', () => {
  test.each(VM_STATES)('VM state %s renders an icon and its word', state => {
    const { container } = render(<VmStatus state={state} />);
    const st = container.querySelector('.st')!;
    expect(st.getAttribute('data-status')).toBe(state);
    expect(st.textContent).toBe(state);
    const icon = st.querySelector('svg')!;
    expect(icon.getAttribute('aria-hidden')).toBe('true');
    expect(icon.getAttribute('data-icon')).toBe(VM_STATUS[state].icon);
    expect(icon.children.length).toBeGreaterThan(0);
  });

  test.each(OUTCOMES)('activity outcome %s renders an icon and its word', outcome => {
    const { container } = render(<OutcomeStatus outcome={outcome} />);
    const st = container.querySelector('.st')!;
    expect(st.getAttribute('data-status')).toBe(outcome);
    expect(st.textContent).toBe(outcome);
    expect(st.querySelector('svg')!.getAttribute('data-icon')).toBe(OUTCOME_STATUS[outcome].icon);
  });

  test.each(REACHABILITY)('host reachability %s renders an icon and its word', reachability => {
    const { container } = render(<HostStatus reachability={reachability} />);
    expect(container.querySelector('.st')!.textContent).toBe(reachability);
    expect(container.querySelector('svg')!.getAttribute('data-icon')).toBe(REACHABILITY_STATUS[reachability].icon);
  });

  test('the maps cover exactly the API unions, and states that need the owner read as bad', () => {
    expect(Object.keys(VM_STATUS).sort()).toEqual([...VM_STATES].sort());
    expect(Object.keys(OUTCOME_STATUS).sort()).toEqual([...OUTCOMES].sort());
    expect(Object.keys(REACHABILITY_STATUS).sort()).toEqual([...REACHABILITY].sort());
    expect(VM_STATUS.failed.tone).toBe('bad');
    expect(OUTCOME_STATUS.refused.tone).toBe('bad');
    expect(REACHABILITY_STATUS.degraded.tone).toBe('bad');
    expect(VM_STATUS.creating.tone).toBe('busy');
    expect(VM_STATUS.stopped.tone).toBe('off');
  });

  test('distinct VM states never share an icon, so the glyph alone still tells them apart', () => {
    const icons = VM_STATES.map(s => VM_STATUS[s].icon);
    expect(new Set(icons).size).toBe(icons.length);
    const outcomeIcons = OUTCOMES.map(o => OUTCOME_STATUS[o].icon);
    expect(new Set(outcomeIcons).size).toBe(outcomeIcons.length);
  });
});

describe('kind marker', () => {
  test('test VMs get a ring and the word, static VMs a diamond and the word', () => {
    const { container, rerender } = render(<KindMarker kind="test" />);
    const marker = container.querySelector('.kind-marker')!;
    expect(marker.getAttribute('title')).toBe('Test VM, expires');
    expect(marker.querySelector('.kind')!.getAttribute('data-kind')).toBe('test');
    expect(marker.querySelector('.kind')!.getAttribute('aria-hidden')).toBe('true');
    expect(screen.getByText('test').className).toBe('kindw');
    rerender(<KindMarker kind="static" />);
    expect(container.querySelector('.kind')!.getAttribute('data-kind')).toBe('static');
    expect(container.querySelector('.kind-marker')!.getAttribute('title')).toBe('Static VM, never expires');
    expect(screen.getByText('static')).toBeTruthy();
  });

  test('without the visible word the kind is still read out', () => {
    render(<KindMarker kind="static" word={false} />);
    expect(screen.getByText('static').className).toBe('visually-hidden');
  });
});

describe('arch badge', () => {
  test.each(['x86_64', 'arm64'] as const)('%s is named for every reader', arch => {
    const { container } = render(<ArchBadge arch={arch} />);
    const badge = container.querySelector('.arch')!;
    expect(badge.getAttribute('title')).toBe(`Machine architecture: ${arch}`);
    expect(badge.getAttribute('data-arch')).toBe(arch);
    expect(badge.textContent).toBe(`Architecture ${arch}`);
  });
});

describe('hatched null', () => {
  test('a null value shows the hatched reason, never 0', () => {
    const { container } = render(<Maybe value={null as number | null} reason="usage unavailable">{n => <span>{n}%</span>}</Maybe>);
    const na = container.querySelector('.na')!;
    expect(na.getAttribute('data-null')).toBe('true');
    expect(na.textContent).toBe('usage unavailable');
    expect(container.textContent).not.toMatch(/0/);
  });

  test('undefined is also missing', () => {
    const { container } = render(<Maybe value={undefined as number | undefined} reason="unknown">{n => n}</Maybe>);
    expect(container.querySelector('.na')).toBeTruthy();
  });

  test('a real 0 is a value and renders as 0', () => {
    const { container } = render(<Maybe value={0} reason="usage unavailable">{n => <span>{n}%</span>}</Maybe>);
    expect(container.querySelector('.na')).toBeNull();
    expect(container.textContent).toBe('0%');
  });

  test('NullValue alone carries its reason as text', () => {
    render(<NullValue reason="memory unknown" />);
    expect(screen.getByText('memory unknown').className).toBe('na');
  });
});

describe('VM card', () => {
  test('shows name link, guest, arch, kind, size through formatBytes, status, expiry and readiness', () => {
    render(<VmCard vm={VM} now={NOW} />);
    const card = screen.getByRole('article', { name: 't-ubuntu-77c1, test VM, running' });
    expect(within(card).getByRole('link', { name: 't-ubuntu-77c1' }).getAttribute('href')).toBe('#/vms/vm-1');
    expect(within(card).getByText('Ubuntu 24.04')).toBeTruthy();
    expect(within(card).getByTitle('Machine architecture: x86_64')).toBeTruthy();
    expect(card.querySelector('.os-glyph')!.getAttribute('data-os')).toBe('linux');
    expect(card.querySelector('.kind')!.getAttribute('data-kind')).toBe('test');
    expect(within(card).getByText('2 vCPU')).toBeTruthy();
    expect(within(card).getByText('4.0 GiB')).toBeTruthy();
    expect(card.querySelector('.st')!.getAttribute('data-status')).toBe('running');
    const exp = card.querySelector('.vm-card-exp')!;
    expect(exp.textContent).toBe('expires in 2h 05m');
    expect(exp.hasAttribute('data-soon')).toBe(false);
    expect(within(card).getByText('guest ready')).toBeTruthy();
  });

  test('null vCPU and memory are hatched with a reason, never 0', () => {
    render(<VmCard vm={{ ...VM, cpus: null, memoryBytes: null }} now={NOW} />);
    const card = screen.getByRole('article');
    expect(within(card).getByText('vCPU unknown').className).toBe('na');
    expect(within(card).getByText('memory unknown').className).toBe('na');
    expect(card.querySelector('.vm-card-size')!.textContent).not.toMatch(/\b0\b/);
  });

  test('a static VM never expires; a test VM near expiry is marked soon; an expired one says so', () => {
    const { rerender } = render(<VmCard vm={{ ...VM, kind: 'static', expiresAt: null }} now={NOW} />);
    expect(screen.getByRole('article').querySelector('.vm-card-exp')!.textContent).toBe('never expires');
    expect(screen.getByRole('article').querySelector('.kind')!.getAttribute('data-kind')).toBe('static');
    rerender(<VmCard vm={{ ...VM, expiresAt: at(14 * 60e3) }} now={NOW} />);
    const soon = screen.getByRole('article').querySelector('.vm-card-exp')!;
    expect(soon.textContent).toBe('expires in 14m');
    expect(soon.hasAttribute('data-soon')).toBe(true);
    rerender(<VmCard vm={{ ...VM, expiresAt: at(-1000) }} now={NOW} />);
    expect(screen.getByRole('article').querySelector('.vm-card-exp')!.textContent).toBe('expired');
  });

  test('readiness shows only for states where the guest can answer', () => {
    const { rerender } = render(<VmCard vm={{ ...VM, ready: false }} now={NOW} />);
    expect(screen.getByText('guest not ready')).toBeTruthy();
    for (const state of ['creating', 'stopped', 'failed', 'destroying'] as const) {
      rerender(<VmCard vm={{ ...VM, state }} now={NOW} />);
      expect(screen.queryByText(/guest (not )?ready/)).toBeNull();
    }
  });

  test('the actions slot sits in the title row', () => {
    render(<VmCard vm={VM} now={NOW} actions={<button type="button">Menu</button>} />);
    expect(screen.getByRole('button', { name: 'Menu' }).closest('.vm-card-title')).toBeTruthy();
  });

  describe('without a pinned clock', () => {
    afterEach(() => vi.useRealTimers());

    test('the expiry counts down on its own', () => {
      vi.useFakeTimers();
      vi.setSystemTime(NOW);
      render(<VmCard vm={{ ...VM, expiresAt: at(3 * 60e3) }} />);
      const exp = () => screen.getByRole('article').querySelector('.vm-card-exp')!.textContent;
      expect(exp()).toBe('expires in 3m');
      act(() => { vi.advanceTimersByTime(60_000); });
      expect(exp()).toBe('expires in 2m');
    });
  });
});

describe('time left', () => {
  test('formats hours, minutes, the last minute, expiry and bad input', () => {
    expect(formatTimeLeft(at(2 * 3600e3 + 5 * 60e3), NOW)).toBe('in 2h 05m');
    expect(formatTimeLeft(at(59 * 60e3 + 59e3), NOW)).toBe('in 59m');
    expect(formatTimeLeft(at(30e3), NOW)).toBe('in under a minute');
    expect(formatTimeLeft(at(0), NOW)).toBe('expired');
    expect(formatTimeLeft('not a date', NOW)).toBe('—');
  });
});

describe('logo', () => {
  test('is decorative and gives each copy its own gradient id', () => {
    const { container } = render(<><Logo /><Logo /></>);
    const svgs = container.querySelectorAll('svg.logo');
    expect(svgs).toHaveLength(2);
    const ids = Array.from(container.querySelectorAll('linearGradient'), g => g.id);
    expect(new Set(ids).size).toBe(2);
    svgs.forEach((svg, i) => {
      expect(svg.getAttribute('aria-hidden')).toBe('true');
      expect(svg.querySelector('.lg-c')!.getAttribute('stroke')).toBe(`url(#${ids[i]})`);
    });
  });
});
