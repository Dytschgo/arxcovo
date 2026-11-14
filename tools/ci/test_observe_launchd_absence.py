"""Pure scripted transport tests: no subprocesses, pipes, clocks or host reads."""
import contextlib
import io
import json
import subprocess
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch

import observe_launchd_absence as subject


class Harness:
    def __init__(self, out=(), err=(), code=113):
        self.now = 0
        self.events = {10: list(out), 11: list(err)}
        self.reads = []
        self.child = Mock(stdout=Mock(), stderr=Mock())
        self.child.stdout.fileno.return_value = 10
        self.child.stderr.fileno.return_value = 11
        self.child.poll.return_value = code

    def clock(self):
        self.now += 0.001
        return self.now

    def sleep(self, seconds):
        self.now += seconds

    def read(self, fd, size):
        self.reads.append(fd)
        event = self.events[fd].pop(0) if self.events[fd] else b""
        if isinstance(event, Exception):
            raise event
        if len(event) > size:
            self.events[fd].insert(0, event[size:])
        return event[:size]

    def run(self, setup_error=None):
        with patch.object(subject.subprocess, "Popen", return_value=self.child) as spawn, \
             patch.object(subject.os, "set_blocking", side_effect=setup_error), \
             patch.object(subject.os, "read", side_effect=self.read), \
             patch.object(subject.time, "monotonic", side_effect=self.clock), \
             patch.object(subject.time, "sleep", side_effect=self.sleep):
            self.spawn = spawn
            return subject.capture()


class CaptureTests(unittest.TestCase):
    def test_fixed_vector_environment_and_stream_identity(self):
        h = Harness([b"out"], [b"err"], 113)
        r = h.run()
        self.assertTrue(r.stdout == b"out" and r.stderr == b"err", "stream-identity")
        self.assertEqual(r.code, 113, "real-exit")
        self.assertEqual(h.spawn.call_args.args, (["/bin/launchctl", "print", "system/com.arxcovo.arxd"],))
        self.assertEqual(h.spawn.call_args.kwargs,
                         dict(stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, shell=False, cwd="/", bufsize=0,
                              env={"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C"}), "fixed-invocation")
        h.child.kill.assert_not_called()
        self.assertTrue(all(r.eof), "both-eof")

    def test_exact_caps_plus_eof_and_first_excess_fail_closed(self):
        for index, cap in enumerate((65536, 4096)):
            for extra in (0, 1):
                streams = [[b"x" * 65536], [b"y" * 4096]]
                streams[index] = [b"z" * (cap + extra)]
                h = Harness(*streams)
                r = h.run()
                self.assertEqual(r.status, "overflow" if extra else "completed", "independent-bound")
                self.assertEqual(r.truncated[index], bool(extra), "first-excess-truncated")
                self.assertTrue(len(r.stdout) <= 65536 and len(r.stderr) <= 4096, "retained-bound")
                self.assertTrue(len(r.stdout) + len(r.stderr) <= 69632, "aggregate-bound")
                self.assertEqual(h.reads[:2], [10, 11], "fair-first-pass")

    def test_exit_before_eof_inherited_pipe_times_out_and_cleanup_bounded(self):
        h = Harness([BlockingIOError()] * 1000, [], 113)
        h.child.wait.side_effect = subprocess.TimeoutExpired("fixed", 1)
        r = h.run()
        self.assertEqual(r.status, "timeout", "exit-is-not-eof")
        self.assertFalse(r.eof[0], "incomplete-stream")
        self.assertEqual(r.cleanup, "reap-timeout")
        h.child.kill.assert_called_once()
        self.assertTrue(0 <= h.child.wait.call_args.kwargs["timeout"] <= 1, "cleanup-one-second")
        self.assertTrue(h.now < 2.1, "query-two-seconds")

    def test_eof_before_exit_waits_for_real_exit(self):
        h = Harness(code=None)
        h.child.poll.side_effect = [None, None, 7]
        r = h.run()
        self.assertEqual(r.status, "completed")
        self.assertEqual(r.code, 7, "real-exit-after-eof")
        self.assertEqual(h.child.poll.call_count, 3)

    def test_flood_cannot_starve_stderr_or_deadline(self):
        h = Harness([b"x"] * 10000, [b"y"] * 10000, None)
        r = h.run()
        self.assertEqual(r.status, "timeout", "flood-deadline")
        self.assertTrue(h.now < 2.1, "absolute-query-bound")
        self.assertTrue(h.reads == [10, 11] * (len(h.reads) // 2), "fair-every-pass")
        self.assertFalse(all(r.eof), "flood-not-complete")

    def test_signal_never_invented_exit_code(self):
        r = Harness(code=-9).run()
        self.assertEqual(r.status, "signalled")
        self.assertIsNone(r.code, "no-128-plus-signal")

    def test_transport_failures_and_closure(self):
        for fault in ("spawn", "setup", "read", "wait", "kill", "close"):
            h = Harness([OSError("PRIVATE")], code=None)
            if fault == "wait":
                h.events[10] = []
                h.child.poll.side_effect = OSError("PRIVATE")
            if fault == "kill":
                h.child.kill.side_effect = OSError("PRIVATE")
            if fault == "close":
                h.child.stdout.close.side_effect = OSError("PRIVATE")
            if fault == "spawn":
                with patch.object(subject.subprocess, "Popen", side_effect=OSError("PRIVATE")), \
                     patch.object(subject.time, "monotonic", return_value=0):
                    r = subject.capture()
            else:
                r = h.run(OSError("PRIVATE") if fault == "setup" else None)
                h.child.stderr.close.assert_called_once()
            self.assertNotEqual(r.status, "completed", "failure-precedence")
            self.assertFalse("PRIVATE" in json.dumps(subject.summarize(r)), "error-privacy")


class PublicationTests(unittest.TestCase):
    def candidate(self):
        return subject.Capture("completed", *subject.CANDIDATE, [True, True], [False, False])

    def test_whole_tuple_only_never_113_alone_or_stream_swaps(self):
        r = self.candidate()
        self.assertEqual(subject.summarize(r)["tuple"], "synthetic-candidate-1", "exact-tuple")
        for code, out, err in ((113, b"", b""), (1, r.stdout, r.stderr),
                               (113, r.stderr, r.stdout), (113, b"private", r.stderr),
                               (113, r.stdout, r.stderr + b"\n"), (0, b"state = running", b"")):
            row = subject.summarize(subject.Capture("completed", code, out, err, [True, True], [False, False]))
            self.assertEqual(row["tuple"], "unsupported", "whole-tuple-stream-code")
            self.assertEqual(row["query"], "Unknown", "no-production-acceptance")
            self.assertFalse(row["fixture_credit"], "no-fixture-credit")

    def test_incomplete_truncated_and_over_bound_never_candidate(self):
        for attribute, value in (("eof", [False, True]), ("eof", [True, False]),
                                 ("truncated", [True, False]), ("truncated", [False, True]),
                                 ("status", "timeout"), ("stdout", b"x" * 65537),
                                 ("stderr", b"x" * 4097)):
            r = self.candidate()
            setattr(r, attribute, value)
            self.assertEqual(subject.summarize(r)["tuple"], "unsupported", "complete-bounded-tuple")

    def test_private_unknown_loaded_never_serialized_or_hashed(self):
        for raw in (b"SECRET home=/Users/PRIVATE uid=501", b"\x00\xff", b"state = running"):
            row = subject.summarize(subject.Capture("completed", 113, raw, raw, [True, True], [False, False]))
            expected = {"status": "completed", "exit_code": 113, "stdout_eof": True,
                        "stderr_eof": True, "stdout_truncated": False, "stderr_truncated": False,
                        "cleanup": "not-needed", "query": "Unknown", "fixture_credit": False,
                        "tuple": "unsupported", "diagnostics": "withheld"}
            self.assertTrue(row == expected, "closed-publication-schema")
        self.assertFalse("SECRET" in repr(subject.Capture(stdout=b"SECRET")), "no-debug-bytes")

    def test_arguments_platform_and_bad_provenance_never_capture(self):
        for argv, system, failure in ((["fixed", "SECRET"], "darwin", False),
                                      (["fixed"], "linux", False), (["fixed"], "darwin", True)):
            with patch.object(subject.sys, "argv", argv), patch.object(subject.sys, "platform", system), \
                 patch.object(subject, "provenance", side_effect=ValueError("SECRET") if failure else None), \
                 patch.object(subject, "capture") as capture, contextlib.redirect_stderr(io.StringIO()) as err:
                self.assertEqual(subject.main(), 2)
                capture.assert_not_called()
                self.assertFalse("SECRET" in err.getvalue(), "entry-privacy")

    def test_provenance_fixed_path_bounded_no_follow_and_closed_metadata(self):
        metadata = {"ProductVersion": "26.1", "ProductBuildVersion": "25B78", "PRIVATE": "SECRET"}
        env = {"GITHUB_SHA": "a" * 40, "ImageOS": "macos26-arm64", "ImageVersion": "20261102.0473.1", "HOME": "SECRET"}
        for size in (100, 16385):
            with patch.dict(subject.os.environ, env, clear=True), \
                 patch.object(subject.os, "O_NOFOLLOW", 256, create=True), \
                 patch.object(subject.os, "O_NONBLOCK", 2048, create=True), \
                 patch.object(subject.os, "open", return_value=10) as opened, \
                 patch.object(subject.os, "fstat", return_value=SimpleNamespace(st_mode=0o100644, st_size=size)), \
                 patch.object(subject.os, "fdopen") as wrapped, patch.object(subject.os, "close"), \
                 patch.object(subject.plistlib, "loads", return_value=metadata):
                wrapped.return_value.__enter__.return_value.read.return_value = b"fixed"
                if size > 16384:
                    with self.assertRaises(ValueError):
                        subject.provenance()
                else:
                    row = subject.provenance()
                    self.assertFalse("SECRET" in json.dumps(row), "metadata-allowlist")
                    self.assertEqual(row["source_pin"], "a" * 40)
                    wrapped.return_value.__enter__.return_value.read.assert_called_once_with(16385)
                self.assertEqual(opened.call_args.args[0], "/System/Library/CoreServices/SystemVersion.plist")
                self.assertTrue(opened.call_args.args[1] & 256, "no-follow")


if __name__ == "__main__":
    unittest.main()
