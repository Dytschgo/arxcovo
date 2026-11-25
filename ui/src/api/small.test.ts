import { describe, expect, test } from 'vitest';
import { incompatibility, formatBytes } from '../app/format';
import { href, parseRoute, type Route } from '../app/router';
import { exampleCommand, parseCommandLine } from './command';
import { chooseAdapter } from './select';

describe('adapter selection', () => {
  test('built bundles always use HTTP, even when the stand-in is requested', () => {
    expect(chooseAdapter({ standinAllowed: false })).toBe('http');
    expect(chooseAdapter({ standinAllowed: false, VITE_ARX_API: 'standin' })).toBe('http');
  });

  test('the dev server and tests default to the stand-in and can opt into HTTP', () => {
    expect(chooseAdapter({ standinAllowed: true })).toBe('standin');
    expect(chooseAdapter({ standinAllowed: true, VITE_ARX_API: 'http' })).toBe('http');
  });
});

describe('command example', () => {
  test('is built from the first discovered tool and never invents git', () => {
    expect(exampleCommand([{ name: 'cargo' }, { name: 'node' }])).toBe('cargo --version');
    expect(exampleCommand([])).toBe('tool name and arguments');
  });

  test('an empty line is told to type a tool without naming a tool the host may lack', () => {
    const empty = parseCommandLine('  ');
    expect(empty).toEqual({ ok: false, error: 'Type a tool name followed by its arguments.' });
  });
});

describe('command line parsing', () => {
  test('splits on whitespace and honours quotes and escapes', () => {
    expect(parseCommandLine('  git   commit -m "fix: a \\"quoted\\" word"  ')).toEqual({ ok: true, command: { tool: 'git', args: ['commit', '-m', 'fix: a "quoted" word'] } });
    expect(parseCommandLine("cargo test -- 'name with spaces' ''")).toEqual({ ok: true, command: { tool: 'cargo', args: ['test', '--', 'name with spaces', ''] } });
  });

  test('refuses empty lines, open quotes, paths as tools and too many arguments', () => {
    expect(parseCommandLine('   ').ok).toBe(false);
    expect(parseCommandLine('git "oops').ok).toBe(false);
    expect(parseCommandLine('/bin/sh -c x').ok).toBe(false);
    expect(parseCommandLine('..\\cmd.exe').ok).toBe(false);
    expect(parseCommandLine(`git ${'a '.repeat(129)}`).ok).toBe(false);
  });
});

describe('routing', () => {
  test('every route round-trips through its hash', () => {
    const routes: Route[] = [
      { name: 'login' }, { name: 'hosts' }, { name: 'host', id: 'atlas' }, { name: 'vms' }, { name: 'vm', id: 'win11-ci-01' },
      { name: 'console', id: 'web-static' }, { name: 'activity' }, { name: 'terminal', hostId: null }, { name: 'terminal', hostId: 'harbor' },
    ];
    for (const route of routes) expect(parseRoute(href(route))).toEqual(route);
    expect(parseRoute('')).toEqual({ name: 'hosts' });
  });

  test('unsafe or unknown paths are not_found', () => {
    expect(parseRoute('#/hosts/..').name).toBe('not_found');
    expect(parseRoute('#/hosts/%2e%2e').name).toBe('not_found');
    expect(parseRoute('#/vms/x/console/extra').name).toBe('not_found');
    expect(parseRoute('#/admin').name).toBe('not_found');
  });
});

describe('formatting and compatibility', () => {
  test('bytes use binary units', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1.5 * 1024 ** 3)).toBe('1.5 GiB');
    expect(formatBytes(128 * 1024 ** 3)).toBe('128 GiB');
  });

  test('architecture and OS mismatches explain why a host is greyed out', () => {
    const linuxArm = { arch: 'arm64' as const, os: 'linux' as const, reachability: 'online', name: 'pebble' };
    const linuxX86 = { arch: 'x86_64' as const, os: 'linux' as const, reachability: 'online', name: 'atlas' };
    expect(incompatibility({ arch: 'x86_64', guestOs: 'linux' }, linuxArm)).toBe('arm64 host cannot run x86_64 guests');
    expect(incompatibility({ arch: 'x86_64', guestOs: 'windows' }, linuxX86)).toBeNull();
    expect(incompatibility({ arch: 'arm64', guestOs: 'linux' }, linuxArm)).toBeNull();
    expect(incompatibility({ arch: 'arm64', guestOs: 'linux' }, { ...linuxArm, reachability: 'offline' })).toBe('pebble is offline');
  });
});
