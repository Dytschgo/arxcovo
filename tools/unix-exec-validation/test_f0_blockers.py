"""Bounded F0 regression and compiled mutation checks; all runtime effects mocked."""
import ast
import hashlib
import io
import json
from pathlib import Path
import stat
import sys
import types
import unittest
from unittest.mock import MagicMock, patch
import driver
from test_driver import valid_env


class F0Blockers(unittest.TestCase):
    def setUp(self):
        self.enterContext(patch.object(driver.sys, 'platform', 'linux'))
        for key, val in [('phase', 'initial'), ('first', None), ('mutation', 'none'), ('cleanup', 'not_reached')]:
            self.enterContext(patch.object(driver, 'feedback_' + key, val))

    def scenario(self, body=None, cleanup=None, pre=None, service=False, success=False, artifact_refusal=False):
        stage = MagicMock()
        stage.__truediv__.return_value = stage
        stage.stat.return_value.st_size = 10
        stage.read_text.return_value = json.dumps(dict(reason='compiler-artifact', executable='/artifact', profile={'test': True}))
        stage.is_relative_to.return_value = True
        stage.exists.side_effect = [True, False] if success else None
        stage.exists.return_value = False
        def protect(path, site):
            if site == pre or (artifact_refusal and site == 'run_artifact'):
                raise driver.Refusal(site, driver.feedback_phase, [['link', False, 18]])
        def clean(_):
            self.assertEqual(driver.feedback_phase, 'final', 'cleanup phase must be final')
            self.assertEqual(driver.feedback_cleanup, 'unconfirmed', 'cleanup confirmation must follow removal')
            if cleanup:
                raise cleanup
        with patch.object(driver, 'plan', return_value=(stage, 'unit')), \
             patch.object(driver, 'Path', return_value=stage) as paths, \
             patch.object(driver.os, 'getuid', return_value=0, create=True), \
             patch.object(driver.os, 'geteuid', return_value=0, create=True), \
             patch.object(driver, 'cg_preflight'), patch.object(driver, 'protected', side_effect=protect), \
             patch.object(driver, 'copy_tree'), patch.object(driver, 'fixture_cleanup', side_effect=clean), \
             patch.object(driver.shutil, 'rmtree') as remove, \
             patch.object(driver.subprocess, 'check_output', side_effect=[b'not-found', valid_env()['GITHUB_SHA'].encode()]), \
             patch.object(driver.subprocess, 'run', side_effect=([types.SimpleNamespace(returncode=0), types.SimpleNamespace(returncode=0 if success else 1), types.SimpleNamespace(returncode=0)] if service or success else body or [types.SimpleNamespace(returncode=0 if artifact_refusal else 1)])), \
             patch.dict(driver.os.environ, valid_env(), clear=True), \
             patch.object(driver.sys, 'argv', ['driver.py', 'run']), \
             patch.object(driver.sys, 'stdout', new_callable=io.StringIO) as out, \
             patch.object(driver.sys, 'stderr', new_callable=io.StringIO) as err:
            paths.cwd.return_value = stage
            remove.avoids_symlink_attacks = True
            self.assertEqual(driver.entry(), int(not success or cleanup is not None or artifact_refusal))
            self.assertEqual(err.getvalue(), '')
            line = out.getvalue()
            driver.replay(line)
            record = json.loads(line)
            self.assertEqual(remove.call_count, int(not cleanup and not pre))
            self.assertEqual(record['schema'], 'unix-protection-f0')
            self.assertEqual(record['version'], 1)
            e = valid_env()
            self.assertEqual(record['provenance'], dict(source_sha=e['ARX_REVIEWED_SHA'], workflow_sha=e['ARX_WORKFLOW_SHA'], run=e['GITHUB_RUN_ID'], attempt=e['GITHUB_RUN_ATTEMPT']))
            return record

    def test_run_orderings(self):
        for body_code, service in [('E_BUILD', False), ('E_SERVICE_OR_TEST', True)]:
            for cleanup in [None, RuntimeError('E_QUARANTINE_CGROUP')]:
                r = self.scenario(cleanup=cleanup, service=service)
                self.assertEqual(r['first'], {'code': body_code})
                self.assertEqual(r['final'], {'code': 'E_QUARANTINE_CGROUP' if cleanup else body_code})
                self.assertEqual(r['mutation'], 'service' if service else 'staged')
                self.assertEqual(r['cleanup'], 'unconfirmed' if cleanup else 'confirmed')
                if cleanup:
                    self.assertNotEqual(r['first'], r['final'])
        r = self.scenario(pre='run_parent')
        self.assertEqual(r['first'], r['final'])
        self.assertEqual((r['mutation'], r['cleanup']), ('none', 'not_reached'))
        self.assertEqual(r['final']['site'], 'run_parent')

    def test_success_and_both_refusals(self):
        r = self.scenario(success=True)
        expected = self.valid()
        expected['provenance'] = r['provenance']
        self.assertEqual(r, expected)
        final = driver.Refusal('run_cleanup', 'final', [['link', False, 18]])
        r = self.scenario(artifact_refusal=True, cleanup=final)
        self.assertEqual(r['first'], driver.Refusal('run_artifact', 'initial', [['link', False, 18]]).record)
        self.assertEqual(r['final'], final.record)
        self.assertEqual((r['mutation'], r['cleanup']), ('staged', 'unconfirmed'))

    def test_cleanup_only_and_refusal(self):
        # Success body requires the passed marker, while cgroup absence stays false.
        # Patch exists by call role: first is passed, second is cgroup.
        r = self.scenario(cleanup=RuntimeError('E_QUARANTINE_CGROUP'), success=True)
        self.assertIsNone(r['first'])
        self.assertEqual(r['final'], {'code':'E_QUARANTINE_CGROUP'})
        self.assertEqual(r['cleanup'], 'unconfirmed')
        # A build failure followed by a real final-phase refusal proves entry separation.
        error = driver.Refusal('run_cleanup', 'final', [['dir', False, 0]])
        r = self.scenario(cleanup=error)
        self.assertEqual(r['first'], {'code': 'E_BUILD'})
        self.assertEqual(r['final'], error.record)

    def valid(self):
        return dict(schema='unix-protection-f0', version=1,
                    provenance=dict(source_sha='a'*40, workflow_sha='a'*40, run='1', attempt='1'),
                    first=None, final=None, mutation='service', cleanup='confirmed')

    def test_counterfactuals(self):
        line = driver.encode_record(self.valid())
        self.assertEqual(driver.replay(line), {})
        negatives = [line.replace('"version":1', '"version":1,"version":1'),
                     ' ' * 4096 + line, line.replace('"schema"', '"schéma"')]
        # The ASCII counterfactual must fail at transport (E_RECORD_BOUND),
        # not later schema validation (E_RECORD).
        for i, bad in enumerate(negatives):
            with self.assertRaisesRegex(RuntimeError, 'E_RECORD$' if i == 0 else 'E_RECORD_BOUND$'): driver.replay(bad)
        r = self.valid(); r['provenance']['workflow_sha'] = 'b'*40
        with self.assertRaises(RuntimeError): driver.encode_record(r)
        for change in [dict(first={'code':'E_BUILD'}), dict(cleanup='not_reached'), dict(mutation='none')]:
            r = self.valid(); r.update(change)
            with self.assertRaises(RuntimeError): driver.encode_record(r)

    def test_protected_phase_site_bound(self):
        path = MagicMock(); path.is_absolute.return_value = True; path.parts = (); path.parents = []
        path.lstat.return_value = types.SimpleNamespace(st_mode=stat.S_IFLNK | 0o777, st_uid=12)
        driver.feedback_phase = 'final'
        with self.assertRaises(driver.Refusal) as caught: driver.protected(path, 'run_cleanup')
        self.assertEqual(caught.exception.record['phase'], 'final')
        with self.assertRaisesRegex(RuntimeError, '^E_SITE$'): driver.protected(path, 'invalid')
        path.parents = [path]*32
        path.lstat.reset_mock()
        with self.assertRaises(RuntimeError): driver.protected(path, 'run_parent')
        path.lstat.assert_not_called()
        self.assertEqual(driver.decide([['link', False, 18]]), (0, 'link'))

    def test_closed_codes(self):
        for code in driver.CODES:
            self.assertEqual(driver.exception_record(RuntimeError(code)), {'code':code})
        class Sub(driver.Refusal): pass
        self.assertEqual(driver.exception_record(Sub('run_parent','initial',[['link',True,0]])), {'code':'E_RUNTIME'})
        for error, code in [(PermissionError(13, 'private'), 'E_OS'), (OSError(99999,'private'),'E_OS'),
                            (driver.subprocess.TimeoutExpired('private',1),'E_COMMAND_TIMEOUT'),
                            (driver.subprocess.CalledProcessError(1,'private'),'E_COMMAND_FAILED'),
                            (RuntimeError(['private']),'E_RUNTIME')]:
            self.assertEqual(driver.exception_record(error), {'code':code})

    def test_gate_fallback(self):
        for mode in ('guard', 'run'):
            env = valid_env(); env['CI'] = 'false'
            with patch.dict(driver.os.environ, env, clear=True), patch.object(driver.sys, 'argv', ['driver.py', mode]), patch.object(driver.sys, 'stdout', new_callable=io.StringIO) as out, patch.object(driver.sys, 'stderr', new_callable=io.StringIO) as err:
                self.assertEqual(driver.entry(), 1)
                self.assertEqual(out.getvalue(), '')
                self.assertEqual(err.getvalue(), 'unix experiment refused/failed: E_GATE_CI\n')

    def test_subprocess_sinks(self):
        tree = ast.parse(SOURCE)
        sites = []
        for node in ast.walk(tree):
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == 'protected':
                sites.append(node.args[1].value)
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and isinstance(node.func.value, ast.Name) and node.func.value.id == 'subprocess':
                keys = {k.arg for k in node.keywords}
                self.assertIn('stderr', keys)
                if node.func.attr != 'check_output': self.assertIn('stdout', keys)
        self.assertEqual(len(sites), 13)
        self.assertEqual(set(sites), set(driver.SITES))

SOURCE = Path(__file__).with_name('driver.py').read_text()


MUTANTS = {
    'run_drops_first_capture': ("feedback_first = exception_record(error)\n        raise\n    finally:", "feedback_first = None\n        raise\n    finally:"),
    'run_drops_final_phase': ("feedback_phase = 'final'", "feedback_phase = 'initial'"),
    'cleanup_confirmed_early': ("feedback_cleanup = 'unconfirmed'", "feedback_cleanup = 'confirmed'"),
    'mutation_staged_dropped': ("feedback_mutation = 'staged'", "feedback_mutation = 'none'"),
    'mutation_service_dropped': ("feedback_mutation = 'service'", "feedback_mutation = 'staged'"),
    'protected_ignores_phase': ("raise Refusal(site, feedback_phase, triples)", "raise Refusal(site, 'initial', triples)"),
    'replay_dup_keys_allowed': ("require(key not in out, 'E_RECORD')", "require(True, 'E_RECORD')"),
    'replay_input_bound_dropped': ("len(line) <= MAX_LINE", "True"),
    'replay_ascii_dropped': ("line.isascii()", "True"),
    'sha_equality_dropped': ("p['source_sha'] == p['workflow_sha']", "True"),
    'protected_bound_dropped': ("len(items) <= MAX_TRIPLES", "True"),
    'conjunct_owner_first': ("(('link', kind == 'link'),\n                                   ('owner', not owner_zero)", "(('owner', not owner_zero),\n                                   ('link', kind == 'link')"),
    'entry_first_is_final': ("feedback_first, final), end='')", "final, final), end='')"),
    'refusal_exact_type_relaxed': ("type(error) is Refusal", "isinstance(error, Refusal)"),
    'site_token_unchecked': ("require(site in SITES, 'E_SITE')", "require(True, 'E_SITE')"),
}


def mutations():
    baseline = unittest.TestResult()
    unittest.defaultTestLoader.loadTestsFromTestCase(F0Blockers).run(baseline)
    if not baseline.wasSuccessful():
        raise AssertionError('baseline must pass before mutations')
    original = driver
    digest = hashlib.sha256(Path(__file__).with_name('driver.py').read_bytes()).hexdigest()
    for name, (before, after) in MUTANTS.items():
        if SOURCE.count(before) != 1:
            raise AssertionError('mutation anchor: ' + name)
        changed = SOURCE.replace(before, after)
        code = compile(changed, '<in-memory-f0-mutant>', 'exec')
        module = types.ModuleType('f0_mutant')
        exec(code, module.__dict__)
        globals()['driver'] = module
        try:
            suite = unittest.defaultTestLoader.loadTestsFromTestCase(F0Blockers)
            result = unittest.TestResult()
            suite.run(result)
            if result.errors or not result.failures:
                raise AssertionError('no assertion kill: ' + name)
            print('KILLED ' + name + ' assertion=' + ','.join(t._testMethodName for t, _ in result.failures))
        finally:
            globals()['driver'] = original
    if hashlib.sha256(Path(__file__).with_name('driver.py').read_bytes()).hexdigest() != digest:
        raise AssertionError('source restoration hash mismatch')
    print('source unchanged sha256=' + digest)


if __name__ == '__main__':
    if sys.argv[1:] == ['--mutants']:
        mutations()
    else:
        unittest.main()
