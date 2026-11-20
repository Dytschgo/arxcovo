"""Pure fault-driven launcher tests: no Docker, sudo, network or fixture provisioning."""
import importlib.util
import io
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

    def test_tool_ownership_reports_each_predicate_without_weakening(self):
        for uid, mode, path, unsafe, predicate in (
            (1001, 0o100755, '/usr/bin/docker', False, 'owner_uid_root'),
            (0, 0o100775, '/usr/bin/docker', False, 'not_group_or_other_writable'),
            (0, 0o100755, '/usr/local/bin/docker', False, 'canonical_system_directory'),
            (0, 0o100755, '/usr/bin/docker', True, 'safe_regular_single_link_path'),
            (1001, 0o100755, '/private/account-token/docker', False, 'owner_uid_root'),
        ):
            metadata = SimpleNamespace(st_uid=uid, st_gid=123, st_mode=mode)
            records = []
            with self.subTest(predicate=predicate, path=path), patch.object(run.shutil, 'which', return_value=path), patch.object(run.Path, 'resolve', side_effect=lambda: run.Path(path)), patch.object(run.Path, 'lstat', return_value=metadata), patch.object(run, 'safe', return_value=metadata, side_effect=run.Refusal('UnsafePath') if unsafe else None):
                with self.assertRaisesRegex(run.Refusal, 'ToolOwnership'):
                    run.trusted_tool('docker', lambda event, **fields: records.append(dict(event=event, **fields)))
            self.assertEqual(len(records), 1)
            self.assertEqual(records[0], dict(event='tool-refusal', tool='docker',
                             resolved_path=path if path.startswith('/usr/') else '<outside-system-path>',
                             uid=uid, gid=123, mode=format(mode & 0o7777, '04o'), predicate=predicate))
            self.assertNotIn('account-token', json.dumps(records))
        with patch.object(run.shutil, 'which', return_value='/usr/bin/docker'), patch.object(run.Path, 'resolve', return_value=run.Path('/usr/bin/docker')), patch.object(run.Path, 'lstat', return_value=SimpleNamespace(st_uid=0, st_gid=0, st_mode=0o100755)), patch.object(run, 'safe', return_value=SimpleNamespace(st_uid=0, st_gid=0, st_mode=0o100755)):
            self.assertEqual(run.trusted_tool('docker', lambda *args, **kwargs: self.fail('unexpected refusal')), run.Path('/usr/bin/docker'))

    def test_tool_lookup_ignores_inherited_path_shadow(self):
        metadata = SimpleNamespace(st_uid=0, st_gid=0, st_mode=0o104755)
        with patch.dict(run.os.environ, {'PATH': '/private/account-token'}), patch.object(run.shutil, 'which', return_value='/usr/bin/sudo') as lookup, patch.object(run.Path, 'resolve', return_value=run.Path('/usr/bin/sudo')), patch.object(run.Path, 'lstat', return_value=metadata), patch.object(run, 'safe', return_value=metadata):
            self.assertEqual(run.trusted_tool('sudo', lambda *args, **kwargs: self.fail('unexpected refusal')), run.Path('/usr/bin/sudo'))
            lookup.assert_called_once_with('sudo', path='/usr/bin:/usr/sbin:/bin:/sbin')

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
        record = json.loads(output.getvalue())
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


if __name__ == '__main__':
    unittest.main()
