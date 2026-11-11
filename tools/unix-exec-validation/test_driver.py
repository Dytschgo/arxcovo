"""Pure tests: never invoke provisioning, sudo, systemd, accounts or cgroups."""
import ast
import io
import threading
import os
from pathlib import Path
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
        self.assertEqual(driver.plan(run), (Path('/opt/arx-ci-123-1'), 'arx-ci-123-1.service'))

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
        tree = ast.parse((ROOT / 'tools/unix-exec-validation/driver.py').read_text())
        run = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'run')
        self.assertTrue(any(isinstance(n, ast.Try) and n.finalbody for n in ast.walk(run)))
        source = (ROOT / 'tools/unix-exec-validation/driver.py').read_text()
        self.assertIn('deadline = time.monotonic() + 120', source)
        self.assertIn("['/usr/bin/timeout', '--signal=KILL', '120s']", source)
        self.assertIn('RuntimeMaxSec=140', source)
        self.assertIn('KillMode=control-group', source)
        self.assertIn("write_text('+memory +pids')", source)



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

    def test_harness_exact_argv_and_collisions(self):
        binary = str(self.stage / 'binary')
        (self.stage / 'artifact').write_text(binary)
        for args in ([binary], [binary, driver.TEST, '--ignored'],
                     [binary, driver.TEST, '--ignored', '--exact', '--nocapture']):
            with patch.object(driver.os, 'execve', side_effect=AssertionError('exec')):
                with self.assertRaisesRegex(RuntimeError, 'E_TEST_ARGS'):
                    driver.harness(self.stage, args)
        with self.assertRaisesRegex(RuntimeError, 'E_ACCOUNT_COLLISION'):
            driver.harness(self.stage, [binary, driver.TEST, '--ignored', '--exact'])

    def test_fixture_path_protection_failure_is_quarantined(self):
        with patch.object(driver, 'protected', side_effect=RuntimeError('E_PROTECTION')):
            self.refuse_cleanup('E_PROTECTION')

    def test_harness_positive_and_path_collision(self):
        binary = str(self.stage / 'binary')
        (self.stage / 'artifact').write_text(binary)
        args = [binary, driver.TEST, '--ignored', '--exact']
        with patch.object(driver.os.path, 'lexists', return_value=True), patch.object(driver.os, 'execve') as execute:
            with self.assertRaisesRegex(RuntimeError, 'E_FIXTURE_COLLISION'):
                driver.harness(self.stage, args)
            execute.assert_not_called()
        (self.stage / 'harness-pid').unlink()
        def missing(_):
            raise KeyError()
        with patch.dict(sys.modules, {'pwd': SimpleNamespace(getpwnam=missing), 'grp': SimpleNamespace(getgrnam=missing)}), patch.object(driver.os.path, 'lexists', return_value=False), patch.object(driver.os, 'execve') as execute:
            driver.harness(self.stage, args)
            execute.assert_called_once_with(binary, args, dict(os.environ))
            self.assertEqual((self.stage / 'harness-pid').read_text(), str(os.getpid()))

    def test_zero_test_success_is_failure(self):
        output = self.stage / 'test-output'
        output.write_text('test result: ok. 0 passed; 0 failed;')
        with self.assertRaisesRegex(RuntimeError, 'E_TEST'):
            driver.verify_test_result(self.stage, 0)
        output.write_text(f'test {driver.TEST} ... ok\ntest result: ok. 1 passed; 0 failed;')
        driver.verify_test_result(self.stage, 0)
        with self.assertRaisesRegex(RuntimeError, 'E_TEST'):
            driver.verify_test_result(self.stage, 1)
        output.write_bytes(b'x' * (1024 * 1024 + 1))
        with self.assertRaisesRegex(RuntimeError, 'E_TEST_LOG'):
            driver.verify_test_result(self.stage, 0)


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

if __name__ == '__main__':
    unittest.main()
