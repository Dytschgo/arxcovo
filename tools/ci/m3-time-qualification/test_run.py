import contextlib
import io
from pathlib import Path
import struct
import subprocess
import unittest
from unittest.mock import Mock, patch

import run


class QualificationTests(unittest.TestCase):
    def test_elapsed_edge_requires_full_dwell_and_past_expiry(self):
        self.assertEqual(run.check_elapsed(1, run.DWELL_NS + 1, 900_000, 900_001), run.DWELL_NS)
        with self.assertRaisesRegex(run.Failure, "elapsed-short"):
            run.check_elapsed(1, run.DWELL_NS, 900_000, 900_001)
        with self.assertRaisesRegex(run.Failure, "checked-not-past-expiry"):
            run.check_elapsed(1, run.DWELL_NS + 1, 900_000, 900_000)

    def test_reviewed_source_image_and_attempt(self):
        sha = "a" * 40
        env = dict(GITHUB_EVENT_NAME="workflow_dispatch", GITHUB_REPOSITORY="Dytschgo/arxcovo",
                   GITHUB_ACTOR="Dytschgo", GITHUB_TRIGGERING_ACTOR="Dytschgo",
                   GITHUB_REF="refs/heads/main", GITHUB_RUN_ATTEMPT="1", GITHUB_JOB="qualification",
                   GITHUB_WORKFLOW_REF="Dytschgo/arxcovo/.github/workflows/m3-time-qualification.yml@refs/heads/main",
                   RUNNER_ARCH="X64", ImageOS="ubuntu24", ImageVersion="20261120.11.1",
                   M3_TIME_REVIEWED_IMAGE="20261120.11.1", GITHUB_SHA=sha, GITHUB_WORKFLOW_SHA=sha,
                   M3_TIME_REVIEWED_SHA=sha, GITHUB_RUN_ID="1234")
        self.assertEqual(run.checked_run(env), (sha, "20261120.11.1", "1234"))
        for key in ("GITHUB_RUN_ATTEMPT", "GITHUB_ACTOR", "GITHUB_WORKFLOW_REF", "GITHUB_WORKFLOW_SHA",
                    "M3_TIME_REVIEWED_SHA", "M3_TIME_REVIEWED_IMAGE", "GITHUB_REF"):
            with self.subTest(key=key), self.assertRaises(run.Failure):
                run.checked_run({**env, key: "wrong"})

    def test_timeout_preserves_first_failure_and_reports_cleanup(self):
        with patch.object(run.time, "monotonic", return_value=100):
            self.assertEqual(run.remaining(102, 1), 1)
            with self.assertRaises(TimeoutError):
                run.remaining(100, 1)
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            first = run.remember(None, "elapsed-dwell", subprocess.TimeoutExpired("redacted", 1))
            first = run.remember(first, "owned-close", run.Failure("unconfirmed-owned-close"))
        self.assertEqual(first, ("elapsed-dwell", "timeout"))
        self.assertIn('"event": "first-failure"', stream.getvalue())
        self.assertIn('"event": "additional-failure"', stream.getvalue())
        self.assertNotIn("redacted", stream.getvalue())

    def test_wait_refuses_when_dwell_reserve_is_unavailable(self):
        with patch.object(run.time, "monotonic_ns", return_value=0), \
                patch.object(run.time, "monotonic", return_value=0):
            with self.assertRaisesRegex(run.Failure, "no-dwell-reserve"):
                run.wait_dwell(Mock(), 0, 900)

    def test_dwell_stops_if_the_owned_founder_exits(self):
        founder = Mock()
        founder.poll.return_value = 0
        with patch.object(run.time, "monotonic_ns", return_value=0), \
                patch.object(run.time, "monotonic", return_value=0):
            with self.assertRaisesRegex(run.Failure, "founder-exited-during-dwell"):
                run.wait_dwell(founder, 0, 903)

    def test_reference_rejects_missing_cached_ambiguous_or_invalid_dates(self):
        date = ("Date", "Sat, 21 Nov 2026 12:00:00 GMT")
        self.assertEqual(run.date_evidence([date]), 1_795_262_400_000)
        for headers in ([], [date, date], [date, ("Age", "0")], [date, ("CF-Cache-Status", "HIT")],
                        [("Date", "Sat, 21 Nov 2026 12:00:00 -0000")]):
            with self.subTest(headers=headers), self.assertRaises(run.Failure):
                run.date_evidence(headers)

    def test_each_reference_budget_includes_accuracy_and_resolution(self):
        self.assertEqual(run.utc_bounds(100_000, 100_100, 102_000), (-4000, 100))
        self.assertEqual(run.utc_bounds(100_000, 100_100, 128_000), (-30000, -25900))
        for sample in ((100_000, 100_100, 128_001), (100_000, 107_000, 102_000),
                       (100_100, 100_000, 102_000), (100_000, 100_100, None)):
            with self.subTest(sample=sample), self.assertRaises(run.Failure):
                run.utc_bounds(*sample)

    def test_reference_uses_fixed_tls_requests_and_bounds_trace(self):
        context = Mock()
        connection = Mock()
        response = connection.getresponse.return_value
        response.getheaders.return_value = [("Date", "Sat, 21 Nov 2026 12:00:00 GMT")]
        with patch.object(run.ssl, "create_default_context", return_value=context), \
                patch.object(run.http.client, "HTTPSConnection", return_value=connection) as https:
            response.status = 204
            self.assertEqual(run.reference_request(0), 1_795_262_400_000)
            https.assert_called_once_with("www.google.com", timeout=3, context=context)
            self.assertEqual(connection.request.call_args.args, ("HEAD", "/generate_204"))
            response.read.assert_not_called()
            https.reset_mock()
            response.status, response.length = 200, 0
            response.read.return_value = b"ip=ignored\nloc=ignored\nts=1795262400.125\n"
            self.assertEqual(run.reference_request(1), 1_795_262_400_000)
            https.assert_called_once_with("1.1.1.1", timeout=3, context=context)
            connection.request.assert_called_with(
                "GET", "/cdn-cgi/trace", headers={
                    "User-Agent": "Time check", "Cache-Control": "no-cache, no-store",
                    "Connection": "close"})
            response.read.assert_called_once_with(1024)
            response.length = 1
            with self.assertRaisesRegex(run.Failure, "utc-trace-bound"):
                run.reference_request(1)

    def test_trace_requires_a_single_timestamp_within_two_seconds(self):
        run.trace_agreement(b"ts=1795262402\n", 1_795_262_400_000)
        for body, error in (
            (b"ts=1795262402.001\n", "utc-trace-date-mismatch"),
            (b"ip=ignored\n", "utc-trace-timestamp"),
            (b"ts=1795262400\nts=1795262400\n", "utc-trace-timestamp"),
        ):
            with self.subTest(body=body), self.assertRaisesRegex(run.Failure, error):
                run.trace_agreement(body, 1_795_262_400_000)

    def test_cross_origin_agreement_is_required_at_checkpoints(self):
        timestamp = 1_795_262_400_000
        self.assertEqual(run.origin_agreement(timestamp, timestamp + 5000), 5000)
        founder = Mock()
        founder.evidence.return_value.checkpoint_ms = timestamp
        with patch.object(run, "reference_at", side_effect=[timestamp, timestamp + 5001]), \
                patch.object(run, "emit"):
            with self.assertRaisesRegex(run.Failure, "utc-origin-mismatch"):
                run.utc_witness(founder, "mock-stage", 5)

    def test_preflight_failure_stops_before_telemetry_without_retry(self):
        with patch.object(run, "qualified_source", return_value=("sha", "image", "run")), \
                patch.object(run, "c13_tools"), patch.object(run.time, "monotonic", return_value=0), \
                patch.object(run, "run_helper", return_value=(1, b"")) as helper, \
                patch.object(run, "clock_telemetry") as telemetry, patch.object(run, "emit"):
            with self.assertRaisesRegex(run.Failure, "utc-reference-failed"):
                run.main(["--preflight"])
        helper.assert_called_once()
        self.assertEqual(helper.call_args.args[1], 3)
        telemetry.assert_not_called()

    def test_clock_telemetry_exports_only_status_and_tolerates_missing_tools(self):
        with patch.object(run, "run_helper", side_effect=[
            FileNotFoundError, (0, b"ignored: private detail\nstatus: 64\n"),
        ]), patch.object(run, "emit") as emit:
            run.clock_telemetry(2)
        emit.assert_called_once_with("utc-telemetry", tool="adjtimex", status="64")
        with patch.object(run, "run_helper", side_effect=FileNotFoundError), \
                patch.object(run, "emit") as emit:
            run.clock_telemetry(2)
        emit.assert_called_once_with("utc-telemetry", status="unavailable")

    def test_reference_rejects_redirect_without_retry(self):
        context = Mock()
        connection = Mock()
        connection.getresponse.return_value.status = 302
        with patch.object(run.ssl, "create_default_context", return_value=context), \
                patch.object(run.http.client, "HTTPSConnection", return_value=connection) as https:
            with self.assertRaisesRegex(run.Failure, "utc-source"):
                run.reference_request(2)
            https.assert_not_called()
            with self.assertRaisesRegex(run.Failure, "utc-http-status"):
                run.reference_request(1)
        https.assert_called_once_with("1.1.1.1", timeout=3, context=context)
        connection.request.assert_called_once_with(
            "GET", "/cdn-cgi/trace", headers={
                "User-Agent": "Time check", "Cache-Control": "no-cache, no-store", "Connection": "close"})
        connection.close.assert_called_once()

    def test_witness_stops_after_failed_origin_without_replacement(self):
        founder = Mock()
        founder.evidence.return_value.checkpoint_ms = 1_795_262_400_000
        with patch.object(run, "run_helper", return_value=(1, b"")) as helper, patch.object(run, "emit"):
            with self.assertRaisesRegex(run.Failure, "utc-reference-failed"):
                run.utc_witness(founder, "mock-stage", 0)
        helper.assert_called_once()
        founder.evidence.assert_called_once()

    def test_observer_checks_handle_outcome_ordinal_and_checked_bracket(self):
        paste = run.Paste(b"a" * 32, b"", b"")
        handle = run.digest(run.DOMAIN, paste.query)
        body = bytes((1, 2, 2, 1, 1)) + struct.pack(">QQ", 100, 101) + handle
        entry = run.observation(body)
        self.assertEqual(run.query_evidence(entry, paste, 2, 2, (99, 101)), 100)
        for ordinal, result, bracket in ((1, 2, (99, 101)), (2, 4, (99, 101)), (2, 2, (101, 102))):
            with self.assertRaisesRegex(run.Failure, "query-evidence"):
                run.query_evidence(entry, paste, ordinal, result, bracket)
        with self.assertRaisesRegex(run.Failure, "query-evidence"):
            run.query_evidence(entry, run.Paste(b"b" * 32, b"", b""), 2, 2, (99, 101))

    def test_observer_rejects_missing_failed_or_malformed_samples(self):
        for status, message in ((0, "sample-missing"), (2, "sample-failed"), (3, "invalid-sample-status")):
            with self.subTest(status=status), self.assertRaisesRegex(run.Failure, message):
                run.observation(bytes((1, 0, 0, 0, status)) + bytes(48))
        with self.assertRaisesRegex(run.Failure, "observer-format"):
            run.observation(bytes(52))
        body = bytes((1, 2, 4, 2, 1)) + struct.pack(">QQ", 0, 101) + b"a" * 32
        with self.assertRaisesRegex(run.Failure, "query-evidence"):
            run.query_evidence(run.observation(body), run.Paste(b"a" * 32, b"", b""), 2, 2, (99, 101))

    def test_tls_end_distinguishes_close_notify_from_transport_eof(self):
        client = object.__new__(run.EnrollmentTls)
        client.tls = Mock()
        client.outgoing = Mock(pending=0)
        client.tls.read.side_effect = run.ssl.SSLZeroReturnError
        self.assertEqual(client.read(1), b"")
        client.tls.read.side_effect = run.ssl.SSLEOFError
        with self.assertRaises(run.ssl.SSLEOFError):
            client.read(1)

    def test_query_only_accepts_uniform_refusal_with_clean_end(self):
        tls = Mock()
        paste = run.Paste(b"query", b"", b"")
        tls.read_frame.return_value = b"\x01"
        tls.read.return_value = b""
        self.assertIsNone(run.anchor_exchange(tls, paste, b"", late=True))
        tls.write_frame.assert_called_once_with(1, b"query")
        tls.read.return_value = b"extraneous"
        with self.assertRaisesRegex(run.Failure, "late-uniform-refusal"):
            run.anchor_exchange(tls, paste, b"", late=True)

    def test_mint_envelope_binds_query_route_ca_and_anchor(self):
        endpoint, root = ("192.168.2.7", 1201), b"synthetic root"
        query, key, anchor = b"a" * 32, b"b" * 32, b"c" * 32
        raw = query + key + run.hashlib.sha256(root).digest() + anchor
        code = raw + run.digest(b"arxcovo/join/check/v2", raw)[:4]
        transfer = (b"\x04" + run.socket.inet_aton(endpoint[0])
                    + struct.pack(">HH", endpoint[1], len(root)) + root + b"arxj2-" + code.hex().encode())
        checked = transfer + run.digest(b"arxcovo/join/transfer/v1", transfer)[:4]
        wire = b"arxje1-" + checked.hex().encode()
        self.assertEqual(run.parse_paste(wire, endpoint, root), run.Paste(query, anchor, b""))
        self.assertEqual(run.parse_paste(wire, endpoint, root, keep_secret=True),
                         run.Paste(query, anchor, key))
        with self.assertRaisesRegex(run.Failure, "mint-route"):
            run.parse_paste(wire, ("192.168.2.7", 1202), root)
        with self.assertRaisesRegex(run.Failure, "mint-root"):
            run.parse_paste(wire, endpoint, b"wrong root")

    def test_positive_anchor_requires_the_expected_single_founder(self):
        query, key = b"a" * 32, b"b" * 32
        body = b"\x01" + struct.pack(">Q", 1) + key
        paste = run.Paste(query, run.digest(b"arxcovo/join/anchor/v1", query[:16] + body), b"")
        tls = Mock()
        tls.read_frame.return_value = b"\0" + b"C" * 16 + body
        self.assertEqual(run.anchor_exchange(tls, paste, key, late=False), b"C" * 16)
        with self.assertRaisesRegex(run.Failure, "positive-anchor"):
            run.anchor_exchange(tls, paste, b"d" * 32, late=False)

    def test_workflow_is_manual_and_first_attempt_only(self):
        workflow = (Path(__file__).resolve().parents[3] /
                    ".github/workflows/m3-time-qualification.yml").read_text()
        self.assertIn("\non:\n  workflow_dispatch:\npermissions:\n  contents: read\n", workflow)
        self.assertIn("github.run_attempt == 1", workflow)
        self.assertIn("github.sha == vars.M3_TIME_REVIEWED_SHA", workflow)
        self.assertIn("github.workflow_sha == vars.M3_TIME_REVIEWED_SHA", workflow)
        self.assertIn("timeout-minutes: 30", workflow)
        self.assertLess(workflow.index("run.py --preflight"), workflow.index("cargo build"))


if __name__ == "__main__":
    unittest.main()
