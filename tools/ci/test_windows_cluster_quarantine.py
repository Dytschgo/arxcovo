"""Pin the fixture quarantines and keep test bodies compiled everywhere."""
from pathlib import Path
import re
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = ['t3_snapshot_catchup_then_consecutive_append', 't1_follower_offline_enrollment_and_reopen', 't2_leader_offline_reads_and_unknown_retry', 'u6_cancelled_callers_keep_engine_permits_until_resolution', 't4_registry_revocation_snapshot_and_prune', 'r6_shutdown_report_cancelled_proposal', 'r6_shutdown_report_store_only', 'r6_shutdown_report_normal_and_reaped_panic']
REASON = 'Windows three-node quarantine: docs/reviews/m3-cluster-third-attempt-owner-disposition.md'
WINDOWS_UNSAMPLED = ['r1_apply_wait_deadline', 'r2_isolated_read_refusal', 'r3_store_wait_deadline_and_heartbeat', 'r4_read_admission_and_caller_abort', 'r5_shutdown_read_owners']
ISSUANCE = ['i1_happy_retry', 'i2_follower_no_append', 'i3_single_consumption_race', 'i4_leadership_loss', 'i5_remove_between_guards', 'i6_actual_post_wait_clock', 'i7_adapter_rejects_forged_shapes_and_mismatches', 'i8_removed_leaf_remains_chain_valid_stale_registry', 'i9_private_material_never_persisted', 'hook_store_identity_and_admission', 'retained_record_is_required_even_while_member_remains_authorized']
ISSUANCE_REQUIRED = ['anchored_single_voter_receipt_replay_and_revocation']
REASON_FIXTURE = 'Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md'
PURE = 'read_failure_attribution_covers_all_errors'
TLS_SENDER_MISMATCH = 'authenticated_sender_mismatch_and_assumed_founder_control'
TLS_NON_WINDOWS_REASON = 'intermittent NotLeader quarantine: docs/ROADMAP.md Known issues (Engine fix hold)'
TLS_NON_WINDOWS_GATE = (
    '#[cfg_attr(\n    not(target_os = "windows"),\n    ignore = "'
    + TLS_NON_WINDOWS_REASON + '"\n)]\n'
)
TLS_FIXTURE = ['enrolled_target_binding_revocation_and_stale_acceptance', TLS_SENDER_MISMATCH, 'store_lock_wait_does_not_block_runtime']
TLS_REQUIRED = ['live_verifier_does_not_retain_store_and_poison_refuses', 'founder_joiner_mutual_tls_real_identities', 'learner_catch_up_gives_joiner_own_registry']
TLS_PURE = ['tls_single_node_directory_precondition']
MEMBERSHIP_UNSAMPLED = ['interrupted_joint_exact_guarded_flatten']


def assert_membership_contract(source, parent):
    assert_fixture_quarantine(source, ((MEMBERSHIP_UNSAMPLED, REASON_FIXTURE),))
    binding = '#[cfg(test)]\nmod membership_contract_tests;'
    assert parent.count(binding) == 1
    assert parent.count('mod membership_contract_tests;') == 1
    # No parent predicate may silently hide the new module on Unix.
    prefix = parent.split('mod membership_contract_tests;')[0]
    attrs = re.search(r'((?:#\[[^\]]*\]\s*)+)$', prefix)
    assert attrs and attrs[1].strip() == '#[cfg(test)]'
    assert len(EXPECTED) == 8
    assert set(MEMBERSHIP_UNSAMPLED).isdisjoint(EXPECTED + WINDOWS_UNSAMPLED)
    assert len(EXPECTED + WINDOWS_UNSAMPLED + ISSUANCE + TLS_FIXTURE + MEMBERSHIP_UNSAMPLED) == 28


class MembershipQuarantineTests(unittest.TestCase):
    def test_unsampled_membership_contract(self):
        base = ROOT / 'crates/arxd/src/cluster'
        source = (base / 'consensus/membership_contract_tests.rs').read_text(encoding='utf-8')
        parent = (base / 'consensus.rs').read_text(encoding='utf-8')
        assert_membership_contract(source, parent)
        gate = '#[cfg_attr(\n    target_os = "windows",\n    ignore = "' + REASON_FIXTURE + '"\n)]'
        name = MEMBERSHIP_UNSAMPLED[0]
        signature = 'async fn ' + name + '('
        test = '#[tokio::test(flavor = "current_thread")]\n' + gate + '\n' + signature
        mutants = [
            source.replace(gate, ''),
            source.replace(gate, gate.replace('windows', 'linux')),
            source.replace(gate, gate.replace('target_os = "windows"', 'target_family = "unix"')),
            source.replace(gate, gate.replace('target_os = "windows"', 'all()')),
            source.replace(REASON_FIXTURE, 'wrong reason'),
            source.replace(signature, 'async fn renamed('),
            source.replace(test, 'async fn removed('),
            source + '\n' + test + ') {}\n',
            source + '\n#[test]\nfn hidden() {}\n',
            '#![cfg(unix)]\n' + source,
            '#![ignore]\n' + source,
        ]
        for mutant in mutants:
            self.assertNotEqual(source, mutant)
            with self.assertRaises(AssertionError):
                assert_membership_contract(mutant, parent)
        binding = '#[cfg(test)]\nmod membership_contract_tests;'
        for replacement in ['', 'mod membership_contract_tests;', '#[cfg(unix)]\nmod membership_contract_tests;',
                            binding + '\n' + binding, '#[cfg(unix)]\n' + binding]:
            mutant = parent.replace(binding, replacement)
            self.assertNotEqual(parent, mutant)
            with self.assertRaises(AssertionError):
                assert_membership_contract(source, mutant)
        for variable, value in [('MEMBERSHIP_UNSAMPLED', []), ('EXPECTED', EXPECTED + MEMBERSHIP_UNSAMPLED)]:
            with patch(__name__ + '.' + variable, value):
                with self.assertRaises(AssertionError):
                    assert_membership_contract(source, parent)
        # Pin the unchanged sampler selection independently of the Rust guard.
        import windows_diag_sample as sample
        self.assertEqual(set(sample.IDS), set(EXPECTED))
        self.assertEqual(len(sample.IDS), 8)
        self.assertNotIn('membership_contract_tests', sample.FILTER)


def assert_fixture_quarantine(source, groups, pure=(), *, required=()):
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
            elif name in required:
                assert 'tokio::test' in attrs
                assert not re.search(r'\bcfg(?:_attr)?\b|\bignore\b', attrs)
            else:
                assert 'tokio::test' in attrs and name in names
    assert sorted(actual_tests) == sorted(names + list(pure) + list(required))
    assert source.count('tokio::test') == len(names) + len(required)


def assert_windows_quarantine(source):
    assert_fixture_quarantine(source, ((EXPECTED, REASON), (WINDOWS_UNSAMPLED, REASON_FIXTURE)), (PURE,))


def assert_issuance_quarantine(source):
    assert_fixture_quarantine(source, ((ISSUANCE, REASON_FIXTURE),), required=ISSUANCE_REQUIRED)


REAL_REQUIRED = ('founder_init(', '.found(', '.enroll(', 'peer_registry()', 'pki::pump(')
REAL_FORBIDDEN = ('assume(', 'test_ticket', 'pki::leaf(', 'pki::registry(',
                  'pki::connections(', 'handshake(', 'Cluster', '[250')


PROOF_TOKENS = {
    'founder_joiner_mutual_tls_real_identities': (REAL_REQUIRED, REAL_FORBIDDEN),
    'learner_catch_up_gives_joiner_own_registry': (
        REAL_REQUIRED + ('add_learner(', 'applied_index_at_least('),
        REAL_FORBIDDEN + ('change_membership', 'initialize(', '.install(', 'AddVoter', 'authenticated'),
    ),
}


def assert_real_identity_body(source):
    segments = re.split(r'(?m)^((?:async )?fn [a-z0-9_]+\()', source)
    for name, (required, forbidden) in PROOF_TOKENS.items():
        bodies = [body for signature, body in zip(segments[1::2], segments[2::2])
                  if signature == 'async fn ' + name + '(']
        assert len(bodies) == 1
        for token in required:
            assert token in bodies[0], (name, token)
        for token in forbidden:
            assert token not in bodies[0], (name, token)


def assert_tls_engine_quarantine(source):
    assert_real_identity_body(source)
    # Strip only the pinned exception, then apply the unchanged Windows fixture rules.
    windows_gate = '#[cfg_attr(\n    target_os = "windows",\n    ignore = "' + REASON_FIXTURE + '"\n)]\n'
    windows_binding = windows_gate + 'async fn ' + TLS_SENDER_MISMATCH + '('
    binding = TLS_NON_WINDOWS_GATE + windows_binding
    assert source.count(binding) == 1
    assert_fixture_quarantine(source.replace(binding, windows_binding, 1),
                              ((TLS_FIXTURE, REASON_FIXTURE),), TLS_PURE, required=TLS_REQUIRED)
    segments = re.split(r'(?m)^((?:async )?fn [a-z0-9_]+\()', source)
    constructors = []
    for signature, body in zip(segments[1::2], segments[2::2]):
        if re.search(r'Cluster::(?:new|unfounded)\(', body):
            constructors.append(re.search(r'fn ([a-z0-9_]+)', signature)[1])
    assert sorted(constructors) == sorted(TLS_FIXTURE)
    assert source.count('use super::super::three_node::{Cluster, code, mint};') == 1


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
        (issue, '#[cfg(test)]\n#[path = "tls_engine_tests.rs"]\nmod tls_engine_tests;'),
        (pki, '#[cfg(test)]\nmod tests;'),
        (pki, '#[cfg(test)]\nmod tls_tests;'),
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
        for relative in ["consensus/issue_tests.rs", "pki/tests.rs", "consensus/tls_engine_tests.rs", "pki/tls_tests.rs"]:
            source = (base / relative).read_text(encoding="utf-8")
            (assert_issuance_quarantine if relative == "consensus/issue_tests.rs" else assert_tls_engine_quarantine if relative == "consensus/tls_engine_tests.rs" else assert_issuance_required)(source)
            for mutation in mutations:
                with self.subTest(relative=relative, mutation=mutation):
                    with self.assertRaises(AssertionError):
                        (assert_issuance_quarantine if relative == "consensus/issue_tests.rs" else assert_tls_engine_quarantine if relative == "consensus/tls_engine_tests.rs" else assert_issuance_required)(mutation + source)
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

        with self.assertRaises(AssertionError):
            assert_issuance_modules(issue.replace('mod tls_engine_tests;', 'mod absent;'), pki)
        with self.assertRaises(AssertionError):
            assert_issuance_modules(issue, pki.replace('mod tls_tests;', 'mod absent;'))

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

    def test_real_identity_proof_tokens(self):
        source = (ROOT / 'crates/arxd/src/cluster/consensus/tls_engine_tests.rs').read_text(encoding='utf-8')
        assert_real_identity_body(source)
        for name, (required, forbidden) in PROOF_TOKENS.items():
            signature = 'async fn ' + name + '() {'
            prefix, rest = source.split(signature)
            body, separator, suffix = rest.partition('\n#[tokio::test')
            for token in forbidden:
                with self.subTest(name=name, forbidden=token):
                    with self.assertRaises(AssertionError):
                        assert_real_identity_body(prefix + signature + '\n// ' + token + '\n' + rest)
            for token in required:
                with self.subTest(name=name, required=token):
                    with self.assertRaises(AssertionError):
                        assert_real_identity_body(prefix + signature + body.replace(token, 'removed_token') + separator + suffix)

    def test_anchored_single_voter_required(self):
        source = (ROOT / "crates/arxd/src/cluster/consensus/issue_tests.rs").read_text(encoding="utf-8")
        name = ISSUANCE_REQUIRED[0]
        assert ISSUANCE_REQUIRED == ['anchored_single_voter_receipt_replay_and_revocation']
        signature = 'async fn ' + name + '() {'
        body = source.split(signature, 1)[1]
        assert body.count('ConsensusNode::start(') == 1
        for token in ('founder_init(', '.found(', '.enroll('):
            assert token in body
        assert not re.search(r'\bCluster\b', body)
        for mutant in (source.replace(signature, 'async fn renamed() {'),
                       source[:source.rfind('#[tokio::test')],
                       source.replace(signature, '#[ignore]\n' + signature),
                       source.replace(signature, '#[cfg(unix)]\n' + signature)):
            with self.assertRaises(AssertionError):
                assert_issuance_quarantine(mutant)
        with patch(__name__ + '.ISSUANCE_REQUIRED', []):
            with self.assertRaises(AssertionError):
                assert_issuance_quarantine(source)

    def test_exact_non_windows_sender_mismatch_quarantine(self):
        source = (ROOT / 'crates/arxd/src/cluster/consensus/tls_engine_tests.rs').read_text(encoding='utf-8')
        assert_tls_engine_quarantine(source)
        gate = TLS_NON_WINDOWS_GATE
        other = 'async fn store_lock_wait_does_not_block_runtime('
        with_extra_gate = source.replace(other, gate + other)
        mutations = {
            'removed': source.replace(gate, ''),
            'moved to another test': with_extra_gate.replace(gate, '', 1),
            'added to another test': with_extra_gate,
            'broader predicate': source.replace(gate, gate.replace('not(target_os = "windows")', 'all()')),
            'narrower predicate': source.replace(gate, gate.replace('not(target_os = "windows")', 'target_os = "linux"')),
            'missing reason': source.replace(gate, gate.replace(' = "' + TLS_NON_WINDOWS_REASON + '"', '')),
            'wrong reason': source.replace(TLS_NON_WINDOWS_REASON, 'unapproved reason'),
        }
        for label, mutant in mutations.items():
            with self.subTest(mutant=label):
                self.assertNotEqual(mutant, source)
                with self.assertRaises(AssertionError):
                    assert_tls_engine_quarantine(mutant)

    def test_exact_windows_only_quarantine(self):
        base = ROOT / "crates/arxd/src/cluster/consensus"
        for filename, check, names in (
            ("three_node.rs", assert_windows_quarantine, EXPECTED + WINDOWS_UNSAMPLED),
            ("issue_tests.rs", assert_issuance_quarantine, ISSUANCE),
            ("tls_engine_tests.rs", assert_tls_engine_quarantine, TLS_FIXTURE),
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
        tls = (base / "tls_engine_tests.rs").read_text(encoding="utf-8")
        gate = '#[cfg_attr(\n    target_os = "windows",\n    ignore = "' + REASON_FIXTURE + '"\n)]\n'
        for name in TLS_REQUIRED + TLS_PURE:
            signature = ('async fn ' if name in TLS_REQUIRED else 'fn ') + name + '('
            with self.subTest(required_tls=name):
                with self.assertRaises(AssertionError):
                    assert_tls_engine_quarantine(tls.replace(signature, gate + signature))
        signature = 'async fn ' + TLS_REQUIRED[0] + '() {'
        mutant = tls.replace(signature, signature + '\n    Cluster::new(Params::default()).await;')
        self.assertNotEqual(mutant, tls)
        with self.assertRaises(AssertionError):
            assert_tls_engine_quarantine(mutant)
        with self.assertRaises(AssertionError):
            assert_tls_engine_quarantine(tls.replace('use super::super::three_node::{Cluster, code, mint};', ''))
        source = (base / "three_node.rs").read_text(encoding="utf-8")
        with patch(__name__ + '.EXPECTED', EXPECTED + WINDOWS_UNSAMPLED):
            with self.assertRaises(AssertionError):
                assert_windows_quarantine(source)
        for variable, value, check, filename in (
            ('WINDOWS_UNSAMPLED', WINDOWS_UNSAMPLED[:-1], assert_windows_quarantine, 'three_node.rs'),
            ('ISSUANCE', ISSUANCE[:-1], assert_issuance_quarantine, 'issue_tests.rs'),
            ('TLS_FIXTURE', TLS_FIXTURE[:-1], assert_tls_engine_quarantine, 'tls_engine_tests.rs'),
            ('TLS_REQUIRED', TLS_REQUIRED[:-1], assert_tls_engine_quarantine, 'tls_engine_tests.rs'),
        ):
            with self.subTest(inventory_drift=variable):
                with patch(__name__ + '.' + variable, value):
                    with self.assertRaises(AssertionError):
                        check((base / filename).read_text(encoding="utf-8"))
        with self.assertRaises(AssertionError):
            assert_windows_quarantine(source.replace('#[test]', '#[test]\n#[ignore]'))


ANCHORED_JOIN_REQUIRED = ['anchored_preproof_single_voter']


def assert_anchored_preproof(source, parent):
    assert ANCHORED_JOIN_REQUIRED == ['anchored_preproof_single_voter']
    assert_fixture_quarantine(source, (), required=ANCHORED_JOIN_REQUIRED)
    binding = '#[cfg(test)]\nmod anchored_join_tests;'
    assert parent.count(binding) == 1
    assert parent.count('mod anchored_join_tests;') == 1
    prefix = parent.split('mod anchored_join_tests;')[0]
    attrs = re.search(r'((?:#\[[^\]]*\]\s*)+)$', prefix)
    assert attrs and attrs[1].strip() == '#[cfg(test)]'
    assert source.count('ConsensusNode::start(') == 1
    for token in ('founder_init(', '.found(', '.enroll(', 'BoundVoters::observe(',
                  'spawn_blocking(', 'drop(store)', 'exchange(', 'identity_commit',
                  'shutdown()', 'sm_weak.upgrade()', 'log_weak.upgrade()'):
        assert token in source, token
    for token in ('Cluster::', 'test_ticket', 'assume(', 'pki::leaf(', 'seedregistry'):
        assert token not in source, token


class AnchoredPreproofTests(unittest.TestCase):
    def test_required_binding_and_single_start(self):
        base = ROOT / 'crates/arxd/src/cluster'
        source = (base / 'consensus/anchored_join_tests.rs').read_text(encoding='utf-8')
        parent = (base / 'consensus.rs').read_text(encoding='utf-8')
        assert_anchored_preproof(source, parent)
        name = ANCHORED_JOIN_REQUIRED[0]
        for mutant in (source.replace(name, 'renamed'), '', '#![cfg(unix)]\n' + source,
                       source.replace('async fn ' + name, '#[ignore]\nasync fn ' + name),
                       source + '\nfn hidden_helper(){ ConsensusNode::start(); }',
                       source + '\nfn hidden_helper(){ Cluster::new(); }'):
            with self.assertRaises(AssertionError):
                assert_anchored_preproof(mutant, parent)
        binding = '#[cfg(test)]\nmod anchored_join_tests;'
        for replacement in ('', '#[cfg(unix)]\nmod anchored_join_tests;',
                            '#[cfg(unix)]\n' + binding, binding + '\n' + binding):
            with self.assertRaises(AssertionError):
                assert_anchored_preproof(source, parent.replace(binding, replacement))
        with patch(__name__ + '.ANCHORED_JOIN_REQUIRED', []):
            with self.assertRaises(AssertionError):
                assert_anchored_preproof(source, parent)



QUERY_REQUIRED = ['owned_anchor_query_single_voter']
QUERY_LOCAL_REQUIRED = [
    'checked_clock_refusal_has_no_submission_and_malformed_has_no_sample',
    'owned_query_dropped_callers_and_read_admission',
    'owned_query_timeout_shutdown_retains_submitted_resources',
    'owned_query_bounds_poison_and_no_submission',
    'owned_query_consumed_tls_zero_proof',
]


def assert_owned_query(source, module, parent):
    assert QUERY_REQUIRED == ['owned_anchor_query_single_voter']
    assert len(QUERY_LOCAL_REQUIRED) == 5
    assert_fixture_quarantine(source, (), required=QUERY_REQUIRED + QUERY_LOCAL_REQUIRED)
    assert_issuance_required(source)
    assert_issuance_required(parent)
    # All helpers and ancestors participate: no hidden platform predicates.
    assert_issuance_required(module.replace('#[cfg(not(test))]\n            drop(handle);', ''))
    for text, binding in (
        (parent, 'pub(in crate::cluster) mod anchor_query;'),
        (module, '#[cfg(test)]\n#[path = "anchor_query_tests.rs"]\nmod anchor_query_tests;'),
    ):
        assert text.count(binding) == 1
        prefix = text.split(binding)[0]
        assert not re.search(r'#\[[^\]]*\]\s*$', prefix)
    assert module.count('mod anchor_query_tests;') == 1
    assert parent.count('mod anchor_query;') == 1
    assert source.count('ConsensusNode::start(') == 1
    for token in ('Cluster::', 'test_ticket', 'assume(', 'pki::leaf('):
        assert token not in source
    for token in ('.anchor_query(', '.shutdown()', 'EngineEnd::Absent',
                  'M1 post-abort read retention', 'M2 consumed query server plaintext',
                  'is_finished()', 'tls_query(', '.found(', '.enroll('):
        assert token in source


class OwnedQueryTests(unittest.TestCase):
    def test_query_required_inventory_and_helpers(self):
        base = ROOT / 'crates/arxd/src/cluster'
        source = (base / 'consensus/anchor_query_tests.rs').read_text(encoding='utf-8')
        module = (base / 'consensus/anchor_query.rs').read_text(encoding='utf-8')
        parent = (base / 'consensus.rs').read_text(encoding='utf-8')
        assert_owned_query(source, module, parent)
        for name in QUERY_REQUIRED + QUERY_LOCAL_REQUIRED:
            signature = 'async fn ' + name + '('
            for mutation in (
                source.replace(signature, 'async fn renamed('),
                source.replace(signature, '#[ignore]\n' + signature),
                source.replace(signature, '#[cfg(unix)]\n' + signature),
            ):
                with self.assertRaises(AssertionError):
                    assert_owned_query(mutation, module, parent)
        for mutation in ('', '#![cfg(unix)]\n' + source,
                         source + '\nfn hidden(){ ConsensusNode::start(); }',
                         source + '\n#[cfg(unix)]\nfn hidden(){}',
                         source + '\n#[ignore]\nfn hidden(){}'):
            with self.assertRaises(AssertionError):
                assert_owned_query(mutation, module, parent)
        with self.assertRaises(AssertionError):
            assert_owned_query(source, module, '#![cfg(unix)]\n' + parent)
        for text, token, which in (
            (parent, 'pub(in crate::cluster) mod anchor_query;', 'parent'),
            (module, 'mod anchor_query_tests;', 'module'),
        ):
            for replacement in ('', '#[cfg(unix)]\n' + token, token + token):
                changed = text.replace(token, replacement)
                with self.assertRaises(AssertionError):
                    assert_owned_query(source, changed if which == 'module' else module,
                                       changed if which == 'parent' else parent)
        with patch(__name__ + '.QUERY_REQUIRED', []):
            with self.assertRaises(AssertionError):
                assert_owned_query(source, module, parent)


IDENTITY_REQUIRED = ['peer_identity_retains_verified_tuple',
                     'peer_identity_fresh_binding_controls',
                     'peer_identity_assumed_founder_denied']


def assert_identity_inventory(source, tls, pki):
    assert IDENTITY_REQUIRED == ['peer_identity_retains_verified_tuple',
                                 'peer_identity_fresh_binding_controls',
                                 'peer_identity_assumed_founder_denied']
    for text in (source, tls, pki):
        # Fail closed rather than parse nested Rust block comments. These three
        # guarded files currently use line comments only.
        assert "/*" not in text and "*/" not in text
        assert_issuance_required(text)
    functions = re.findall(r'(?m)^fn ([a-z0-9_]+)\(', source)
    assert len(functions) == len(set(functions))
    tests = re.findall(r'(?m)^((?:#\[[^\]]*\]\s*)+)(async )?fn ([a-z0-9_]+)\(', source)
    actual = []
    for attrs, async_prefix, name in tests:
        assert not async_prefix
        if 'test' in attrs:
            assert attrs.strip() == '#[test]'
            actual.append(name)
    assert sorted(actual) == sorted(IDENTITY_REQUIRED)
    assert source.count('#[test]') == 3
    for parent, name, binding in (
        (pki, 'tls_tests', '#[cfg(test)]\nmod tls_tests;'),
        (tls, 'authenticated_identity_tests', '#[cfg(test)]\n#[path = "authenticated_identity_tests.rs"]\nmod authenticated_identity_tests;'),
    ):
        assert parent.count(binding) == 1
        assert len(re.findall(r"(?m)^" + re.escape(binding), parent)) == 1
        assert parent.count('mod ' + name + ';') == 1
        prefix = parent.split('mod ' + name + ';')[0]
        attrs = re.search(r'((?:#\[[^\]]*\]\s*)+)$', prefix)
        assert attrs and attrs[1].strip() == binding.rsplit('\nmod ', 1)[0]


class IdentityInventoryTests(unittest.TestCase):
    def test_exact_plain_tests_and_both_bindings(self):
        base = ROOT / 'crates/arxd/src/cluster'
        source = (base / 'pki/authenticated_identity_tests.rs').read_text(encoding='utf-8')
        tls = (base / 'pki/tls_tests.rs').read_text(encoding='utf-8')
        pki = (base / 'pki.rs').read_text(encoding='utf-8')
        assert_identity_inventory(source, tls, pki)
        mutants = ['', source.replace(IDENTITY_REQUIRED[0], 'renamed'),
                   source.replace('#[test]', '', 1), source + source]
        for mutant in mutants:
            with self.assertRaises(AssertionError):
                assert_identity_inventory(mutant, tls, pki)
        hiding = ['#[ignore]', '#![cfg(unix)]', '#[cfg(not(windows))]',
                  '#[cfg(windows)]', '#[cfg(target_os = "linux")]',
                  '#[cfg_attr(all(), ignore)]', '#[cfg(any())]',
                  '#[cfg(target_env = "msvc")]', '#[cfg(feature = "hidden")]']
        for attr in hiding:
            for index, text in enumerate((source, tls, pki)):
                # Includes helper-level and parent-level hiding, not just tests.
                replacements = [attr + '\n' + text]
                if index == 0:
                    replacements += [text.replace('fn ', attr + '\nfn ', 1),
                                     text.replace('#[test]', attr + '\n#[test]', 1)]
                for replacement in replacements:
                    args = [source, tls, pki]
                    args[index] = replacement
                    with self.assertRaises(AssertionError):
                        assert_identity_inventory(*args)
        for index, name, binding in (
            (1, 'authenticated_identity_tests', '#[cfg(test)]\n#[path = "authenticated_identity_tests.rs"]\nmod authenticated_identity_tests;'),
            (2, 'tls_tests', '#[cfg(test)]\nmod tls_tests;'),
        ):
            original = [source, tls, pki][index]
            for replacement in ('', binding + '\n' + binding,
                                binding.replace(name, 'absent'),
                                '/*\n' + binding + '\n*/',
                                '#[cfg(unix)]\n' + binding,
                                binding.replace('#[cfg(test)]', ''),
                                binding.replace('#[cfg(test)]', '// #[cfg(test)]'),
                                binding.replace('authenticated_identity_tests.rs', 'absent.rs')):
                if replacement == binding:
                    continue
                args = [source, tls, pki]
                args[index] = original.replace(binding, replacement)
                with self.assertRaises(AssertionError):
                    assert_identity_inventory(*args)
        for name in IDENTITY_REQUIRED:
            signature = 'fn ' + name + '('
            for mutation in (source.replace(signature, 'async ' + signature),
                             '/*\n' + source + '\n*/',
                             source.replace('#[test]\n' + signature,
                                            '/* #[test] */\n' + signature)):
                with self.assertRaises(AssertionError):
                    assert_identity_inventory(mutation, tls, pki)
        for weakened in ([], IDENTITY_REQUIRED[:-1], IDENTITY_REQUIRED + [IDENTITY_REQUIRED[0]]):
            with patch(__name__ + '.IDENTITY_REQUIRED', weakened):
                with self.assertRaises(AssertionError):
                    assert_identity_inventory(source, tls, pki)




# Kept in this module because the existing static CI step selects this file.
HEAVY_FILTER = (
    'test(/^service::unix_bridge::tests::e6::/) | '
    'test(/^service::unix_bridge::tests::e7::/) | '
    'test(/^service::executor::tests::/)'
)


C15_FILTER = ('test(=cluster::consensus::bound_start::enrollment_ingress::hosted::'
              'clock_reuse_campaign::retained_issuer_replays_exact_request_and_refuses_controlled_expiry)')


def assert_heavy_service_group(source):
    import tomllib
    # Exact semantic shape forbids retries, timeout/fail-fast/default-slot changes,
    # widened platform scope, extra overrides and accidental extra membership.
    # Amendment A14 admits only the Linux C15 success-output override.
    assert tomllib.loads(source) == {
        'test-groups': {'windows-heavy-service': {'max-threads': 2}},
        'profile': {'default': {'overrides': [{
            'platform': 'cfg(windows)',
            'filter': HEAVY_FILTER,
            'test-group': 'windows-heavy-service',
        }, {
            'platform': 'cfg(target_os = "linux")',
            'filter': C15_FILTER,
            'success-output': 'final',
        }]}},
    }


class HeavyServiceGroupTests(unittest.TestCase):
    def test_windows_only_group_contract_and_mutants(self):
        source = (ROOT / '.config/nextest.toml').read_text(encoding='utf-8')
        assert_heavy_service_group(source)
        mutants = [
            '',
            source.replace('windows-heavy-service', 'other-group'),
            source.replace('max-threads = 2', 'max-threads = 1'),
            source.replace('max-threads = 2', 'max-threads = 4'),
            source.replace("platform = 'cfg(windows)'", ''),
            source.replace('cfg(windows)', 'cfg(any(windows, unix))'),
            source.replace('cfg(windows)', 'cfg(unix)'),
            source + '\nretries = 1\n',
            source + '\n[profile.ci]\nretries = 1\n',
            source.replace("[[profile.default.overrides]]",
                           "[profile.default]\nretries = 1\n\n[[profile.default.overrides]]", 1),
            source.replace('success-output = "final"', 'success-output = "immediate"'),
            source.replace('success-output = "final"\n', ''),
            source.replace('success-output = "final"', 'success-output = "final"\nretries = 2'),
            source.replace('success-output = "final"',
                           'success-output = "final"\nslow-timeout = "5m"'),
            source.replace('success-output = "final"',
                           'success-output = "final"\nthreads-required = 2'),
            source.replace('cfg(target_os = "linux")', 'cfg(unix)'),
            source.replace(C15_FILTER, 'test(clock_reuse_campaign)'),
        ]
        filters = HEAVY_FILTER.split(' | ')
        for item in filters:
            mutants.append(source.replace(HEAVY_FILTER,
                                           ' | '.join(f for f in filters if f != item)))
        for mutant in mutants:
            with self.subTest(mutant=mutant):
                with self.assertRaises(AssertionError):
                    assert_heavy_service_group(mutant)


# Kept in this module because the existing static CI step selects this file.
B2_TEST = "b2_snapshot_barrier_races_busy_unpublish_pending_removal_and_old_origin"
B2_TEST_ATTR = "#[tokio::test]"
B2_TESTS_DECL = "pub(crate) mod tests {"
# Raw substring counts after the macOS gate was removed.
B2_TESTS_CFG = 23
B2_TESTS_IGNORE = 0
B2_DECL_CFG = 1
B2_PATH_ATTRS = 0


def _strip_block_comments(text):
    # Join tokens a block comment splits (`ig/**/nore`) without dropping a
    # `cfg` or `ignore` that is written in the raw text, including inside
    # comments and strings. Those raw substrings are counted separately.
    return re.sub(r"/\*.*?\*/", "", text, flags=re.S)


def _norm(text):
    return re.sub(r"\s+", "", text)


def _tests_parts(source):
    if source.count(B2_TESTS_DECL) != 1:
        raise AssertionError("vm tests module binding required")
    prefix, tests = source.split(B2_TESTS_DECL, 1)
    start = prefix.rfind("\n}")
    if start < 0:
        raise AssertionError("vm tests module binding required")
    # Attributes on `mod tests`, through the declaration, including any path.
    declaration = prefix[start:] + B2_TESTS_DECL
    return declaration, tests


def assert_b2_runs_on_all_platforms(source):
    declaration, tests = _tests_parts(source)
    stripped = _strip_block_comments(tests)
    if (tests.count("cfg"), tests.count("ignore"),
            stripped.count("cfg"), stripped.count("ignore")) != (
            B2_TESTS_CFG, B2_TESTS_IGNORE, B2_TESTS_CFG, B2_TESTS_IGNORE):
        raise AssertionError("tests module cfg or ignore token count drifted")
    decl_stripped = _strip_block_comments(declaration)
    if (declaration.count("cfg"), decl_stripped.count("cfg")) != (B2_DECL_CFG, B2_DECL_CFG):
        raise AssertionError("tests module declaration cfg count drifted")
    file_stripped = _strip_block_comments(source)
    if (source.count("#[path"), file_stripped.count("#[path")) != (B2_PATH_ATTRS, B2_PATH_ATTRS):
        raise AssertionError("vm.rs path attribute would redirect the tests module")
    masked = _mask_comments(tests)
    starts = list(re.finditer(r"^[ \t]*async fn " + B2_TEST + r"\(", masked, re.M))
    if len(starts) != 1:
        raise AssertionError("b2 snapshot barrier test must have one active definition")
    start = starts[0].start()
    cut = _attribute_cut(masked[:start])
    if _norm(tests[cut:start]) != B2_TEST_ATTR:
        raise AssertionError("b2 snapshot barrier must stay an unconditional test")


class B2SnapshotBarrierCoverageTests(unittest.TestCase):
    def test_b2_snapshot_barrier_runs_on_all_platforms(self):
        source = (ROOT / "crates/arxd/src/mcp/vm.rs").read_text(encoding="utf-8")
        assert_b2_runs_on_all_platforms(source)
        signature = "async fn " + B2_TEST + "("
        target = B2_TEST_ATTR + "\n    " + signature
        other = "async fn stale_copy_cleanup_preserves_committed_static_point_and_active_media("
        mutants = {
            "macOS gate restored": source.replace(
                signature,
                '#[cfg_attr(target_os = "macos", ignore = "flaky")]\n    ' + signature,
                1,
            ),
            "test attribute removed": source.replace(target, signature, 1),
            "test attribute commented out": source.replace(target, "// " + target, 1),
            "renamed function": source.replace(signature, "async fn renamed(", 1),
            "neighbor ignore": source.replace(other, "#[ignore]\n    " + other, 1),
            "cfg on target function": source.replace(
                target,
                '    #[cfg(not(target_os="linux"))]\n    ' + target,
                1,
            ),
            "cfg on neighbor": source.replace(
                other, '#[cfg(not(target_os="linux"))]\n    ' + other, 1
            ),
            "cfg on tests module": source.replace(
                "#[cfg(test)]\n" + B2_TESTS_DECL,
                '#[cfg(not(target_os="linux"))]\n#[cfg(test)]\n' + B2_TESTS_DECL,
                1,
            ),
            "inner cfg on tests module": source.replace(
                B2_TESTS_DECL + "\n",
                B2_TESTS_DECL + "\n    #![cfg(not(windows))]\n",
                1,
            ),
            "comment cfg on target": source.replace(
                target,
                "    #[/* Fixture limitation */ cfg(not(target_os = \"windows\"))] "
                "// Platform restriction\n    " + target,
                1,
            ),
            "comment cfg on neighbor": source.replace(
                "    #[tokio::test]\n    " + other,
                "    #[/* Fixture limitation */ cfg(not(target_os = \"windows\"))]\n"
                "    #[tokio::test]\n    " + other,
                1,
            ),
            "raw cfg identifier": source.replace(
                target,
                "    #[r#cfg(not(target_os = \"windows\"))] // raw\n    " + target,
                1,
            ),
            "macro generated cfg": source.replace(
                B2_TESTS_DECL + "\n",
                B2_TESTS_DECL + "\n"
                "    macro_rules! hide { ($item:item) => { "
                "#[cfg(not(target_os = \"windows\"))] $item }; }\n",
                1,
            ),
            "comment split ignore": source.replace(
                other, "#[ig/**/nore]\n    " + other, 1
            ),
        }
        for label, mutant in mutants.items():
            with self.subTest(mutant=label):
                self.assertNotEqual(mutant, source)
                with self.assertRaises(AssertionError):
                    assert_b2_runs_on_all_platforms(mutant)


# The C11 case is compiled only inside hosted, which is Linux-only. The
# ignore still names target_os = "linux" so the gate cannot widen later.
C11_TEST = "retained_joiners_promote_and_reconcile"
C11_REASON = (
    "intermittent Linux C11 retained-joiners refusal: docs/ROADMAP.md Known issues"
)
C11_PREDICATE = 'target_os = "linux"'
C11_GATE = (
    "#[cfg_attr(\n"
    "    " + C11_PREDICATE + ",\n"
    '    ignore = "' + C11_REASON + '"\n'
    ")]"
)
C11_MODULE = (
    "crates/arxd/src/cluster/consensus/bound_start/enrollment_ingress/hosted/"
    "promotion_campaign.rs"
)
C11_PARENT = "crates/arxd/src/cluster/consensus/bound_start/enrollment_ingress.rs"
C11_HOSTED_DECL = '#[cfg(all(test, target_os = "linux"))]\nmod hosted {'
C11_PROMO_DECL = "    mod promotion_campaign;"
# promotion_campaign.rs on main had no `cfg` or `ignore` substring. The gate
# adds one of each (`cfg_attr` contributes the `cfg`).
C11_CFG = 1
C11_IGNORE = 1
C11_LIB = "crates/arxd/src/lib.rs"
C11_CLUSTER = "crates/arxd/src/cluster.rs"
C11_CONSENSUS = "crates/arxd/src/cluster/consensus.rs"
C11_BOUND = "crates/arxd/src/cluster/consensus/bound_start.rs"
C11_ENROLL_DECL = '#[cfg(target_os = "linux")]\nmod enrollment_ingress;'
# Whole-file raw cfg/ignore counts from main, also required after block-comment
# removal. The target file is main's 0,0 plus the one gate. See docs/ROADMAP.md
# Known issues, "Quarantined intermittent Linux
# `retained_joiners_promote_and_reconcile`". Re-pin a count, attribute block,
# or inner attribute only after confirming the test still compiles on Linux.
# A pin edit that hides the module is not a quarantine update.
C11_INNER_NONE = ()
C11_INNER_ALLOW = ("#![allow(dead_code)]",)
C11_FILES = (
    # path, raw cfg, raw ignore, whitespace-normalized inner attributes
    (C11_LIB, 1, 0, C11_INNER_NONE),
    (C11_CLUSTER, 2, 0, C11_INNER_NONE),
    (C11_CONSENSUS, 63, 0, C11_INNER_ALLOW),
    (C11_BOUND, 106, 0, C11_INNER_ALLOW),
    (C11_PARENT, 14, 0, C11_INNER_NONE),
    (C11_MODULE, C11_CFG, C11_IGNORE, C11_INNER_NONE),
)
# Attribute block directly above the active `mod` line, whitespace-normalized,
# comments included. The hosted gate is this pin, not a separate substring.
C11_BLOCK_CLUSTER = ""
C11_BLOCK_CONSENSUS = ""
C11_BLOCK_BOUND = ""
C11_BLOCK_ENROLL = '#[cfg(target_os="linux")]'
C11_BLOCK_HOSTED = '#[cfg(all(test,target_os="linux"))]'
C11_BLOCK_PROMOTION = ""
C11_BLOCK_TEST = _norm('#[tokio::test(flavor = "current_thread")]' + C11_GATE)
# Normalized active declaration. Promotion must stay `;` so an inline `{`
# module cannot replace the file. A same-line attribute fails the line match.
C11_LINE_CLUSTER = "pubmodcluster;"
C11_LINE_CONSENSUS = "pubmodconsensus;"
C11_LINE_BOUND = "modbound_start;"
C11_LINE_ENROLL = "modenrollment_ingress;"
C11_LINE_HOSTED = "modhosted{"
C11_LINE_PROMOTION = "modpromotion_campaign;"
C11_DECLS = (
    (C11_LIB, "cluster", C11_BLOCK_CLUSTER, C11_LINE_CLUSTER),
    (C11_CLUSTER, "consensus", C11_BLOCK_CONSENSUS, C11_LINE_CONSENSUS),
    (C11_CONSENSUS, "bound_start", C11_BLOCK_BOUND, C11_LINE_BOUND),
    (C11_BOUND, "enrollment_ingress", C11_BLOCK_ENROLL, C11_LINE_ENROLL),
    (C11_PARENT, "hosted", C11_BLOCK_HOSTED, C11_LINE_HOSTED),
    (C11_PARENT, "promotion_campaign", C11_BLOCK_PROMOTION, C11_LINE_PROMOTION),
)
_C11_MOD_LINE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod %s\s*(?:;|\{)\s*$"
)


def _mask_comments(source):
    """Blank comments and string bodies. Indexes and newlines stay put.

    A `;` or `mod` line inside `//` or `/* */` must not end an attribute
    block or count as a declaration. The compared block is still the raw text.
    """
    out = []
    i = 0
    n = len(source)
    while i < n:
        c = source[i]
        nxt = source[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            while i < n and source[i] != "\n":
                out.append(" ")
                i += 1
        elif c == "/" and nxt == "*":
            out.append(" ")
            out.append(" ")
            i += 2
            while i < n and not (
                source[i] == "*" and i + 1 < n and source[i + 1] == "/"
            ):
                out.append("\n" if source[i] == "\n" else " ")
                i += 1
            if i < n:
                out.append(" ")
                out.append(" ")
                i += 2
        elif c == '"':
            out.append('"')
            i += 1
            while i < n:
                if source[i] == "\\":
                    out.append(" ")
                    i += 1
                    if i < n:
                        out.append("\n" if source[i] == "\n" else " ")
                        i += 1
                    continue
                if source[i] == '"':
                    out.append('"')
                    i += 1
                    break
                out.append("\n" if source[i] == "\n" else " ")
                i += 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


def _attribute_cut(masked):
    """Start of the trailing attribute, doc-comment, comment, and blank block."""
    i = len(masked)
    while i > 0:
        j = i - 1
        while j >= 0 and masked[j].isspace():
            j -= 1
        if j < 0:
            return 0
        if masked[j] != "]":
            return j + 1
        depth = 1
        k = j - 1
        while k >= 0 and depth:
            if masked[k] == "]":
                depth += 1
            elif masked[k] == "[":
                depth -= 1
            k -= 1
        if depth:
            return j + 1
        while k >= 0 and masked[k].isspace():
            k -= 1
        if k >= 0 and masked[k] == "!":
            k -= 1
            while k >= 0 and masked[k].isspace():
                k -= 1
        if k >= 0 and masked[k] == "#":
            i = k
            continue
        return j + 1
    return 0


def _bracket_end(masked, open_at):
    depth = 0
    i = open_at
    n = len(masked)
    while i < n:
        if masked[i] == "[":
            depth += 1
        elif masked[i] == "]":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return -1


def _inner_attributes(source, masked):
    found = []
    i = 0
    n = len(masked)
    while i < n:
        if masked[i] != "#":
            i += 1
            continue
        j = i + 1
        while j < n and masked[j].isspace():
            j += 1
        if j >= n or masked[j] != "!":
            i += 1
            continue
        j += 1
        while j < n and masked[j].isspace():
            j += 1
        if j >= n or masked[j] != "[":
            i += 1
            continue
        end = _bracket_end(masked, j)
        if end < 0:
            break
        found.append(_norm(source[i:end]))
        i = end
    return tuple(found)


def _reject_c11_counts(path, source, cfg, ignore):
    stripped = _strip_block_comments(source)
    raw_cfg = source.count("cfg")
    raw_ignore = source.count("ignore")
    strip_cfg = stripped.count("cfg")
    strip_ignore = stripped.count("ignore")
    if (raw_cfg, raw_ignore, strip_cfg, strip_ignore) != (cfg, ignore, cfg, ignore):
        raise AssertionError(
            f"{path}: cfg/ignore raw {raw_cfg}/{raw_ignore} "
            f"block-stripped {strip_cfg}/{strip_ignore}, "
            f"expected raw {cfg}/{ignore} block-stripped {cfg}/{ignore}"
        )
    raw_path = source.count("#[path")
    strip_path = stripped.count("#[path")
    if (raw_path, strip_path) != (0, 0):
        raise AssertionError(
            f"{path}: #[path raw {raw_path} block-stripped {strip_path}, "
            "expected raw 0 block-stripped 0"
        )


def _reject_c11_decl(path, source, masked, name, block, line):
    pattern = re.compile(_C11_MOD_LINE.pattern % re.escape(name))
    raw_lines = source.split("\n")
    masked_lines = masked.split("\n")
    hits = [i for i, item in enumerate(masked_lines) if pattern.match(item)]
    if len(hits) != 1:
        raise AssertionError(
            f"{path}: mod {name} expected 1 active declaration, found {len(hits)}"
        )
    actual_line = _norm(masked_lines[hits[0]])
    if actual_line != line:
        raise AssertionError(
            f"{path}: mod {name} declaration expected {line!r} actual {actual_line!r}"
        )
    prefix = "\n".join(raw_lines[: hits[0]])
    masked_prefix = "\n".join(masked_lines[: hits[0]])
    actual_block = _norm(prefix[_attribute_cut(masked_prefix) :])
    if actual_block != block:
        raise AssertionError(
            f"{path}: mod {name} attribute block expected {block!r} "
            f"actual {actual_block!r}"
        )


def assert_c11_linux_quarantine(files):
    expected = {path for path, *_rest in C11_FILES}
    if set(files) != expected:
        raise AssertionError(
            f"module chain files expected {sorted(expected)} actual {sorted(files)}"
        )
    masked = {}
    for path, cfg, ignore, inners in C11_FILES:
        source = files[path]
        _reject_c11_counts(path, source, cfg, ignore)
        masked[path] = _mask_comments(source)
        actual_inners = _inner_attributes(source, masked[path])
        if actual_inners != inners:
            raise AssertionError(
                f"{path}: inner attributes expected {inners!r} actual {actual_inners!r}"
            )
    for path, name, block, line in C11_DECLS:
        _reject_c11_decl(path, files[path], masked[path], name, block, line)
    module = files[C11_MODULE]
    gate = _norm('#[cfg_attr(' + C11_PREDICATE + ', ignore = "' + C11_REASON + '")]')
    found = _norm(module).count(gate + "asyncfn" + C11_TEST + "(")
    if found != 1:
        raise AssertionError(
            f"{C11_MODULE}: expected 1 C11 linux ignore gate, found {found}"
        )
    # The test's own attribute block must stay exactly main's test attribute
    # plus the gate, so commenting out or replacing `#[tokio::test]` fails.
    fn_line = re.compile(r"^[ \t]*async fn " + C11_TEST + r"\(", re.M)
    starts = [m.start() for m in fn_line.finditer(masked[C11_MODULE])]
    if len(starts) != 1:
        raise AssertionError(
            f"{C11_MODULE}: async fn {C11_TEST} expected 1 active definition, "
            f"found {len(starts)}"
        )
    cut = _attribute_cut(masked[C11_MODULE][: starts[0]])
    actual_block = _norm(module[cut : starts[0]])
    if actual_block != C11_BLOCK_TEST:
        raise AssertionError(
            f"{C11_MODULE}: {C11_TEST} attribute block expected {C11_BLOCK_TEST!r} "
            f"actual {actual_block!r}"
        )


def _c11_gate(predicate):
    return C11_GATE.replace(C11_PREDICATE, predicate, 1)


def _c11_sources():
    return {path: (ROOT / path).read_text(encoding="utf-8") for path, *_rest in C11_FILES}


def _c11_with(files, path, text):
    changed = dict(files)
    changed[path] = text
    return changed


class C11LinuxQuarantineTests(unittest.TestCase):
    def test_exact_linux_only_c11_retained_joiners_quarantine(self):
        files = _c11_sources()
        assert_c11_linux_quarantine(files)
        module = files[C11_MODULE]
        parent = files[C11_PARENT]
        bound = files[C11_BOUND]
        other = "async fn close_learner("
        target = "#[tokio::test(flavor = \"current_thread\")]\n" + C11_GATE
        mutants = {
            "gate removed": (C11_MODULE, module.replace(C11_GATE + "\n", "", 1)),
            "predicate unix": (C11_MODULE, module.replace(C11_GATE, _c11_gate("unix"), 1)),
            "predicate all()": (C11_MODULE, module.replace(C11_GATE, _c11_gate("all()"), 1)),
            "predicate not(windows)": (
                C11_MODULE, module.replace(C11_GATE, _c11_gate("not(windows)"), 1)
            ),
            "wrong reason": (C11_MODULE, module.replace(C11_REASON, "unapproved reason", 1)),
            "renamed function": (
                C11_MODULE,
                module.replace("async fn " + C11_TEST + "(", "async fn renamed(", 1),
            ),
            "second ignore": (C11_MODULE, module.replace(other, "#[ignore]\n" + other, 1)),
            "cfg on target function": (
                C11_MODULE, module.replace(target, '#[cfg(not(target_os="linux"))]\n' + target, 1)
            ),
            "cfg on neighbor": (
                C11_MODULE, module.replace(other, '#[cfg(not(target_os="linux"))]\n' + other, 1)
            ),
            "inner cfg on test module": (C11_MODULE, "#![cfg(not(windows))]\n" + module),
            "comment cfg on target": (
                C11_MODULE,
                module.replace(
                    target,
                    "#[/* Fixture limitation */ cfg(not(target_os = \"windows\"))] "
                    "// Platform restriction\n" + target,
                    1,
                ),
            ),
            "comment cfg on neighbor": (
                C11_MODULE,
                module.replace(
                    other,
                    "#[/* Fixture limitation */ cfg(not(target_os = \"windows\"))]\n" + other,
                    1,
                ),
            ),
            "raw cfg identifier": (
                C11_MODULE,
                module.replace(
                    target, "#[r#cfg(not(target_os = \"windows\"))] // raw\n" + target, 1
                ),
            ),
            "macro generated cfg": (
                C11_MODULE,
                "macro_rules! hide { ($item:item) => { "
                "#[cfg(not(target_os = \"windows\"))] $item }; }\n" + module,
            ),
            "comment split ignore": (
                C11_MODULE, module.replace(other, "#[ig/**/nore]\n" + other, 1)
            ),
            "path on promotion module": (
                C11_PARENT,
                parent.replace(
                    C11_PROMO_DECL, '    #[path = "other.rs"]\n' + C11_PROMO_DECL, 1
                ),
            ),
            "comment split path": (
                C11_PARENT,
                parent.replace(
                    C11_PROMO_DECL, '    #[pa/**/th = "other.rs"]\n' + C11_PROMO_DECL, 1
                ),
            ),
            "hosted predicate unix": (
                C11_PARENT,
                parent.replace(C11_HOSTED_DECL, '#[cfg(all(test, unix))]\nmod hosted {', 1),
            ),
            "cfg on promotion declaration": (
                C11_PARENT,
                parent.replace(C11_PROMO_DECL, "    #[cfg(unix)]\n" + C11_PROMO_DECL, 1),
            ),
            "comment cfg on hosted declaration": (
                C11_PARENT,
                parent.replace(
                    C11_HOSTED_DECL,
                    "#[/* cfg */ cfg(all(test, target_os = \"linux\"))]\nmod hosted {",
                    1,
                ),
            ),
            # Reviewer reproducer: a semicolon in the trailing comment used to
            # end the declaration region before this cfg.
            "comment semicolon cfg before promotion module": (
                C11_PARENT,
                parent.replace(
                    C11_PROMO_DECL,
                    "    #[cfg(any())] // Fixture is disabled;\n" + C11_PROMO_DECL,
                    1,
                ),
            ),
            "comment semicolon cfg before hosted module": (
                C11_PARENT,
                parent.replace(
                    C11_HOSTED_DECL,
                    "#[cfg(any())] // Fixture is disabled;\n" + C11_HOSTED_DECL,
                    1,
                ),
            ),
            "cfg on enrollment parent file": (
                C11_BOUND,
                bound.replace(
                    C11_ENROLL_DECL,
                    "#[cfg(any())] // Fixture is disabled;\n" + C11_ENROLL_DECL,
                    1,
                ),
            ),
            # Review 2. Dropping the redundant durable_join cfg keeps the count
            # at 14 while `#[cfg(any())]` removes promotion_campaign on Linux.
            "count-preserving cfg swap before promotion module": (
                C11_PARENT,
                parent.replace(
                    "    #[cfg(test)]\n    mod durable_join_campaign;",
                    "    mod durable_join_campaign;",
                    1,
                ).replace(
                    C11_PROMO_DECL,
                    "    #[cfg(any())] // Fixture is disabled;\n" + C11_PROMO_DECL,
                    1,
                ),
            ),
            "commented-out promotion module": (
                C11_PARENT,
                parent.replace(C11_PROMO_DECL, "//" + C11_PROMO_DECL, 1),
            ),
            # Review 3: without its test attribute the case leaves discovery.
            "commented-out test attribute": (
                C11_MODULE, module.replace(target, "// " + target, 1)
            ),
            "removed test attribute": (
                C11_MODULE,
                module.replace(target, C11_GATE, 1),
            ),
            "block-commented test attribute": (
                C11_MODULE,
                module.replace(
                    target,
                    "/* #[tokio::test(flavor = \"current_thread\")] */\n" + C11_GATE,
                    1,
                ),
            ),
            "different test attribute": (
                C11_MODULE,
                module.replace(
                    target, "#[tokio::test(flavor = \"multi_thread\")]\n" + C11_GATE, 1
                ),
            ),
            "same-line cfg on promotion module": (
                C11_PARENT,
                parent.replace(
                    "    #[cfg(test)]\n    mod durable_join_campaign;",
                    "    mod durable_join_campaign;",
                    1,
                ).replace(
                    C11_PROMO_DECL,
                    "    #[cfg(any())] mod promotion_campaign;",
                    1,
                ),
            ),
            "count-preserving inner cfg on enrollment file": (
                C11_PARENT,
                "#![cfg(any())]\n"
                + parent.replace(
                    "    #[cfg(test)]\n    mod durable_join_campaign;",
                    "    mod durable_join_campaign;",
                    1,
                ),
            ),
            "doc comment hides promotion module": (
                C11_PARENT,
                parent.replace(C11_PROMO_DECL, "///" + C11_PROMO_DECL, 1),
            ),
            "block comment hides promotion module": (
                C11_PARENT,
                parent.replace(
                    C11_PROMO_DECL, "/*\n" + C11_PROMO_DECL + "\n*/", 1
                ),
            ),
        }
        for label, (path, text) in mutants.items():
            with self.subTest(mutant=label):
                self.assertNotEqual(text, files[path])
                with self.assertRaises(AssertionError):
                    assert_c11_linux_quarantine(_c11_with(files, path, text))


# Hosted Windows PowerShell `Ok(TimedOut)` quarantine (ROADMAP known issue).
# Every function on the outcome path of the four affected tests is pinned to
# its exact text, so an early return, a widened condition or a new caller
# fails here. The `0x80131623` exit-code route stays a separate predicate.
POWERSHELL_TIMEOUT_TESTS = (
    "wrapper_evaluates_utf8_multiline_with_implicit_success",
    "wrapper_reports_implicit_error",
    "production_identity_runs_powershell_and_removes_its_request_directory",
    "self_test_admits_the_production_identity",
)
POWERSHELL_CRASH_CONST = "const HOSTED_POWERSHELL_CRASH: u32 = 0x8013_1623;"
# Exact source bodies (whitespace-insensitive), keyed by (indent, fn name).
POWERSHELL_EXACT = {
    (4, "is_hosted_win25"): r'''
        ci.is_some_and(|value| value.eq_ignore_ascii_case("true"))
            && runner == Some("github-hosted")
            && image.is_some_and(|value| value.starts_with("win25"))
''',
    (4, "is_known_hosted_crash"): r'''
        is_hosted_win25(ci, runner, image)
            && outcome
                == Ok(ScriptOutcome::Exited {
                    exit_code: HOSTED_POWERSHELL_CRASH,
                })
''',
    (8, "from_env"): r'''
            Self {
                ci: std::env::var("CI").ok(),
                environment: std::env::var("RUNNER_ENVIRONMENT").ok(),
                image: std::env::var("ImageOS").ok(),
            }
''',
    (4, "quarantine_hosted_crash"): r'''
        if outcome == Ok(expected) {
            return false;
        }
        let ci = runner.ci.as_deref();
        let environment = runner.environment.as_deref();
        let image = runner.image.as_deref();
        let os = std::env::var("RUNNER_OS").ok();
        eprintln!(
            "restricted PowerShell runner: CI={ci:?} RUNNER_ENVIRONMENT={environment:?} \
             ImageOS={image:?} RUNNER_OS={os:?}"
        );
        let known = is_known_hosted_crash(outcome, ci, environment, image);
        if known {
            hosted_restricted_powershell_ladder(profile);
        }
        known
''',
    (4, "assert_powershell"): r'''
        if outcome != Ok(expected) {
            panic!(
                "PowerShell returned {outcome:?}, expected {expected:?}\ndiagnostics:\n{}",
                diagnose()
            );
        }
''',
    (4, "is_known_hosted_timeout"): r'''
        is_hosted_win25(
            runner.ci.as_deref(),
            runner.environment.as_deref(),
            runner.image.as_deref(),
        ) && outcome == Ok(ScriptOutcome::TimedOut)
''',
    (4, "settle_wrapper_outcome"): r'''
        if is_known_hosted_timeout(outcome, runner) {
            eprintln!("KNOWN-ISSUE hosted-windows-powershell-wrapper: Ok(TimedOut)");
            return true;
        }
        assert_powershell(outcome, expected, diagnose);
        false
''',
    (4, "settle_restricted_outcome"): r'''
        if quarantine_hosted_crash(outcome, expected, runner, profile) {
            return true;
        }
        if is_known_hosted_timeout(outcome, runner) {
            eprintln!("KNOWN-ISSUE hosted-windows-powershell-restricted-identity: Ok(TimedOut)");
            return true;
        }
        assert_powershell(outcome, expected, diagnose);
        false
''',
    (4, "assert_powershell_wrapper"): r'''
        let root = TestRoot::new("wrapper");
        let (windows, system) = system_paths().unwrap();
        let command = CommandLine::powershell(&windows, &system, &root.0).unwrap();
        let stop = Arc::new(event().unwrap());
        let worker_stop = Arc::clone(&stop);
        let worker_script = script.to_vec();
        let deadline = Instant::now() + POWERSHELL_GUARD;
        let outcome = supervise(&stop, deadline, CLEANUP_BUDGET, move || {
            prepare(
                &command,
                Identity::TestCurrent,
                &current_user_sid(),
                worker_script,
                &worker_stop,
                deadline,
                |_| Ok(()),
            )
        });
        settle_wrapper_outcome(
            outcome,
            ScriptOutcome::Exited {
                exit_code: expected,
            },
            &HostedRunner::from_env(),
            || {
                let rerun = TestRoot::new("wrapper-diagnostic");
                capture_powershell(
                    Identity::TestCurrent,
                    &current_user_sid(),
                    &rerun.0,
                    script,
                    None,
                )
            },
        );
''',
}
# Test bodies are pinned with comments and string contents masked, so fixture
# scripts and messages may change but no statement can be added or altered.
POWERSHELL_TEST_CODE = {
    "wrapper_evaluates_utf8_multiline_with_implicit_success":
        'assert_powershell_wrapper("".as_bytes(), 0);',
    "wrapper_reports_implicit_error": 'assert_powershell_wrapper(b"", 1);',
    "production_identity_runs_powershell_and_removes_its_request_directory": r'''
        let root = TestRoot::new("");
        let profile = Arc::new(test_profile(&root.0));
        let stop = Arc::new(event().unwrap());
        let script = "";
        let outcome = run_profile(
            &profile,
            &stop,
            script.as_bytes().to_vec(),
            Instant::now() + POWERSHELL_GUARD,
        );
        let expected = ScriptOutcome::Exited { exit_code: 7 };
        settle_restricted_outcome(
            outcome,
            expected,
            &HostedRunner::from_env(),
            &profile,
            || restricted_failure_report(&profile, script.as_bytes()),
        );
        assert_eq!(root.entries(), 0, "");
''',
    "self_test_admits_the_production_identity": r'''
        let root = TestRoot::new("");
        let stop = Arc::new(event().unwrap());
        let profile = Arc::new(test_profile(&root.0));
        let outcome = run_profile(
            &profile,
            &stop,
            SELF_TEST.as_bytes().to_vec(),
            Instant::now() + POWERSHELL_GUARD,
        );
        let sentinel = ScriptOutcome::Exited {
            exit_code: SELF_TEST_SENTINEL,
        };
        let tolerated = settle_restricted_outcome(
            outcome,
            sentinel,
            &HostedRunner::from_env(),
            &profile,
            || restricted_failure_report(&profile, SELF_TEST.as_bytes()),
        );
        if !tolerated {
            let admitted = admit(test_profile(&root.0), &clean_policy(), |_| outcome);
            assert!(matches!(admitted, Ok(Some(_))));
        }
        assert_eq!(
            root.entries(),
            0,
            ""
        );
''',
}
# Every function that may name each pinned item. The first entry is the
# definition itself; the rest are its only callers.
POWERSHELL_CALLERS = {
    "is_known_hosted_crash": (
        "is_known_hosted_crash",
        "quarantine_hosted_crash",
        "hosted_exception_requires_a_win25_image_and_the_exact_crash_exit",
        "hosted_timeout_and_crash_predicates_stay_distinct",
    ),
    "quarantine_hosted_crash": ("quarantine_hosted_crash", "settle_restricted_outcome"),
    "is_known_hosted_timeout": (
        "is_known_hosted_timeout",
        "settle_wrapper_outcome",
        "settle_restricted_outcome",
        "hosted_timeout_and_crash_predicates_stay_distinct",
    ),
    "settle_wrapper_outcome": (
        "settle_wrapper_outcome",
        "assert_powershell_wrapper",
        "wrapper_family_tolerates_only_the_hosted_timeout",
    ),
    "settle_restricted_outcome": (
        "settle_restricted_outcome",
        POWERSHELL_TIMEOUT_TESTS[2],
        POWERSHELL_TIMEOUT_TESTS[3],
        "restricted_identity_family_tolerates_only_the_hosted_timeout",
    ),
    "assert_powershell_wrapper": (
        "assert_powershell_wrapper",
        POWERSHELL_TIMEOUT_TESTS[0],
        POWERSHELL_TIMEOUT_TESTS[1],
    ),
    "from_env": ("from_env", "assert_powershell_wrapper") + POWERSHELL_TIMEOUT_TESTS[2:],
}
_POWERSHELL_FN = re.compile(r"^(?: {4}| {8})fn (\w+)\b", re.M)


def _powershell_function(source, masked, indent, name):
    pad = " " * indent
    matches = list(re.finditer(
        rf"^{pad}fn {name}\b[^{{]*\{{(.*?)^{pad}\}}", masked, re.M | re.S
    ))
    assert len(matches) == 1, name
    match = matches[0]
    return match.start(), source[match.start(1):match.end(1)], match.group(1)


def assert_powershell_timeout_quarantine(source):
    masked = _mask_comments(source)
    assert source.count(POWERSHELL_CRASH_CONST) == 1
    for (indent, name), expected in POWERSHELL_EXACT.items():
        _, body, _ = _powershell_function(source, masked, indent, name)
        assert _norm(body) == _norm(expected), name
    for name, expected in POWERSHELL_TEST_CODE.items():
        start, _, code = _powershell_function(source, masked, 4, name)
        assert _norm(code) == _norm(_mask_comments(expected)), name
        assert _norm(masked[_attribute_cut(masked[:start]):start]) == "#[test]", name
    for token, allowed in POWERSHELL_CALLERS.items():
        for match in re.finditer(rf"\b{token}\b", masked):
            owners = [m.group(1) for m in _POWERSHELL_FN.finditer(masked, 0, match.end())]
            assert owners and owners[-1] in allowed, (token, owners[-1:])


class PowerShellTimeoutQuarantineTests(unittest.TestCase):
    SOURCE = ROOT / "crates/arx-guest/src/platform/windows.rs"

    def test_exact_hosted_powershell_timeout_quarantine(self):
        assert_powershell_timeout_quarantine(self.SOURCE.read_text(encoding="utf-8"))

    def test_mutations_widening_the_quarantine_fail(self):
        source = self.SOURCE.read_text(encoding="utf-8")
        early = "        if outcome.is_err() {\n            return;\n        }\n"
        mutations = {
            "tolerate any error": (
                ") && outcome == Ok(ScriptOutcome::TimedOut)",
                ") && (outcome.is_err() || outcome == Ok(ScriptOutcome::TimedOut))",
            ),
            "timeout off CI": (
                ") && outcome == Ok(ScriptOutcome::TimedOut)",
                ") || outcome == Ok(ScriptOutcome::TimedOut)",
            ),
            "absent CI counts": (
                'ci.is_some_and(|value| value.eq_ignore_ascii_case("true"))',
                'ci.is_none_or(|value| value.eq_ignore_ascii_case("true"))',
            ),
            "self-hosted runner": ('Some("github-hosted")', 'Some("self-hosted")'),
            "any Windows image": ('value.starts_with("win25")', 'value.starts_with("win")'),
            "forced CI": ('ci: std::env::var("CI").ok(),', 'ci: Some("true".to_owned()),'),
            "harness early return": (
                "\n        settle_wrapper_outcome(\n",
                "\n" + early + "        settle_wrapper_outcome(\n",
            ),
            "wrapper settle early return": (
                "        assert_powershell(outcome, expected, diagnose);\n        false\n",
                "        if outcome.is_err() {\n            return true;\n        }\n"
                "        assert_powershell(outcome, expected, diagnose);\n        false\n",
            ),
            "restricted settle early return": (
                "restricted-identity: Ok(TimedOut)\");\n            return true;\n        }\n",
                "restricted-identity: Ok(TimedOut)\");\n            return true;\n        }\n"
                "        if outcome.is_err() {\n            return true;\n        }\n",
            ),
            "identity test early return": (
                "        let expected = ScriptOutcome::Exited { exit_code: 7 };\n",
                "        let expected = ScriptOutcome::Exited { exit_code: 7 };\n" + early,
            ),
            "self-test early return": (
                "        let tolerated = settle_restricted_outcome(\n",
                early + "        let tolerated = settle_restricted_outcome(\n",
            ),
            "self-test skips admission": ("        if !tolerated {", "        if !tolerated && outcome.is_ok() {"),
            "strict assertion widened": (
                "        if outcome != Ok(expected) {\n            panic!(",
                "        if outcome != Ok(expected) && outcome.is_ok() {\n            panic!(",
            ),
            "crash route widened": (
                "let known = is_known_hosted_crash(outcome, ci, environment, image);",
                "let known = outcome.is_err() || is_known_hosted_crash(outcome, ci, environment, image);",
            ),
            "crash code changed": (POWERSHELL_CRASH_CONST, "const HOSTED_POWERSHELL_CRASH: u32 = 0x8013_1624;"),
            "new timeout caller": (
                "    fn admission_requires_the_exact_sentinel_and_no_policy_conflict() {\n",
                "    fn admission_requires_the_exact_sentinel_and_no_policy_conflict() {\n"
                "        let _ = is_known_hosted_timeout;\n",
            ),
            "new restricted settle caller": (
                "    fn admission_requires_the_exact_sentinel_and_no_policy_conflict() {\n",
                "    fn admission_requires_the_exact_sentinel_and_no_policy_conflict() {\n"
                "        let _ = settle_restricted_outcome::<fn() -> String>;\n",
            ),
        }
        for name in POWERSHELL_TIMEOUT_TESTS:
            mutations[f"ignored {name}"] = (f"    fn {name}()", f"    #[ignore]\n    fn {name}()")
        for label, (original, replacement) in mutations.items():
            with self.subTest(mutation=label):
                mutant = source.replace(original, replacement, 1)
                self.assertNotEqual(mutant, source)
                with self.assertRaises(AssertionError):
                    assert_powershell_timeout_quarantine(mutant)


if __name__ == "__main__":
    unittest.main()
