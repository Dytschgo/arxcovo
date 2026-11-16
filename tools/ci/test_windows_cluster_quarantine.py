"""Pin the narrow platform quarantine; keep test bodies compiled everywhere."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
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


def assert_founder_binding(found):
    assert_issuance_required(found)
    if found.count('#[cfg(test)]\n#[path = "found_tests.rs"]\nmod found_tests;') != 1:
        raise AssertionError("founder test binding required")


def assert_subject_seal(sources):
    issue = sources["consensus/issue.rs"]
    if len(re.findall(r"(?m)^mod sealed\s*\{", issue)) != 1:
        raise AssertionError("private seal required")
    if re.search(r"pub(?:\([^)]*\))?\s+mod sealed", issue):
        raise AssertionError("seal must be private")
    traits = [(p, len(re.findall(r"trait LeafSubject\b", text))) for p, text in sources.items()]
    if [(p,n) for p,n in traits if n] != [("consensus/issue.rs",1)]:
        raise AssertionError("trait must live beside seal")
    impls = sorted((p, subject) for p, text in sources.items()
                   for subject in re.findall(r"impl\s+(?:\w+::)*Sealed\s+for\s+(\w+)", text))
    if impls != [("consensus/found.rs", "FounderTicket"), ("consensus/issue.rs", "IssueTicket")]:
        raise AssertionError("unexpected seal implementation")


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

    def test_founder_binding_and_seal(self):
        base = ROOT / "crates/arxd/src/cluster"
        found = (base / "consensus/found.rs").read_text(encoding="utf-8")
        tests = (base / "consensus/found_tests.rs").read_text(encoding="utf-8")
        assert_founder_binding(found)
        assert_issuance_required(tests)
        for mutation in ['#[ignore]', '#![cfg(unix)]', '#![cfg(not(windows))]',
                         '#[cfg(windows)]', '#[cfg(unix)]', '#[cfg(target_os = "linux")]',
                         '#[cfg_attr(target_family = "unix", ignore)]', '#[cfg_attr(all(), ignore)]',
                         '#[cfg_attr(debug_assertions, ignore)]', '#[cfg(any())]',
                         '#[cfg(target_env = "msvc")]', '#[cfg(target_vendor = "pc")]',
                         '#[cfg(feature = "hidden")]']:
            with self.subTest(mutation=mutation):
                with self.assertRaises(AssertionError):
                    assert_issuance_required(mutation + tests)
                with self.assertRaises(AssertionError):
                    assert_founder_binding(mutation + found)
        with self.assertRaises(AssertionError):
            assert_founder_binding(found.replace('mod found_tests;', 'mod absent;'))
        src = ROOT / "crates/arxd/src"
        sources = {p.relative_to(base).as_posix() if p.is_relative_to(base) else p.relative_to(src).as_posix(): p.read_text(encoding="utf-8") for p in src.rglob("*.rs")}
        assert_subject_seal(sources)
        for visibility in ['pub', 'pub(crate)']:
            mutated = dict(sources)
            mutated['consensus/issue.rs'] = mutated['consensus/issue.rs'].replace('mod sealed', visibility + ' mod sealed')
            with self.assertRaises(AssertionError):
                assert_subject_seal(mutated)
        moved = dict(sources)
        moved['consensus/issue.rs'] = moved['consensus/issue.rs'].replace('trait LeafSubject', 'trait MovedSubject')
        moved['other.rs'] = 'trait LeafSubject {}'
        with self.assertRaises(AssertionError):
            assert_subject_seal(moved)
        extra = dict(sources, **{'other.rs': 'impl issue::sealed::Sealed for Outsider {}'})
        with self.assertRaises(AssertionError):
            assert_subject_seal(extra)

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
