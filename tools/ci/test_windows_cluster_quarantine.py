"""Pin the narrow platform quarantine; keep test bodies compiled everywhere."""
from pathlib import Path
import re
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = ['t3_snapshot_catchup_then_consecutive_append', 't1_follower_offline_enrollment_and_reopen', 't2_leader_offline_reads_and_unknown_retry', 'u6_cancelled_callers_keep_engine_permits_until_resolution', 't4_registry_revocation_snapshot_and_prune', 'r6_shutdown_report_cancelled_proposal', 'r6_shutdown_report_store_only', 'r6_shutdown_report_normal_and_reaped_panic']
REASON = 'Windows three-node quarantine: docs/reviews/m3-cluster-third-attempt-owner-disposition.md'
WINDOWS_UNSAMPLED = ['r5_shutdown_read_owners']
REASON_R5 = 'Windows R5 post-release quarantine: docs/adr/0024-windows-three-node-test-quarantine.md'


def assert_windows_quarantine(source):
    functions = re.findall(r'^async fn ([a-z0-9_]+)\(', source, re.M)
    assert set(EXPECTED).isdisjoint(WINDOWS_UNSAMPLED)
    for names, reason in ((EXPECTED, REASON), (WINDOWS_UNSAMPLED, REASON_R5)):
        actual = re.findall(
            r'#\[cfg_attr\(\s*target_os\s*=\s*"windows",\s*ignore\s*=\s*"'
            + re.escape(reason) + r'"\s*\)\]\s*async fn ([a-z0-9_]+)\(', source)
        assert sorted(actual) == sorted(names)
        for name in names:
            assert functions.count(name) == 1
    assert source.count('ignore') == len(EXPECTED) + len(WINDOWS_UNSAMPLED)
    assert '#[cfg(target_os' not in source
    assert '#[cfg(not(target_os' not in source

def assert_issuance_required(source):
    # Both inner and outer attributes are covered. Only cfg(test) is permitted;
    # no conditional predicate may remove or skip issuance coverage.
    for attribute in re.findall(r"#\s*!?\s*\[([^\]]*)\]", source):
        if re.search(r"\bignore\b", attribute):
            raise AssertionError("issuance tests must run on every OS")
        if re.match(r"\s*cfg(?:_attr)?\b", attribute):
            if re.sub(r"\s+", "", attribute) != "cfg(test)":
                raise AssertionError("issuance tests must run on every OS")


def assert_issuance_modules(issue, pki):
    assert_issuance_required(issue)
    assert_issuance_required(pki)
    for source, declaration in [
        (issue, '#[cfg(test)]\n#[path = "issue_tests.rs"]\nmod issue_tests;'),
        (pki, '#[cfg(test)]\nmod tests;'),
    ]:
        if source.count(declaration) != 1:
            raise AssertionError("issuance test module must retain its binding")


class QuarantineTests(unittest.TestCase):
    def test_issuance_required_on_every_os(self):
        base = ROOT / "crates/arxd/src/cluster"
        issue = (base / "consensus/issue.rs").read_text(encoding="utf-8")
        pki = (base / "pki.rs").read_text(encoding="utf-8")
        assert_issuance_modules(issue, pki)
        mutations = [
            '#[ignore]', '#![cfg(unix)]', '#![cfg(not(windows))]',
            '#[cfg(windows)]', '#[cfg(unix)]', '#[cfg(target_os = "linux")]',
            '#[cfg_attr(target_family = "unix", ignore)]',
            '#[cfg_attr(all(), ignore)]', '#[cfg_attr(debug_assertions, ignore)]',
            '#[cfg(any())]', '#[cfg(target_env = "msvc")]',
            '#[cfg(target_vendor = "pc")]', '#[cfg(feature = "hidden")]',
        ]
        for relative in ["consensus/issue_tests.rs", "pki/tests.rs"]:
            source = (base / relative).read_text(encoding="utf-8")
            assert_issuance_required(source)
            for mutation in mutations:
                with self.subTest(relative=relative, mutation=mutation):
                    with self.assertRaises(AssertionError):
                        assert_issuance_required(mutation + source)
        for mutation in mutations:
            for parent in ("issue", "pki"):
                with self.subTest(parent=parent, mutation=mutation):
                    with self.assertRaises(AssertionError):
                        assert_issuance_modules(
                            mutation + issue if parent == "issue" else issue,
                            mutation + pki if parent == "pki" else pki,
                        )
        with self.assertRaises(AssertionError):
            assert_issuance_modules(issue.replace('mod issue_tests;', 'mod absent;'), pki)
        with self.assertRaises(AssertionError):
            assert_issuance_modules(issue, pki.replace('mod tests;', 'mod absent;'))

    def test_exact_windows_only_quarantine(self):
        source = (ROOT / "crates/arxd/src/cluster/consensus/three_node.rs").read_text(encoding="utf-8")
        assert_windows_quarantine(source)
        gate = '#[cfg_attr(\n    target_os = "windows",\n    ignore = "' + REASON_R5 + '"\n)]'
        mutants = {
            "removed gate": source.replace(gate, ''),
            "unix family": source.replace(gate, gate.replace('target_os = "windows"', 'target_family = "unix"')),
            "all platforms": source.replace(gate, gate.replace('target_os = "windows"', 'all()')),
            "R5 sampled reason": source.replace(REASON_R5, REASON),
            "sample unsampled reason": source.replace(REASON, REASON_R5, 1),
            "tenth ignore": source + '\n#[ignore]\nasync fn extra() {}\n',
            "renamed R5": source.replace('async fn r5_shutdown_read_owners(', 'async fn renamed('),
            "removed R5": source.replace('async fn r5_shutdown_read_owners(', ''),
            "duplicate R5": source + '\nasync fn r5_shutdown_read_owners() {}\n',
            "cfg ban": '#[cfg(target_os = "windows")]\n' + source,
            "cfg not ban": '#[cfg(not(target_os = "windows"))]\n' + source,
        }
        for name, mutant in mutants.items():
            with self.subTest(mutant=name):
                self.assertNotEqual(mutant, source)
                with self.assertRaises(AssertionError):
                    assert_windows_quarantine(mutant)
        with patch(__name__ + '.EXPECTED', EXPECTED + WINDOWS_UNSAMPLED):
            with self.assertRaises(AssertionError):
                assert_windows_quarantine(source)

if __name__ == "__main__":
    unittest.main()
