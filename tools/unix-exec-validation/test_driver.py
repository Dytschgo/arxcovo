"""Pure tests: never invoke provisioning, sudo, systemd, accounts or cgroups."""
import ast
import io
import threading
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import unittest
import tempfile
import json
import stat
from types import SimpleNamespace
from contextlib import ExitStack
from unittest.mock import patch

import driver

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / '.github/workflows/unix-execution-experiment.yml'


def valid_env():
    return dict(CI='true', GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted',
                RUNNER_OS='Linux', GITHUB_EVENT_NAME='workflow_dispatch',
                GITHUB_REPOSITORY='Dytschgo/arxcovo', GITHUB_ACTOR='Dytschgo',
                GITHUB_TRIGGERING_ACTOR='Dytschgo', GITHUB_REF='refs/heads/main',
                GITHUB_SHA='a' * 40, ARX_REVIEWED_SHA='a' * 40,
                ARX_WORKFLOW_SHA='a' * 40, GITHUB_RUN_ID='123', GITHUB_RUN_ATTEMPT='1')


def workflow_contract(text):
    required = ["github.event_name == 'workflow_dispatch'", "github.repository == 'Dytschgo/arxcovo'",
                "github.actor == 'Dytschgo'", "github.triggering_actor == 'Dytschgo'",
                "github.ref == 'refs/heads/main'", "vars.UNIX_EXECUTION_REVIEWED_SHA != ''",
                'github.sha == vars.UNIX_EXECUTION_REVIEWED_SHA',
                'github.workflow_sha == vars.UNIX_EXECUTION_REVIEWED_SHA',
                'runs-on: ubuntu-24.04', 'timeout-minutes: 10', 'persist-credentials: false',
                'ref: ${{ github.sha }}', 'contents: read', 'sudo -n /usr/bin/env -i',
                'toolchain: 1.95.0',
                'actions/checkout@11d5960a326750d5838078e36cf38b85af677262',
                'dtolnay/rust-toolchain@683b588c487a69d01f6c6ddf40f036ff682cc2c8']
    return all(token in text for token in required) and all(
        token not in text for token in ('pull_request:', '\n  push:', 'inputs:',
                                       'secrets.', 'id-token:', 'self-hosted'))


class Guards(unittest.TestCase):
    def test_allowed_plan(self):
        run = driver.gate(valid_env(), 'linux')
        self.assertEqual(driver.plan(run), (Path('/var/lib/arx-ci-123-1'), 'arxd.service'))

    def test_every_gate_missing_or_changed_refuses(self):
        for key in valid_env():
            for value in ('', 'wrong'):
                with self.subTest(key=key, value=value):
                    env = valid_env()
                    env[key] = value
                    with self.assertRaises(RuntimeError):
                        driver.gate(env, 'linux')

    def test_wrong_host_event_actor_and_hashes(self):
        for key, value in [('RUNNER_ENVIRONMENT', 'self-hosted'),
                           ('GITHUB_EVENT_NAME', 'pull_request'),
                           ('GITHUB_REF', 'refs/heads/feature'),
                           ('GITHUB_ACTOR', 'outsider'),
                           ('GITHUB_TRIGGERING_ACTOR', 'outsider'),
                           ('GITHUB_REPOSITORY', 'outsider/arxcovo'),
                           ('GITHUB_SHA', 'b' * 40), ('ARX_WORKFLOW_SHA', 'b' * 40),
                           ('ARX_REVIEWED_SHA', 'A' * 40)]:
            env = valid_env()
            env[key] = value
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                driver.gate(env, 'linux')
        for platform in ('win32', 'darwin'):
            with self.assertRaisesRegex(RuntimeError, 'E_PLATFORM'):
                driver.gate(valid_env(), platform)

    def test_numeric_names_only(self):
        for value in ('0', '-1', '../1', '1/2', '1\n', '1;id', '1' * 21, '01'):
            env = valid_env()
            env['GITHUB_RUN_ID'] = value
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                driver.gate(env, 'linux')
        for value in ('../123-1', '123-0', '123-1.service', '123-1\n'):
            with self.assertRaises(RuntimeError):
                driver.plan(value)

    def test_refusal_before_any_runtime_call(self):
        # Poison runtime entry points; wrong context must never reach them.
        with patch.object(driver.sys, 'platform', 'linux'), patch.object(driver.os, 'getuid', create=True, side_effect=AssertionError('uid probe')), patch.object(driver, 'cg_preflight', side_effect=AssertionError('probe')):
            with patch.object(driver.subprocess, 'run', side_effect=AssertionError('mutation')):
                with self.assertRaisesRegex(RuntimeError, "E_PLATFORM|E_GATE_"):
                    driver.run({})

    def test_cli_wrong_platform_refuses(self):
        if sys.platform != 'linux':
            result = subprocess.run([sys.executable, '-I', str(ROOT / 'tools/unix-exec-validation/driver.py'), 'run'],
                                    env=os.environ.copy(), capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn('E_PLATFORM', result.stderr)

    def test_exact_command_and_clean_environment(self):
        stage, _ = driver.plan('123-1')
        self.assertEqual(driver.command(stage)[1:],
                         ['test', '-p', 'arxd', '--locked', '--lib', driver.TEST])
        env = driver.cargo_env(stage)
        self.assertEqual(set(env), {'PATH', 'LANG', 'HOME', 'CARGO_HOME', 'CARGO_NET_OFFLINE', 'RUSTC', 'RUSTDOC'})
        self.assertEqual(env['CARGO_NET_OFFLINE'], 'true')

    def test_workflow_and_guard_removal_mutations(self):
        text = WORKFLOW.read_text()
        self.assertTrue(workflow_contract(text))
        for token in ("github.actor == 'Dytschgo'", "github.triggering_actor == 'Dytschgo'",
                      "github.event_name == 'workflow_dispatch'", "github.ref == 'refs/heads/main'",
                      "vars.UNIX_EXECUTION_REVIEWED_SHA != ''",
                      'github.sha == vars.UNIX_EXECUTION_REVIEWED_SHA',
                      'github.workflow_sha == vars.UNIX_EXECUTION_REVIEWED_SHA',
                      'persist-credentials: false', 'runs-on: ubuntu-24.04'):
            with self.subTest(token=token):
                self.assertFalse(workflow_contract(text.replace(token, 'REMOVED')))
    def test_executable_gate_mutations_are_detected(self):
        source = (ROOT / 'tools/unix-exec-validation/driver.py').read_text()
        # Delete each expected-value guard in the actual function AST, then
        # show the bad context accepted by that mutant is refused by production.
        for key in ('CI', 'GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS',
                    'GITHUB_EVENT_NAME', 'GITHUB_REPOSITORY', 'GITHUB_ACTOR',
                    'GITHUB_TRIGGERING_ACTOR', 'GITHUB_REF'):
            tree = ast.parse(source)
            gate = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'gate')
            assignment = gate.body[1]
            index = next(i for i, k in enumerate(assignment.value.keys) if k.value == key)
            del assignment.value.keys[index]
            del assignment.value.values[index]
            namespace = {'__name__': 'mutant'}
            exec(compile(tree, '<guard mutant>', 'exec'), namespace)
            env = valid_env()
            env[key] = 'untrusted'
            self.assertEqual(namespace['gate'](env, 'linux'), '123-1')
            with self.assertRaises(RuntimeError):
                driver.gate(env, 'linux')

    def test_time_bounds_and_finally_are_present(self):
        source = (ROOT / 'tools/unix-exec-validation/driver.py').read_text()
        tree = ast.parse(source)
        run = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'run_body')
        self.assertTrue(any(isinstance(n, ast.Try) and n.finalbody for n in ast.walk(run)))
        self.assertIn('deadline = time.monotonic() + 120', source)
        self.assertIn('RuntimeMaxSec=140', source)



class RuntimeGuards(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.stage = Path(self.temp.name)
        self.base = self.stage / 'fixture'
        self.base.mkdir()
        self.proc = self.stage / 'proc'
        self.proc.mkdir()
        self.account = SimpleNamespace(pw_uid=1234, pw_gid=1234, pw_shell='/usr/sbin/nologin')
        self.group = SimpleNamespace(gr_gid=1234, gr_mem=[])
        (self.stage / 'harness-pid').write_text('123')
        (self.stage / 'fixture-identity').write_text(json.dumps({'uid':1234, 'gid':1234}))
        st = self.base.stat()
        (self.stage / 'fixture-directory').write_text(json.dumps([st.st_dev, st.st_ino]))
        stack = self.enterContext(ExitStack())
        stack.enter_context(patch.dict(sys.modules, {
            'pwd': SimpleNamespace(getpwnam=lambda _: self.account),
            'grp': SimpleNamespace(getgrnam=lambda _: self.group)}))
        real_path = Path
        def path(value):
            return self.base.parent if str(value) == '/var/lib' else self.proc if str(value) == '/proc' else real_path(value)
        # /var/lib/arxexec123 maps only to this test's temporary directory.
        self.base.rename(self.stage / 'arxexec123')
        self.base = self.stage / 'arxexec123'
        stack.enter_context(patch.object(driver, 'Path', side_effect=path))
        stack.enter_context(patch.object(driver, 'protected'))
        self.calls = stack.enter_context(patch.object(driver, 'call'))
        self.remove = stack.enter_context(patch.object(driver.shutil, 'rmtree'))
        self.remove.avoids_symlink_attacks = True

    def refuse_cleanup(self, code):
        with self.assertRaisesRegex(RuntimeError, code):
            driver.fixture_cleanup(self.stage)
        self.calls.assert_not_called()
        self.remove.assert_not_called()

    def test_cleanup_positive(self):
        driver.fixture_cleanup(self.stage)
        self.assertEqual(self.calls.call_args_list[0].args[0], ['/usr/sbin/userdel', 'arxexec123'])
        self.remove.assert_called_once_with(self.base)
        self.assertEqual(self.calls.call_args_list[1].args[0], ['/usr/sbin/groupdel', 'arxexec123'])

    def test_fixture_check_mode_scans_captured_uid_and_gid(self):
        unit = 'arx-unit.service'
        with patch.object(driver.sys, 'platform', 'linux'), \
             patch.object(driver.sys, 'argv', ['driver.py', 'fixture-check', '123-1']), \
             patch.object(driver.os, 'getuid', return_value=0, create=True), \
             patch.object(driver.os, 'geteuid', return_value=0, create=True), \
             patch.object(driver, '__file__', str(self.stage / 'source/tools/unix-exec-validation/driver.py'), create=True), \
             patch.object(driver, 'plan', return_value=(self.stage, unit)), \
             patch.object(driver, 'capture_fixture') as capture, \
             patch.object(driver, 'cg_scan') as scan:
            driver.main()
            capture.assert_called_once_with(self.stage)
            scan.assert_called_once_with(Path('/sys/fs/cgroup/system.slice') / unit, 1234, 1234)

    def test_inode_replacement_quarantines_before_deletion(self):
        (self.stage / 'fixture-directory').write_text('[0, 0]')
        self.refuse_cleanup('E_FIXTURE_INODE')

    def test_live_uid_quarantines_before_deletion(self):
        (self.proc / '456').mkdir()
        original = Path.stat
        def stat_proc(path, *a, **kw):
            if path == self.proc / '456':
                return SimpleNamespace(st_uid=1234)
            return original(path, *a, **kw)
        with patch.object(Path, 'stat', stat_proc):
            self.refuse_cleanup('E_UID_LIVE')

    def test_identity_and_group_guards(self):
        for attr, value in [('pw_uid', 0), ('pw_gid', 999), ('pw_shell', '/bin/sh')]:
            old = getattr(self.account, attr)
            setattr(self.account, attr, value)
            self.refuse_cleanup('E_FIXTURE_UID')
            setattr(self.account, attr, old)
        self.group.gr_mem = ['member']
        self.refuse_cleanup('E_FIXTURE_UID')

    def test_missing_captures_quarantine(self):
        for name, code in [('fixture-directory', 'E_QUARANTINE_PATH_UNCAPTURED'),
                           ('fixture-identity', 'E_QUARANTINE_UID_UNCAPTURED')]:
            p = self.stage / name
            data = p.read_text()
            p.unlink()
            self.refuse_cleanup(code)
            p.write_text(data)

    def test_orphan_group_refuses(self):
        self.base.rmdir()
        self.account = None
        with patch.dict(sys.modules, {'pwd': SimpleNamespace(getpwnam=lambda _: (_ for _ in ()).throw(KeyError()))}):
            self.refuse_cleanup('E_GROUP_ORPHAN')


    def test_fixture_path_protection_failure_is_quarantined(self):
        with patch.object(driver, 'protected', side_effect=RuntimeError('E_PROTECTION')):
            self.refuse_cleanup('E_PROTECTION')



class CgroupScan(unittest.TestCase):
    def scan(self, mode=stat.S_IFREG | 0o644, uid=1001, gid=1001, name='user.slice', unit=None, fixture=None):
        with patch.object(driver.os, 'walk', return_value=[('/sys/fs/cgroup', [], [name])]), patch.object(Path, 'lstat', return_value=SimpleNamespace(st_mode=mode, st_uid=uid, st_gid=gid)):
            driver.cg_scan(unit, *(fixture or (None, None)))

    def test_unrelated_delegation_allowed(self):
        self.scan(unit=Path('/sys/fs/cgroup/system.slice/unit'))

    def test_global_write_and_link_refusal(self):
        for mode in (stat.S_IFREG | 0o664, stat.S_IFREG | 0o646, stat.S_IFLNK | 0o777):
            with self.assertRaisesRegex(RuntimeError, 'E_CGROUP_WRITABLE_HIERARCHY'):
                self.scan(mode=mode)

    def test_unit_ancestry_and_subtree_root_ownership(self):
        unit = Path('/sys/fs/cgroup/system.slice/unit')
        for name in ('system.slice', 'system.slice/unit', 'system.slice/unit/runs/cgroup.procs'):
            with self.assertRaisesRegex(RuntimeError, 'E_CGROUP_UNIT_OWNER'):
                self.scan(name=name, unit=unit)
        self.scan(uid=0, gid=0, name='system.slice/unit/runs', unit=unit)

    def test_fixture_uid_or_gid_anywhere_refuses(self):
        for uid, gid in ((1234, 0), (0, 1234)):
            with self.assertRaisesRegex(RuntimeError, 'E_CGROUP_FIXTURE_OWNER'):
                self.scan(uid=uid, gid=gid, fixture=(1234, 1234))

    def test_scan_bound(self):
        with patch.object(driver.os, 'walk', return_value=[('/sys/fs/cgroup', [], ['x'] * 20001)]), patch.object(Path, 'lstat', return_value=SimpleNamespace(st_mode=0o644, st_uid=0, st_gid=0)):
            with self.assertRaisesRegex(RuntimeError, 'E_CGROUP_SCAN_BOUND'):
                driver.cg_scan()


class CopyGuards(unittest.TestCase):
    def test_opened_inode_replacement_is_refused(self):
        with tempfile.TemporaryDirectory() as temp, ExitStack() as stack:
            for flag, value in [('O_DIRECTORY', 0x10000), ('O_NOFOLLOW', 0x20000), ('O_NONBLOCK', 0x40000)]:
                stack.enter_context(patch.object(driver.os, flag, value, create=True))
            opened = stack.enter_context(patch.object(driver.os, 'open', return_value=100))
            stack.enter_context(patch.object(driver.os, 'close'))
            stack.enter_context(patch.object(driver.os, 'listdir', return_value=['changed']))
            stack.enter_context(patch.object(driver.os, 'stat', return_value=SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_dev=1, st_ino=1)))
            stack.enter_context(patch.object(driver.os, 'fstat', return_value=SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_dev=1, st_ino=2)))
            with self.assertRaisesRegex(RuntimeError, 'E_COPY_RACE'):
                driver.copy_tree(Path('source'), Path(temp) / 'copy')
            self.assertEqual(opened.call_count, 2)
            self.assertTrue(all(call.args[1] & driver.os.O_NOFOLLOW for call in opened.call_args_list))
            self.assertEqual(opened.call_args_list[1].kwargs, {'dir_fd': 100})

    def test_output_cap_does_not_limit_artifact_files(self):
        exceeded = threading.Event()
        out = io.BytesIO()
        driver.drain_output(io.BytesIO(b'x' * 200), out, exceeded, limit=100)
        self.assertEqual(out.getvalue(), b'x' * 100)
        self.assertTrue(exceeded.is_set())
        exceeded.clear()
        out = io.BytesIO()
        driver.drain_output(io.BytesIO(b'x' * 100), out, exceeded, limit=100)
        self.assertFalse(exceeded.is_set())

    def test_link_and_special_file_refuse_before_copy(self):
        for mode in (stat.S_IFLNK | 0o777, stat.S_IFIFO | 0o600, stat.S_IFSOCK | 0o600):
            with tempfile.TemporaryDirectory() as temp, ExitStack() as stack:
                stack.enter_context(patch.object(driver.os, 'O_DIRECTORY', 0x10000, create=True))
                stack.enter_context(patch.object(driver.os, 'O_NOFOLLOW', 0x20000, create=True))
                stack.enter_context(patch.object(driver.os, 'O_NONBLOCK', 0x40000, create=True))
                opened = stack.enter_context(patch.object(driver.os, 'open', return_value=100))
                stack.enter_context(patch.object(driver.os, 'close'))
                stack.enter_context(patch.object(driver.os, 'listdir', return_value=['bad']))
                stack.enter_context(patch.object(driver.os, 'stat', return_value=SimpleNamespace(st_mode=mode)))
                stack.enter_context(patch.object(driver.os, 'fstat', side_effect=AssertionError('unsafe child opened')))
                with self.assertRaisesRegex(RuntimeError, 'E_COPY_TYPE'):
                    driver.copy_tree(Path('source'), Path(temp) / 'copy')
                self.assertEqual(opened.call_count, 1)  # root only, never bad child

class F0Mechanism(unittest.TestCase):
    """Synthetic mechanism credit only. No filesystem observation or reproduction."""
    def setUp(self):
        for name, value in (('feedback_phase', 'initial'), ('feedback_mutation', 'staged'),
                            ('feedback_cleanup', 'unconfirmed'), ('feedback_first', None), ('feedback_internal', None)):
            self.enterContext(patch.object(driver, name, value))
        self.enterContext(patch.object(driver.sys, 'platform', 'linux'))

    def refusal(self, phase='initial'):
        return driver.Refusal('run_parent', phase, [['dir', True, 0], ['other', False, 2]])

    def record(self):
        return json.loads(driver.feedback_line(valid_env(), driver.exception_record(self.refusal()), driver.exception_record(self.refusal('final'))))

    def test_real_predicate_and_order(self):
        for kind in ('dir', 'other'):
            self.assertIsNone(driver.decide([[kind, True, 0]]))
        for triple, conjunct in ((['link', False, 18], 'link'), (['other', False, 18], 'owner'),
                                  (['dir', True, 2], 'write')):
            self.assertEqual(driver.decide([['dir', True, 0], triple]), (1, conjunct))
        self.assertIsNone(driver.decide([['dir', True, 0]] * 32))
        with self.assertRaises(RuntimeError):
            driver.decide([['dir', True, 0]] * 33)

    def test_protected_uses_real_predicate_without_filesystem(self):
        path = SimpleNamespace(is_absolute=lambda: True, parts=(), parents=[])
        path.lstat = lambda: SimpleNamespace(st_mode=stat.S_IFREG | 0o644, st_uid=0)
        driver.protected(path, 'run_parent')
        path.lstat = lambda: SimpleNamespace(st_mode=stat.S_IFLNK | 0o777, st_uid=123)
        with self.assertRaises(driver.Refusal) as raised:
            driver.protected(path, 'run_parent')
        self.assertEqual(raised.exception.record['kind'], 'link')
        self.assertEqual(raised.exception.record['write_bits'], 18)

    def test_both_exception_orderings_replay(self):
        for first, final in ((self.refusal(), RuntimeError('private')),
                             (RuntimeError('private'), self.refusal('final'))):
            line = driver.feedback_line(valid_env(), driver.exception_record(first), driver.exception_record(final))
            result = driver.replay(line)
            self.assertEqual(result, {'first' if type(first) is driver.Refusal else 'final': (1, 'owner')})
            self.assertNotIn('private', line)
            self.assertLessEqual(len(line.encode('ascii')), 4096)

    def test_hostile_payload_never_formatted(self):
        class Hostile(RuntimeError):
            def __str__(self):
                raise AssertionError('payload formatted')
        self.assertEqual(driver.exception_record(Hostile()), {'code': 'E_RUNTIME'})
        self.assertEqual(driver.exception_record(RuntimeError('E_PRIVATE')), {'code': 'E_RUNTIME'})

    def test_unknown_fields_all_levels(self):
        for target in ('outer', 'provenance', 'first'):
            r = self.record()
            (r if target == 'outer' else r[target])['unknown'] = 'private'
            with self.assertRaises(RuntimeError):
                driver.encode_record(r)

    def test_types_and_values(self):
        r = self.record()
        for key in r:
            for bad in ([], {}, True, 0, 'unknown'):
                copy = self.record(); copy[key] = bad
                with self.subTest(key=key, bad=bad), self.assertRaises((RuntimeError, TypeError)):
                    driver.encode_record(copy)
        for key in r['provenance']:
            for bad in ('', 'x'*41, '0', 1, True, [], {}):
                copy = self.record(); copy['provenance'][key] = bad
                with self.assertRaises(RuntimeError):
                    driver.encode_record(copy)
        for key, bad in (('ordinal', True), ('ordinal', -1), ('site', 'unknown'),
                         ('phase', 'unknown'), ('kind', 'file'), ('owner_zero', 0),
                         ('write_bits', True), ('write_bits', 1), ('predecessors', [])):
            copy = self.record(); copy['first'][key] = bad
            with self.assertRaises(RuntimeError):
                driver.encode_record(copy)

    def test_bounds_and_malformed(self):
        for triples in ([], [['dir', True, 0]]*33, [['dir', True]], [['dir', True, 0, 0]],
                        [['dir', 1, 0]], [['dir', True, 32]], [['unknown', True, 0]]):
            with self.assertRaises(RuntimeError):
                driver.decide(triples)
        for line in ('{}', '{}\n\n', 'x'*4096+'\n', '{}\nprivate', '{"a":1,"a":2}\n',
                     '[]\n', 'null\n', '[['*1000+'\n', '\u00e4\n'):
            with self.assertRaises(RuntimeError):
                driver.replay(line)
        with patch.object(driver, 'MAX_LINE', 10), self.assertRaises(RuntimeError):
            driver.encode_record(self.record())
        r = self.record(); r['first']['predecessors'] = [['link', True, 0]]
        with self.assertRaises(RuntimeError):
            driver.encode_record(r)

    def test_entry_fixed_failure_exit_and_single_line(self):
        for first, final in ((self.refusal(), RuntimeError('private')), (RuntimeError('private'), self.refusal('final'))):
            def mocked_main():
                driver.feedback_first = driver.exception_record(first)
                raise final
            with patch.object(driver, 'main', side_effect=mocked_main), \
                 patch.object(driver, 'feedback_first', None), \
                 patch.dict(driver.os.environ, valid_env(), clear=True), \
                 patch.object(driver.sys, 'argv', ['driver.py', 'run']), \
                 patch.object(driver.sys, 'stdout', new_callable=io.StringIO) as out:
                self.assertEqual(driver.entry(), 1)
                self.assertEqual(out.getvalue().count('\n'), 1)
                driver.replay(out.getvalue())



class A1A2(unittest.TestCase):
    """Pure plans and mocked lifecycle only; no host filesystem or process probes."""
    def parent_attempt(self, bad=None, mode=None, uid=0):
        events = []
        def metadata(path):
            events.append(('check', str(path), driver.feedback_mutation))
            return SimpleNamespace(st_uid=uid if str(path) == bad else 0,
                st_mode=mode if str(path) == bad else stat.S_IFDIR | 0o755)
        def mkdir(path, **kwargs):
            events.append(('mkdir', str(path), kwargs))
            raise FileExistsError('mock exclusive collision')
        lock = unittest.mock.MagicMock()
        with ExitStack() as stack:
            # This legacy lifecycle fixture exercises A5; A4 admission has separate mocks.
            stack.enter_context(patch.object(driver, 'TEST', 'exec::disposable::disposable_linux_a5'))
            stack.enter_context(patch.object(driver, 'Path', PurePosixPath))
            stack.enter_context(patch.object(PurePosixPath, 'lstat', metadata, create=True))
            stack.enter_context(patch.object(PurePosixPath, 'mkdir', mkdir, create=True))
            stack.enter_context(patch.object(driver.sys, 'platform', 'linux'))
            for name in ('getuid', 'geteuid'):
                stack.enter_context(patch.object(driver.os, name, return_value=0, create=True))
            for key, value in [('phase', 'initial'), ('mutation', 'none'), ('first', None), ('cleanup', 'not_reached')]:
                stack.enter_context(patch.object(driver, 'feedback_' + key, value))
            stack.enter_context(patch.object(driver, 'cg_preflight'))
            reserve = stack.enter_context(patch.object(driver, 'reserve_unit', return_value=(77, lock, None)))
            stack.enter_context(patch.object(driver, 'refuse_unit_collision'))
            stack.enter_context(patch.object(driver, 'same_object'))
            stack.enter_context(patch.object(driver.os, 'close'))
            effects = [stack.enter_context(patch.object(owner, name)) for owner, name in
                       ((driver, 'copy_tree'), (driver, 'cleanup_a1a2'), (driver, 'install_runtime'),
                        (driver.subprocess, 'run'), (driver.subprocess, 'check_output'))]
            with self.assertRaises(driver.Refusal if bad else FileExistsError):
                driver.run_body(valid_env())
            for effect in effects:
                effect.assert_not_called()
            self.assertEqual(reserve.call_count, 0 if bad else 1)
            self.assertEqual(driver.feedback_cleanup, 'not_reached')
        return events

    def test_parent_checked_before_exclusive_stage_creation(self):
        self.assertEqual(self.parent_attempt(), [
            ('check', '/', 'none'), ('check', '/var', 'none'), ('check', '/var/lib', 'none'),
            ('mkdir', '/var/lib/arx-ci-123-1', {'mode': 0o700})])

    def test_new_parent_keeps_every_ancestor_guard(self):
        for path in ('/', '/var', '/var/lib'):
            for mode, uid in ((stat.S_IFLNK | 0o755, 0), (stat.S_IFDIR | 0o755, 12),
                              (stat.S_IFDIR | 0o775, 0), (stat.S_IFDIR | 0o757, 0)):
                with self.subTest(path=path, mode=mode, uid=uid):
                    events = self.parent_attempt(path, mode, uid)
                    self.assertEqual(events[-1], ('check', path, 'none'))
                    self.assertTrue(all(e[0] == 'check' for e in events))

    def test_no_staging_parent_override(self):
        env = valid_env()
        env.update(ARX_STAGE_PARENT='/owner', HOME='/owner')
        self.assertEqual(driver.plan(driver.gate(env, 'linux'))[0], Path('/var/lib/arx-ci-123-1'))
        with self.assertRaises(TypeError):
            driver.plan('123-1', '/owner')
        for run in ('/owner', '../123-1', '123-1/owner'):
            with self.assertRaisesRegex(RuntimeError, 'E_ID'):
                driver.plan(run)

    def test_native_writable_parent_regression(self):
        # Real closed F0 record, not a synthetic pass or historical-run diagnosis.
        line = (ROOT / 'docs/evidence/linux-stagef-a1a2-run-43622157531.json').read_text()
        self.assertEqual(driver.replay(line), {'first': (1, 'write'), 'final': (1, 'write')})
        record = json.loads(line)
        self.assertEqual(record['provenance']['run'], '43622157531')
        self.assertEqual((record['mutation_lower_bound'], record['cleanup']), ('none', 'not_reached'))
        seen = []
        def metadata(path):
            seen.append(str(path))
            return SimpleNamespace(st_uid=0, st_mode=stat.S_IFDIR | (0o777 if str(path) == '/opt' else 0o755))
        with patch.object(PurePosixPath, 'lstat', metadata, create=True):
            with self.assertRaises(driver.Refusal) as caught:
                driver.protected(PurePosixPath('/opt'), 'run_parent')
            self.assertEqual(caught.exception.record, record['first'])
            seen.clear()
            with patch.object(driver, 'Path', PurePosixPath):
                stage, _ = driver.plan('43622157531-1')
            try:
                driver.protected(stage.parent, 'run_parent')
            except driver.Refusal:
                self.fail('planned staging parent reproduces the observed writable /opt refusal')
        self.assertEqual(seen, ['/', '/var', '/var/lib'])
        self.assertEqual(str(stage), '/var/lib/arx-ci-43622157531-1')

    def test_launch_contract(self):
        stage = Path('/var/lib/arx-ci-123-1')
        artifact = str(stage / 'source/target/debug/deps/arxd-abc')
        self.assertEqual(driver.plan('123-1')[1], 'arxd.service')
        for selected in (
            'exec::disposable::disposable_linux_a1_a2',
            'exec::disposable::disposable_linux_a3_standard',
            'exec::disposable::disposable_linux_a3_none',
            'exec::disposable::disposable_linux_a5',
        ):
            with self.subTest(selected=selected), patch.object(driver, 'TEST', selected):
                self.assertEqual(driver.test_argv(artifact), [artifact, selected,
                    '--ignored', '--exact', '--test-threads=1', '--color=never'])
        self.assertEqual(driver.fixture_override(stage, '123-1').splitlines(), [
            '[Service]', 'ExecStart=',
            'ExecStart=/usr/bin/python3 -I ' + str(stage / 'source/tools/unix-exec-validation/driver.py') + ' service 123-1',
            'Restart=no', 'RuntimeMaxSec=140'])
        self.assertEqual(driver.test_env('@notify'), dict(CI='true', GITHUB_ACTIONS='true',
            RUNNER_ENVIRONMENT='github-hosted', ARX_DISPOSABLE_EXEC_TEST='1', NOTIFY_SOCKET='@notify'))

    def monitor(self, states, size=0, pid='42', actual='42', exe=None, placement=None, ticks=None, tasks=b'42\n', result='success', status='0'):
        stage = Path('/var/lib/arx-ci-123-1')
        artifact = str(stage / 'source/target/debug/deps/arxd-abc')
        events = []
        def read(path, *args):
            if path.name == 'harness-pid': return pid
            if path.name == 'cgroup': return placement or '0::/system.slice/arxd.service/supervisor\n'
            raise AssertionError('unexpected read')
        with patch.object(driver, 'unit_properties', side_effect=[dict(ActiveState=s,
                MainPID=actual, Result=result, ExecMainStatus=status) for s in states]), \
             patch.object(driver, 'verify_runtime', side_effect=lambda *a: events.append('attribution')), \
             patch.object(driver, 'private_read', side_effect=read), \
             patch.object(driver, 'capture_size', return_value=size), \
             patch.object(driver.os, 'readlink', return_value=exe or artifact), \
             patch.object(driver.os, 'O_NOFOLLOW', 0x20000, create=True), \
             patch.object(driver.os, 'O_CLOEXEC', 0x80000, create=True), \
             patch.object(driver.os, 'open', return_value=77), \
             patch.object(driver.os, 'read', side_effect=[(placement or '0::/system.slice/arxd.service/supervisor\n').encode(), tasks]), \
             patch.object(driver.os, 'close'), \
             patch.object(driver, 'exclusive_file', side_effect=lambda p,b: events.append(p.name)), \
             patch.object(driver, 'verify_test_result', side_effect=lambda *a: events.append('summary')), \
             patch.object(driver.time, 'monotonic', side_effect=ticks or ([0] + list(range(1,1000)))), \
             patch.object(driver.time, 'sleep'):
            driver.monitor_a1a2(stage, artifact)
        return events

    def test_ack_before_exit_before_summary(self):
        self.assertEqual(self.monitor(['active', 'inactive']), ['attribution', 'identity-ack', 'summary'])

    def test_mainpid_and_placement_required(self):
        for kwargs in (dict(actual='43'), dict(exe='/other'), dict(placement='0::/other/supervisor\n')):
            with self.subTest(kwargs=kwargs), self.assertRaises(RuntimeError):
                self.monitor(['active', 'inactive'], **kwargs)

    def test_overflow_while_held_refuses(self):
        with self.assertRaisesRegex(RuntimeError, 'E_TEST_LOG'):
            self.monitor(['active'], size=1024*1024+1)

    def test_deadline_required(self):
        with self.assertRaisesRegex(RuntimeError, 'E_TEST_TIMEOUT'):
            self.monitor(['activating','inactive'], ticks=[0,121,122])

    def test_zero_tests_refused(self):
        stage = unittest.mock.MagicMock()
        (stage / 'test-output').stat.return_value.st_size = 100
        (stage / 'test-output').read_text.return_value = 'test result: ok. 0 passed; 0 failed;'
        with patch.object(driver, 'private_read', return_value='test result: ok. 0 passed; 0 failed;'), self.assertRaisesRegex(RuntimeError, 'E_TEST'):
            driver.verify_test_result(stage, 0)

    def test_build_failure_before_install(self):
        # build/install ordering is covered through run_body by F0Blockers.
        with patch.object(driver, 'private_read', return_value='not-an-artifact'), \
             patch.object(driver, 'protected'), patch.object(driver.os, 'execve') as execute:
            with self.assertRaises(RuntimeError):
                driver.harness(Path('/var/lib/arx-ci-123-1'), ['wrong'])
            execute.assert_not_called()

    def test_collision_refuses_before_mutation(self):
        with patch.object(driver, 'unit_properties', return_value={'LoadState':'loaded', 'Id':'arxd.service'}), \
             patch.object(driver.subprocess, 'check_output') as command, \
             patch.object(driver.os.path, 'lexists', return_value=False), \
             patch.object(driver.os, 'walk', return_value=[]) as walk:
            with self.assertRaisesRegex(RuntimeError, 'E_UNIT_COLLISION'):
                driver.refuse_unit_collision()
            command.assert_not_called()
            walk.assert_not_called()

    def test_cleanup_cannot_stop_another_unit(self):
        with patch.object(driver, 'verify_runtime'), \
             patch.object(driver.subprocess, 'run', return_value=SimpleNamespace(returncode=0)) as stop, \
             patch.object(driver, 'unit_properties', return_value={'FragmentPath':'/run/systemd/system/arxd.service'}), \
             patch.object(driver, 'snapshot_internal'), patch.object(driver, 'remove_runtime'), \
             patch.object(driver.os.path, 'lexists', return_value=False):
            with self.assertRaisesRegex(RuntimeError, 'E_UNIT_COLLISION'):
                driver.cleanup_a1a2('123-1', 'other.service', None, False, None, True, [])
            stop.assert_not_called()

    def test_cleanup_replacement_quarantines(self):
        owned = [(Path('/run/systemd/system/arxd.service'), object())]
        with patch.object(driver, 'verify_runtime'), \
             patch.object(driver, 'same_object', side_effect=RuntimeError('E_FIXTURE_INODE')), \
             patch.object(driver.subprocess, 'run') as stop, \
             patch.object(driver, 'remove_runtime') as remove:
            with self.assertRaisesRegex(RuntimeError, 'E_FIXTURE_INODE'):
                driver.cleanup_a1a2('123-1', 'arxd.service', None, False, None, True, owned)
            stop.assert_not_called()
            remove.assert_not_called()

    def test_launcher_exec_and_single_capture_owner(self):
        stage = Path('/var/lib/arx-ci-123-1')
        binary = str(stage / 'source/target/debug/deps/arxd-abc')
        artifact = unittest.mock.MagicMock()
        artifact.parent = stage / 'source/target/debug/deps'
        artifact.name = 'arxd-abc'
        artifact.lstat.return_value = SimpleNamespace(st_mode=stat.S_IFREG | 0o700)
        events = []
        with ExitStack() as stack:
            for flag in ('O_NOFOLLOW', 'O_CLOEXEC'):
                stack.enter_context(patch.object(driver.os, flag, 0x80000, create=True))
            stack.enter_context(patch.object(driver, 'Path', return_value=artifact))
            stack.enter_context(patch.object(driver, 'protected'))
            stack.enter_context(patch.object(driver, 'private_read', return_value=binary))
            created = stack.enter_context(patch.object(driver, 'exclusive_file', side_effect=lambda *a:events.append('pid')))
            opened = stack.enter_context(patch.object(driver.os, 'open', side_effect=lambda *a:events.append('capture') or 77))
            stack.enter_context(patch.object(driver.os, 'dup2', side_effect=lambda *a:events.append('dup')))
            stack.enter_context(patch.object(driver.os, 'close'))
            stack.enter_context(patch.object(driver.os, 'chdir'))
            stack.enter_context(patch.object(driver, 'mark_cgroup'))
            execute = stack.enter_context(patch.object(driver.os, 'execve', side_effect=lambda *a:events.append('exec')))
            process = stack.enter_context(patch.object(driver.subprocess, 'run'))
            stack.enter_context(patch.dict(driver.os.environ, {'NOTIFY_SOCKET':'@notify','HOSTILE':'secret'}, clear=True))
            args = driver.test_argv(binary)
            driver.harness(stage, args)
            self.assertEqual(events, ['pid','capture','dup','dup','exec'])
            execute.assert_called_once_with(binary, args, driver.test_env('@notify'))
            process.assert_not_called()
            self.assertEqual(opened.call_args.args[0], stage / 'test-output')
            self.assertTrue(opened.call_args.args[1] & os.O_EXCL)
            self.assertTrue(opened.call_args.args[1] & driver.os.O_NOFOLLOW)
            self.assertEqual(opened.call_args.args[2], 0o600)

    def test_reservation_is_exclusive_nonblocking(self):
        lock = unittest.mock.MagicMock()
        flock = unittest.mock.MagicMock(LOCK_EX=2, LOCK_NB=4)
        with ExitStack() as stack:
            stack.enter_context(patch.dict(sys.modules, {'fcntl':flock}))
            for flag in ('O_NOFOLLOW', 'O_CLOEXEC'):
                stack.enter_context(patch.object(driver.os, flag, 0x80000, create=True))
            stack.enter_context(patch.object(driver, 'Path', return_value=lock))
            stack.enter_context(patch.object(driver, 'protected'))
            opened = stack.enter_context(patch.object(driver.os, 'open', return_value=77))
            stack.enter_context(patch.object(driver.os, 'fstat', return_value='identity'))
            stack.enter_context(patch.object(driver.os, 'close'))
            self.assertEqual(driver.reserve_unit(), (77,lock,'identity'))
            self.assertTrue(opened.call_args.args[1] & os.O_EXCL)
            flock.flock.assert_called_once_with(77, 6)
            opened.side_effect = FileExistsError()
            flock.flock.reset_mock()
            with self.assertRaises(FileExistsError):driver.reserve_unit()
            flock.flock.assert_not_called()

    def test_runtime_attribution_lifecycle(self):
        stage = unittest.mock.MagicMock()
        stage.__truediv__.return_value = stage
        stage.parent = stage
        stage.name = 'arxd-abc'
        stage.stat.return_value.st_size = 10
        stage.read_text.return_value = json.dumps(dict(reason='compiler-artifact', executable='/artifact', profile={'test':True}))
        events = []
        with ExitStack() as stack:
            # This legacy lifecycle fixture exercises A5; A4 admission has separate mocks.
            stack.enter_context(patch.object(driver, 'TEST', 'exec::disposable::disposable_linux_a5'))
            for name, value in [('phase','initial'), ('mutation','none'), ('cleanup','not_reached'), ('first',None), ('internal',None)]:
                stack.enter_context(patch.object(driver, 'feedback_' + name, value))
            stack.enter_context(patch.object(driver.sys, 'platform', 'linux'))
            stack.enter_context(patch.object(driver, 'plan', return_value=(stage, 'arxd.service')))
            stack.enter_context(patch.object(driver, 'Path', return_value=stage))
            for name in ('getuid', 'geteuid'):
                stack.enter_context(patch.object(driver.os, name, return_value=0, create=True))
            stack.enter_context(patch.object(driver.os, 'close'))
            stack.enter_context(patch.object(driver.os.path, 'lexists', return_value=False))
            stack.enter_context(patch.object(driver, 'reserve_unit', return_value=(77, stage, None)))
            for name in ('cg_preflight', 'protected', 'copy_tree', 'refuse_unit_collision', 'same_object', 'exclusive_file', 'install_runtime', 'snapshot_internal', 'remove_runtime'):
                stack.enter_context(patch.object(driver, name))
            remove = stack.enter_context(patch.object(driver.shutil, 'rmtree'))
            remove.avoids_symlink_attacks = True
            stack.enter_context(patch.object(driver.subprocess, 'check_output', return_value=valid_env()['GITHUB_SHA'].encode()))
            def process(argv, **kwargs):
                events.append('stop' if argv[0] == '/usr/bin/systemctl' else 'build')
                return SimpleNamespace(returncode=0)
            stack.enter_context(patch.object(driver.subprocess, 'run', side_effect=process))
            stack.enter_context(patch.object(driver, 'call', side_effect=lambda argv: events.append('start') if 'start' in argv else None))
            stack.enter_context(patch.object(driver, 'verify_runtime', side_effect=lambda *a: events.append('attribution')))
            stack.enter_context(patch.object(driver, 'monitor_a1a2', side_effect=lambda *a: events.append('monitor')))
            driver.run_body(valid_env())
        self.assertEqual(events, ['build', 'attribution', 'start', 'monitor', 'attribution', 'stop'])

    def test_closed_selection_before_effects(self):
        for value in ('', 'exec::disposable::disposable_linux_cross_account',
                      'exec::disposable::disposable_linux_a4',
                      'exec::disposable::disposable_linux_a5_none',
                      'exec::disposable::disposable_linux_a5_standard',
                      'exec::disposable::disposable_linux_a5x',
                      'exec::disposable::disposable_linux_a5\n', driver.TEST + 'x'):
            with patch.object(driver, 'TEST', value), \
                 patch.object(driver.os, 'getuid', create=True, side_effect=AssertionError('probe')), \
                 patch.object(driver, 'cg_preflight', side_effect=AssertionError('probe')):
                with self.assertRaisesRegex(RuntimeError, 'E_TEST_ARGS'):
                    driver.standard_profile()
                with self.assertRaisesRegex(RuntimeError, 'E_TEST_ARGS'):
                    driver.test_argv('/binary')
                with self.assertRaisesRegex(RuntimeError, 'E_TEST_ARGS'):
                    driver.run_body(valid_env())
                with self.assertRaisesRegex(RuntimeError, 'E_TEST_ARGS'):
                    driver.main()

    def test_profile_pairings_and_install_bytes(self):
        stage = Path('/var/lib/arx-ci-123-1')
        package = ROOT / 'packaging/linux'
        unit = (package / 'arxd.service').read_bytes()
        profile = (package / '50-arxcovo-standard-rights.conf').read_bytes()
        root = Path('/run/systemd/system')
        for name, present in [('a1_a2', True), ('a3_standard', True), ('a3_none', False), ('a5', True), ('a4', True)]:
            with self.subTest(name=name), ExitStack() as stack:
                selected = 'exec::disposable::disposable_linux_' + name
                if name == 'a4':
                    selected = 'exec::disposable::a4::disposable_linux_a4'
                stack.enter_context(patch.object(driver, 'TEST', selected))
                stack.enter_context(patch.object(Path, 'read_bytes', lambda p: profile if p.name.startswith('50-') else unit))
                stack.enter_context(patch.object(driver, 'protected'))
                mkdir = stack.enter_context(patch.object(Path, 'mkdir'))
                stack.enter_context(patch.object(Path, 'lstat', return_value='dir-identity'))
                write = stack.enter_context(patch.object(driver, 'exclusive_file', return_value='file-identity'))
                owned = []
                driver.install_runtime(stage, '123-1', owned)
                expected = [(root / 'arxd.service', unit)]
                if present:
                    expected.append((root / 'arxd.service.d/50-arxcovo-standard-rights.conf', profile))
                expected.append((root / 'arxd.service.d/90-disposable-a1a2.conf', driver.fixture_override(stage, '123-1').encode('ascii')))
                self.assertEqual([c.args for c in write.call_args_list], expected)
                self.assertEqual([p for p, _ in owned], [root / 'arxd.service', root / 'arxd.service.d'] + [p for p, _ in expected[1:]])
                mkdir.assert_called_once_with(mode=0o700)
                self.assertEqual(driver.standard_profile(), present)
                self.assertEqual(driver.test_argv('/binary')[1], selected)

    def attribution(self, name='a3_standard', change=None, disk_extra=False, bad_bytes=False, mode=0o700):
        stage = Path('/var/lib/arx-ci-123-1')
        root = Path('/run/systemd/system')
        names = ['90-disposable-a1a2.conf']
        if name != 'a3_none': names.insert(0, '50-arxcovo-standard-rights.conf')
        props = dict(LoadState='loaded', Id='arxd.service', Names='arxd.service',
                     FragmentPath='/run/systemd/system/arxd.service',
                     DropInPaths=' '.join('/run/systemd/system/arxd.service.d/' + n for n in names))
        props.update(change or {})
        disk = [root / 'arxd.service.d' / n for n in names]
        if disk_extra: disk.append(root / 'arxd.service.d/foreign.conf')
        with ExitStack() as stack:
            stack.enter_context(patch.object(driver, 'TEST', 'exec::disposable::disposable_linux_' + name))
            stack.enter_context(patch.object(Path, 'read_bytes', return_value=b'package'))
            stack.enter_context(patch.object(Path, 'lstat', return_value=SimpleNamespace(st_mode=stat.S_IFDIR | mode, st_uid=0)))
            stack.enter_context(patch.object(Path, 'iterdir', return_value=iter(disk)))
            def read(p):
                if bad_bytes: return 'changed'
                return driver.fixture_override(stage, '123-1') if p.name.startswith('90-') else 'package'
            stack.enter_context(patch.object(driver, 'private_read', side_effect=read))
            driver.verify_runtime(stage, '123-1', props)

    def test_exact_runtime_attribution(self):
        for name in ('a1_a2', 'a3_standard', 'a3_none', 'a5'):
            self.attribution(name)
            for kwargs in (dict(change={'DropInPaths':''}),
                           dict(change={'DropInPaths':'/foreign/identical.conf'}),
                           dict(change={'Names':'arxd.service alias.service'}),
                           dict(change={'Id':'alias.service'}),
                           dict(change={'FragmentPath':'/foreign/arxd.service'}),
                           dict(disk_extra=True), dict(bad_bytes=True), dict(mode=0o755)):
                with self.subTest(name=name, kwargs=kwargs), self.assertRaisesRegex(RuntimeError, 'E_UNIT_COLLISION'):
                    self.attribution(name, **kwargs)

    def test_sole_supervisor_pid_required(self):
        for tasks in (b'', b'42\n43\n', b'43\n', b'42 '):
            with self.subTest(tasks=tasks), self.assertRaisesRegex(RuntimeError, 'E_UNIT_CGROUP'):
                self.monitor(['active', 'inactive'], tasks=tasks)

    def test_source_selection_is_a4(self):
        self.assertEqual(driver.TEST, 'exec::disposable::a4::disposable_linux_a4')

    def test_closed_mapping(self):
        # Independent expectations: never obtain the cases/profile from the driver.
        for selected, present in (
            ('exec::disposable::disposable_linux_a1_a2', True),
            ('exec::disposable::disposable_linux_a3_standard', True),
            ('exec::disposable::disposable_linux_a3_none', False),
            ('exec::disposable::disposable_linux_a5', True),
        ):
            with self.subTest(selected=selected), patch.object(driver, 'TEST', selected):
                try:
                    profile = driver.standard_profile()
                    argv = driver.test_argv('/binary')
                except RuntimeError as error:
                    self.fail('closed mapping refused: ' + selected + ': ' + str(error))
                self.assertIs(profile, present)
                self.assertEqual(argv, ['/binary', selected, '--ignored', '--exact',
                                        '--test-threads=1', '--color=never'])

    def test_a5_reuses_existing_fences(self):
        # These helpers mock every host effect; A5 adds no lifecycle of its own.
        with patch.object(driver, 'TEST', 'exec::disposable::disposable_linux_a5'):
            self.test_ack_before_exit_before_summary()
            self.test_mainpid_and_placement_required()
            self.test_sole_supervisor_pid_required()
            self.test_exit_requires_ack_and_real_success()
            self.test_overflow_while_held_refuses()
            self.test_deadline_required()
            self.test_launcher_exec_and_single_capture_owner()
            self.test_runtime_attribution_lifecycle()
            self.test_reservation_is_exclusive_nonblocking()
            self.test_collision_refuses_before_mutation()
            self.test_cleanup_cannot_stop_another_unit()
            self.test_cleanup_replacement_quarantines()

    def test_all_four_exact_summaries(self):
        selections = ('exec::disposable::disposable_linux_a1_a2',
                      'exec::disposable::disposable_linux_a3_standard',
                      'exec::disposable::disposable_linux_a3_none',
                      'exec::disposable::disposable_linux_a5')
        stage = unittest.mock.MagicMock()
        (stage / 'test-output').stat.return_value.st_size = 100
        for selected in selections:
            with self.subTest(selected=selected), patch.object(driver, 'TEST', selected):
                self.test_exact_summary()
                for other in selections:
                    if other == selected:
                        continue
                    text = f'test {other} ... ok\ntest result: ok. 1 passed; 0 failed;'
                    with patch.object(driver, 'private_read', return_value=text), self.assertRaisesRegex(RuntimeError, 'E_TEST'):
                        driver.verify_test_result(stage, 0)

    def test_exit_requires_ack_and_real_success(self):
        for states, kwargs in [(['inactive'], {}), (['active','failed'], {'result':'exit-code'}),
                               (['active','inactive'], {'status':'1'})]:
            with self.assertRaisesRegex(RuntimeError, 'E_SERVICE_OR_TEST'):
                self.monitor(states, **kwargs)

    def test_private_runtime_read_metadata(self):
        for mode, uid, links, size in [(0o600,0,1,4), (0o644,0,1,4),
                                        (0o600,1,1,4), (0o600,0,2,4), (0o600,0,1,4097)]:
            with ExitStack() as stack:
                for flag in ('O_NOFOLLOW', 'O_NONBLOCK', 'O_CLOEXEC'):
                    stack.enter_context(patch.object(driver.os, flag, 0x80000, create=True))
                opened = stack.enter_context(patch.object(driver.os, 'open', return_value=77))
                stack.enter_context(patch.object(driver.os, 'fstat', return_value=SimpleNamespace(st_mode=stat.S_IFREG | mode, st_uid=uid, st_nlink=links, st_size=size)))
                read = stack.enter_context(patch.object(driver.os, 'read', return_value=b'unit'))
                closed = stack.enter_context(patch.object(driver.os, 'close'))
                if (mode, uid, links, size) == (0o600,0,1,4):
                    self.assertEqual(driver.private_read(Path('/runtime')), 'unit')
                    read.assert_called_once_with(77, 4097)
                else:
                    with self.assertRaisesRegex(RuntimeError, 'E_PATH'):
                        driver.private_read(Path('/runtime'))
                    read.assert_not_called()
                self.assertTrue(opened.call_args.args[1] & driver.os.O_NOFOLLOW)
                closed.assert_called_once_with(77)

    def test_runtime_exclusive_file_mode(self):
        with ExitStack() as stack:
            for flag in ('O_NOFOLLOW', 'O_CLOEXEC'):
                stack.enter_context(patch.object(driver.os, flag, 0x80000, create=True))
            opened = stack.enter_context(patch.object(driver.os, 'open', return_value=77))
            stack.enter_context(patch.object(driver.os, 'fstat', return_value='identity'))
            stack.enter_context(patch.object(driver.os, 'dup', return_value=78))
            sink = unittest.mock.MagicMock()
            stack.enter_context(patch.object(driver.os, 'fdopen', return_value=sink))
            closed = stack.enter_context(patch.object(driver.os, 'close'))
            self.assertEqual(driver.exclusive_file(Path('/runtime'), b'bytes'), 'identity')
            self.assertEqual(opened.call_args.args[2], 0o600)
            for flag in (os.O_EXCL, driver.os.O_NOFOLLOW, os.O_CREAT):
                self.assertTrue(opened.call_args.args[1] & flag)
            sink.__enter__.return_value.write.assert_called_once_with(b'bytes')
            closed.assert_called_once_with(77)

    def test_remove_validates_entire_set_before_deleting(self):
        first, second = unittest.mock.MagicMock(), unittest.mock.MagicMock()
        info = SimpleNamespace(st_mode=stat.S_IFREG | 0o600)
        with patch.object(driver, 'same_object', side_effect=[None, RuntimeError('E_FIXTURE_INODE')]), \
             patch.object(driver, 'call') as reload:
            with self.assertRaisesRegex(RuntimeError, 'E_FIXTURE_INODE'):
                driver.remove_runtime([(first, info), (second, info)])
            first.unlink.assert_not_called()
            second.unlink.assert_not_called()
            reload.assert_not_called()

    def test_capture_mode_and_nlink(self):
        for mode, uid, links in [(0o644,0,1), (0o600,1,1), (0o600,0,2)]:
            with patch.object(Path, 'lstat', return_value=SimpleNamespace(st_mode=stat.S_IFREG | mode, st_uid=uid, st_nlink=links)), self.assertRaisesRegex(RuntimeError, 'E_TEST_LOG'):
                driver.capture_size(Path('/stage'))

    def test_exact_summary(self):
        stage = unittest.mock.MagicMock()
        (stage / 'test-output').stat.return_value.st_size = 100
        for summary in ('0 passed; 0 failed;', '11 passed; 0 failed;', '1 passed; 1 failed;'):
            text = f'test {driver.TEST} ... ok\ntest result: ok. {summary}'
            with patch.object(driver, 'private_read', return_value=text), self.assertRaisesRegex(RuntimeError, 'E_TEST'):
                driver.verify_test_result(stage, 0)
        text = f'test {driver.TEST} ... ok\ntest result: ok. 1 passed; 0 failed;'
        with patch.object(driver, 'private_read', return_value=text.replace(driver.TEST, 'wrong')), self.assertRaisesRegex(RuntimeError, 'E_TEST'):
            driver.verify_test_result(stage, 0)
        with patch.object(driver, 'private_read', return_value=text), self.assertRaisesRegex(RuntimeError, 'E_TEST'):
            driver.verify_test_result(stage, 1)
        with patch.object(driver, 'private_read', return_value=text):
            try:
                driver.verify_test_result(stage, 0)
            except RuntimeError as error:
                self.fail('exact selected summary refused: ' + str(error))


class A4Delivery(unittest.TestCase):
    selected = 'exec::disposable::a4::disposable_linux_a4'

    def test_closed_mapping_and_default(self):
        self.assertEqual(driver.TEST, 'exec::disposable::a4::disposable_linux_a4')
        with patch.object(driver, 'TEST', self.selected):
            try:
                self.assertTrue(driver.standard_profile())
            except RuntimeError:
                self.fail('closed A4 mapping refused')
            self.assertEqual(driver.test_argv('/binary'),
                ['/binary', self.selected, '--ignored', '--exact', '--test-threads=1', '--color=never'])
            self.assertTrue(driver.a4_selected())
        with patch.object(driver, 'TEST', 'exec::disposable::disposable_linux_a5'), \
             patch.object(driver.os, 'open') as opened:
            with self.assertRaisesRegex(RuntimeError, 'E_TEST_ARGS'):
                driver.a4_delivery()
            with self.assertRaisesRegex(RuntimeError, 'E_TEST_ARGS'):
                driver.install_a4_helper(b'', {})
            opened.assert_not_called()

    def test_source_and_recipe_independent_of_image(self):
        root = ROOT / 'tools/unix-exec-validation/a4-helper'
        for bad in ('helper.rs', 'recipe.json'):
            reads = []
            def read(name, bound):
                reads.append(name)
                if name not in ('helper.rs', 'recipe.json'):
                    self.fail('unreviewed source reached image provenance')
                return b'changed' if name == bad else (root / name).read_bytes()
            with patch.object(driver, 'TEST', self.selected), \
                 patch.object(driver, 'a4_input', side_effect=read), \
                 patch.object(driver.os, 'open') as opened:
                with self.assertRaisesRegex(RuntimeError, 'E_ARTIFACT'):
                    driver.a4_delivery()
                self.assertNotIn('out/a4-helper', reads)
                opened.assert_not_called()

    def test_size_and_hash_both_required_before_open(self):
        for data in (b'', b'x' * 3680, b'x' * 3681):
            with self.assertRaisesRegex(RuntimeError, 'E_ARTIFACT'):
                driver.a4_image(data)
            with patch.object(driver, 'TEST', self.selected), \
                 patch.object(driver.os, 'open') as opened:
                with self.assertRaisesRegex(RuntimeError, 'E_ARTIFACT'):
                    driver.install_a4_helper(data, {})
                opened.assert_not_called()

    def test_bad_a4_input_refuses_before_privileged_probe(self):
        with patch.object(driver, 'TEST', self.selected), \
             patch.object(driver.sys, 'platform', 'linux'), \
             patch.object(driver, 'a4_delivery', side_effect=RuntimeError('E_ARTIFACT')), \
             patch.object(driver.os, 'getuid', create=True) as uid, \
             patch.object(driver, 'reserve_unit') as reserve:
            with self.assertRaisesRegex(RuntimeError, 'E_ARTIFACT'):
                driver.run_body(valid_env())
            uid.assert_not_called()
            reserve.assert_not_called()

    def test_linux_provenance_and_fixed_pin_before_image(self):
        root = ROOT / 'tools/unix-exec-validation/a4-helper'
        inputs = {name: (root / name).read_bytes() for name in ('helper.rs', 'recipe.json', 'linux-image-pin.json')}
        pin = json.loads(inputs['linux-image-pin.json'])
        candidate = dict(pin, compiler_identity=pin['compiler_identity'].replace(
            'x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu'))
        for key, bad in [('compiler_identity', pin['compiler_identity'].replace('x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc')), ('compiler_sha256', ''),
                         ('flags', []), ('image_sha256', '0' * 64), ('source_sha256', '0' * 64)]:
            altered = dict(candidate, **{key: bad})
            def read(name, bound):
                if name == 'out/a4-helper':
                    self.fail('invalid provenance reached delivery')
                return json.dumps(altered).encode() if name == 'out/candidate-pin.json' else inputs[name]
            with patch.object(driver, 'TEST', self.selected), patch.object(driver, 'a4_input', side_effect=read):
                with self.assertRaisesRegex(RuntimeError, 'E_ARTIFACT'):
                    driver.a4_delivery()
        inputs['out/candidate-pin.json'] = json.dumps(candidate).encode()
        inputs['out/a4-helper'] = b'synthetic reviewed image'
        with patch.object(driver, 'TEST', self.selected), \
             patch.object(driver, 'a4_input', side_effect=lambda name, bound: inputs[name]), \
             patch.object(driver, 'a4_image', return_value=b'held bytes') as image:
            self.assertEqual(driver.a4_delivery(), b'held bytes')
            image.assert_called_once_with(b'synthetic reviewed image')

    @staticmethod
    def info(mode=stat.S_IFDIR | 0o755, uid=0, inode=1, size=3680, links=1):
        return SimpleNamespace(st_mode=mode, st_uid=uid, st_dev=1, st_ino=inode,
                               st_size=size, st_nlink=links)

    def staging(self, **options):
        stack = ExitStack()
        self.addCleanup(stack.close)
        stack.enter_context(patch.object(driver, 'os', SimpleNamespace(**vars(os))))
        for flag in ('O_NOFOLLOW', 'O_CLOEXEC', 'O_DIRECTORY', 'O_NONBLOCK'):
            stack.enter_context(patch.object(driver.os, flag, getattr(os, flag, 0x80000), create=True))
        stack.enter_context(patch.object(driver, 'TEST', self.selected))
        mocks = {'image': stack.enter_context(patch.object(driver, 'a4_image', side_effect=lambda data: data))}
        defaults = dict(open=[10, 11, 12, 13, 14], fstat=self.info(),
                        stat=self.info(), write=3680, read=b'x' * 3680,
                        listdir=['a4-helper'])
        for name in ('open', 'close', 'fstat', 'stat', 'mkdir', 'write', 'fchmod',
                     'lseek', 'read', 'listdir', 'unlink', 'rmdir'):
            value = options.get(name, defaults.get(name))
            sequential = isinstance(value, BaseException) or (isinstance(value, list) and name != 'listdir')
            kw = {'side_effect': value} if sequential else {'return_value': value}
            mocks[name] = stack.enter_context(patch.object(driver.os, name, create=True, **kw))
        return mocks

    def test_stage_descriptor_bytes_and_owned_cleanup(self):
        file = self.info(stat.S_IFREG | 0o755)
        mocks = self.staging(fstat=[self.info()] * 4 + [file, file, self.info(), self.info()])
        owned = {}
        driver.install_a4_helper(b'x' * 3680, owned)
        self.assertEqual(set(owned), {'parent', 'created', 'directory', 'fd', 'image'})
        opened = mocks['open'].call_args_list
        self.assertEqual([c.args[0] for c in opened], ['/', 'usr', 'libexec', 'arxcovo', 'a4-helper'])
        self.assertTrue(all(c.args[1] & driver.os.O_NOFOLLOW for c in opened))
        self.assertTrue(opened[-1].args[1] & os.O_EXCL)
        mocks['read'].assert_called_once_with(14, 3681)
        self.assertEqual(mocks['image'].call_count, 2)
        mocks['fchmod'].assert_called_once_with(14, 0o755)
        mocks['stat'].side_effect = [owned['directory'], owned['image']]
        driver.remove_a4_helper(owned)
        mocks['unlink'].assert_called_once_with('a4-helper', dir_fd=13)
        mocks['rmdir'].assert_called_once_with('arxcovo', dir_fd=12)

    def test_collision_never_adopted_or_deleted(self):
        mocks = self.staging(mkdir=FileExistsError())
        owned = {}
        with self.assertRaises(FileExistsError):
            driver.install_a4_helper(b'x' * 3680, owned)
        driver.remove_a4_helper(owned)
        self.assertNotIn('directory', owned)
        mocks['unlink'].assert_not_called()
        mocks['rmdir'].assert_not_called()

    def test_untrusted_ancestor_never_mutates(self):
        for info in (self.info(uid=1000), self.info(stat.S_IFDIR | 0o777),
                     self.info(stat.S_IFLNK | 0o755), self.info(stat.S_IFREG | 0o755)):
            mocks = self.staging(fstat=info)
            with self.assertRaisesRegex(RuntimeError, 'E_PATH'):
                driver.install_a4_helper(b'x' * 3680, {})
            mocks['mkdir'].assert_not_called()

    def test_replaced_or_extra_asset_quarantines(self):
        for replacement, contents in [(self.info(inode=2), ['a4-helper']),
                                      (self.info(), ['a4-helper', 'foreign'])]:
            mocks = self.staging(stat=replacement, listdir=contents)
            mocks['stat'].side_effect = lambda name, **kw: replacement if name == 'arxcovo' else self.info()
            owned = dict(parent=12, fd=13, directory=self.info(), image=self.info())
            with self.assertRaises(RuntimeError):
                driver.remove_a4_helper(owned)
            mocks['unlink'].assert_not_called()
            mocks['rmdir'].assert_not_called()

    def test_short_write_cannot_launch(self):
        mocks = self.staging(write=3695)
        with self.assertRaisesRegex(RuntimeError, 'E_ARTIFACT'):
            driver.install_a4_helper(b'x' * 3680, {})
        mocks['fchmod'].assert_not_called()
        mocks['close'].assert_any_call(14)

    def test_uncaptured_created_directory_quarantines(self):
        mocks = self.staging()
        for owned in ({'created': True}, {'created': True, 'directory': self.info()}):
            with self.assertRaisesRegex(RuntimeError, 'E_QUARANTINE_CGROUP'):
                driver.remove_a4_helper(owned)
        mocks['unlink'].assert_not_called()
        mocks['rmdir'].assert_not_called()

    def test_fixed_input_no_follow_bounded_regular_single_link(self):
        for info in (self.info(stat.S_IFLNK), self.info(stat.S_IFIFO),
                     self.info(stat.S_IFREG, size=3681), self.info(stat.S_IFREG, links=2)):
            mocks = self.staging(open=[10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20], fstat=info)
            with patch.object(driver.Path, 'cwd', return_value=PurePosixPath('/checkout')):
                with self.assertRaisesRegex(RuntimeError, 'E_ARTIFACT'):
                    driver.a4_input('out/a4-helper', 3680)
            self.assertTrue(all(c.args[1] & driver.os.O_NOFOLLOW for c in mocks['open'].call_args_list))
            mocks['read'].assert_not_called()
        with self.assertRaisesRegex(RuntimeError, 'E_PATH'):
            driver.a4_input('../foreign', 3680)

    def test_cleanup_after_stop_before_runtime_removal(self):
        events = []
        stage = Path('/var/lib/arx-ci-123-1')
        with patch.object(driver, 'verify_runtime'), \
             patch.object(driver.subprocess, 'run', side_effect=lambda *a, **kw: events.append('stop') or SimpleNamespace(returncode=0)), \
             patch.object(driver, 'snapshot_internal'), \
             patch.object(driver.os.path, 'lexists', return_value=False), \
             patch.object(driver, 'remove_a4_helper', side_effect=lambda x: events.append('helper')), \
             patch.object(driver, 'remove_runtime', side_effect=lambda x: events.append('runtime')):
            driver.cleanup_a1a2('123-1', 'arxd.service', stage, False, None, True, [], {})
        self.assertEqual(events, ['stop', 'helper', 'runtime'])


if __name__ == '__main__':
    unittest.main()
