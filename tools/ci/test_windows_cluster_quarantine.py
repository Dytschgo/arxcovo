"""Pin the narrow platform quarantine; keep test bodies compiled everywhere."""
from pathlib import Path
import re
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
R5_FILTER = 'package(=arxd) & kind(lib) & test(=cluster::consensus::three_node::r5_shutdown_read_owners)'
NEXTEST = (
    "# R5 post-release observation only: show this one test's passing output.\n"
    "# Contract: docs/briefs/m3-r5-postrelease-observation.md\n"
    "[[profile.default.overrides]]\n"
    'filter = "' + R5_FILTER + '"\n'
    'success-output = "immediate"\n'
)
NEXTEST_DATA = {"profile": {"default": {"overrides": [{"filter": R5_FILTER, "success-output": "immediate"}]}}}
EXPECTED = ['t3_snapshot_catchup_then_consecutive_append', 't1_follower_offline_enrollment_and_reopen', 't2_leader_offline_reads_and_unknown_retry', 'u6_cancelled_callers_keep_engine_permits_until_resolution', 't4_registry_revocation_snapshot_and_prune', 'r6_shutdown_report_cancelled_proposal', 'r6_shutdown_report_store_only', 'r6_shutdown_report_normal_and_reaped_panic']
REASON = 'Windows three-node quarantine: docs/reviews/m3-cluster-third-attempt-owner-disposition.md'

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


def assert_nextest_structure(data):
    # Exactly one output-only override; every other setting keeps nextest defaults.
    if data != NEXTEST_DATA:
        raise AssertionError("nextest config may only show exact R5 success output")


def assert_nextest_config(text):
    if text != NEXTEST:
        raise AssertionError("nextest config must stay byte-exact")
    assert_nextest_structure(tomllib.loads(text))


class QuarantineTests(unittest.TestCase):
    def test_nextest_config_is_exact_r5_success_output_only(self):
        text = (ROOT / ".config/nextest.toml").read_bytes().decode("ascii")
        assert_nextest_config(text)
        override = '[[profile.default.overrides]]\n'
        mutants = {
            "broad test": text.replace('test(=cluster::consensus::three_node::r5_shutdown_read_owners)', 'test(r5_shutdown_read_owners)'),
            "any package": text.replace('package(=arxd) & ', ''),
            "any kind": text.replace(' & kind(lib)', ''),
            "all tests": text.replace(R5_FILTER, 'all()'),
            "other test": text.replace('r5_shutdown_read_owners', 'r6_shutdown_report_store_only'),
            "second override": text + override + 'filter = "all()"\nsuccess-output = "immediate"\n',
            "threads": text + 'threads-required = 2\n',
            "group": text + 'test-group = "serial"\n',
            "priority": text + 'priority = 10\n',
            "profile threads": '[profile.default]\ntest-threads = 1\n' + text,
            "override retries": text + 'retries = 2\n',
            "profile retries": '[profile.default]\nretries = 1\n' + text,
            "override slow": text + 'slow-timeout = "60s"\n',
            "override leak": text + 'leak-timeout = "1s"\n',
            "profile global": '[profile.default]\nglobal-timeout = "10m"\n' + text,
            "failure output": text + 'failure-output = "final"\n',
            "final output": text.replace('"immediate"', '"final"'),
            "ci profile": text.replace('profile.default.', 'profile.ci.'),
        }
        for name, mutant in mutants.items():
            with self.subTest(mutant=name):
                self.assertNotEqual(mutant, text)
                with self.assertRaises(AssertionError):
                    assert_nextest_config(mutant)
                with self.assertRaises(AssertionError):
                    assert_nextest_structure(tomllib.loads(mutant))

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
        gate = '#[cfg_attr(target_os = "windows", ignore = "' + REASON + '")]'
        functions = re.findall(r'^async fn ([a-z0-9_]+)\(', source, re.M)
        self.assertEqual(set(functions) & set(EXPECTED), set(EXPECTED))
        actual = re.findall(r'#\[cfg_attr\(\s*target_os\s*=\s*"windows",\s*ignore\s*=\s*"' + re.escape(REASON) + r'"\s*\)\]\s*async fn ([a-z0-9_]+)\(', source)
        self.assertEqual(sorted(actual), sorted(EXPECTED))
        self.assertEqual(source.count('ignore'), len(EXPECTED))
        self.assertNotIn('#[cfg(target_os', source)
        self.assertNotIn('#[cfg(not(target_os', source)

if __name__ == "__main__":
    unittest.main()
