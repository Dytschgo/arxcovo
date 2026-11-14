"""Finite in-memory assertion mutants. Only the audited pure/mock subset runs."""
import hashlib
import io
from pathlib import Path
import types
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import driver
import test_driver
import test_f0_blockers
import test_f0b_transport

CASES = [
 ('restore writable parent', "return Path('/var/lib/arx-ci-' + run), 'arxd.service'", "return Path('/opt/arx-ci-' + run), 'arxd.service'", 'test_native_writable_parent_regression'),
 ('skip parent protection', "protected(stage.parent, 'run_parent')", 'pass', 'test_parent_checked_before_exclusive_stage_creation'),
 ('reuse staging directory', 'stage.mkdir(mode=0o700)', 'stage.mkdir(mode=0o700, exist_ok=True)', 'test_parent_checked_before_exclusive_stage_creation'),
 ('build after start', '        build_env = cargo_env(stage)', "        call(['/usr/bin/systemctl', 'start', '--no-block', unit])\n        build_env = cargo_env(stage)", 'F0Blockers.test_run_orderings'),
 ('fixed unit', "return Path('/var/lib/arx-ci-' + run), 'arxd.service'", "return Path('/var/lib/arx-ci-' + run), 'other.service'", 'test_launch_contract'),
 ('wrong test', "TEST = 'exec::disposable::disposable_linux_a1_a2'", "TEST = 'exec::disposable::disposable_linux_cross_account'", 'test_launch_contract'),
 ('fork instead of exec', 'os.execve(binary, args, env)', 'subprocess.run(args, env=env)', 'test_launcher_exec_and_single_capture_owner'),
 ('cargo in unit', "return [binary, TEST, '--ignored', '--exact', '--test-threads=1', '--color=never']", "return ['/usr/bin/cargo', TEST, '--ignored', '--exact', '--test-threads=1', '--color=never']", 'test_launch_contract'),
 ('broaden override', r'Restart=no\nRuntimeMaxSec=140', r'Restart=always\nRuntimeMaxSec=140', 'test_launch_contract'),
 ('omit MainPID', "require(state['MainPID'] == pid, 'E_SERVICE_OR_TEST')", "require(True, 'E_SERVICE_OR_TEST')", 'test_mainpid_and_placement_required'),
 ('zero tests', "re.findall(r'^test result: ok\\. ([0-9]+) passed; ([0-9]+) failed;', text, re.M) == [('1', '0')]", "True", 'test_exact_summary'),
 ('omit deadline', "require(time.monotonic() < deadline, 'E_TEST_TIMEOUT')", "require(True, 'E_TEST_TIMEOUT')", 'test_deadline_required'),
 ('cleanup another unit', "require(unit == 'arxd.service', 'E_UNIT_COLLISION')", "require(True, 'E_UNIT_COLLISION')", 'test_cleanup_cannot_stop_another_unit'),
 ('bypass collision', "props.get('LoadState') == 'not-found'", 'True', 'test_collision_refuses_before_mutation'),
]

def run_case(module, name):
    owner = test_f0_blockers if name.startswith('F0Blockers.') else test_driver
    with patch.object(owner, 'driver', module):
        suite = unittest.defaultTestLoader.loadTestsFromName(name if owner is test_f0_blockers else 'A1A2.' + name, owner)
        result = unittest.TextTestRunner(stream=io.StringIO()).run(suite)
    return result

def safe_suite():
    # Guard CLI, RuntimeGuards and CopyGuards deliberately excluded. CgroupScan
    # uses mocked os.walk/Path.lstat; EntryRun mocks every host effect.
    names = [n for n in unittest.defaultTestLoader.getTestCaseNames(test_driver.Guards)
             if n != 'test_cli_wrong_platform_refuses']
    suite = unittest.TestSuite(test_driver.Guards(n) for n in names)
    for case in (test_driver.A1A2, test_driver.CgroupScan, test_f0_blockers.F0Blockers):
        suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(case))
    suite.addTests(test_f0b_transport.suite())
    return suite


def main():
    baseline = unittest.TextTestRunner().run(safe_suite())
    assert baseline.wasSuccessful(), 'audited baseline must be green'

    path = Path(driver.__file__)
    source = path.read_text()
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    for label, before, after, name in CASES:
        baseline = run_case(driver, name)
        assert baseline.wasSuccessful(), 'baseline: ' + name
        assert source.count(before) == 1, 'anchor: ' + label
        module = types.ModuleType('driver_mutant')
        exec(compile(source.replace(before, after), '<in-memory-mutant>', 'exec'), module.__dict__)
        result = run_case(module, name)
        assert not result.errors, 'errors earn no credit: ' + label + str(result.errors)
        assert result.failures and all(t.id().endswith(name) or name in t.id() for t,_ in result.failures), 'SURVIVED: ' + label
        print('KILLED ' + label + ' assertion=' + (name if name.startswith('F0Blockers.') else 'A1A2.'+name))
    assert hashlib.sha256(path.read_bytes()).hexdigest() == digest, 'source changed'
    print('source unchanged sha256=' + digest)




def rust_mutants():
    """Compile stdin slices of the real pure ports; never include a native adapter.

    The fixture helper is test-only and shared with the ignored Linux test.
    Exclude fake.rs H15 and every native/CLI test from the generated harness.
    No source file is mutated or written, including for mutants.
    """
    root = Path(__file__).resolve().parents[2]
    base = root / 'crates/arxd/src'
    paths = [base / p for p in ('exec/hierarchy/parse.rs', 'exec/hierarchy/setup.rs',
                                'exec/hierarchy/fake.rs', 'development/types.rs')]
    fingerprints = [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths]
    parse, setup, fake, types_source = [p.read_text() for p in paths]
    parse = parse[:parse.index('pub(crate) fn bin_alias')]
    parse = parse.replace('use crate::service::{config::Rights, preflight::ExecutionUnavailable};\n', '').replace('use serde::Deserialize;\n', '')
    cancellation = types_source[types_source.index('#[derive(Clone, Default)]'):types_source.index('\n}', types_source.index('impl Cancellation'))+2]
    prefix = fake[:fake.index('#[test]')]
    focused = fake[fake.index('#[test]\nfn a1a2_production_setup_before_acceptance'):]
    source = ('#![allow(dead_code)]\nmod development { use std::sync::{Arc,atomic::{AtomicBool,Ordering}};\n' + cancellation +
              '\n}\nmod hierarchy { pub(crate) use parse::locate;\nmod parse {\n' + parse +
              '\n}\nmod setup {\n' + setup + '\n}\nmod fake {\n' + prefix + focused + '\n}}')
    mutants = [
      ('omit setup', 'setup(fs, &location, clock, token)', 'SetupOutcome::Ready(SetupEvidence { leaves_removed: 0, runs_created: true, unit_written: true, runs_written: true })', 'a1a2_production_setup_before_acceptance'),
      ('runs before setup', 'let SetupOutcome::Ready(evidence) = setup(fs, &location, clock, token)', 'let _ = fs.mkdir_runs(&fs.open_child(&fs.open_root().unwrap(), b"arxd.service").unwrap()); let SetupOutcome::Ready(evidence) = setup(fs, &location, clock, token)', 'a1a2_production_setup_before_acceptance'),
      ('continue Unavailable', 'let SetupOutcome::Ready(evidence) = setup(fs, &location, clock, token) else {\n        return false;\n    };', 'let evidence = match setup(fs, &location, clock, token) { SetupOutcome::Ready(e) => e, SetupOutcome::Unavailable(_) => return true };', 'a1a2_refusals_never_release_acceptance'),
      ('skip A2 postcondition', 'info.uid != 0 || info.mode & 0o777 != 0o700 || info.magic != CGROUP_MAGIC', 'info.uid != 0 || info.magic != CGROUP_MAGIC', 'a1a2_cancel_timeout_and_postcondition_refuse'),
    ]
    artifacts = root / 'target'
    artifacts.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='a1a2-pure-', dir=artifacts) as tmp:
        exe = Path(tmp) / 'pure-tests.exe'
        def compile_run(code):
            built = subprocess.run(['rustc','--edition=2024','--test','-','-o',str(exe)], input=code, text=True, capture_output=True, timeout=60)
            assert built.returncode == 0, built.stderr
            return subprocess.run([str(exe),'a1a2_','--test-threads=1'], capture_output=True, text=True, timeout=20)
        baseline = compile_run(source)
        assert baseline.returncode == 0, baseline.stdout + baseline.stderr
        print('Rust pure baseline: 3 passed')
        for label, before, after, test in mutants:
            assert source.count(before) == 1, 'Rust anchor: ' + label
            result = compile_run(source.replace(before,after))
            assert result.returncode == 101 and 'panicked at' in result.stdout and 'assertion' in result.stdout and ('hierarchy::fake::'+test+' ... FAILED') in result.stdout, 'no assertion credit: '+label+' '+result.stdout+result.stderr
            print('KILLED '+label+' assertion='+test)
    assert [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths] == fingerprints
    print('Rust source fingerprints unchanged')

if __name__ == '__main__':
    main()
    rust_mutants()
