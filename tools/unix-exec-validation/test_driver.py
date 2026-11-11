"""Pure tests: never invoke provisioning, sudo, systemd, accounts or cgroups."""
import ast
import os
from pathlib import Path
import subprocess
import sys
import unittest
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
        with patch.object(driver, 'cg_preflight', side_effect=AssertionError('probe')):
            with patch.object(driver.subprocess, 'run', side_effect=AssertionError('mutation')):
                with self.assertRaises(RuntimeError):
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


if __name__ == '__main__':
    unittest.main()
