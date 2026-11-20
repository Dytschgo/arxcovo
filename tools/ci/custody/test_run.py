"""Pure fault-driven launcher tests: no Docker, sudo, network or fixture provisioning."""
import importlib.util
import io
import os
import json
from contextlib import redirect_stdout
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('custody_launcher', Path(__file__).with_name('run.py'))
run = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(run)
IMAGE = 'sha256:' + 'a' * 64
CONTAINER = 'b' * 64
GOOD = b'running 1 test\nARX_PASS selected=1 passed=1\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 829 filtered out; finished in 0.01s\n'
READY = b'ARX_READY 42 3,4,5 ' + b'a' * 256 + b'\n'


class Clock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class FakeChild:
    def __init__(self, clock, ready_at=1, terminal_at=1000):
        self.clock = clock
        self.ready_at, self.terminal_at = ready_at, terminal_at
        self.deadline = 30
        self.out = bytearray()
        self.reaped = False
        self.killed = False

    def pump(self):
        pass

    def alive(self):
        run.require(self.clock() < self.deadline, 'HolderTimeout')
        run.require(self.clock() < self.terminal_at, 'HolderExit')
        if self.clock() >= self.ready_at and not self.out:
            self.out.extend(READY)

    def finish(self):
        if self.clock() < self.terminal_at:
            return False
        self.out.extend(GOOD)
        self.reaped = True
        return True

    def kill(self):
        self.killed = self.reaped = True


def inspection():
    return {'Config': {'Labels': {'arxcovo.custody.run': 'c'*16}}, 'Image': IMAGE,
            'HostConfig': {'ShmSize': run.MIB, 'IpcMode': 'private', 'Memory': 256*run.MIB,
                           'MemorySwap': 256*run.MIB, 'PidsLimit': 64, 'NanoCpus': 1_000_000_000,
                           'ReadonlyRootfs': True, 'NetworkMode': 'none', 'CapDrop': ['ALL']},
            'Mounts': [{'Destination': '/wrap', 'Source': '/fixture/wrap', 'Type': 'bind'},
                       {'Destination': '/store', 'Source': '/fixture/store', 'Type': 'bind'}]}


class Filesystem:
    """Descriptor model: opened objects survive pathname replacement."""
    O_PATH, O_NOFOLLOW, O_CLOEXEC, O_RDONLY = 1, 2, 4, 8

    def __init__(self):
        self.nodes, self.fds, self.nextfd = {}, {}, 10
        self.opened = []
        self.knob = b'1\n'
        self.add(run.HARDLINKS, 0o100644)
        self.add('/', 0o40755)
        for path in ('/usr', '/usr/bin', '/usr/sbin', '/etc', '/etc/alternatives',
                     '/usr/lib', '/usr/lib/cargo', '/usr/lib/cargo/bin'):
            self.add(path, 0o40755)
        self.add(run.SUDO_TARGET, 0o104755)
        self.link('/usr/bin/sudo', '/etc/alternatives/sudo')
        self.link('/etc/alternatives/sudo', run.SUDO_TARGET)
        for role in run.TOOLS:
            if role != 'sudo':
                self.add('/usr/bin/' + role, 0o100755)

    def add(self, path, mode):
        self.nodes[path] = SimpleNamespace(st_mode=mode, st_uid=0, st_gid=123,
            st_dev=1, st_ino=len(self.nodes)+100, st_nlink=1)

    def link(self, path, target):
        self.add(path, 0o120777)
        self.nodes[path].target = target

    def name(self, name, dir_fd):
        return name if name.startswith('/') else self.fds[dir_fd][0].rstrip('/') + '/' + name

    def open(self, name, flags, dir_fd=None):
        assert flags == (self.O_RDONLY if name == run.HARDLINKS else self.O_PATH) | self.O_NOFOLLOW | self.O_CLOEXEC
        path = self.name(name, dir_fd)
        self.opened.append(path)
        if path not in self.nodes:
            raise FileNotFoundError('private-sentinel')
        self.nextfd += 1
        self.fds[self.nextfd] = (path, self.nodes[path])
        return self.nextfd

    def read(self, fd, size):
        assert self.fds[fd][0] == run.HARDLINKS and size == 16
        if isinstance(self.knob, Exception):
            raise self.knob
        return self.knob[:size]

    def fstat(self, fd):
        return self.fds[fd][1]

    def readlink(self, name, dir_fd):
        return self.nodes[self.name(name, dir_fd)].target

    def stat(self, name, dir_fd, follow_symlinks):
        assert not follow_symlinks
        return self.nodes[self.name(name, dir_fd)]

    def close(self, fd):
        del self.fds[fd]


class ToolTests(unittest.TestCase):
    def tool(self, fs, role='sudo', entry=None, records=None):
        return run.trusted_tool(role, lambda event, **facts: records.append(dict(event=event, **facts))
            if records is not None else None, fs, entry or '/usr/bin/' + role)

    def refused(self, fs, role='sudo', entry=None):
        records = []
        with self.assertRaisesRegex(run.Refusal, 'ToolOwnership'):
            self.tool(fs, role, entry, records)
        refusals = [r for r in records if r['event'] == 'tool-refusal']
        self.assertEqual(len(refusals), 1)
        self.assertNotIn('private-sentinel', json.dumps(records))
        self.assertLess(len(json.dumps(refusals)), 1024)
        self.assertFalse(fs.fds)
        return refusals[0]

    def test_exact_sudo_and_all_normal_roles(self):
        fs = Filesystem()
        for role in run.TOOLS:
            obj = self.tool(fs, role)
            self.assertEqual(obj.recheck(), run.SUDO_TARGET if role == 'sudo' else '/usr/bin/' + role)
            obj.close()
            self.assertFalse(fs.fds)
        fs.add('/usr/sbin/mke2fs', 0o100755)
        fs.link('/usr/bin/mkfs.ext4', '../sbin/mke2fs')
        obj = self.tool(fs, 'mkfs.ext4')
        self.assertEqual(obj.path, '/usr/sbin/mke2fs')
        obj.close()

    def test_every_ancestor_owner_and_write_bits(self):
        baseline = Filesystem()
        ancestors = [p for p, m in baseline.nodes.items() if run.stat.S_ISDIR(m.st_mode) and p != '/usr/sbin']
        for path in ancestors:
            for mutation in ('uid', 0o020, 0o002, 0o1002):
                with self.subTest(path=path, mutation=mutation):
                    fs = Filesystem()
                    if mutation == 'uid':
                        fs.nodes[path].st_uid = 1001
                    else:
                        fs.nodes[path].st_mode |= mutation
                    self.refused(fs)

    def test_target_predicates_and_opened_metadata(self):
        for attr, value in (('st_uid', 1001), ('st_mode', 0o104775), ('st_mode', 0o104757),
                            ('st_mode', 0o100755), ('st_mode', 0o104655),
                            ('st_mode', 0o44755), ('st_nlink', 2)):
            fs = Filesystem()
            setattr(fs.nodes[run.SUDO_TARGET], attr, value)
            facts = self.refused(fs)
            self.assertEqual(facts['uid'], fs.nodes[run.SUDO_TARGET].st_uid)
        fs = Filesystem()
        original = fs.fstat
        def fstat(fd):
            m = original(fd)
            if fs.fds[fd][0] == run.SUDO_TARGET:
                return SimpleNamespace(**{**vars(m), 'st_uid': 1001})
            return m
        fs.fstat = fstat
        self.assertEqual(self.refused(fs)['predicate'], 'owner_uid_root')

    def test_exact_location_has_no_prefix_or_role_waiver(self):
        for path in ('/usr/lib/cargo/bin/sudo-extra', '/usr/lib/cargo/bin/other',
                     '/usr/lib/cargo/bin/sub/sudo', '/usr/lib/cargo/bin2/sudo',
                     '/opt/private-sentinel/sudo'):
            fs = Filesystem()
            parts = path.split('/')[1:]
            for i in range(1, len(parts)):
                parent = '/' + '/'.join(parts[:i])
                if parent not in fs.nodes:
                    fs.add(parent, 0o40755)
            fs.add(path, 0o104755)
            fs.link('/etc/alternatives/sudo', path)
            self.assertEqual(self.refused(fs)['predicate'], 'canonical_system_directory')
        for role in ('docker', 'python3'):
            fs = Filesystem()
            fs.link('/usr/bin/' + role, run.SUDO_TARGET)
            self.refused(fs, role)

    def test_links_bounds_errors_and_erased_unsafe_traversal(self):
        for target in ('/private-sentinel/missing', '/usr/bin/sudo', '/' + 'x'*4097):
            fs = Filesystem()
            fs.link('/etc/alternatives/sudo', target)
            self.refused(fs)
        fs = Filesystem()
        fs.add('/usr/bin/unsafe', 0o40777)
        fs.link('/usr/bin/sudo', 'unsafe/../sudo-real')
        fs.add('/usr/bin/sudo-real', 0o104755)
        self.assertEqual(self.refused(fs)['predicate'], 'not_group_or_other_writable')
        fs = Filesystem()
        fs.nodes['/etc'].st_mode = 0o100755
        self.assertEqual(self.refused(fs)['predicate'], 'directory_component')
        for method in ('open', 'fstat', 'stat', 'readlink'):
            fs = Filesystem()
            with patch.object(fs, method, side_effect=PermissionError('private-sentinel')):
                facts = self.refused(fs)
                self.assertEqual(facts['predicate'], 'filesystem_error')
                self.assertIsNone(facts['uid'])

    def test_replacement_at_lookup_open_and_before_use(self):
        fs = Filesystem()
        with patch.object(run.shutil, 'which', return_value='/usr/bin/sudo'):
            fs.nodes[run.SUDO_TARGET].st_uid = 1001
            with self.assertRaises(run.Refusal):
                run.trusted_tool('sudo', lambda *a, **k: None, fs)
        for kind in ('path', 'fd', 'link'):
            fs = Filesystem()
            records = []
            obj = self.tool(fs, records=records)
            if kind == 'path':
                fs.nodes[run.SUDO_TARGET] = SimpleNamespace(**{**vars(fs.nodes[run.SUDO_TARGET]), 'st_ino': 999})
            elif kind == 'fd':
                fs.nodes[run.SUDO_TARGET].st_mode |= 0o020
            else:
                fs.link('/etc/alternatives/sudo', '/usr/bin/git')
            with self.assertRaisesRegex(run.Refusal, 'ToolOwnership'):
                obj.recheck()
            self.assertEqual(sum(r['event'] == 'tool-refusal' for r in records), 1)
            obj.close()
            self.assertFalse(fs.fds)

    def test_fixed_lookup_and_captured_absolute_spawn(self):
        fs = Filesystem()
        with patch.dict(run.os.environ, {'PATH': '/private-sentinel'}), patch.object(
                run.shutil, 'which', return_value='/usr/bin/sudo') as which:
            obj = run.trusted_tool('sudo', lambda *a, **k: None, fs)
            which.assert_called_once_with('sudo', path=run.SEARCH)
            obj.close()
        clock = Clock()
        captured = []
        def spawn(argv, deadline, _clock):
            captured.append(argv)
            return FakeChild(clock, terminal_at=0)
        h = run.Harness(clock, spawn)
        h.tools = {role: self.tool(fs, role) for role in run.TOOLS}
        for role in ('mkfs.ext4', 'losetup', 'mount', 'umount', 'findmnt', 'mkdir', 'chown'):
            h.command(['sudo', '-n', role, 'fixed-arg'], 'prepare')
            self.assertEqual(captured[-1], [run.SUDO_TARGET, '-n', '/usr/bin/' + role, 'fixed-arg'])
        for role in ('docker', 'readelf', 'git', 'python3'):
            h.command([role, 'fixed-arg'], 'preflight')
            self.assertEqual(captured[-1], ['/usr/bin/' + role, 'fixed-arg'])
        args = run.exec_argv(CONTAINER, 'release')
        h.command(args, 'probe')
        self.assertEqual(captured[-1], ['/usr/bin/docker'] + args[1:])
        for obj in h.tools.values():
            obj.close()
        self.assertFalse(fs.fds)

    def test_refusal_latches_before_image_and_closes_tools(self):
        fs = Filesystem()
        fs.nodes['/usr'].st_mode |= 0o020
        h = run.Harness(spawn=lambda *args: self.fail('spawned'))
        def qualify():
            h.tools = {}
            h.tools['sudo'] = self.tool(fs)
        with patch.object(h, 'qualify', side_effect=qualify), redirect_stdout(io.StringIO()):
            self.assertEqual(h.run(), 1)
        self.assertEqual(h.first, 'ToolOwnership')
        self.assertFalse(fs.fds)


class AmendmentTests(unittest.TestCase):
    tool = ToolTests.tool
    refused = ToolTests.refused
    def multicall(self):
        fs = Filesystem()
        fs.add('/usr/lib/cargo/bin/coreutils', 0o40755)
        for role, path in run.MULTICALL_TARGETS.items():
            fs.add(path, 0o100755)
            fs.nodes[path].st_nlink = 116
            fs.link('/usr/bin/' + role, path)
        return fs

    def accepted(self, fs, role):
        return run.trusted_tool(role, lambda *a, **k: None, fs, '/usr/bin/' + role, True)

    def test_exact_map_dispatch_and_link_counts(self):
        self.assertEqual(run.MULTICALL_TARGETS, {
            'mkdir': '/usr/lib/cargo/bin/coreutils/mkdir',
            'chown': '/usr/lib/cargo/bin/coreutils/chown'})
        for links in (1, 2, 116):
            fs = self.multicall()
            h = run.Harness()
            h.tools = {'sudo': self.tool(fs)}
            for role, path in run.MULTICALL_TARGETS.items():
                fs.nodes[path].st_nlink = links
                h.tools[role] = self.accepted(fs, role)
            for role, args in (('mkdir', ['-m', '0700', '/fixture/fs/founder-wrap']),
                               ('chown', ['21001:21001', '/fixture/fs/founder-wrap'])):
                self.assertEqual(h.bound_argv(['sudo', '-n', role] + args),
                                 [run.SUDO_TARGET, '-n', run.MULTICALL_TARGETS[role]] + args)
            for obj in h.tools.values():
                obj.close()
            self.assertFalse(fs.fds)
        for role in ('mount', 'umount'):
            fs = Filesystem()
            fs.nodes['/usr/bin/' + role].st_mode |= run.stat.S_ISUID
            self.tool(fs, role).close()

    def test_multicall_conjunctive_predicates_and_ancestors(self):
        for role, path in run.MULTICALL_TARGETS.items():
            for attr, value in (('st_uid', 1), ('st_mode', 0o100775), ('st_mode', 0o100757),
                                ('st_mode', 0o40755), ('st_mode', 0o100655),
                                ('st_mode', 0o104755), ('st_mode', 0o102755),
                                ('st_nlink', 0), ('st_nlink', -1)):
                fs = self.multicall()
                setattr(fs.nodes[path], attr, value)
                with self.subTest(role=role, attr=attr, value=value), self.assertRaises(run.Refusal):
                    self.accepted(fs, role)
                self.assertFalse(fs.fds)
            for ancestor in ('/', '/usr', '/usr/lib', '/usr/lib/cargo',
                             '/usr/lib/cargo/bin', '/usr/lib/cargo/bin/coreutils', '/usr/bin'):
                for attr, value in (('st_uid', 1), ('st_mode', 0o40775), ('st_mode', 0o40757)):
                    fs = self.multicall()
                    setattr(fs.nodes[ancestor], attr, value)
                    with self.assertRaises(run.Refusal):
                        self.accepted(fs, role)
                    self.assertFalse(fs.fds)
            fs = self.multicall()
            fs.nodes['/usr/bin/' + role].st_uid = 1
            with self.assertRaises(run.Refusal):
                self.accepted(fs, role)
            self.assertFalse(fs.fds)

    def test_role_swaps_lookalikes_and_ordinary_multilink_refuse(self):
        for role in run.TOOLS:
            for target in (*run.MULTICALL_TARGETS.values(),
                           '/usr/lib/cargo/bin/coreutils/mkdir-extra',
                           '/usr/lib/cargo/bin/coreutils2/mkdir',
                           '/usr/lib/cargo/bin/coreutils/sub/mkdir',
                           '/usr/lib/cargo/bin/arbitrary'):
                if run.MULTICALL_TARGETS.get(role) == target:
                    continue
                fs = self.multicall()
                parts = target.split('/')[1:]
                for i in range(1, len(parts)):
                    parent = '/' + '/'.join(parts[:i])
                    if parent not in fs.nodes:
                        fs.add(parent, 0o40755)
                if target not in fs.nodes:
                    fs.add(target, 0o100755)
                fs.link('/usr/bin/' + role, target)
                with self.assertRaises(run.Refusal):
                    self.accepted(fs, role)
                self.assertFalse(fs.fds)
        fs = Filesystem()
        fs.nodes['/usr/bin/git'].st_nlink = 2
        self.refused(fs, 'git')

    def test_bounded_sysctl_failures_and_redaction(self):
        for data in (b'1', b'1\n', b'0\n', b'2\n', b'', b'private-sentinel', b'1\n'*20,
                     b'1 ', b'01', PermissionError('private-sentinel')):
            fs = Filesystem()
            fs.knob = data
            records = []
            record = lambda event, **facts: records.append(dict(event=event, **facts))
            if data in (b'1', b'1\n'):
                run.protected_hardlinks(record, fs)
            else:
                with self.assertRaisesRegex(run.Refusal, 'ToolOwnership'):
                    run.protected_hardlinks(record, fs)
            self.assertFalse(fs.fds)
            self.assertNotIn('private-sentinel', json.dumps(records))
        for mutation in ('missing', 'symlink', 'owner', 'writable'):
            fs = Filesystem()
            if mutation == 'missing':
                del fs.nodes[run.HARDLINKS]
            elif mutation == 'symlink':
                fs.nodes[run.HARDLINKS].st_mode = 0o120777
            elif mutation == 'owner':
                fs.nodes[run.HARDLINKS].st_uid = 1
            else:
                fs.nodes[run.HARDLINKS].st_mode |= 0o002
            with self.assertRaises(run.Refusal):
                run.protected_hardlinks(lambda *a, **k: None, fs)
            self.assertFalse(fs.fds)

    def test_preuse_sysctl_drift_link_drift_and_route_replacement(self):
        for mutation in ('zero', 'error', 'links', 'route', 'opened'):
            fs = self.multicall()
            obj = self.accepted(fs, 'mkdir')
            if mutation == 'zero':
                fs.knob = b'0\n'
            elif mutation == 'error':
                fs.knob = PermissionError('private-sentinel')
            elif mutation == 'links':
                fs.nodes[obj.path].st_nlink = 2
            elif mutation == 'opened':
                fs.nodes[obj.path].st_uid = 1
            else:
                fs.link('/usr/bin/mkdir', run.MULTICALL_TARGETS['chown'])
            with self.assertRaises(run.Refusal):
                obj.recheck()
            # Unrelated cleanup remains usable with a refused knob.
            cleanup = self.tool(fs, 'umount')
            self.assertEqual(cleanup.recheck(), '/usr/bin/umount')
            cleanup.close()
            obj.close()
            self.assertFalse(fs.fds)

    def test_multicall_unsafe_relative_traversal_and_open_metadata(self):
        for mutation in ('relative', 'owner', 'symlink-drift'):
            fs = self.multicall()
            if mutation == 'relative':
                fs.add('/usr/bin/unsafe', 0o40777)
                fs.link('/usr/bin/mkdir', 'unsafe/../../lib/cargo/bin/coreutils/mkdir')
            elif mutation == 'owner':
                original = fs.fstat
                def changed(fd):
                    m = original(fd)
                    return SimpleNamespace(**{**vars(m), 'st_uid': 1}) if fs.fds[fd][0] == run.MULTICALL_TARGETS['mkdir'] else m
                fs.fstat = changed
            else:
                original = fs.stat
                fs.stat = lambda *a, **k: SimpleNamespace(**{**vars(original(*a, **k)), 'st_ino': 9999})
            with self.assertRaises(run.Refusal):
                self.accepted(fs, 'mkdir')
            self.assertFalse(fs.fds)

    def test_success_reaches_pure_suite_and_failure_spawns_nothing(self):
        for succeeds in (True, False):
            fs = self.multicall()
            if not succeeds:
                fs.nodes['/usr/bin/docker'].st_uid = 1
            h = run.Harness(spawn=lambda *a: self.fail('spawned'))
            actual = h.qualify_tools
            with patch.object(h, 'qualify_tools', side_effect=lambda: actual(fs)), \
                 patch.object(h, 'command', side_effect=run.Refusal('PureSuiteReached')) as command, \
                 patch.object(run.shutil, 'which', side_effect=lambda role, **k: '/usr/bin/' + role), \
                 patch.object(run.sys, 'executable', '/usr/bin/python3'), \
                 patch.object(run.sys, 'argv', ['run.py']), patch.object(run.sys, 'platform', 'linux'), \
                 patch.object(run.platform, 'machine', return_value='x86_64'), \
                 patch.dict(run.os.environ, {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted'}), \
                 redirect_stdout(io.StringIO()):
                self.assertEqual(h.run(), 1)
            self.assertEqual(command.call_count, int(succeeds))
            if succeeds:
                self.assertEqual(command.call_args.args[0][1:5], ['-I', '-B', '-m', 'unittest'])
            self.assertFalse(fs.fds)

    def batch(self, fs, clock=None):
        h = run.Harness(clock or run.time.monotonic, spawn=lambda *a: self.fail('spawned'))
        records = []
        h.record = lambda event, **facts: records.append(dict(event=event, **facts))
        with patch.object(run.shutil, 'which', side_effect=lambda role, **k: '/usr/bin/' + role), \
             patch.object(run.sys, 'executable', '/usr/bin/python3'):
            try:
                h.qualify_tools(fs)
            except run.Refusal:
                pass
        return h, records

    def test_batch_multiple_failures_python_sysctl_and_close(self):
        fs = self.multicall()
        fs.knob = b'0\n'
        fs.nodes['/usr/bin/docker'].st_uid = 1
        fs.nodes['/usr/bin/python3'].st_mode |= 0o020
        with patch.object(run.sys, 'version_info', (3, 10, 1)):
            h, records = self.batch(fs)
        self.assertEqual(h.first, 'ToolOwnership')
        self.assertIn('PythonVersion', h.secondary)
        self.assertEqual([r['tool'] for r in records if r['event'] == 'tool-terminal'], list(run.TOOLS))
        self.assertEqual({r['tool'] for r in records if r['event'] == 'tool-refusal'},
                         {'docker', 'mkdir', 'chown', 'python3'})
        self.assertEqual(h.tools, {})
        self.assertFalse(fs.fds)
        self.assertLess(len(json.dumps(records)), run.LIMIT)

    def test_batch_success_timeout_and_partial_open_close(self):
        fs = self.multicall()
        h, records = self.batch(fs)
        self.assertIsNone(h.first)
        self.assertEqual(len(h.tools), 12)
        for obj in h.tools.values():
            obj.close()
        self.assertFalse(fs.fds)
        clock = Clock()
        fs = self.multicall()
        original = fs.open
        def opened(name, flags, dir_fd=None):
            fd = original(name, flags, dir_fd)
            if name == 'docker':
                clock.now = 21
            return fd
        fs.open = opened
        h, records = self.batch(fs, clock)
        self.assertEqual(h.first, 'PhaseTimeout')
        self.assertEqual(sum(r.get('status') == 'not-evaluated' for r in records), 11)
        self.assertFalse(fs.fds)
        fs = self.multicall()
        original = fs.fstat
        def broken(fd):
            if fs.fds[fd][0] == '/usr/bin/docker':
                raise OSError('private-sentinel')
            return original(fd)
        fs.fstat = broken
        h, records = self.batch(fs)
        self.assertEqual(h.first, 'ToolOwnership')
        self.assertFalse(fs.fds)


class LauncherTests(unittest.TestCase):
    def test_rejects_unsafe_roots_and_context(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            run.safe(root)
            with self.assertRaises(run.Refusal):
                run.safe(root, root)
            with self.assertRaises(run.Refusal):
                run.safe(Path('relative'))
            with self.assertRaises(run.Refusal):
                run.safe(root / '..' / root.name)
        for context in ('', 'a'*1025, '../secret', 'f'*255, 'g'*256):
            with self.assertRaises(run.Refusal):
                run.exec_argv(CONTAINER, 'restart-founder', context)
        for target in ((1, [3, 4, 5]), (42, [2, 4, 5]), (42, [3, 4]), (4194305, [3, 4, 5])):
            with self.assertRaises(run.Refusal):
                run.exec_argv(CONTAINER, 'deny-build', target=target)

    def test_requires_one_exact_probe_and_matching_hash(self):
        listing = (run.PROBE + ': test\n1 test, 0 benchmarks\n').encode()
        candidate = ('one', listing, (1, 2, 'hash'), (1, 2, 'hash'))
        self.assertEqual(run.choose([candidate]), 'one')
        for candidates in ([], [candidate, candidate], [('x', b'other: test', 1, 1)], [('x', listing, 1, 2)]):
            with self.assertRaises(run.Refusal):
                run.choose(candidates)

    def test_exact_operational_probe_accepts_real_filtered_counts(self):
        for filtered in (0, 1, 829):
            transcript = (f"running 1 test\ntest {run.PROBE} ... ARX_PASS selected=1 passed=1\n"
                          f"ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out; finished in 0.01s\n").encode()
            with self.subTest(filtered=filtered):
                self.assertEqual(run.terminal(transcript)['filtered'], filtered)
                for bad in (transcript.replace(b'running 1 test', b'running 0 tests'),
                            transcript.replace(b'ok. 1 passed; 0 failed', b'FAILED. 0 passed; 1 failed'),
                            transcript.replace(b'ARX_PASS selected=1 passed=1', b'')):
                    with self.assertRaisesRegex(run.Refusal, 'MissingOracle'):
                        run.terminal(bad)

    def test_fixed_argv_environment_and_limits(self):
        argv = run.create_argv('arx-' + 'c'*16 + '-founder', 'c'*16, '/fixture', 'founder', IMAGE)
        for key, value in (('--shm-size', '1m'), ('--memory', '256m'), ('--cpus', '1'), ('--pids-limit', '64'), ('--network', 'none'), ('--user', '21001:21001')):
            self.assertEqual(argv[argv.index(key)+1], value)
        exec_args = run.exec_argv(CONTAINER, 'deny-build', target=(42, [3, 4, 5]))
        self.assertEqual(exec_args[exec_args.index('/usr/bin/env')+1:][:5], ['-i', 'PATH=/usr/bin:/bin', 'LANG=C', 'TZ=UTC', 'ARX_CUSTODY_HOSTED_MODE=deny-build'])
        self.assertEqual(exec_args[3], '21002:21002')
        for mode in ('shell', '../release', 'hold'):
            with self.assertRaises(run.Refusal):
                run.exec_argv(CONTAINER, mode)

    def test_deadline_reserves_cleanup_and_kills_children(self):
        clock = Clock()
        child = FakeChild(clock)
        harness = run.Harness(clock, lambda *args: child)
        clock.now = 129
        self.assertEqual(harness.budget.deadline('probe', 3), 130)
        with patch.object(run.time, 'sleep', clock.sleep):
            with self.assertRaisesRegex(run.Refusal, 'CommandTimeout'):
                harness.command(['fixed'], 'probe')
        self.assertTrue(child.killed)
        self.assertLess(clock(), 150)
        with self.assertRaisesRegex(run.Refusal, 'WorkTimeout'):
            harness.budget.deadline('probe', 3)

    def test_first_failure_survives_cleanup_failure(self):
        harness = run.Harness()
        harness.fail(run.Refusal('ReadyTimeout'))
        harness.nodes[CONTAINER] = 'founder'
        with patch.object(harness, 'remove_node', side_effect=run.Refusal('UnknownCleanup')):
            harness.cleanup()
        self.assertEqual(harness.first, 'ReadyTimeout')
        self.assertIn('UnknownCleanup', harness.secondary)

    def test_success_requires_cleanup(self):
        harness = run.Harness()
        with patch.object(harness, 'qualify'), patch.object(harness, 'cleanup', side_effect=lambda: harness.fail(run.Refusal('Cleanup'))):
            self.assertEqual(harness.run(), 1)
        self.assertEqual(harness.first, 'Cleanup')

    def test_rejects_unknown_container_mount_loop_ownership(self):
        roots = {'/wrap': '/fixture/wrap', '/store': '/fixture/store'}
        run.inspect_node(inspection(), 'c'*16, IMAGE, roots)
        for field in ('image', 'label', 'source'):
            data = inspection()
            if field == 'image':
                data['Image'] = 'other'
            elif field == 'label':
                data['Config']['Labels'].clear()
            else:
                data['Mounts'][0]['Source'] = '/owner'
            with self.assertRaises(run.Refusal):
                run.inspect_node(data, 'c'*16, IMAGE, roots)
        harness = run.Harness()
        harness.root = Path('/fixture')
        harness.loop = '/dev/loop42'
        with patch.object(harness, 'command', return_value=b'{"loopdevices":[{"name":"/dev/loop42","back-file":"/owner","maj:min":"7:42"}]}'):
            with self.assertRaisesRegex(run.Refusal, 'UnknownCleanup'):
                harness.verify_loop()

    def test_zero_selected_or_missing_oracle_fails(self):
        run.terminal(GOOD)
        for output in (b'', READY, GOOD.replace(b'1 passed;', b'0 passed;'), GOOD.replace(b'selected=1', b'selected=0'), GOOD+GOOD):
            with self.assertRaises(run.Refusal):
                run.terminal(output)

    def test_holder_readiness_lifetime_release_and_deadlines(self):
        clock = Clock()
        holder = FakeChild(clock)
        harness = run.Harness(clock, lambda *args: holder)
        with patch.object(run.time, 'sleep', clock.sleep):
            context, target = harness.start_holder(['fixed'])
            self.assertLess(clock(), 5)
            self.assertEqual(target, (42, [3, 4, 5]))
            self.assertEqual(context, 'a'*256)
            with self.assertRaises(run.Refusal):
                run.terminal(holder.out)
            clock.now = 7
            holder.alive()
            self.assertFalse(holder.reaped)
        for start, ready_at, expected in ((0, 6, 'ReadyTimeout'), (0, 1, 'HolderTimeout'), (129, 131, 'WorkTimeout')):
            clock = Clock()
            harness = run.Harness(clock)
            clock.now = start
            holder = FakeChild(clock, ready_at)
            def spawn(_argv, deadline, _clock):
                holder.deadline = deadline
                return holder
            harness.spawn = spawn
            with patch.object(run.time, 'sleep', clock.sleep):
                if expected == 'HolderTimeout':
                    harness.start_holder(['fixed'])
                    clock.now = 30
                    with self.assertRaisesRegex(run.Refusal, expected):
                        holder.alive()
                else:
                    with self.assertRaisesRegex(run.Refusal, expected):
                        harness.start_holder(['fixed'])
            harness.fail(run.Refusal(expected))
            harness.nodes[CONTAINER] = 'founder'
            events = []
            def remove(*args, **kwargs):
                events.append('remove-container')
                harness.nodes.clear()
            with patch.object(harness, 'remove_node', side_effect=remove):
                harness.cleanup()
            self.assertTrue(holder.killed)
            self.assertEqual(events, ['remove-container'])
            self.assertEqual(harness.first, expected)

    def release_fixture(self, holder_at, release_at, holder_output=GOOD, release_output=GOOD):
        clock = Clock()
        events = []

        class Process(FakeChild):
            def __init__(self, name, at, output):
                super().__init__(clock, terminal_at=at)
                self.name, self.output = name, output

            def finish(self):
                if self.reaped:
                    return True
                if clock() < self.terminal_at:
                    return False
                self.out.extend(self.output)
                self.reaped = True
                events.append(self.name + '-reaped')
                return True

            def kill(self):
                events.append(self.name + '-killed')
                super().kill()

        holder = Process('holder', holder_at, holder_output)
        release = Process('release', release_at, release_output)
        restart = Process('restart', 0, GOOD)

        def spawn(argv, deadline, _clock):
            mode = next(v for v in argv if v.startswith('ARX_CUSTODY_HOSTED_MODE='))
            name = mode.split('=')[1]
            events.append(name + '-spawn')
            if name == 'release':
                return release
            self.assertTrue(holder.reaped)
            self.assertIsNone(harness.holder)
            return restart

        harness = run.Harness(clock, spawn)
        harness.holder = holder
        harness.children.append(holder)
        return harness, clock, events, holder, release

    def test_release_restart_accepts_both_completion_orders(self):
        for holder_at, release_at in ((.02, .05), (.05, .02)):
            with self.subTest(holder_at=holder_at):
                harness, clock, events, holder, release = self.release_fixture(holder_at, release_at)
                with patch.object(run.time, 'sleep', clock.sleep):
                    harness.release_and_restart(CONTAINER, 'founder', 'a'*256)
                self.assertEqual(events, ['release-spawn', 'release-reaped', 'holder-reaped',
                                          'restart-founder-spawn', 'restart-reaped'])
                self.assertTrue(holder.reaped and release.reaped)
                self.assertFalse(harness.releasing)
                self.assertIsNone(harness.first)

    def test_release_restart_rejects_premature_holder_exit(self):
        harness, clock, events, holder, _ = self.release_fixture(0, .05)
        with self.assertRaisesRegex(run.Refusal, 'HolderExit'):
            harness.release_and_restart(CONTAINER, 'founder', 'a'*256)
        self.assertEqual(events, [])
        # Ordinary probe waits must still monitor holder liveness.
        holder.terminal_at = .02
        with patch.object(run.time, 'sleep', clock.sleep):
            with self.assertRaisesRegex(run.Refusal, 'HolderExit'):
                harness.command(['ARX_CUSTODY_HOSTED_MODE=release'], 'probe')
        self.assertEqual(harness.first, 'HolderExit')
        self.assertIn('release-killed', events)

    def test_release_restart_terminal_failure_forbids_restart(self):
        for holder_output, release_output in ((b'', GOOD), (GOOD, b'')):
            with self.subTest(holder_output=holder_output):
                harness, clock, events, _, _ = self.release_fixture(.02, .05, holder_output, release_output)
                with patch.object(run.time, 'sleep', clock.sleep):
                    with self.assertRaisesRegex(run.Refusal, 'MissingOracle'):
                        harness.release_and_restart(CONTAINER, 'founder', 'a'*256)
                self.assertFalse(any('restart' in event for event in events))
                self.assertFalse(harness.releasing)

    def test_release_restart_deadlines_force_cleanup(self):
        for kind in ('holder', 'work', 'command', 'terminal'):
            with self.subTest(kind=kind):
                harness, clock, events, holder, release = self.release_fixture(1000, 1000)
                expected = 'CommandTimeout'
                if kind == 'holder':
                    holder.deadline = .02
                    expected = 'HolderTimeout'
                elif kind == 'work':
                    clock.now = 129.98
                    holder.deadline = 150
                    expected = 'WorkTimeout'
                elif kind == 'terminal':
                    release.terminal_at = .02
                    holder.deadline = .05
                    expected = 'HolderTimeout'
                harness.nodes[CONTAINER] = 'founder'

                def remove(*args, **kwargs):
                    events.append('container-removed')
                    harness.nodes.clear()

                with patch.object(run.time, 'sleep', clock.sleep), patch.object(harness, 'remove_node', side_effect=remove):
                    with patch.object(harness, 'qualify', side_effect=lambda: harness.release_and_restart(CONTAINER, 'founder', 'a'*256)):
                        self.assertEqual(harness.run(), 1)
                self.assertEqual(harness.first, expected)
                self.assertTrue(holder.killed)
                self.assertLess(events.index('container-removed'), events.index('holder-killed'))
                self.assertFalse(any('restart' in event for event in events))
                self.assertLess(clock(), 150)

    def test_command_latches_failure_before_kill_and_preserves_exception(self):
        for cause in ('timeout', 'probe', 'holder'):
            with self.subTest(cause=cause):
                clock = Clock()
                child = FakeChild(clock)
                harness = run.Harness(clock, lambda *args: child)
                original = run.Refusal('ProbeFailure')
                expected = {'timeout': 'CommandTimeout', 'probe': 'ProbeFailure', 'holder': 'HolderExit'}[cause]
                if cause == 'probe':
                    child.finish = lambda: (_ for _ in ()).throw(original)
                if cause == 'holder':
                    harness.holder = FakeChild(clock, terminal_at=0)

                def kill():
                    self.assertEqual(harness.first, expected)
                    raise RuntimeError('synthetic teardown failure')

                child.kill = kill
                with patch.object(run.time, 'sleep', clock.sleep):
                    with self.assertRaisesRegex(run.Refusal, expected) as caught:
                        harness.command(['fixed'], 'probe', seconds=.02)
                if cause == 'probe':
                    self.assertIs(caught.exception, original)
                self.assertEqual(harness.first, expected)
                self.assertEqual(harness.secondary, ['RuntimeError'])

    def test_successful_command_failed_teardown_fails_as_cleanup(self):
        clock = Clock()
        child = FakeChild(clock)
        harness = run.Harness(clock, lambda *args: child)
        # Command has completed, but its resources still require teardown.
        child.finish = lambda: True
        child.kill = lambda: (_ for _ in ()).throw(RuntimeError('synthetic teardown failure'))
        with self.assertRaises(RuntimeError):
            harness.command(['fixed'], 'probe')
        self.assertEqual(harness.first, 'Cleanup')
        self.assertEqual(harness.secondary, ['RuntimeError'])

    def test_invocation_evidence_has_node_mode_counts_and_duration(self):
        for node in ('founder', 'learner1', 'learner2'):
            for mode in ('deny-build', 'same-uid-control', 'release', 'restart-founder', 'restart-learner'):
                with self.subTest(node=node, mode=mode):
                    clock = Clock()
                    child = FakeChild(clock, terminal_at=.03)
                    # libtest prefixes the first nocapture line with the test name.
                    child.out.extend(('test ' + run.PROBE + ' ... ARX_FACTS uid=21001 caps=0 nnp=1 core=0\n').encode())
                    child.out.extend(b'ARX_MOUNT slot=/dev/shm capacity=1048576 used=0\nARX_DENIAL errno=1\n')
                    harness = run.Harness(clock, lambda *args: child)
                    harness.nodes[CONTAINER] = node
                    output = io.StringIO()
                    with redirect_stdout(output), patch.object(run.time, 'sleep', clock.sleep):
                        harness.operational(CONTAINER, mode)
                    records = [json.loads(line) for line in output.getvalue().splitlines()]
                    self.assertEqual([r['event'] for r in records], ['probe', 'probe', 'probe', 'case'])
                    self.assertEqual(records[1]['facts'], 'ARX_MOUNT slot=/dev/shm capacity=1048576 used=0')
                    self.assertEqual(records[2]['facts'], 'ARX_DENIAL errno=1')
                    record = records[-1]
                    self.assertEqual((record['node'], record['mode'], record['result']), (node, mode, 'pass'))
                    self.assertEqual(record['counts'], dict(oracle_selected=1, oracle_passed=1, selected=1,
                                                          passed=1, failed=0, ignored=0, measured=0,
                                                          filtered=829, libtest_ok=True))
                    self.assertAlmostEqual(record['seconds'], .03)

    def test_invocation_evidence_rejects_missing_zero_and_unknown_records(self):
        failed = GOOD.replace(b'ARX_PASS selected=1 passed=1\n', b'').replace(b'ok. 1 passed; 0 failed;', b'FAILED. 0 passed; 1 failed;')
        for transcript in (b'', GOOD.replace(b'running 1 test', b'running 0 tests'),
                           GOOD.replace(b'selected=1', b'selected=0'),
                           GOOD.replace(b'test result:', b'absent result:'),
                           GOOD + b'ARX_UNKNOWN private-sentinel\n',
                           GOOD + b'ARX_PASS selected=1 passed=1\n', failed):
            with self.subTest(transcript=transcript):
                clock = Clock()
                child = FakeChild(clock)
                child.out.extend(transcript)
                def finish():
                    child.reaped = True
                    return True
                child.finish = finish
                harness = run.Harness(clock, lambda *args: child)
                harness.nodes[CONTAINER] = 'founder'
                output = io.StringIO()
                with redirect_stdout(output):
                    with self.assertRaises(run.Refusal):
                        harness.operational(CONTAINER, 'deny-build')
                records = [json.loads(line) for line in output.getvalue().splitlines()]
                self.assertEqual(len(records), 1)
                self.assertEqual(records[0]['result'], 'fail')
                self.assertNotIn('private-sentinel', output.getvalue())
                if transcript == failed:
                    self.assertEqual(records[0]['counts']['passed'], 0)
                    self.assertEqual(records[0]['counts']['failed'], 1)

    def test_nonzero_command_emits_observed_failure_counts(self):
        clock = Clock()
        child = FakeChild(clock)
        child.out.extend(b'running 1 test\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n')
        def finish():
            child.reaped = True
            raise run.Refusal('CommandExit')
        child.finish = finish
        harness = run.Harness(clock, lambda *args: child)
        output = io.StringIO()
        with redirect_stdout(output):
            with self.assertRaisesRegex(run.Refusal, 'CommandExit'):
                harness.operational(CONTAINER, 'deny-build')
        records = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(records[0]['event'], 'command-refusal')
        record = records[-1]
        self.assertEqual(record['result'], 'fail')
        self.assertEqual(record['counts']['selected'], 1)
        self.assertEqual(record['counts']['passed'], 0)
        self.assertEqual(record['counts']['failed'], 1)
        self.assertEqual(harness.first, 'CommandExit')

    def test_holder_and_cleanup_evidence_includes_all_timings(self):
        for node, role in (('founder', 'founder'), ('learner1', 'learner'), ('learner2', 'learner')):
            with self.subTest(node=node):
                clock = Clock()
                holder = FakeChild(clock, ready_at=.01, terminal_at=.075)
                release = FakeChild(clock, terminal_at=.035)
                restart = FakeChild(clock, terminal_at=.095)
                children = iter((holder, release, restart))
                harness = run.Harness(clock, lambda *args: next(children))
                harness.nodes[CONTAINER] = node
                def qualify():
                    harness.start_holder(run.exec_argv(CONTAINER, 'create-' + role), node, 'create-' + role)
                    harness.release_and_restart(CONTAINER, role, 'a'*256)
                def remove(*args, **kwargs):
                    clock.sleep(.03)
                    harness.nodes.clear()
                output = io.StringIO()
                with redirect_stdout(output), patch.object(run.time, 'sleep', clock.sleep), patch.object(harness, 'qualify', side_effect=qualify), patch.object(harness, 'remove_node', side_effect=remove):
                    self.assertEqual(harness.run(), 0)
                records = [json.loads(line) for line in output.getvalue().splitlines()]
                ready = next(r for r in records if r['event'] == 'holder-ready')
                self.assertAlmostEqual(ready['seconds'], .01)
                cases = [r for r in records if r['event'] == 'case']
                self.assertEqual([r['mode'] for r in cases], ['release', 'create-' + role, 'restart-' + role])
                self.assertTrue(all(r['node'] == node and r['result'] == 'pass' for r in cases))
                self.assertTrue(all(r['counts']['selected'] == r['counts']['passed'] == 1 for r in cases))
                self.assertAlmostEqual(cases[1]['lifetime_seconds'], .08)
                self.assertAlmostEqual(cases[1]['reap_seconds'], .04)
                self.assertNotIn('a'*256, output.getvalue())
                cleanup = next(r for r in records if r['event'] == 'cleanup')
                self.assertEqual(cleanup['result'], 'verified')
                self.assertAlmostEqual(cleanup['seconds'], .03)
                result = records[-1]
                self.assertEqual(set(result['phases']), set(run.Budget.CAPS) | {'cleanup'})
                self.assertAlmostEqual(result['phases']['ready'], .01)
                self.assertAlmostEqual(result['phases']['restart'], .09)
                self.assertAlmostEqual(result['phases']['cleanup'], .03)
                self.assertAlmostEqual(result['seconds'], .13)

    def test_failed_holder_and_cleanup_emit_failure_and_duration(self):
        clock = Clock()
        holder = FakeChild(clock, ready_at=6)
        harness = run.Harness(clock, lambda *args: holder)
        def qualify():
            harness.start_holder(['fixed'], 'learner1', 'create-learner')
        def remove(*args, **kwargs):
            clock.sleep(.02)
            raise run.Refusal('UnknownCleanup')
        harness.nodes[CONTAINER] = 'learner1'
        output = io.StringIO()
        with redirect_stdout(output), patch.object(run.time, 'sleep', clock.sleep), patch.object(harness, 'qualify', side_effect=qualify), patch.object(harness, 'remove_node', side_effect=remove):
            self.assertEqual(harness.run(), 1)
        records = [json.loads(line) for line in output.getvalue().splitlines()]
        case = next(r for r in records if r['event'] == 'case')
        self.assertEqual((case['node'], case['mode'], case['result']), ('learner1', 'create-learner', 'fail'))
        self.assertGreaterEqual(case['lifetime_seconds'], 5)
        cleanup = next(r for r in records if r['event'] == 'cleanup')
        self.assertEqual(cleanup['result'], 'unknown')
        self.assertAlmostEqual(cleanup['seconds'], .02)
        self.assertEqual(records[-1]['first_failure'], 'ReadyTimeout')

    def test_shm_and_runtime_mount_accounting(self):
        self.assertEqual((17+1, 18+64, 256-82), (18, 82, 174))
        roots = {'/wrap': '/fixture/wrap', '/store': '/fixture/store'}
        for bad in (None, 0, 64*run.MIB):
            data = inspection()
            data['HostConfig']['ShmSize'] = bad
            with self.assertRaisesRegex(run.Refusal, 'Limits'):
                run.inspect_node(data, 'c'*16, IMAGE, roots)
        data = inspection()
        data['Mounts'].append({'Type': 'volume', 'Destination': '/owner', 'Source': '/private'})
        with self.assertRaisesRegex(run.Refusal, 'MountOwnership'):
            run.inspect_node(data, 'c'*16, IMAGE, roots)




class DiscoveryIntegrationTests(unittest.TestCase):
    def setup_tree(self, root):
        deps = root / 'target/debug/deps'
        deps.mkdir(parents=True)
        binary = deps / 'arxd-test'
        binary.write_bytes(b'\x7fELFpublic-test')
        binary.chmod(0o755)
        context = root / 'context'
        context.mkdir()
        blobs = {}
        for relative in ('tools/ci/custody/Containerfile', 'tools/ci/custody/run.py',
                         'crates/arxd/src/cluster/pki/custody/hosted_probe.rs', '.github/workflows/ci.yml'):
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            blobs[relative] = ('public-source-' + relative).encode()
            path.write_bytes(blobs[relative])
        return deps, binary, context, blobs

    def exercise(self, root, context, blobs, hook=None, listings=None):
        h = run.Harness()
        calls, records = [], []
        h.record = lambda event, **facts: records.append(dict(event=event, **facts))
        def command(argv, phase, *args, **kwargs):
            calls.append(argv)
            if hook:
                hook(argv)
            if argv[0] == 'git':
                return blobs[argv[2].split(':', 1)[1]]
            if argv[0] == 'readelf':
                return b'/lib64/ld-linux-x86-64.so.2 NEEDED GLIBC_2.38'
            if argv[1:] == ['--list', '--exact', run.PROBE]:
                return listings[Path(argv[0]).name] if listings is not None else ((run.PROBE + ': test\n1 test, 0 benchmarks\n').encode(), 0)
            self.fail('unexpected downstream acquisition')
        h.command = command
        token = run.DIAGNOSTIC.set(h.record)
        try:
            with patch.object(run.Path, 'cwd', return_value=root), patch.dict(run.os.environ, {'CARGO_TARGET_DIR': str(root / 'target')}):
                h.discover_and_stage('a'*40, context)
        finally:
            run.DIAGNOSTIC.reset(token)
            self.last_records, self.last_calls = records, calls
        return records

    def test_real_discovery_multilink_source_absolute_git_inputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            deps, binary, context, blobs = self.setup_tree(root)
            os.link(binary, root / 'binary-alias')
            recipe = root / 'tools/ci/custody/Containerfile'
            os.link(recipe, root / 'recipe-alias')
            records = self.exercise(root, context, blobs)
            self.assertEqual((context / 'arxd-tests').read_bytes(), binary.read_bytes())
            self.assertEqual(records[-1]['event'], 'inputs')
            self.assertEqual(set(records[-1]['hashes']), set(blobs))
            self.assertEqual(records[-2]['required_glibc'], ['GLIBC_2.38'])
            run.safe(context / 'arxd-tests', directory=False)
            with self.assertRaises(run.Refusal):
                run.safe(binary, directory=False)

    def test_discovery_pin_drift_refuses_before_readelf_or_image(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            _, _, context, blobs = self.setup_tree(root)
            blobs['tools/ci/custody/Containerfile'] = b'private-sentinel'
            with self.assertRaisesRegex(run.Refusal, 'SourcePin'):
                self.exercise(root, context, blobs)
            self.assertFalse(any(c[0] in ('docker', 'readelf') for c in self.last_calls))
            self.assertNotIn('private-sentinel', json.dumps(self.last_records))
            self.assertEqual(self.last_records[-1]['predicate'], 'immutable_git_blob')

    def test_discovery_aggregates_unsafe_candidates_without_acquisition(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            deps, binary, context, blobs = self.setup_tree(root)
            binary.unlink()
            binary.mkdir()
            (deps / 'arxd-second').mkdir()
            with self.assertRaisesRegex(run.Refusal, 'UnsafePath'):
                self.exercise(root, context, blobs)
            refusals = [r for r in self.last_records if r['event'] == 'refusal']
            self.assertEqual(len(refusals), 2)
            self.assertEqual({r['subject'] for r in refusals}, {'candidate-0', 'candidate-1'})
            self.assertEqual(self.last_calls, [])
            self.assertEqual(list(context.iterdir()), [])

    def test_retained_input_detects_same_bytes_inode_and_hash_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            path = root / 'input'
            path.write_bytes(b'public-original')
            with run.BuildInput(path, root, 'binary', lambda *a, **k: None) as obj:
                original = run.safe
                def replaced(*a, **k):
                    m = original(*a, **k)
                    fields = {key: getattr(m, key) for key in ('st_dev', 'st_ino', 'st_mode', 'st_uid', 'st_gid', 'st_nlink', 'st_size', 'st_mtime_ns', 'st_ctime_ns')}
                    fields['st_ino'] += 1
                    return SimpleNamespace(**fields)
                with patch.object(run, 'safe', side_effect=replaced):
                    with self.assertRaisesRegex(run.Refusal, 'StagingIdentity'):
                        obj.stage(root / 'staged')
                self.assertFalse((root / 'staged').exists())
                path.write_bytes(b'public-modified')
                with self.assertRaisesRegex(run.Refusal, 'StagingIdentity|StagingHash'):
                    obj.stage(root / 'staged')

    def test_safe_predicate_records_redact_routes(self):
        records = []
        token = run.DIAGNOSTIC.set(lambda event, **facts: records.append(dict(event=event, **facts)))
        try:
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp).resolve()
                path = root / 'private-sentinel'
                path.write_bytes(b'private-sentinel')
                for value, parent, directory, predicate in (
                    (Path('private-sentinel'), None, True, 'absolute_bounded_path'),
                    (root / '..' / root.name, None, True, 'canonical_spelling'),
                    (root, root, True, 'strict_descendant'),
                    (path, None, True, 'directory_type'),
                    (root, None, False, 'regular_type')):
                    with self.assertRaises(run.Refusal):
                        run.safe(value, parent, directory)
                    self.assertEqual(records[-1]['predicate'], predicate)
                with patch.object(run.Path, 'is_symlink', return_value=True):
                    with self.assertRaises(run.Refusal):
                        run.safe(path, directory=False)
                self.assertEqual(records[-1]['predicate'], 'no_symlink_component')
                os.link(path, root / 'alias')
                with self.assertRaises(run.Refusal):
                    run.safe(path, directory=False)
                self.assertEqual(records[-1]['predicate'], 'single_link')
        finally:
            run.DIAGNOSTIC.reset(token)
        self.assertNotIn('private-sentinel', json.dumps(records))

    def test_discovery_zero_duplicate_nonelf_and_count_bounds(self):
        for scenario in ('zero', 'duplicate', 'nonelf', 'count'):
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp).resolve()
                deps, binary, context, blobs = self.setup_tree(root)
                expected = 'ProbeSelection'
                if scenario == 'zero':
                    binary.unlink()
                elif scenario == 'nonelf':
                    binary.write_bytes(b'not ELF')
                elif scenario == 'duplicate':
                    second = deps / 'arxd-second'
                    second.write_bytes(binary.read_bytes())
                    second.chmod(0o755)
                else:
                    for i in range(16):
                        p = deps / ('arxd-extra' + str(i))
                        p.write_bytes(b'not ELF')
                        p.chmod(0o755)
                    expected = 'CandidateBounds'
                with self.assertRaisesRegex(run.Refusal, expected):
                    self.exercise(root, context, blobs)
                self.assertFalse(any(c[0] in ('docker', 'readelf') for c in self.last_calls))
                self.assertEqual(list(context.iterdir()), [])

    def test_staging_copy_drift_and_staged_alias_refuse(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            path = root / 'input'
            path.write_bytes(b'public-original')
            with run.BuildInput(path, root, 'binary', lambda *a, **k: None) as obj:
                original = obj.hash
                def changed(output=None):
                    result = original(output)
                    if output is not None:
                        path.write_bytes(b'public-modified')
                    return result
                with patch.object(obj, 'hash', side_effect=changed):
                    with self.assertRaisesRegex(run.Refusal, 'StagingIdentity|StagingHash'):
                        obj.stage(root / 'staged')
            os.link(root / 'staged', root / 'alias')
            with self.assertRaisesRegex(run.Refusal, 'UnsafePath'):
                run.digest(root / 'staged')



    def test_a3_mixed_nonmatches_and_single_eligible_probe(self):
        listing = (run.PROBE + ': test\n1 test, 0 benchmarks\n').encode()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            deps, binary, context, blobs = self.setup_tree(root)
            outputs = {'arxd-test': (listing, 0)}
            for index, (output, code) in enumerate(((b'', 1), (b"private-sentinel GLIBC_2.40' not found", 127), (b'private-sentinel', 2))):
                name = 'arxd-other' + str(index)
                path = deps / name
                path.write_bytes(b'\x7fELFpublic-nonmatch')
                path.chmod(0o755)
                outputs[name] = output, code
            records = self.exercise(root, context, blobs, listings=outputs)
            evidence = [r for r in records if r['event'] == 'candidate-list']
            self.assertEqual(len(evidence), 4)
            self.assertEqual({r['diagnostic'] for r in evidence}, {'empty', 'glibc-missing', 'other'})
            self.assertEqual({r['returncode'] for r in evidence}, {0, 1, 2, 127})
            self.assertEqual({r['candidate'] for r in evidence}, {0, 1, 2, 3})
            self.assertTrue(all(set(r) == {'event', 'candidate', 'returncode', 'seconds', 'diagnostic'} for r in evidence))
            self.assertNotIn('private-sentinel', json.dumps(records))
            self.assertEqual(records[-1]['event'], 'inputs')

    def test_a3_nonzero_probe_record_conflict_zero_and_multiple(self):
        listing = (run.PROBE + ': test\n1 test, 0 benchmarks\n').encode()
        for scenario in ('conflict', 'extra-record-conflict', 'zero', 'multiple'):
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp).resolve()
                deps, binary, context, blobs = self.setup_tree(root)
                outputs = {'arxd-test': (listing, 0)}
                if scenario == 'zero':
                    outputs['arxd-test'] = b'', 1
                else:
                    other = deps / 'arxd-other'
                    other.write_bytes(binary.read_bytes())
                    other.chmod(0o755)
                    outputs['arxd-other'] = (listing if scenario != 'extra-record-conflict' else b'other: test\n' + listing), (0 if scenario == 'multiple' else 1)
                with self.assertRaisesRegex(run.Refusal, 'ProbeSelection'):
                    self.exercise(root, context, blobs, listings=outputs)
                self.assertEqual(list(context.iterdir()), [])
                self.assertFalse(any(c[0] in ('docker', 'readelf', 'git') for c in self.last_calls))

    def test_a3_identity_and_hash_checked_after_nonzero(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            deps, binary, context, blobs = self.setup_tree(root)
            def drift(argv):
                if argv[1:] == ['--list', '--exact', run.PROBE]:
                    binary.write_bytes(b'\x7fELFpublic-drift')
            with self.assertRaisesRegex(run.Refusal, 'StagingIdentity|StagingHash'):
                self.exercise(root, context, blobs, hook=drift, listings={'arxd-test': (b'', 1)})
            self.assertFalse(any(r['event'] == 'candidate-list' for r in self.last_records))
            self.assertEqual(list(context.iterdir()), [])


class A3ProcessTests(unittest.TestCase):
    def child(self, clock, code=1, output=b'', exit_at=0, drain_at=0, read_error=None, reap_error=None):
        child = run.Child.__new__(run.Child)
        child.clock, child.deadline = clock, 2
        child.out, child.reaped = bytearray(output), False
        child.killed = False
        waits = []
        def poll():
            return code if clock() >= exit_at else None
        def wait(timeout):
            waits.append(timeout)
            if reap_error:
                raise reap_error
            return code
        child.p = SimpleNamespace(poll=poll, wait=wait, returncode=code)
        child.selector = SimpleNamespace(get_map=lambda: {1: True} if clock() < drain_at else {},
                                         close=lambda: None)
        def pump():
            if read_error:
                raise read_error
        child.pump = pump
        def kill():
            child.killed = child.reaped = True
        child.kill = kill
        return child, waits

    def listing(self, harness):
        return harness.command([str(Path('D:/fixed/arxd-test') if os.name == 'nt' else Path('/fixed/arxd-test')), '--list', '--exact', run.PROBE],
                               'preflight', 2, discovery=True)

    def test_a3_positive_exit_requires_completion_drain_and_reap(self):
        clock = Clock()
        child, waits = self.child(clock, code=2, output=b'private-sentinel', exit_at=.02, drain_at=.04)
        h = run.Harness(clock, lambda *a: child)
        with patch.object(run.time, 'sleep', clock.sleep):
            output, code = self.listing(h)
        self.assertEqual((output, code), (b'private-sentinel', 2))
        self.assertGreaterEqual(clock(), .04)
        self.assertEqual(waits, [0])
        self.assertTrue(child.reaped)
        self.assertIsNone(h.first)

    def test_a3_signal_timeout_overflow_io_reap_and_spawn_are_fatal(self):
        cases = (
            ('signal', dict(code=-9), 'CommandExit'),
            ('timeout-running', dict(exit_at=3), 'CommandTimeout'),
            ('timeout-undrained', dict(drain_at=3), 'CommandTimeout'),
            ('overflow', dict(read_error=run.Refusal('PipeOverflow')), 'PipeOverflow'),
            ('read', dict(read_error=OSError('private-sentinel')), 'OSError'),
            ('reap', dict(reap_error=OSError('private-sentinel')), 'OSError'),
            ('spawn', {}, 'OSError'))
        for label, options, expected in cases:
            with self.subTest(label=label):
                clock = Clock()
                child, _ = self.child(clock, **options)
                def spawn(*a):
                    if label == 'spawn':
                        raise OSError('private-sentinel')
                    return child
                h = run.Harness(clock, spawn)
                output = io.StringIO()
                with redirect_stdout(output), patch.object(run.time, 'sleep', clock.sleep):
                    with self.assertRaises((run.Refusal, OSError)):
                        self.listing(h)
                self.assertEqual(h.first, expected)
                self.assertNotIn('private-sentinel', output.getvalue())
                if label != 'spawn':
                    self.assertTrue(child.reaped)

    def test_a3_normal_commands_still_latch_nonzero_and_preserve_prior_failure(self):
        for existing in (None, 'SourcePin'):
            clock = Clock()
            child, _ = self.child(clock, code=1)
            h = run.Harness(clock, lambda *a: child)
            h.first = existing
            with redirect_stdout(io.StringIO()):
                with self.assertRaisesRegex(run.Refusal, 'CommandExit'):
                    h.command(['fixed'], 'probe')
            self.assertEqual(h.first, existing or 'CommandExit')
            self.assertEqual(h.secondary, ['CommandExit'] if existing else [])
            self.assertTrue(child.reaped)
        clock = Clock()
        child, _ = self.child(clock, code=1)
        h = run.Harness(clock, lambda *a: child)
        h.first = 'SourcePin'
        self.listing(h)
        self.assertEqual(h.first, 'SourcePin')

    def test_a3_scope_cannot_cover_other_commands(self):
        h = run.Harness(Clock(), lambda *a: self.fail('must not spawn'))
        for argv, phase, seconds, cleanup in ((['docker', '--list', '--exact', run.PROBE], 'preflight', 2, False),
                                              (['/fixed/arxd', '--list', '--exact', run.PROBE], 'image', 2, False),
                                              (['/fixed/arxd', '--list', '--exact', run.PROBE], 'preflight', 3, False),
                                              (['/fixed/arxd', '--list', '--exact', run.PROBE], 'preflight', 2, True)):
            with self.subTest(argv=argv, phase=phase, seconds=seconds, cleanup=cleanup):
                with self.assertRaises(run.Refusal):
                    h.command(argv, phase, seconds, cleanup, discovery=True)



    def test_a3_failure_cleanup_and_first_error_preserved(self):
        clock = Clock()
        child, _ = self.child(clock, code=-9)
        h = run.Harness(clock, lambda *a: child)
        h.qualify = lambda: self.listing(h)
        cleaned = []
        h.cleanup = lambda: cleaned.append((h.first, child.reaped))
        with redirect_stdout(io.StringIO()):
            self.assertEqual(h.run(), 1)
        self.assertEqual(cleaned, [('CommandExit', True)])
        self.assertEqual(h.first, 'CommandExit')

    def test_evidence_overflow_has_bounded_sanitized_refusal(self):
        h = run.Harness()
        output = io.StringIO()
        with redirect_stdout(output):
            with self.assertRaisesRegex(run.Refusal, 'EvidenceOverflow'):
                h.record('synthetic', payload='private-sentinel' * run.LIMIT)
        records = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(len(records), 1)
        self.assertEqual(records[0]['code'], 'EvidenceOverflow')
        self.assertNotIn('private-sentinel', output.getvalue())
        self.assertLessEqual(h.evidence, run.LIMIT)

    def test_synchronous_discovery_time_counts_against_preflight(self):
        clock = Clock()
        h = run.Harness(clock)
        h.budget.used['preflight'] = 1
        def discovery(source, context, checkpoint):
            clock.sleep(2)
            checkpoint()
            self.assertAlmostEqual(h.budget.used['preflight'], 3)
            clock.sleep(18)
            checkpoint()
        h._discover_and_stage = discovery
        with self.assertRaisesRegex(run.Refusal, 'PhaseTimeout'):
            h.discover_and_stage('a'*40, Path('unused'))
        self.assertAlmostEqual(h.budget.used['preflight'], 21)


if __name__ == '__main__':
    unittest.main()
