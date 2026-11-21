import contextlib
import io
import os
from pathlib import Path
import sqlite3
import stat
import struct
import subprocess
import tempfile
from types import SimpleNamespace
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

    def test_sudo_failure_writes_one_sanitized_record(self):
        c13 = Mock(HOSTED_ENV={})
        c13.stage_binary.return_value = "a" * 64
        for result, error in (
            (subprocess.CompletedProcess([], 1), "privileged-launch"),
            (FileNotFoundError("private detail"), "FileNotFoundError"),
            (subprocess.TimeoutExpired("private detail", 1), "timeout"),
            (subprocess.CompletedProcess([], run.PRIVILEGED_FAILURE), None),
        ):
            with self.subTest(error=error), \
                    patch.dict(run.os.environ, {"CARGO_TARGET_DIR": str(run.ROOT / "target")}), \
                    patch.object(run, "c13_tools", return_value=c13), \
                    patch.object(run, "qualified_source", return_value=("head", "image", "run")), \
                    patch.object(run.tempfile, "TemporaryDirectory"), \
                    patch.object(Path, "resolve", return_value=run.ROOT / "target"), \
                    patch.object(run.subprocess, "run") as invoke, patch.object(run, "emit") as emit:
                if isinstance(result, Exception):
                    invoke.side_effect = result
                else:
                    invoke.return_value = result
                self.assertFalse(run.launch())
                invoke.assert_called_once()
            records = [call for call in emit.call_args_list if call.args[0] == "first-failure"]
            self.assertEqual(len(records), int(error is not None))
            if error is not None:
                self.assertEqual(records[0].kwargs, {"stage": "sudo", "category": error})

    def test_privileged_validation_failure_writes_first_failure(self):
        with patch.object(run, "privileged", side_effect=OSError("private detail")), \
                patch.object(run, "emit") as emit:
            result = run.main(["--staged", "/tmp/arx-d5-abcdefgh", "a" * 64, "123"])
        self.assertEqual(result, run.PRIVILEGED_FAILURE)
        emit.assert_called_once_with("first-failure", stage="privileged-entry", category="OSError")
        with patch.object(run, "emit") as emit:
            self.assertEqual(run.main(["--staged", "private detail"]), run.PRIVILEGED_FAILURE)
        emit.assert_called_once_with("first-failure", stage="privileged-entry", category="staging-pin")

    def test_protected_fixture_requires_root_owned_nonwritable_ancestors(self):
        path = Path(tempfile.gettempdir()).resolve() / "var/lib/arx-d5-fixture-abcdefgh"
        protected = SimpleNamespace(st_mode=stat.S_IFDIR | 0o755, st_uid=0)
        with patch.object(Path, "lstat", return_value=protected):
            self.assertIs(run.protected_directory(path), protected)
        for metadata in (
            SimpleNamespace(st_mode=stat.S_IFLNK | 0o755, st_uid=0),
            SimpleNamespace(st_mode=stat.S_IFDIR | 0o775, st_uid=0),
            SimpleNamespace(st_mode=stat.S_IFDIR | 0o755, st_uid=1),
        ):
            with self.subTest(metadata=metadata), \
                    patch.object(Path, "lstat", autospec=True,
                                 side_effect=lambda candidate: metadata if candidate == path.parent else protected):
                with self.assertRaisesRegex(run.Failure, "fixture-ancestry"):
                    run.protected_directory(path)

    @unittest.skipUnless(os.name == "posix", "requires POSIX descriptor and journal locking")
    def test_database_read_stays_with_open_descriptor_after_path_swap(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            root.chmod(0o700)
            path = root / "cluster-state.sqlite3"
            replacement = root / "replacement.sqlite3"
            for candidate, value in ((path, 7), (replacement, 42)):
                with contextlib.closing(sqlite3.connect(candidate)) as db:
                    db.execute("CREATE TABLE witness(value)")
                    db.execute("INSERT INTO witness VALUES (?)", (value,))
                    db.commit()
                candidate.chmod(0o600)
            (root / (path.name + "-journal")).touch(mode=0o600)
            original_open = os.open

            def swap_after_open(name, flags, **kwargs):
                fd = original_open(name, flags, **kwargs)
                if name == path.name:
                    try:
                        self.assertTrue(flags & os.O_NOFOLLOW)
                        path.rename(root / "opened.sqlite3")
                        replacement.rename(path)
                    except BaseException:
                        os.close(fd)
                        raise
                return fd

            with patch.object(run, "DAEMON", os.getuid()), patch.object(run.os, "open", side_effect=swap_after_open):
                with run.database(path) as db:
                    self.assertEqual(db.execute("SELECT value FROM witness").fetchone(), (7,))
            journal = root / (path.name + "-journal")
            journal.write_bytes(b"hot")
            with patch.object(run, "DAEMON", os.getuid()), self.assertRaisesRegex(run.Failure, "database-sidecar"):
                with run.database(path):
                    self.fail("read uncommitted database")

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

    def test_reference_uses_fixed_requests_with_tls_hostname_verification(self):
        context = Mock()
        raw, tls = Mock(), Mock()
        raw.__enter__ = Mock(return_value=raw)
        raw.__exit__ = Mock(return_value=False)
        tls.__enter__ = Mock(return_value=tls)
        tls.__exit__ = Mock(return_value=False)
        context.wrap_socket.return_value = tls
        utc = b"Date: Sat, 21 Nov 2026 12:00:00 GMT\r\n"
        with patch.object(run.ssl, "create_default_context", return_value=context), \
                patch.object(run.socket, "create_connection", return_value=raw) as connect:
            tls.makefile.return_value = io.BytesIO(b"HTTP/1.1 204 No Content\r\n" + utc + b"\r\n")
            self.assertEqual(run.reference_request(0), 1_795_262_400_000)
            connect.assert_called_once_with(("www.google.com", 443), timeout=3)
            context.wrap_socket.assert_called_once_with(raw, server_hostname="www.google.com")
            self.assertTrue(tls.sendall.call_args.args[0].startswith(b"HEAD /generate_204 HTTP/1.1\r\n"))
            connect.reset_mock()
            context.wrap_socket.reset_mock()
            body = b"ip=ignored\nloc=ignored\nts=1795262400.125\n"
            tls.makefile.return_value = io.BytesIO(b"HTTP/1.1 200 OK\r\n" + utc +
                f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
            self.assertEqual(run.reference_request(1), 1_795_262_400_125)
            connect.assert_called_once_with(("1.1.1.1", 443), timeout=3)
            context.wrap_socket.assert_called_once_with(raw, server_hostname="1.1.1.1")
            tls.sendall.assert_called_with(
                b"GET /cdn-cgi/trace HTTP/1.1\r\nHost: 1.1.1.1\r\nUser-Agent: Time check\r\n"
                b"Cache-Control: no-cache, no-store\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n")

    def test_trace_http_rejects_ambiguous_status_headers_and_body(self):
        good_status = b"HTTP/1.1 200 OK\r\n"
        date = b"Date: Sat, 21 Nov 2026 12:00:00 GMT\r\n"
        ts_line = b"ts=1795262400.125\n"
        length = f"Content-Length: {len(ts_line)}\r\n".encode()
        for status, headers, body, error in (
            (b"HTTP/1.1 200 OK\n", date + length, ts_line, "utc-http-status"),
            (b"HTTP/1.1 200 Fine\r\n", date + length, ts_line, "utc-http-status"),
            (good_status, date + length + b"Content-Length: 32\r\n", ts_line, "utc-http-framing"),
            (good_status, date + length + length, ts_line, "utc-http-framing"),
            (good_status, date + length + b"Transfer-Encoding: chunked\r\n", ts_line, "utc-http-framing"),
            (good_status, date, ts_line, "utc-http-framing"),
            (good_status, date + date + length, ts_line, "utc-missing-or-ambiguous"),
            (good_status, date + b"Content-Length: 1025\r\n", ts_line, "utc-trace-bound"),
            (good_status, date + length, ts_line[:-1], "utc-trace-bound"),
        ):
            with self.subTest(status=status, headers=headers), self.assertRaisesRegex(run.Failure, error):
                run.reference_response(io.BytesIO(status + headers + b"\r\n" + body), good_status)

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
        trace = run.trace_agreement(b"ts=1795262407\n", timestamp + 5000)
        with patch.object(run, "reference_at", side_effect=[timestamp, trace]), \
                patch.object(run, "emit"):
            with self.assertRaisesRegex(run.Failure, "utc-origin-mismatch"):
                run.utc_witness(founder, "mock-stage", 5)

    def test_local_bracket_checks_trace_even_when_cloudflare_date_fits(self):
        checked = 1_795_262_400_000
        run.utc_bounds(checked, checked, checked + 28_000)
        trace = run.trace_agreement(b"ts=1795262428.001\n", checked + 28_000)
        founder = Mock()
        founder.evidence.return_value.checkpoint_ms = checked
        with patch.object(run, "reference_at", side_effect=[checked, trace]), patch.object(run, "emit"):
            with self.assertRaisesRegex(run.Failure, "utc-outside-premise"):
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
        raw, tls = Mock(), Mock()
        raw.__enter__ = Mock(return_value=raw)
        raw.__exit__ = Mock(return_value=False)
        tls.__enter__ = Mock(return_value=tls)
        tls.__exit__ = Mock(return_value=False)
        context.wrap_socket.return_value = tls
        tls.makefile.return_value = io.BytesIO(b"HTTP/1.1 302 Found\r\n\r\n")
        with patch.object(run.ssl, "create_default_context", return_value=context), \
                patch.object(run.socket, "create_connection", return_value=raw) as connect:
            with self.assertRaisesRegex(run.Failure, "utc-source"):
                run.reference_request(2)
            connect.assert_not_called()
            with self.assertRaisesRegex(run.Failure, "utc-http-status"):
                run.reference_request(1)
        connect.assert_called_once_with(("1.1.1.1", 443), timeout=3)
        tls.sendall.assert_called_once()

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
