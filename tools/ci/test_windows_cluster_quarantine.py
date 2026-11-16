"""Pin the narrow platform quarantine; keep test bodies compiled everywhere."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = ['t3_snapshot_catchup_then_consecutive_append', 't1_follower_offline_enrollment_and_reopen', 't2_leader_offline_reads_and_unknown_retry', 'u6_cancelled_callers_keep_engine_permits_until_resolution', 't4_registry_revocation_snapshot_and_prune', 'r6_shutdown_report_cancelled_proposal', 'r6_shutdown_report_store_only', 'r6_shutdown_report_normal_and_reaped_panic']
REASON = 'Windows three-node quarantine: docs/reviews/m3-cluster-third-attempt-owner-disposition.md'

def assert_issuance_required(source):
    if re.search(r'#\s*\[\s*(?:ignore\b|cfg(?:_attr)?\s*\([^\]]*(?:\bwindows\b|\bunix\b|target_os|target_family))', source, re.S):
        raise AssertionError("issuance tests must run on every OS")

class QuarantineTests(unittest.TestCase):
    def test_issuance_required_on_every_os(self):
        for relative in ["consensus/issue_tests.rs", "pki/tests.rs"]:
            source = (ROOT / "crates/arxd/src/cluster" / relative).read_text(encoding="utf-8")
            assert_issuance_required(source)
            for mutation in ['#[ignore]', '#[cfg(windows)]', '#[cfg(unix)]', '#[cfg(target_os = "linux")]', '#[cfg_attr(target_family = "unix", ignore)]']:
                with self.assertRaises(AssertionError):
                    assert_issuance_required(mutation + source)

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
