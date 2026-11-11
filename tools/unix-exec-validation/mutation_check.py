"""Run only pure mocked regressions against independently mutated driver code."""
import ast
import io
from pathlib import Path
import types
import unittest
from unittest.mock import patch

import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))

import driver
import test_driver

CASES = [
    ('fixture inode', 'E_FIXTURE_INODE', 'RuntimeGuards.test_inode_replacement_quarantines_before_deletion'),
    ('live UID', 'E_UID_LIVE', 'RuntimeGuards.test_live_uid_quarantines_before_deletion'),
    ('unit ownership', 'E_CGROUP_UNIT_OWNER', 'CgroupScan.test_unit_ancestry_and_subtree_root_ownership'),
    ('fixture ownership', 'E_CGROUP_FIXTURE_OWNER', 'CgroupScan.test_fixture_uid_or_gid_anywhere_refuses'),
    ('global write/link', 'E_CGROUP_WRITABLE_HIERARCHY', 'CgroupScan.test_global_write_and_link_refusal'),
    ('copy refusal', 'E_COPY_TYPE', 'CopyGuards.test_link_and_special_file_refuse_before_copy'),
    ('exact argv', 'E_TEST_ARGS', 'RuntimeGuards.test_harness_exact_argv_and_collisions'),
    ('zero tests', 'passed', 'RuntimeGuards.test_zero_test_success_is_failure'),
    ('run gate', 'gate', 'Guards.test_refusal_before_any_runtime_call'),
    ('post-account scan', 'fixture-check-call', 'RuntimeGuards.test_fixture_check_mode_scans_captured_uid_and_gid'),
]


def main():
    source = Path(driver.__file__).read_text()
    for label, guard, test in CASES:
        tree = ast.parse(source)
        changed = 0
        for node in ast.walk(tree):
            if guard == 'fixture-check-call' and isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == 'cg_scan' and len(node.args) == 3:
                node.args[1:] = [ast.Constant(None), ast.Constant(None)]
                changed += 1
            elif isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == 'require' and len(node.args) == 2 and isinstance(node.args[1], ast.Constant) and node.args[1].value == guard:
                node.args[0] = ast.Constant(True)
                changed += 1
            elif guard == 'passed' and isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'passed' for t in node.targets):
                node.value = ast.Constant(True)
                changed += 1
            elif guard == 'gate' and isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'run_id' for t in node.targets):
                node.value = ast.Constant('123-1')
                changed += 1
        assert changed == 1, (label, changed)
        mutant = types.ModuleType('driver_mutant')
        exec(compile(ast.fix_missing_locations(tree), '<mutant>', 'exec'), mutant.__dict__)
        with patch.object(test_driver, 'driver', mutant):
            suite = unittest.defaultTestLoader.loadTestsFromName(test, test_driver)
            result = unittest.TextTestRunner(stream=io.StringIO()).run(suite)
        assert not result.wasSuccessful(), 'SURVIVED: ' + label
        print(f'KILLED: {label} (failures={len(result.failures)}, errors={len(result.errors)})')


if __name__ == '__main__':
    main()
