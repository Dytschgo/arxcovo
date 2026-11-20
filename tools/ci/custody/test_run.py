"""Pure fault-driven launcher tests: no Docker, sudo, network or fixture provisioning."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('custody_launcher', Path(__file__).with_name('run.py'))
run = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(run)
IMAGE = 'sha256:' + 'a' * 64
CONTAINER = 'b' * 64
GOOD = b'running 1 test\nARX_PASS selected=1 passed=1\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n'
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
