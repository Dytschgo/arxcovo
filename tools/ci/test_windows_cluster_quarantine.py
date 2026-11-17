"""Pin the narrow platform quarantine; keep test bodies compiled everywhere."""
from pathlib import Path
import re
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = ['t3_snapshot_catchup_then_consecutive_append', 't1_follower_offline_enrollment_and_reopen', 't2_leader_offline_reads_and_unknown_retry', 'u6_cancelled_callers_keep_engine_permits_until_resolution', 't4_registry_revocation_snapshot_and_prune', 'r6_shutdown_report_cancelled_proposal', 'r6_shutdown_report_store_only', 'r6_shutdown_report_normal_and_reaped_panic']
REASON = 'Windows three-node quarantine: docs/reviews/m3-cluster-third-attempt-owner-disposition.md'
WINDOWS_UNSAMPLED = ['r1_apply_wait_deadline', 'r2_isolated_read_refusal', 'r3_store_wait_deadline_and_heartbeat', 'r4_read_admission_and_caller_abort', 'r5_shutdown_read_owners']
ISSUANCE = ['i1_happy_retry', 'i2_follower_no_append', 'i3_single_consumption_race', 'i4_leadership_loss', 'i5_remove_between_guards', 'i6_actual_post_wait_clock', 'i7_adapter_rejects_forged_shapes_and_mismatches', 'i8_removed_leaf_remains_chain_valid_stale_registry', 'i9_private_material_never_persisted', 'hook_store_identity_and_admission', 'retained_record_is_required_even_while_member_remains_authorized']
REASON_FIXTURE = 'Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md'
PURE = 'read_failure_attribution_covers_all_errors'


def assert_fixture_quarantine(source, groups, pure=()):
    names = [name for group, _ in groups for name in group]
    assert len(names) == len(set(names))
    functions = re.findall(r'^async fn ([a-z0-9_]+)\(', source, re.M)
    attributes = re.findall(r"#\s*!?\s*\[([^\]]*)\]", source)
    # No alternate spelling/predicate can skip Unix coverage or hide a new test.
    allowed = {re.sub(r"\s+", "", 'cfg_attr(target_os="windows",ignore="' + reason + '")')
               for _, reason in groups}
    for attribute in attributes:
        if re.match(r"\s*cfg(?:_attr)?\b", attribute) or re.search(r"\bignore\b", attribute):
            assert re.sub(r"\s+", "", attribute) in allowed
    for group, reason in groups:
        actual = re.findall(
            r'#\[cfg_attr\(\s*target_os\s*=\s*"windows",\s*ignore\s*=\s*"'
            + re.escape(reason) + r'"\s*\)\]\s*async fn ([a-z0-9_]+)\(', source)
        assert sorted(actual) == sorted(group)
    assert source.count('ignore') == len(names)
    for name in names:
        assert functions.count(name) == 1
    tests = re.findall(r'((?:#\[[^\]]*\]\s*)+)(?:async )?fn ([a-z0-9_]+)\(', source)
    actual_tests = []
    for attrs, name in tests:
        if 'tokio::test' in attrs or '#[test]' in attrs:
            actual_tests.append(name)
            if name in pure:
                assert attrs.strip() == '#[test]'
            else:
                assert 'tokio::test' in attrs and name in names
    assert sorted(actual_tests) == sorted(names + list(pure))
    assert source.count('tokio::test') == len(names)


def assert_windows_quarantine(source):
    assert_fixture_quarantine(source, ((EXPECTED, REASON), (WINDOWS_UNSAMPLED, REASON_FIXTURE)), (PURE,))


def assert_issuance_quarantine(source):
    assert_fixture_quarantine(source, ((ISSUANCE, REASON_FIXTURE),))


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
            (assert_issuance_quarantine if relative == "consensus/issue_tests.rs" else assert_issuance_required)(source)
            for mutation in mutations:
                with self.subTest(relative=relative, mutation=mutation):
                    with self.assertRaises(AssertionError):
                        (assert_issuance_quarantine if relative == "consensus/issue_tests.rs" else assert_issuance_required)(mutation + source)
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
        base = ROOT / "crates/arxd/src/cluster/consensus"
        for filename, check, names in (
            ("three_node.rs", assert_windows_quarantine, EXPECTED + WINDOWS_UNSAMPLED),
            ("issue_tests.rs", assert_issuance_quarantine, ISSUANCE),
        ):
            source = (base / filename).read_text(encoding="utf-8")
            check(source)
            for name in names:
                reason = REASON if name in EXPECTED else REASON_FIXTURE
                gate = '#[cfg_attr(\n    target_os = "windows",\n    ignore = "' + reason + '"\n)]'
                needle = gate + '\nasync fn ' + name + '('
                mutants = {
                    "removed gate": source.replace(needle, 'async fn ' + name + '('),
                    "wrong OS": source.replace(needle, needle.replace('target_os = "windows"', 'target_os = "linux"')),
                    "Unix family": source.replace(needle, needle.replace('target_os = "windows"', 'target_family = "unix"')),
                    "all OS": source.replace(needle, needle.replace('target_os = "windows"', 'all()')),
                    "wrong reason": source.replace(needle, needle.replace(reason, 'unapproved reason')),
                    "renamed": source.replace('async fn ' + name + '(', 'async fn renamed('),
                    "duplicate": source + '\n' + needle + ') {}\n',
                }
                for label, mutant in mutants.items():
                    with self.subTest(file=filename, test=name, mutant=label):
                        self.assertNotEqual(mutant, source)
                        with self.assertRaises(AssertionError):
                            check(mutant)
            mutants = [
                source + '\n#[tokio::test]\nasync fn extra_fixture() {}\n',
                source + '\n#[test]\nfn extra_fixture() {}\n',
                '#![cfg(unix)]\n' + source,
                source + '\n#[ignore]\nasync fn extra() {}\n',
            ]
            for mutant in mutants:
                with self.subTest(file=filename, mutant=mutant[-65:]):
                    with self.assertRaises(AssertionError):
                        check(mutant)
        source = (base / "three_node.rs").read_text(encoding="utf-8")
        with patch(__name__ + '.EXPECTED', EXPECTED + WINDOWS_UNSAMPLED):
            with self.assertRaises(AssertionError):
                assert_windows_quarantine(source)
        for variable, value, check, filename in (
            ('WINDOWS_UNSAMPLED', WINDOWS_UNSAMPLED[:-1], assert_windows_quarantine, 'three_node.rs'),
            ('ISSUANCE', ISSUANCE[:-1], assert_issuance_quarantine, 'issue_tests.rs'),
        ):
            with self.subTest(inventory_drift=variable):
                with patch(__name__ + '.' + variable, value):
                    with self.assertRaises(AssertionError):
                        check((base / filename).read_text(encoding="utf-8"))
        with self.assertRaises(AssertionError):
            assert_windows_quarantine(source.replace('#[test]', '#[test]\n#[ignore]'))

if __name__ == "__main__":
    unittest.main()
