"""Finite in-memory assertion mutants. Only the audited pure/mock subset runs."""
import hashlib
import io
import os
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
 ('restore positive A4 selection', "TEST = 'exec::disposable::sudo_nnp::disposable_linux_a7_a11'", "TEST = 'exec::disposable::a4::disposable_linux_a4'", 'test_source_selection_is_combined_a7'),
 ('omit closed A5', "'exec::disposable::disposable_linux_a5',", "'exec::disposable::disposable_linux_a4',", 'test_closed_mapping'),
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

def sudo_nnp_delivery_checks():
    """Pure schema/layout extraction only; never link the fixed reader/adapters."""
    root = Path(__file__).resolve().parents[2]
    path = root / 'crates/arxd/src/exec/disposable/sudo_nnp.rs'
    original = path.read_bytes()
    controller = original.decode('utf-8')
    schema = controller[controller.index('#[derive(serde::Deserialize)]'):controller.index('// Every component')]
    tests = controller[controller.index('#[test]\nfn a7_a11_delivery_schema_rejections'):controller.index('#[derive(Clone, Copy)]')]
    source = 'use std::path::{Path,PathBuf};\n' + schema + tests
    deps = Path(os.environ.get('ARX_PURE_DEPS', str(root / 'target/debug/deps')))
    with tempfile.TemporaryDirectory(prefix='a7-a11-delivery-pure-') as tmp:
        exe = Path(tmp) / 'schema.exe'
        def check(code):
            args = ['rustc', '--edition=2024', '--test', '-', '-L', 'dependency=' + str(deps)]
            for name in ('serde', 'serde_json'):
                rlibs = sorted(deps.glob('lib' + name + '-*.rlib'))
                assert rlibs, 'build locked offline dependencies first'
                args += ['--extern', name + '=' + str(rlibs[0])]
            built = subprocess.run(args + ['-o', str(exe)], input='#![allow(dead_code)]\n' + code,
                                   text=True, capture_output=True, timeout=60)
            assert built.returncode == 0, built.stderr
            return subprocess.run([str(exe), 'a7_a11_delivery_schema_rejections', '--exact'],
                                  text=True, capture_output=True, timeout=20)
        baseline = check(source)
        assert baseline.returncode == 0 and '1 passed; 0 failed' in baseline.stdout, baseline.stdout
        print('PASS actual strict delivery schema/layout')
        for term in ('d.run != run', 'd.source_sha != source', 'd.host_profile_sha256 != profile',
                     'source != workflow', 'source != reviewed', 'd.accounts[0].uid == d.accounts[1].uid',
                     'd.accounts[0].gid == d.accounts[1].gid', 'a.uid < 1000', 'a.size > 128 * 1024 * 1024',
                     'm.size == a.size', 'm.dev == a.dev', 'm.ino == a.ino',
                     'm.mode == 0o100755', 'm.uid == 0', 'm.nlink == 1',
                     'uid == a.uid', 'gid == a.gid', 'shell == b"/usr/sbin/nologin"'):
            assert schema.count(term) == 1, term
            rejection = term.startswith(('d.', 'source ', 'a.'))
            result = check(source.replace(schema, schema.replace(term, 'false' if rejection else 'true')))
            assert result.returncode == 101 and 'assertion' in result.stdout, term + result.stdout
            print('KILLED delivery ' + term)
        result = check(source.replace(schema, schema.replace('#[serde(deny_unknown_fields)]', '')))
        assert result.returncode == 101 and 'assertion' in result.stdout, result.stdout
        print('KILLED unknown delivery fields')
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.SudoNnpDelivery)
    assert unittest.TextTestRunner().run(suite).wasSuccessful()
    driver_bytes = Path(driver.__file__).read_bytes()
    driver_source = driver_bytes.decode('utf-8')
    for before, after, test in [
        ('4 * 1024 * 1024 - start', '4 * 1024 * 1024', 'test_shared_build_budget_and_capture'),
        ('remaining = deadline - time.monotonic()', 'remaining = 360', 'test_shared_build_budget_and_capture'),
        ("os.write(dst, data) == len(data)", 'True', 'test_copy_same_harness_bytes_and_partial_failure'),
        ("(copied['size'], copied['sha256']) == (pin['size'], pin['sha256'])", 'True', 'test_copy_same_harness_bytes_and_partial_failure'),
        ('sudo_nnp_digest(src) == pin', 'True', 'test_copy_same_harness_bytes_and_partial_failure'),
    ]:
        assert driver_source.count(before) == 1, before
        module = types.ModuleType('delivery_mutant')
        exec(compile(driver_source.replace(before, after), '<delivery-mutant>', 'exec'), module.__dict__)
        with patch.object(test_driver, 'driver', module):
            result = unittest.TextTestRunner(stream=io.StringIO()).run(
                unittest.TestSuite([test_driver.SudoNnpDelivery(test)]))
        assert result.failures and not result.errors, before + str(result.errors)
        print('KILLED delivery mock ' + before)
    assert Path(driver.__file__).read_bytes() == driver_bytes
    assert path.read_bytes() == original
    print('Delivery source bytes unchanged')


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
    rlibs = sorted((Path(os.environ.get('ARX_PURE_DEPS', str(root / 'target/debug/deps')))).glob('libserde_json-*.rlib'))
    assert rlibs, 'build locked offline Cargo test dependencies first'
    print('offline parser dependency: ' + rlibs[0].name)
    parser_source = 'mod development { ' + tools + '}\n' + parsers + parser_test
    with tempfile.TemporaryDirectory(prefix='a7-a11-records-') as tmp:
        exe = Path(tmp) / 'records.exe'
        def check_records(code):
            built = subprocess.run(['rustc', '--edition=2024', '--test', '-', '-L',
                                    'dependency=' + str(Path(os.environ.get('ARX_PURE_DEPS', str(root / 'target/debug/deps')))), '--extern',
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
        fixture = source[:source.index('# Candidate only:')]
        assert fixture.count(before) == 1
        module = types.ModuleType('sudo_nnp_mutant')
        exec(compile(source.replace(fixture, fixture.replace(before, after)), '<in-memory-mutant>', 'exec'), module.__dict__)
        with patch.object(test_driver, 'driver', module):
            suite = unittest.TestSuite([test_driver.SudoNnp(name)])
            result = unittest.TextTestRunner(stream=io.StringIO()).run(suite)
        # Errors do not count as a killed assertion.
        assert result.failures and not result.errors, name + str(result.errors)
        print('KILLED mock ' + name)
    assert [p.read_bytes() for p in files] == original
    print('A7/A11 pure and mock source bytes unchanged')


def controlled_policy_mutants():
    baseline = unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.ControlledPolicy)
    assert unittest.TextTestRunner().run(baseline).wasSuccessful()
    source = Path(driver.__file__).read_text()
    cases = [
        ("for _ in sudo_nnp_accounts(run, accounts):\n        pass", "prepare_sudo_nnp(run, accounts)", 'test_controlled_preparation_poison_legacy_writer'),
        ("len(mappings) == 5 and set(mappings) == set(expected)", "True", 'test_literal_and_mapping_closed_sets'),
        ("require(unit_gone is True, 'E_QUARANTINE_CGROUP')\n    # Partial/unidentified", "pass\n    # Partial/unidentified", 'test_stage_is_exclusive_private_and_quarantines_short_write'),
        ("pin is not None and controlled_pin(held, directory, view) == pin", "True", 'test_source_inode_and_entry_recheck'),
        ("set(os.listdir(fd)) == expected", "True", 'test_source_inode_and_entry_recheck'),
        ("case0 + ' ALL=(root) NOPASSWD: /usr/bin/true", "_case1 + ' ALL=(root) NOPASSWD: /usr/bin/true", 'test_literal_and_mapping_closed_sets'),
        # D9c fifth empty C2 view source: exact mapping, mode, emptiness and creation.
        ("('c2-empty', '/usr/local/bin'))", "('c2-empty', '/usr/local'))", 'test_literal_and_mapping_closed_sets'),
        ("mode = 0o755 if view else 0o700 if directory else 0o440", "mode = 0o700 if directory else 0o440", 'test_view_pin_mode_owner_and_type'),
        ("os.fchmod(fd, 0o755)  # held new inode", "pass  # held new inode", 'test_stage_is_exclusive_private_and_quarantines_short_write'),
        ("'fx/c2-empty': set()}", "'fx/c2-empty': {'sudo'}}", 'test_nonempty_or_replaced_view_never_deleted'),
        ("os.mkdir(name, mode=0o755 if view else 0o700, dir_fd=parent)", "os.mkdir(name, mode=0o700, dir_fd=parent)", 'test_stage_is_exclusive_private_and_quarantines_short_write'),
        ("len(owned.get('entries', [])) == 7", "len(owned.get('entries', [])) >= 6", 'test_source_inode_and_entry_recheck'),
        ("                require(controlled_pin(fd, directory, view) == pin, 'E_POLICY_IDENTITY')\n            finally:\n                os.close(fd)\n            if directory:",
         "                pass\n            finally:\n                os.close(fd)\n            if directory:", 'test_nonempty_or_replaced_view_never_deleted'),
    ]
    for before, after, name in cases:
        assert source.count(before) == 1, before
        module = types.ModuleType('controlled_mutant')
        exec(compile(source.replace(before, after), '<controlled-mutant>', 'exec'), module.__dict__)
        with patch.object(test_driver, 'driver', module):
            result = unittest.TextTestRunner(stream=io.StringIO()).run(
                unittest.TestSuite([test_driver.ControlledPolicy(name)]))
        assert result.failures and not result.errors, name + str(result.errors)
        print('KILLED controlled ' + before)
    root = Path(__file__).resolve().parents[2]
    controller = (root / 'crates/arxd/src/exec/disposable/sudo_nnp.rs').read_text()
    names = controller[controller.index('fn delivery_names('):controller.index('fn delivery_schema(')]
    ports = controller[controller.index('const CONTROLLED_PLUGIN'):controller.index('fn controlled_read(')]
    tests = controller[controller.index('#[test]\nfn a7_controlled_policy_discriminators'):controller.index('fn recheck_delivery(')]
    code = names + ports + tests
    mutants = [
        ('mappings.len() == 5', 'true'),
        ('seen == [1; 5]', 'true'),
        ('path.starts_with("/usr/local/bin/")', 'false'),
        ('path.starts_with("/etc/")', 'false'),
        ('("c2-empty", "/usr/local/bin"),', '("c2-empty", "/usr/local"),'),
        ('o == "ro"', 'true'),
        ('entries.len() == 4', 'true'),
        ('*data == b', 'true'),
    ]
    with tempfile.TemporaryDirectory(prefix='controlled-ports-') as tmp:
        exe = Path(tmp) / 'ports.exe'
        def check(code):
            built = subprocess.run(['rustc', '--edition=2024', '--test', '-', '-o', str(exe)],
                                   input=code, text=True, capture_output=True, timeout=60)
            assert built.returncode == 0, built.stderr
            return subprocess.run([str(exe), 'a7_controlled_policy_discriminators', '--exact'],
                                  text=True, capture_output=True, timeout=20)
        result = check(code)
        assert result.returncode == 0, result.stdout
        print('PASS actual controlled literal/mapping/mount pure Rust ports')
        for before, after in mutants:
            assert code.count(before) == 1, before
            result = check(code.replace(before, after))
            assert result.returncode == 101 and 'assertion' in result.stdout, before + result.stdout
            print('KILLED controlled Rust ' + before)
        # D9c actual pure empty-view proof over observed held/reopened facts.
        def check_view(code):
            built = subprocess.run(['rustc', '--edition=2024', '--test', '-', '-o', str(exe)],
                                   input=code, text=True, capture_output=True, timeout=60)
            assert built.returncode == 0, built.stderr
            return subprocess.run([str(exe), 'a7_c2_empty_view_discriminators', '--exact'],
                                  text=True, capture_output=True, timeout=20)
        result = check_view(code)
        assert result.returncode == 0 and '1 passed; 0 failed' in result.stdout, result.stdout
        print('PASS actual D9c empty C2 view pure Rust proof')
        for before, after in [('n.mode == 0o040755', 'true'), ('n.uid == 0', 'true'),
                              ('n.gid == 0', 'true'), ('n.dev == f.source[0].dev', 'true'),
                              ('n.ino == f.source[0].ino', 'true'),
                              ('f.source_entries.is_empty()', 'true'),
                              ('f.view_entries.is_empty()', 'true'),
                              ('&& f.readonly', '&& true'),
                              ('f.c2 == [C2::Absent; 2]', 'true'),
                              ('f.source.iter().chain(f.view.iter()).all(same)', 'true')]:
            assert code.count(before) == 1, before
            result = check_view(code.replace(before, after))
            assert result.returncode == 101 and 'assertion' in result.stdout, before + result.stdout
            print('KILLED empty view Rust ' + before)


def a7_integration_checks():
    """Actual fixed scheduling code only: no Runner, files, LinuxFs or helper."""
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.A7Integration)
    assert unittest.TextTestRunner().run(suite).wasSuccessful()
    source = Path(driver.__file__).read_text()
    for before, after, name in [
            ("text.count(marker + '\\n') == 1", 'True', 'test_both_case_markers_and_exact_summary'),
            ("require(deadline > time.monotonic(), 'E_TEST_TIMEOUT')", 'pass', 'test_active_identity_before_ack_and_terminal_deadline'),
            ("ack_start - monitor_start <= 15", 'True', 'test_active_identity_before_ack_and_terminal_deadline'),
            ("time.monotonic() - ack_start <= 5", 'True', 'test_active_identity_before_ack_and_terminal_deadline'),
            ("        if policy:\n            cleanup_controlled_sudo_nnp", "        if policy.get('entries'):\n            cleanup_controlled_sudo_nnp", 'test_cleanup_requires_stop_and_preserves_partial_policy'),
            ("require(proc.stat().st_uid not in uids, 'E_UID_LIVE')", 'pass', 'test_live_fixture_uid_retains_every_fixture_path'),
            ("        a7_uids_released(accounts)\n", '', 'test_live_fixture_uid_retains_every_fixture_path'),
            ("require(item.get('identity') is not None or not item['created'], 'E_QUARANTINE_UID_UNCAPTURED')\n        if item.get('identity') is not None:\n            uids.add", "pass\n        if item.get('identity') is not None:\n            uids.add", 'test_live_fixture_uid_retains_every_fixture_path')]:
        assert source.count(before) == 1, before
        module = types.ModuleType('a7_driver_mutant')
        exec(compile(source.replace(before, after), '<a7-mutant>', 'exec'), module.__dict__)
        with patch.object(test_driver, 'driver', module):
            result = unittest.TextTestRunner(stream=io.StringIO()).run(
                unittest.TestSuite([test_driver.A7Integration(name)]))
        assert result.failures and not result.errors, name + str(result.errors)
        print('KILLED A7 driver ' + before)
    root = Path(__file__).resolve().parents[2]
    controller = (root / 'crates/arxd/src/exec/disposable/sudo_nnp.rs').read_text()
    code = 'use std::time::Duration;\n' + controller[controller.index('fn a7_cases('):controller.index('// Pure discriminators')]
    with tempfile.TemporaryDirectory(prefix='a7-integration-') as tmp:
        exe = Path(tmp) / 'schedule.exe'
        def check(text):
            result = subprocess.run(['rustc','--edition=2024','--test','-','-o',str(exe)],
                input=text, text=True, capture_output=True, timeout=60)
            assert result.returncode == 0, result.stderr
            return subprocess.run([str(exe),'a7_a11_scheduling_counterfactuals','--exact'],
                text=True, capture_output=True, timeout=20)
        assert check(code).returncode == 0
        print('PASS actual a7_a11_scheduling_counterfactuals exact pure Rust')
        for before, after in [('calls[0] <= Duration::from_secs(60)', 'true'),
                ('calls[1] <= Duration::from_secs(15)', 'true'),
                ('checks <= Duration::from_secs(15)', 'true'),
                ('total <= Duration::from_secs(90)', 'true'),
                ('good && joined == [true, true]', 'true && joined == [true, true]'),
                ('elapsed <= Duration::from_secs(10)', 'true'),
                ('joined == [true, true]', 'true')]:
            assert code.count(before) == 1
            result = check(code.replace(before, after))
            assert result.returncode == 101 and 'assertion' in result.stdout, before + result.stdout
            print('KILLED integration ' + before)


def a7_case_mock_checks():
    # Actual case function; every host boundary below is replaced with a mock.
    root = Path(__file__).resolve().parents[2]
    controller = (root / 'crates/arxd/src/exec/disposable/sudo_nnp.rs').read_text()
    case = controller[controller.index('fn a7_case('):controller.index('// Fixed two-case scope')]
    envelope = controller[controller.index('fn a7_envelopes('):controller.index('#[test]\nfn a7_a11_scheduling')]
    mock = r'''
use std::{path::{Path,PathBuf},time::Duration,cell::RefCell};
thread_local! { static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }
fn event(s: &str) { EVENTS.with(|e| e.borrow_mut().push(s.into())); }
struct Config { workspace: PathBuf }
struct Prepared { accounts: [Config;2], arxd: PathBuf, reentry: PathBuf }
struct Cancellation;
struct RunSpec { executable: PathBuf, arguments: Vec<String>, cwd: PathBuf, timeout: Duration, output_bytes: usize }
struct RunResult { step: usize }
struct Runner { fail: usize, residue: bool }
impl Runner {
    fn run_phased(&self,spec: &RunSpec,_: &Cancellation,_: &Cancellation) -> Result<RunResult,()> {
        let step = if spec.executable == Path::new("arxd") { 0 } else { 1 };
        assert_eq!(spec.timeout, Duration::from_secs(if step == 0 {55} else {10}));
        assert_eq!(spec.output_bytes,4096);
        assert_eq!(spec.cwd, Path::new("workspace"));
        if step == 0 { assert_eq!(spec.arguments, ["tool-probe"]); }
        else { assert_eq!(spec.arguments, ["--exact","unix_shim::rights::linux::tests::e4_sudo_nnp_reentry","--ignored","--nocapture","--test-threads=1","--color=never"]); }
        event(if step == 0 {"probe"} else {"reentry"});
        if self.fail == step { Err(()) } else { Ok(RunResult {step}) }
    }
    fn cleanup_residue(&self) -> bool { self.residue }
}
fn recheck_delivery(_: &Config, exe: &Path) -> Option<()> { event(if exe == Path::new("arxd") {"admit-probe"} else {"admit-reentry"}); Some(()) }
fn valid_run(_: &RunResult,_: &Config,residue: bool) -> bool { !residue }
fn output(r: &RunResult) -> Option<Vec<u8>> { Some(vec![r.step as u8]) }
fn probe_records(b: &[u8]) -> bool { b == [0] }
fn reentry_records(b: &[u8]) -> bool { b == [1] }
fn a7_marker(b: &[u8]) -> bool { event(std::str::from_utf8(b).unwrap()); true }
#[test]
fn actual_case_sequence() {
    let prepared = Prepared { accounts: [Config {workspace: "workspace".into()},Config {workspace: "workspace".into()}], arxd: "arxd".into(), reentry: "reentry".into() };
    for index in 0..2 {
        for fail in 0..3 {
            EVENTS.with(|e| e.borrow_mut().clear());
            assert_eq!(a7_case(&prepared,index,&Runner {fail,residue:false},&Cancellation,&Cancellation),fail == 2);
            let expected = if fail == 0 { vec!["admit-probe".to_string(),"probe".into()] }
                else { let mut v = vec!["admit-probe".to_string(),"probe".into(),"admit-reentry".into(),"reentry".into()];
                    if fail == 2 { v.push(format!("ARX_A7_A11_CASE{index}_OK\n")); } v };
            EVENTS.with(|e| assert_eq!(*e.borrow(), expected));
        }
        assert!(!a7_case(&prepared,index,&Runner {fail:2,residue:true},&Cancellation,&Cancellation));
    }
}
'''
    with tempfile.TemporaryDirectory(prefix='a7-case-mocks-') as tmp:
        exe = Path(tmp) / 'case.exe'
        def check(text):
            built = subprocess.run(['rustc','--edition=2024','--test','-','-o',str(exe)],
                input=text,text=True,capture_output=True,timeout=60)
            assert built.returncode == 0, built.stderr
            return subprocess.run([str(exe),'actual_case_sequence','--exact'],text=True,capture_output=True,timeout=20)
        result = check(mock + case + envelope)
        assert result.returncode == 0, result.stdout + result.stderr
        print('PASS actual a7_case with mocked boundaries: both indices, call failures, residue, sequence')
        for before, after in [('55', '20'), ('4096', '8192'), ('probe_records(&bytes)', 'false'),
                ('reentry_records(&bytes)', 'false'), ('valid_run(&result, config, runner.cleanup_residue())', 'true')]:
            assert case.count(before) == 1
            result = check(mock + case.replace(before, after) + envelope)
            assert result.returncode == 101 and 'assertion' in result.stdout, before + result.stdout
            print('KILLED actual case ' + before)


if __name__ == '__main__':
    main()
    rust_mutants()
    controlled_policy_mutants()
    a7_integration_checks()
    a7_case_mock_checks()


def execution_preparation_checks():
    """Actual package_file/runtime_link; all file and syscall ports are fake."""
    import os
    source = (Path(__file__).resolve().parents[2] / 'crates/arxd/src/exec/disposable/sudo_nnp.rs').read_text()
    code = source[source.index('fn package_file('):source.index('fn runtime_package(')]
    code = code.replace('std::fs::File','FakeFile').replace('std::fs::Metadata','FakeMeta')
    code = code.replace('    use std::os::unix::fs::MetadataExt;\n','').replace('    use std::os::fd::AsRawFd;\n','')
    mock = r'''
#![allow(dead_code,non_camel_case_types)]
use std::{path::Path,cell::RefCell,collections::VecDeque};
#[derive(Clone)] struct FakeMeta { kind: bool, uid: u32, mode: u32, nlink: u64, len: u64, ino: u64 }
impl FakeMeta {
 fn is_file(&self)->bool {self.kind} fn uid(&self)->u32 {self.uid}
 fn mode(&self)->u32 {self.mode} fn nlink(&self)->u64 {self.nlink}
 fn len(&self)->u64 {self.len} fn ino(&self)->u64 {self.ino}
 fn dev(&self)->u64 {1} fn gid(&self)->u32 {0}
 fn mtime(&self)->i64 {1} fn mtime_nsec(&self)->i64 {0}
 fn ctime(&self)->i64 {1} fn ctime_nsec(&self)->i64 {0}
}
#[derive(Clone)] struct FakeFile { meta: FakeMeta, bytes: Vec<u8> }
impl FakeFile { fn metadata(&self)->Result<FakeMeta,()> {Ok(self.meta.clone())} fn as_raw_fd(&self)->i32 {17} }
thread_local! { static FILES: RefCell<VecDeque<FakeFile>> = const {RefCell::new(VecDeque::new())};
 static LINK: RefCell<(u32,u32,u64,Vec<u8>,bool)> = RefCell::new((0o120777,0,1,b"usr/bin".to_vec(),false)); }
fn delivery_open(_: &Path,_:bool)->Option<FakeFile> { FILES.with(|q|q.borrow_mut().pop_front()) }
fn controlled_read(f:&FakeFile,_:u64)->Option<Vec<u8>> {Some(f.bytes.clone())}
mod libc {
 pub const AT_SYMLINK_NOFOLLOW:i32=256; pub const S_IFMT:u32=0o170000; pub const S_IFLNK:u32=0o120000;
 pub struct stat {pub st_mode:u32,pub st_uid:u32,pub st_nlink:u64,pub st_dev:u64,pub st_ino:u64,pub st_size:i64,pub st_mtime:i64,pub st_ctime:i64}
 pub unsafe fn fstatat(_:i32,_:*const i8,p:*mut stat,_:i32)->i32 {
  super::LINK.with(|v| {let v=v.borrow(); unsafe {p.write(stat {st_mode:v.0,st_uid:v.1,st_nlink:v.2,st_dev:1,st_ino:2,st_size:v.3.len() as i64,st_mtime:1,st_ctime:1})};0})
 }
 pub unsafe fn readlinkat(_:i32,_:*const i8,p:*mut std::ffi::c_void,n:usize)->isize {
  super::LINK.with(|v| {let v=v.borrow(); if v.4 {return n as isize;} unsafe {std::ptr::copy_nonoverlapping(v.3.as_ptr(),p.cast(),v.3.len().min(n))}; v.3.len() as isize})
 }
}
fn file()->FakeFile {FakeFile {meta:FakeMeta {kind:true,uid:0,mode:0o100755,nlink:1,len:5,ino:1},bytes:b"known".to_vec()}}
#[test] fn runtime_package_counterfactuals() {
 const HASH:&str="7117fff2d0fd294462b3c802b7cb8753579f23f3946b99cf55f38e873f013f10";
 let check=|f:FakeFile,g:FakeFile| {FILES.with(|q|*q.borrow_mut()=VecDeque::from([f,g]));package_file(Path::new("/fixed"),5,HASH).is_some()};
 assert!(check(file(),file()));
 for i in 0..8 {let mut f=file();match i {0=>f.bytes[0]=b'X',1=>{f.bytes.pop();},2=>f.meta.uid=1,3=>f.meta.mode|=2,4=>f.meta.nlink=2,5=>f.meta.kind=false,6=>f.meta.len=4,_=>f.bytes.extend_from_slice(b"x")};assert!(!check(f.clone(),f),"case {i}");}
 let mut swapped=file();swapped.meta.ino=2;assert!(!check(file(),swapped));
 assert!(runtime_link(&file(),c"bin",&[b"usr/bin",b"/usr/bin"]).is_some());
 for i in 0..5 {LINK.with(|v| {let mut v=v.borrow_mut();*v=(0o120777,0,1,b"usr/bin".to_vec(),false);match i {0=>v.0=0o100777,1=>v.1=1,2=>v.2=2,3=>v.3=b"dangling".to_vec(),_=>v.4=true}});assert!(runtime_link(&file(),c"bin",&[b"usr/bin",b"/usr/bin"]).is_none(),"link {i}");}
}
'''
    # Reuse read-only locked development deps; build and temp outputs use D:.
    deps = Path(os.environ.get('ARX_PURE_DEPS', str(Path(os.environ['CARGO_TARGET_DIR']) / 'debug/deps')))
    libs = sorted(deps.glob('libsha2-*.rlib'))
    assert libs, 'existing locked sha2 development rlib required'
    with tempfile.TemporaryDirectory(prefix='a7-runtime-') as tmp:
        exe = Path(tmp) / 'runtime.exe'
        def check(text):
            result = subprocess.run(['rustc','--edition=2024','--test','-','-L','dependency='+str(deps),
                '--extern','sha2='+str(libs[0]),'-o',str(exe)],input=mock+text,text=True,capture_output=True,timeout=60)
            assert result.returncode == 0, result.stderr
            return subprocess.run([str(exe),'runtime_package_counterfactuals','--exact'],capture_output=True,text=True,timeout=20)
        result=check(code)
        assert result.returncode == 0, result.stdout+result.stderr
        print('PASS actual package_file/runtime_link mocked ports')
        for before,after in [('before.uid() != 0','false'),('before.nlink() != 1','false'),
                ('before.mode() & 0o022 != 0','false'),('before.len() != size','false'),
                ('format!("{:x}", Sha256::digest(&bytes)) != digest','false'),
                ('snapshot(&before) != snapshot(&again.metadata().ok()?)','false'),
                ('!targets.contains(&&bytes[..n as usize])','false'),('st.st_uid == 0','true'),
                ('st.st_nlink == 1','true')]:
            assert code.count(before)==1,before
            result=check(code.replace(before,after))
            assert result.returncode==101 and "panicked at" in result.stdout,before+result.stdout+result.stderr
            print('KILLED runtime '+before)


def admission_path_mutants():
    """Audited Python-only admission chain; no compiler, native adapter or files."""
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.AdmissionPathRefusal)
    assert unittest.TextTestRunner().run(suite).wasSuccessful()
    path = Path(driver.__file__)
    original = path.read_bytes()
    source = original.decode('utf-8')
    cases = [
        ('a4_directory', 'info.st_uid == 0', 'True', 'test_each_ancestry_conjunct'),
        ('a4_directory', 'not info.st_mode & 0o022', 'not info.st_mode & 0o002', 'test_each_ancestry_conjunct'),
        ('a4_directory', 'not info.st_mode & 0o022', 'not info.st_mode & 0o020', 'test_each_ancestry_conjunct'),
        ('a4_directory', 'stat.S_ISDIR(info.st_mode)', 'True', 'test_each_ancestry_conjunct'),
        ('sudo_nnp_open', ' | os.O_NOFOLLOW', '', 'test_qualifying_metadata_and_expected_fences'),
        ('sudo_nnp_open', 'if directory:\n            a4_directory(fd)', 'if False:\n            a4_directory(fd)', 'test_each_ancestry_conjunct'),
        ('a7_alias_c2', "True, 'a7_c2_parent'", "True, 'a7_alias_root'", 'test_fixed_role_attribution'),
        ('a7_alias_c2', "feedback_mutation == 'none'", 'True', 'test_only_initial_path_code_is_attributed'),
        # D9c controlled host split: alias/ancestry fences and attribution only.
        ('a7_alias_controlled', "Path('/'), 'a7_alias_root'", "Path('/'), 'a7_alias_target'", 'test_fixed_role_attribution'),
        ('a7_alias_controlled', "feedback_mutation == 'none'", 'True', 'test_only_initial_path_code_is_attributed'),
        ('a7_alias_controlled', "usrbin = open_role(Path('/usr/bin'), 'a7_alias_target')", "usrbin = root", 'test_each_ancestry_conjunct'),
        ('a7_admission', '    a7_alias_controlled()\n', '    a7_alias_c2()\n', 'test_controlled_split_defers_only_host_c2'),
        ('exception_record', "return dict(code='E_PATH', role=error.role)", "return dict(code='E_PATH')", 'test_fixed_role_attribution'),
        ('validate_record', "e['role'] in A7_PATH_ROLES", 'True', 'test_closed_role_schema_and_historical_compatibility'),
        ('validate_record', "record['mutation_lower_bound'] == 'none'", 'True', 'test_closed_role_schema_and_historical_compatibility'),
    ]
    for function, before, after, name in cases:
        start = source.index('def ' + function + '(')
        end = source.index('\n\ndef ', start + 1)
        block = source[start:end]
        assert block.count(before) == 1, before
        changed = source[:start] + block.replace(before, after) + source[end:]
        module = types.ModuleType('path_refusal_mutant')
        module.__file__ = driver.__file__
        exec(compile(changed, '<in-memory-path-mutant>', 'exec'), module.__dict__)
        with patch.object(test_driver, 'driver', module):
            result = unittest.TextTestRunner(stream=io.StringIO()).run(
                unittest.TestSuite([test_driver.AdmissionPathRefusal(name)]))
        assert result.failures and not result.errors, before + str(result.errors)
        print('KILLED path ' + function + ': ' + before)
    assert path.read_bytes() == original
    print('A7 path refusal source bytes unchanged')


def execution_admission_mutants():
    source=Path(driver.__file__).read_text()
    for before,after,case,name in [
        ('require(result == dict(', 'require(True or result == dict(',test_driver.RuntimeAdmission,'test_runtime_refusals_before_effects'),
        ("os.uname().machine == 'x86_64'",'True',test_driver.RuntimeAdmission,'test_runtime_refusals_before_effects'),
        ('    a7_alias_controlled()\n    cg_preflight()', '    cg_preflight()',test_driver.RuntimeAdmission,'test_runtime_refusals_before_effects'),
        ('    a7_alias_controlled()\n    cg_preflight()', '    a7_alias_c2()\n    cg_preflight()',test_driver.RuntimeAdmission,'test_runtime_refusals_before_effects'),
        ("buffer.raw[:length] in (b'usr/bin', b'/usr/bin'), 'E_ALIAS')\n        after = os.stat('bin', dir_fd=root, follow_symlinks=False)\n        require(_stable_stat(before) == _stable_stat(after) and\n                _stable_stat(os.stat('bin', dir_fd=root)) == _stable_stat(os.fstat(usrbin)), 'E_ALIAS')\n    finally",
         "True, 'E_ALIAS')\n        after = os.stat('bin', dir_fd=root, follow_symlinks=False)\n        require(_stable_stat(before) == _stable_stat(after) and\n                _stable_stat(os.stat('bin', dir_fd=root)) == _stable_stat(os.fstat(usrbin)), 'E_ALIAS')\n    finally",test_driver.RuntimeAdmission,'test_alias_controlled_with_only_fake_ports'),
        ("                _stable_stat(os.stat('bin', dir_fd=root)) == _stable_stat(os.fstat(usrbin)), 'E_ALIAS')\n    finally",
         "                True, 'E_ALIAS')\n    finally",test_driver.RuntimeAdmission,'test_alias_controlled_with_only_fake_ports'),
        ("A7_PROFILE = '99e7f5b4b3b63ff13955432bf9b7cde0436c9ab57afbc93fbe99bb45eb17017e'", "A7_PROFILE = '51d4fb5f17f88e41d21472e04859c194e15cb7a4a1ec046c06a5b022a2a8f210'",test_driver.RuntimeAdmission,'test_canonical_profile_label_matches_five_view_facts'),
        ('    cg_preflight()\n    return A7_PROFILE', '    return A7_PROFILE',test_driver.RuntimeAdmission,'test_runtime_refusals_before_effects'),
        ("require(expected_profile == A7_PROFILE, 'E_POLICY_UNATTESTED')",'pass',test_driver.RuntimeAdmission,'test_profile_mismatch_before_handoff'),
        ('        a7_admission()  # before root/host probes, reservation or staging','        pass',test_driver.A7Integration,'test_closed_before_host_and_package_adapter')]:
        assert source.count(before)==1,before
        module=types.ModuleType('runtime_admission_mutant')
        exec(compile(source.replace(before,after),'<mocked-runtime-mutant>','exec'),module.__dict__)
        with patch.object(test_driver,'driver',module):
            result=unittest.TextTestRunner(stream=io.StringIO()).run(unittest.TestSuite([case(name)]))
        assert result.failures and not result.errors,before+str(result.errors)
        print('KILLED admission '+before)
