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
ISSUANCE_REQUIRED = ['anchored_single_voter_receipt_replay_and_revocation']
REASON_FIXTURE = 'Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md'
PURE = 'read_failure_attribution_covers_all_errors'
TLS_FIXTURE = ['enrolled_target_binding_revocation_and_stale_acceptance', 'authenticated_sender_mismatch_and_assumed_founder_control', 'store_lock_wait_does_not_block_runtime']
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
    assert_fixture_quarantine(source, ((TLS_FIXTURE, REASON_FIXTURE),),
                              TLS_PURE, required=TLS_REQUIRED)
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
    'owned_query_dropped_callers_and_read_admission',
    'owned_query_timeout_shutdown_retains_submitted_resources',
    'owned_query_bounds_poison_and_no_submission',
    'owned_query_consumed_tls_zero_proof',
]


def assert_owned_query(source, module, parent):
    assert QUERY_REQUIRED == ['owned_anchor_query_single_voter']
    assert len(QUERY_LOCAL_REQUIRED) == 4
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
        assert_issuance_required(text)
    functions = re.findall(r'(?m)^fn ([a-z0-9_]+)\(', source)
    assert len(functions) == len(set(functions))
    tests = re.findall(r'((?:#\[[^\]]*\]\s*)+)(?:async )?fn ([a-z0-9_]+)\(', source)
    actual = []
    for attrs, name in tests:
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
                                '#[cfg(unix)]\n' + binding,
                                binding.replace('#[cfg(test)]', ''),
                                binding.replace('authenticated_identity_tests.rs', 'absent.rs')):
                if replacement == binding:
                    continue
                args = [source, tls, pki]
                args[index] = original.replace(binding, replacement)
                with self.assertRaises(AssertionError):
                    assert_identity_inventory(*args)
        for weakened in ([], IDENTITY_REQUIRED[:-1], IDENTITY_REQUIRED + [IDENTITY_REQUIRED[0]]):
            with patch(__name__ + '.IDENTITY_REQUIRED', weakened):
                with self.assertRaises(AssertionError):
                    assert_identity_inventory(source, tls, pki)


if __name__ == "__main__":
    unittest.main()
