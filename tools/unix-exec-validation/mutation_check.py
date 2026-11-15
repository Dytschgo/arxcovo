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
 ('restore positive A4 selection', "TEST = 'exec::disposable::a4::disposable_linux_a4_omitted_kill'", "TEST = 'exec::disposable::a4::disposable_linux_a4'", 'test_source_selection_is_a4_omitted_kill'),
 ('omit closed A5', "'exec::disposable::disposable_linux_a5'), 'E_TEST_ARGS')", "'exec::disposable::disposable_linux_a4'), 'E_TEST_ARGS')", 'test_closed_mapping'),
 ('A5 without Standard', "return TEST != 'exec::disposable::disposable_linux_a3_none'", "return TEST not in ('exec::disposable::disposable_linux_a3_none', 'exec::disposable::disposable_linux_a5')", 'test_closed_mapping'),
 ('A5 argv selects A3', "return [binary, TEST, '--ignored', '--exact', '--test-threads=1', '--color=never']", "return [binary, 'exec::disposable::disposable_linux_a3_none' if TEST == 'exec::disposable::disposable_linux_a5' else TEST, '--ignored', '--exact', '--test-threads=1', '--color=never']", 'test_closed_mapping'),
 ('admit arbitrary disposable names', "require(TEST in (", "require(TEST.startswith('exec::disposable::disposable_linux_') or TEST in (", 'test_closed_selection_before_effects'),
 ('summary uses historical None', "f'test {TEST} ... ok'", "'test exec::disposable::disposable_linux_a3_none ... ok'", 'test_all_four_exact_summaries'),
 ('restore writable parent', "return Path('/var/lib/arx-ci-' + run), 'arxd.service'", "return Path('/opt/arx-ci-' + run), 'arxd.service'", 'test_native_writable_parent_regression'),
 ('skip parent protection', "protected(stage.parent, 'run_parent')", 'pass', 'test_parent_checked_before_exclusive_stage_creation'),
 ('reuse staging directory', 'stage.mkdir(mode=0o700)', 'stage.mkdir(mode=0o700, exist_ok=True)', 'test_parent_checked_before_exclusive_stage_creation'),
 ('build after start', '        build_env = cargo_env(stage)', "        call(['/usr/bin/systemctl', 'start', '--no-block', unit])\n        build_env = cargo_env(stage)", 'F0Blockers.test_run_orderings'),
 ('fixed unit', "return Path('/var/lib/arx-ci-' + run), 'arxd.service'", "return Path('/var/lib/arx-ci-' + run), 'other.service'", 'test_launch_contract'),
 ('wrong test', "return [binary, TEST, '--ignored', '--exact', '--test-threads=1', '--color=never']", "return [binary, 'exec::disposable::disposable_linux_cross_account', '--ignored', '--exact', '--test-threads=1', '--color=never']", 'test_launch_contract'),
 ('fork instead of exec', 'os.execve(binary, args, env)', 'subprocess.run(args, env=env)', 'test_launcher_exec_and_single_capture_owner'),
 ('cargo in unit', "return [binary, TEST, '--ignored', '--exact', '--test-threads=1', '--color=never']", "return ['/usr/bin/cargo', TEST, '--ignored', '--exact', '--test-threads=1', '--color=never']", 'test_launch_contract'),
 ('broaden override', r'Restart=no\nRuntimeMaxSec=140', r'Restart=always\nRuntimeMaxSec=140', 'test_launch_contract'),
 ('omit MainPID', "require(state['MainPID'] == pid, 'E_SERVICE_OR_TEST')", "require(True, 'E_SERVICE_OR_TEST')", 'test_mainpid_and_placement_required'),
 ('zero tests', "re.findall(r'^test result: ok\\. ([0-9]+) passed; ([0-9]+) failed;', text, re.M) == [('1', '0')]", "True", 'test_exact_summary'),
 ('omit deadline', "require(time.monotonic() < deadline, 'E_TEST_TIMEOUT')", "require(True, 'E_TEST_TIMEOUT')", 'test_deadline_required'),
 ('cleanup another unit', "require(unit == 'arxd.service', 'E_UNIT_COLLISION')", "require(True, 'E_UNIT_COLLISION')", 'test_cleanup_cannot_stop_another_unit'),
 ('bypass collision', "props.get('LoadState') == 'not-found'", 'True', 'test_collision_refuses_before_mutation'),
 ('swap profile', "return TEST != 'exec::disposable::disposable_linux_a3_none'", "return TEST == 'exec::disposable::disposable_linux_a3_none'", 'test_profile_pairings_and_install_bytes'),
 ('omit profile', '    if present:\n        files.append', '    if False:\n        files.append', 'test_profile_pairings_and_install_bytes'),
 ('skip exact drop-ins', "props.get('DropInPaths') == expected", 'True', 'test_exact_runtime_attribution'),
 ('skip byte comparison', "private_read(path).encode('ascii') == data", 'True', 'test_exact_runtime_attribution'),
 ('skip sole PID', "require(tasks == (pid + '\\n').encode('ascii'), 'E_UNIT_CGROUP')", "require(True, 'E_UNIT_CGROUP')", 'test_sole_supervisor_pid_required'),
 ('skip held attribution', "            verify_runtime(stage, stage.name.removeprefix('arx-ci-'), state)", '            pass', 'test_ack_before_exit_before_summary'),
 ('skip prestart attribution', '        verify_runtime(stage, run_id)\n        call', '        pass\n        call', 'test_runtime_attribution_lifecycle'),
 ('skip cleanup attribution', '        verify_runtime(stage, run_id)\n        stopped', '        pass\n        stopped', 'test_runtime_attribution_lifecycle'),
 ('skip cleanup identity', '        for path, info in owned:\n            same_object(path, info)', '        for path, info in owned:\n            pass', 'test_cleanup_replacement_quarantines'),

]

def run_case(module, name):
    owner = test_f0_blockers if name.startswith('F0Blockers.') else test_driver
    with patch.object(owner, 'driver', module):
        suite = unittest.defaultTestLoader.loadTestsFromName(name if owner is test_f0_blockers or name.startswith('A4Delivery.') else 'A1A2.' + name, owner)
        result = unittest.TextTestRunner(stream=io.StringIO()).run(suite)
    return result


CASES += [
    ('omit closed A4', "                     'exec::disposable::a4::disposable_linux_a4',\n", '', 'A4Delivery.test_closed_mapping_and_default'),
    ('A4 without Standard', "return TEST != 'exec::disposable::disposable_linux_a3_none'", "return TEST not in ('exec::disposable::disposable_linux_a3_none', 'exec::disposable::a4::disposable_linux_a4')", 'A4Delivery.test_closed_mapping_and_default'),
    ('omit A4 source recipe hash', "require(hashlib.sha256(inputs[name]).hexdigest() == digest, 'E_ARTIFACT')", "require(True, 'E_ARTIFACT')", 'A4Delivery.test_source_and_recipe_independent_of_image'),
    ('omit image size and hash', "require(len(data) == A4_IMAGE_SIZE and\n            hashlib.sha256(data).hexdigest() == A4_IMAGE_SHA, 'E_ARTIFACT')", "require(True, 'E_ARTIFACT')", 'A4Delivery.test_size_and_hash_both_required_before_open'),
    ('omit staged held hash', '        a4_image(os.read(fd, A4_IMAGE_SIZE + 1))', '        pass', 'A4Delivery.test_stage_descriptor_bytes_and_owned_cleanup'),
    ('omit helper short write fence', "require(os.write(fd, data) == len(data), 'E_ARTIFACT')", "os.write(fd, data)", 'A4Delivery.test_short_write_cannot_launch'),
    ('adopt replaced helper directory', "same('arxcovo', parent, owned['directory'])", 'pass', 'A4Delivery.test_replaced_or_extra_asset_quarantines'),
]
CASES += [
    ('skip negative finally cleanup', '        remove_a4_helper(helper_owned)', '        pass', 'A4Delivery.test_negative_cleanup_failure_refuses'),
    ('omit negative literal', "                     'exec::disposable::a4::disposable_linux_a4_omitted_kill',\n", '', 'A4Delivery.test_negative_literal_marker_and_lifecycle'),
    ('allow missing negative marker', "text.count('ARX_A4_OMITTED_S7_REJECTED\\n') == 1", "text.count('ARX_A4_OMITTED_S7_REJECTED\\n') <= 1", 'A4Delivery.test_negative_literal_marker_and_lifecycle'),
    ('allow duplicate negative marker', "text.count('ARX_A4_OMITTED_S7_REJECTED\\n') == 1", "text.count('ARX_A4_OMITTED_S7_REJECTED\\n') >= 1", 'A4Delivery.test_negative_literal_marker_and_lifecycle'),
]


def safe_suite():
    # Guard CLI, RuntimeGuards and CopyGuards deliberately excluded. CgroupScan
    # uses mocked os.walk/Path.lstat; EntryRun mocks every host effect.
    names = [n for n in unittest.defaultTestLoader.getTestCaseNames(test_driver.Guards)
             if n != 'test_cli_wrong_platform_refuses']
    suite = unittest.TestSuite(test_driver.Guards(n) for n in names)
    for case in (test_driver.A1A2, test_driver.CgroupScan, test_f0_blockers.F0Blockers):
        suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(case))
    suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.A4Delivery))
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
        print('KILLED ' + label + ' assertion=' + (name if name.startswith(('F0Blockers.', 'A4Delivery.')) else 'A1A2.'+name))
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
                                'exec/hierarchy/fake.rs', 'development/types.rs',
                                'exec/hierarchy/a4.rs', 'exec/hierarchy/a5.rs')]
    fingerprints = [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths]
    parse, setup, fake, types_source, a4, a5 = [p.read_text() for p in paths]
    parse = parse[:parse.index('pub(crate) fn bin_alias')]
    parse = parse.replace('use crate::service::{config::Rights, preflight::ExecutionUnavailable};\n', '').replace('use serde::Deserialize;\n', '')
    cancellation = types_source[types_source.index('#[derive(Clone, Default)]'):types_source.index('\n}', types_source.index('impl Cancellation'))+2]
    prefix = fake[:fake.index('#[test]')]
    # Inline only the pure test modules. Image-pin hashing is covered by the
    # Cargo pure tests; no native adapter or ignored fixture enters this binary.
    a4 = 'use super::*;\n' + a4[a4.index('pub(crate) fn remaining'):]
    pin_start = a4.index('#[test]\nfn source_and_image_pins')
    a4 = a4[:pin_start] + a4[a4.index('struct MockChild', pin_start):]
    for name, body in [('a4', a4), ('a5', a5)]:
        declaration = f'#[path = "{name}.rs"]\npub(crate) mod {name};\n'
        assert prefix.count(declaration) == 1, name + ' module anchor'
        prefix = prefix.replace(declaration, 'pub(crate) mod ' + name + ' {\n' + body.replace('//!', '//') + '\n}\n')
    focused = fake[fake.index('#[test]\nfn a1a2_production_setup_before_acceptance'):]
    source = ('#![allow(dead_code)]\nmod development { use std::sync::{Arc,atomic::{AtomicBool,Ordering}};\n' + cancellation +
              '\n}\nmod hierarchy { pub(crate) use parse::locate;\nmod parse {\n' + parse +
              '\n}\nmod setup {\n' + setup + '\n}\nmod fake {\n' + prefix + focused + '\n}}')
    mutants = [
      ('omit setup', 'let SetupOutcome::Ready(evidence) = setup(fs, &location, clock, token)', 'let SetupOutcome::Ready(evidence) = SetupOutcome::Ready(SetupEvidence { leaves_removed: 0, runs_created: true, unit_written: true, runs_written: true })', 'a1a2_production_setup_before_acceptance'),
      ('runs before setup', 'let SetupOutcome::Ready(evidence) = setup(fs, &location, clock, token)', 'let _ = fs.mkdir_runs(&fs.open_child(&fs.open_root().unwrap(), b"arxd.service").unwrap()); let SetupOutcome::Ready(evidence) = setup(fs, &location, clock, token)', 'a1a2_production_setup_before_acceptance'),
      ('continue Unavailable', 'let SetupOutcome::Ready(evidence) = setup(fs, &location, clock, token) else {\n        return false;\n    };', 'let evidence = match setup(fs, &location, clock, token) { SetupOutcome::Ready(e) => e, SetupOutcome::Unavailable(_) => return true };', 'a1a2_refusals_never_release_acceptance'),
      ('skip A2 postcondition', 'info.uid != 0 || info.mode & 0o777 != 0o700 || info.magic != CGROUP_MAGIC', 'info.uid != 0 || info.magic != CGROUP_MAGIC', 'a1a2_cancel_timeout_and_postcondition_refuse'),
    ]
    negative = 'a4::omitted_s7_observes_real_setup_timeout_without_reap_or_removal'
    mutants += [
        ('forward omitted S7', 'self.omitted.set(self.omitted.get() + 1);', 'self.omitted.set(self.omitted.get() + 1); self.forward_write(d, f, b)?;', negative),
        ('omit shared acceptance assertion', 'assert!(accepted);', 'let _ = accepted;', negative),
        ('swallow KillTimeout', 'check(clock.now().saturating_sub(start) < DEADLINE, KillTimeout)?;', 'if clock.now().saturating_sub(start) >= DEADLINE { break; }', negative),
        ('reap before rejection', 'accepted: accepted && child.killed()', 'accepted: child.killed() && accepted', negative),
    ]
    selected = ('a1a2_production_setup_before_acceptance', 'a1a2_refusals_never_release_acceptance',
                'a1a2_cancel_timeout_and_postcondition_refuse', negative,
                'a4::populated_leaf_requires_s7_and_one_removal',
                'a4::omitted_s7_rejects_wrong_targets_bytes_and_duplicate_attempts',
                'a5::empty_orphan_removed_before_acceptance')
    artifacts = root / 'target'
    artifacts.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='a1a2-pure-', dir=artifacts) as tmp:
        exe = Path(tmp) / 'pure-tests.exe'
        def compile_run(code):
            built = subprocess.run(['rustc','--edition=2024','--test','-','-o',str(exe)], input=code, text=True, capture_output=True, timeout=60)
            assert built.returncode == 0, built.stderr
            return [subprocess.run([str(exe),'hierarchy::fake::'+test,'--exact','--test-threads=1'], capture_output=True, text=True, timeout=20) for test in selected]
        baseline = compile_run(source)
        assert all(r.returncode == 0 and '1 passed; 0 failed' in r.stdout for r in baseline), [(r.stdout,r.stderr) for r in baseline]
        print('Rust pure baseline: 7 passed')
        for label, before, after, test in mutants:
            scope = focused if test.startswith('a1a2_') else source
            assert scope.count(before) == 1, 'Rust anchor: ' + label
            results = compile_run(source.replace(scope, scope.replace(before, after)))
            result = next(r for r in results if ('hierarchy::fake::'+test+' ...') in r.stdout)
            assert result.returncode == 101 and 'panicked at' in result.stdout and 'assertion' in result.stdout and ('hierarchy::fake::'+test+' ... FAILED') in result.stdout, 'no assertion credit: '+label+' '+result.stdout+result.stderr
            print('KILLED '+label+' assertion='+test)
    assert [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths] == fingerprints
    print('Rust source fingerprints unchanged')

def sudo_nnp_source_checks():
    """Only pure extracted Rust and SudoNnp mocks; no native module is linked.

    Call-graph audit: extracted items contain scalar comparisons/assignments and
    assertions only. rustc compiles stdin; generated test binaries have no host
    adapters. Python SudoNnp patches every filesystem/account/process effect.
    """
    root = Path(__file__).resolve().parents[2]
    files = [root / 'crates/arxd/src/exec/disposable/sudo_nnp.rs',
             root / 'crates/arxd/src/unix_shim_rights_linux.rs',
             root / 'crates/arxd/src/unix_shim_rights_linux_tests.rs']
    original = [p.read_bytes() for p in files]
    controller, linux, tests = [b.decode('utf-8') for b in original]
    facts = controller[controller.index('#[derive(Clone, Copy)]'):controller.index('fn output(')]
    fact_test = controller[controller.index('#[test]\nfn a7_a11_discriminators'):controller.index('#[test]\nfn a7_a11_delivery_stays_closed')]
    observation = linux[linux.index('#[cfg(test)]\n#[derive(Clone, Copy, Default)]'):linux.index('#[cfg(test)]\nstd::thread_local!')]
    scalar_test = tests[tests.index('#[test]\nfn s14_sudo_nnp_scalar_discriminators'):]
    sources = [(facts + fact_test, 'a7_a11_discriminators',
                ['f.' + field for field in ('identity', 'exited', 'bounded', 'records', 'rights', 'c1',
                 'alias_skipped', 'c3_unopened', 'path', 'cloexec', 'regular', 'exec_eof',
                 'nonzero', 'both_policy_cases', 'cleanup')]),
               (observation + scalar_test, 's14_sudo_nnp_scalar_discriminators',
                ['o.' + field for field in ('c1', 'alias_skipped', 'path', 'cloexec', 'regular',
                  'same_inode', 'exec_eof', 'nonzero')] +
                ['!o.c3_opened', 'o.uid == uid', 'o.gid == gid', 'uid != 0', 'gid != 0'])]
    with tempfile.TemporaryDirectory(prefix='a7-a11-pure-') as tmp:
        exe = Path(tmp) / 'pure.exe'
        for source, test, terms in sources:
            def check(code):
                result = subprocess.run(['rustc', '--edition=2024', '--test', '-', '-o', str(exe)],
                                        input='#![allow(dead_code)]\n' + code, text=True, capture_output=True, timeout=60)
                assert result.returncode == 0, result.stderr
                return subprocess.run([str(exe), test, '--exact'], text=True, capture_output=True, timeout=20)
            baseline = check(source)
            assert baseline.returncode == 0 and '1 passed; 0 failed' in baseline.stdout, baseline.stdout
            print('PASS pure ' + test)
            for term in terms:
                # Operate only within acceptance function, never its test setup.
                end = source.index('#[test]')
                predicate, assertions = source[:end], source[end:]
                assert predicate.count(term) == 1, term
                result = check(predicate.replace(term, 'true') + assertions)
                assert result.returncode == 101 and 'assertion' in result.stdout, term + result.stdout + result.stderr
                print('KILLED scalar ' + term)
    # Compile the actual bounded record parsers against the already-built locked
    # offline serde_json dependency. No probe, Runner, or native adapter included.
    tools_source = (root / 'crates/arxd/src/development/mod.rs').read_text()
    tools = tools_source[tools_source.index('pub const TOOLS:'):tools_source.index('const ENVIRONMENT:')]
    parsers = controller[controller.index('fn probe_records('):controller.index('fn valid_run(')]
    parser_test = controller[controller.index('#[test]\nfn a7_a11_record_discriminators'):controller.index('#[test]\nfn a7_a11_run_discriminators')]
    rlibs = sorted((root / 'target/debug/deps').glob('libserde_json-*.rlib'))
    assert rlibs, 'build locked offline Cargo test dependencies first'
    print('offline parser dependency: ' + rlibs[0].name)
    parser_source = 'mod development { ' + tools + '}\n' + parsers + parser_test
    with tempfile.TemporaryDirectory(prefix='a7-a11-records-') as tmp:
        exe = Path(tmp) / 'records.exe'
        def check_records(code):
            built = subprocess.run(['rustc', '--edition=2024', '--test', '-', '-L',
                                    'dependency=' + str(root / 'target/debug/deps'), '--extern',
                                    'serde_json=' + str(rlibs[0]), '-o', str(exe)], input=code,
                                   text=True, capture_output=True, timeout=60)
            assert built.returncode == 0, built.stderr
            return subprocess.run([str(exe), 'a7_a11_record_discriminators', '--exact'],
                                  text=True, capture_output=True, timeout=20)
        result = check_records(parser_source)
        assert result.returncode == 0 and '1 passed; 0 failed' in result.stdout, result.stdout
        print('PASS actual bounded record parsers')
        before = 'lines.last() == Some(&r#"{"rights":{"no_new_privs":true,"sudo":"exit_nonzero"}}"#)'
        assert parser_source.count(before) == 1
        result = check_records(parser_source.replace(before, 'true'))
        assert result.returncode == 101 and 'assertion' in result.stdout, result.stdout
        print('KILLED accept unknown/zero/absent rights')
        for term in ("!text.ends_with('\\n')", 'lines.len() != crate::development::TOOLS.len() + 1'):
            assert parsers.count(term) == 1, term
            result = check_records(parser_source.replace(parsers, parsers.replace(term, 'false')))
            assert result.returncode == 101 and 'assertion' in result.stdout, term + result.stdout
            print('KILLED record ' + term)


    exec_source = (root / 'crates/arxd/src/exec/mod.rs').read_text()
    types_source = exec_source[exec_source.index('#[derive(Clone)]\npub struct OwnerConfig'):exec_source.index('#[derive(Debug, PartialEq, Eq)]\npub enum ExecError')]
    output_source = controller[controller.index('fn output('):controller.index('fn probe_records(')]
    valid_source = controller[controller.index('fn valid_run('):controller.index('#[test]\n#[ignore')]
    run_test = controller[controller.index('#[test]\nfn a7_a11_run_discriminators'):]
    with tempfile.TemporaryDirectory(prefix='a7-a11-results-') as tmp:
        exe = Path(tmp) / 'results.exe'
        result_source = ('use std::{collections::BTreeMap,path::PathBuf,time::Duration};\n' +
                         types_source + output_source + valid_source + run_test)
        def check_result(code):
            built = subprocess.run(['rustc','--edition=2024','--test','-','-o',str(exe)],
                                   input=code,text=True,capture_output=True,timeout=60)
            assert built.returncode == 0, built.stderr
            return subprocess.run([str(exe),'a7_a11_run_discriminators','--exact'],
                                  text=True,capture_output=True,timeout=20)
        result = check_result(result_source)
        assert result.returncode == 0 and '1 passed; 0 failed' in result.stdout, result.stdout
        print('PASS actual RunResult identity/status/capture/cleanup predicates')
        for term in ('run.outcome == Outcome::Exited', 'run.exit_code == Some(0)',
                     'run.tree_boundary == TreeBoundary::LinuxCgroupV2',
                     'run.tree_cleanup_confirmed', '!residue'):
            assert valid_source.count(term) == 1, term
            code = result_source.replace(valid_source, valid_source.replace(term, 'true'))
            result = check_result(code)
            assert result.returncode == 101 and 'assertion' in result.stdout, term + result.stdout
            print('KILLED result ' + term)
        identity = valid_source[valid_source.index('run.identity'):valid_source.index('&& run.outcome')]
        result = check_result(result_source.replace(valid_source, valid_source.replace(identity, 'true\n        ')))
        assert result.returncode == 101 and 'assertion' in result.stdout, result.stdout
        print('KILLED actual wrong identity')
        for term in ('run.stdout.dropped_bytes != 0', 'run.stderr.dropped_bytes != 0',
                     '!run.stderr.head.is_empty()', '!run.stderr.tail.is_empty()',
                     'run.stdout.head.len() + run.stdout.tail.len() > 4096'):
            assert output_source.count(term) == 1, term
            result = check_result(result_source.replace(output_source, output_source.replace(term, 'false')))
            assert result.returncode == 101 and 'assertion' in result.stdout, term + result.stdout
            print('KILLED capture ' + term)

    suite = unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.SudoNnp)
    result = unittest.TextTestRunner().run(suite)
    assert result.wasSuccessful()
    source = Path(driver.__file__).read_text()
    cases = [
        ("raise RuntimeError('E_FIXTURE_COLLISION')", 'pass', 'test_collision_no_adoption'),
        ("require(unit_gone is True, 'E_QUARANTINE_CGROUP')", 'pass', 'test_cleanup_requires_stop_and_matching_identity'),
        ("if index == 0:", "if False:", "test_two_cases_and_owned_cleanup"),
    ]
    for before, after, name in cases:
        assert source.count(before) == 1
        module = types.ModuleType('sudo_nnp_mutant')
        exec(compile(source.replace(before, after), '<in-memory-mutant>', 'exec'), module.__dict__)
        with patch.object(test_driver, 'driver', module):
            suite = unittest.TestSuite([test_driver.SudoNnp(name)])
            result = unittest.TextTestRunner(stream=io.StringIO()).run(suite)
        # Errors do not count as a killed assertion.
        assert result.failures and not result.errors, name + str(result.errors)
        print('KILLED mock ' + name)
    assert [p.read_bytes() for p in files] == original
    print('A7/A11 pure and mock source bytes unchanged')


if __name__ == '__main__':
    main()
    rust_mutants()
