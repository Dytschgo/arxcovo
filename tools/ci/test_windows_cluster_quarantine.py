"""Pin the narrow platform quarantine; keep test bodies compiled everywhere."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = ['t3_snapshot_catchup_then_consecutive_append', 't1_follower_offline_enrollment_and_reopen', 't2_leader_offline_reads_and_unknown_retry', 'u6_cancelled_callers_keep_engine_permits_until_resolution', 't4_registry_revocation_snapshot_and_prune', 'r6_shutdown_report_cancelled_proposal', 'r6_shutdown_report_store_only', 'r6_shutdown_report_normal_and_reaped_panic']
REASON = 'Windows three-node quarantine: docs/reviews/m3-cluster-third-attempt-owner-disposition.md'

class QuarantineTests(unittest.TestCase):
    def test_exact_windows_only_quarantine(self):
        source = (ROOT / "crates/arxd/src/cluster/consensus/three_node.rs").read_text(encoding="utf-8")
        gate = '#[cfg_attr(target_os = "windows", ignore = "' + REASON + '")]'
        functions = re.findall(r'^async fn ([a-z0-9_]+)\(', source, re.M)
        self.assertEqual(set(functions) & set(EXPECTED), set(EXPECTED))
        actual = re.findall(re.escape(gate) + r'\nasync fn ([a-z0-9_]+)\(', source)
        self.assertEqual(sorted(actual), sorted(EXPECTED))
        self.assertEqual(source.count('ignore'), len(EXPECTED))
        self.assertNotIn('#[cfg(target_os', source)
        self.assertNotIn('#[cfg(not(target_os', source)

if __name__ == "__main__":
    unittest.main()
