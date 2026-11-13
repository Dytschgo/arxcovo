"""Portable owned-temp fixtures and synthetic Python children; no native/Git/network calls."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("e3_driver", Path(__file__).with_name("driver.py"))
d = importlib.util.module_from_spec(spec)
spec.loader.exec_module(d)


def status(kind, name):
    return f"        {kind} [   0.012s] (1/1) arxd {name}\n"


def baseline():
    return "".join(f"        PASS [   0.028s] ({n:2d}/{len(d.TESTS)}) arxd {name}\n"
                   for n, name in enumerate(d.TESTS, 1))


class HarnessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.target = self.root / d.TARGET
        self.target.parent.mkdir(parents=True)
        self.original = b"fn synthetic() { SAFE; }\n#[cfg(test)]\nmod tests { SAFE; }\n"
        self.target.write_bytes(self.original)
        self.row = {"mutation": "L1_take_raised",
                    "changes": [["fn synthetic() { SAFE; }", "fn synthetic() { MUTANT; }"]],
                    "assertions": [d.TESTS[0]]}

    def test_target_guard_rejects_arbitrary_paths(self):
        for name in ("../linux.rs", "/linux.rs", "Cargo.toml", d.TARGET + "/../linux.rs"):
            with self.subTest(name=name), self.assertRaises(d.GuardError):
                d.regular(self.root, name)

    def test_target_guard_rejects_hardlink(self):
        os.link(self.target, self.root / "alias")
        with self.assertRaises(d.GuardError):
            d.regular(self.root, d.TARGET)

    def test_target_guard_rejects_reparse_attribute(self):
        real = Path.lstat

        def fake(path, *args, **kwargs):
            result = real(path, *args, **kwargs)
            if path == self.target.parent:
                from types import SimpleNamespace
                return SimpleNamespace(st_mode=result.st_mode, st_file_attributes=0x400)
            return result

        with patch.object(Path, "lstat", fake), self.assertRaises(d.GuardError):
            d.regular(self.root, d.TARGET)

    def test_target_guard_rejects_symlink(self):
        self.target.unlink()
        try:
            self.target.symlink_to(self.root / "absent")
        except OSError:
            self.skipTest("symlink creation unavailable; reparse guard tested separately")
        with self.assertRaises(d.GuardError):
            d.regular(self.root, d.TARGET)

    def test_apply_exactly_one_production_occurrence(self):
        result = d.apply(self.original, self.row)
        self.assertIn(b"MUTANT", result)
        self.assertTrue(result.endswith(b"mod tests { SAFE; }\n"))
        for original in (self.original.replace(b"synthetic", b"other"),
                         self.original + b"fn synthetic() { SAFE; }\n"):
            with self.assertRaises(d.GuardError):
                d.apply(original, self.row)
        row = {**self.row, "changes": [["mod tests { SAFE; }", "mod tests {}"]]}
        with self.assertRaises(d.GuardError):
            d.apply(self.original, row)

    def test_restore_original_bytes_on_exception(self):
        before = d.sha(self.target.read_bytes())
        with self.assertRaisesRegex(RuntimeError, "synthetic"):
            with d.mutated(self.root, self.original, self.row):
                self.assertIn(b"MUTANT", self.target.read_bytes())
                raise RuntimeError("synthetic")
        self.assertEqual(self.target.read_bytes(), self.original)
        self.assertEqual(d.sha(self.target.read_bytes()), before)

    def test_refuse_changed_source_without_overwriting(self):
        self.target.write_bytes(b"changed")
        with self.assertRaises(d.GuardError):
            with d.mutated(self.root, self.original, self.row):
                self.fail("must not enter")
        self.assertEqual(self.target.read_bytes(), b"changed")

    def test_named_failure_and_exit_100_required(self):
        name = d.TESTS[0]
        self.assertEqual(d.classify(100, status("FAIL", name) + "error: test run failed\n", name), "killed")
        self.assertEqual(d.classify(0, status("PASS", name), name), "survived")
        for code, output in (
            (101, status("FAIL", name)), (100, "error: compile failed"),
            (100, ""), (4, "no tests run"), (100, status("SKIP", name)),
            (100, status("FAIL", name) + "SKIP E3 owned native tests: root"),
            (100, status("FAIL", name + "_suffix")),
            (100, status("FAIL", d.PREFIX + "l6_e3")),
            (100, status("PASS", name) + status("FAIL", name)),
            (0, status("FAIL", name)), (100, "test " + name + " ... FAILED"),
        ):
            with self.subTest(code=code, output=output):
                self.assertEqual(d.classify(code, output, name), "invalid")

    def test_real_nextest_0_9_151_status_excerpts(self):
        # Verbatim lines captured by the reviewer from nextest 0.9.151.
        failed = "FAIL [   0.015s] (1/1) arxd tests::bad\nerror: test run failed\n"
        passed = "PASS [   0.021s] (2/2) arxd tests::good\n"
        self.assertEqual(d.classify(100, failed, "tests::bad"), "killed")
        self.assertEqual(d.classify(0, passed, "tests::good"), "survived")
        padded = "PASS [   0.028s] ( 1/11) arxd tests::t3\n"
        self.assertEqual(d.classify(0, padded, "tests::t3"), "survived")
        self.assertEqual(d.statuses(padded, "tests::t3"), ["PASS"])

        self.assertEqual(d.statuses(passed.replace("(2/2) ", ""), "tests::good"), ["PASS"])
        for output in (failed.replace("error: test run failed\n", ""),
                       failed + "error: test run failed\n",
                       failed + "error[E0308]: mismatched types\n",
                       failed + "error: could not compile arxd\n",
                       failed.replace("(1/1)", "(1/x)")):
            with self.subTest(output=output):
                self.assertEqual(d.classify(100, output, "tests::bad"), "invalid")
        for error in ("error[E0308]: mismatched types\n", "error: could not compile arxd\n"):
            with self.assertRaises(d.GuardError):
                d.baseline_result(0, baseline() + error)

    def test_baseline_visible_markers_distinguish_skips(self):
        self.assertEqual(d.baseline_result(0, baseline()), {"L7": "passed", "L8": "passed"})
        self.assertEqual(d.baseline_result(0, baseline() +
                         "    SKIP L7: user xattrs refused\n    SKIP L8: unexpected Standard hardening\n"),
                         {"L7": "skipped", "L8": "skipped"})
        for output in ("", baseline().replace(d.TESTS[1], d.TESTS[1] + "_suffix"),
                       baseline() + "SKIP E3 owned native tests: root", baseline() + "SKIP unknown"):
            with self.assertRaises(d.GuardError):
                d.baseline_result(0, output)
        with self.assertRaises(d.GuardError):
            d.baseline_result(100, baseline())

    def child(self, script, seconds=2, limit=1024):
        path = self.root / "fake.py"
        path.write_text(script, encoding="utf-8")
        return d.run_bounded([sys.executable, "-I", "-B", str(path)], self.root, seconds, limit)

    def test_synthetic_process_exit_capture(self):
        code, output, elapsed = self.child("print('synthetic'); raise SystemExit(100)")
        self.assertEqual((code, output.strip()), (100, "synthetic"))
        self.assertGreaterEqual(elapsed, 0)

    def test_deadline_kills_owned_synthetic_child(self):
        started = time.monotonic()
        with self.assertRaisesRegex(d.GuardError, "deadline"):
            self.child("import time; time.sleep(20)", seconds=0.2)
        self.assertLess(time.monotonic() - started, 7)
        with self.assertRaisesRegex(d.GuardError, "deadline"):
            self.child("raise AssertionError('must not start')", seconds=0)

    def test_output_limit_fails_closed(self):
        with self.assertRaisesRegex(d.GuardError, "output_bound"):
            self.child("print('x' * 100000)", limit=100)

    def test_json_output_contains_fixed_metadata_only(self):
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            d.emit("L1_take_raised", "killed", seconds=0.1, exit=100)
        self.assertEqual(json.loads(buffer.getvalue()),
                         {"label": "L1_take_raised", "status": "killed", "seconds": 0.1, "exit": 100})

    def test_bounded_read(self):
        with self.assertRaises(d.GuardError):
            d.bounded_read(self.target, 4)

    def test_tree_fingerprint_detects_content_and_bounds_inventory(self):
        before = d.tree_hash(self.root)
        self.target.write_bytes(self.original + b"changed")
        self.assertNotEqual(d.tree_hash(self.root), before)
        self.target.write_bytes(self.original)
        self.assertEqual(d.tree_hash(self.root), before)
        for number in range(64):
            (self.target.parent / f"synthetic_{number}").write_bytes(b"")
        with self.assertRaisesRegex(d.GuardError, "tree_bound"):
            d.tree_hash(self.root)

    def test_hosted_guard_precedes_source_access(self):
        with patch.object(sys, "argv", ["driver.py"]), patch.object(sys, "platform", "linux"), \
                patch.object(d.os, "getuid", create=True, return_value=1001), \
                patch.object(d.os, "geteuid", create=True, return_value=1001), \
                patch.dict(os.environ, {"GITHUB_ACTIONS": "false"}), \
                contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(d.GuardError, "hosted_only"):
            d.main()

    def test_manifest_pin_and_count(self):
        path = self.root / "tools/ci/e3-native/mutations.jsonl"
        path.parent.mkdir(parents=True)
        data = b"".join(json.dumps({**self.row, "mutation": f"synthetic_{n}"}).encode() + b"\n"
                        for n in range(8))
        path.write_bytes(data)
        with patch.object(d, "MANIFEST_SHA", d.sha(data)):
            self.assertEqual(len(d.load_mutations(self.root)), 8)
            path.write_bytes(data + b" ")
            with self.assertRaisesRegex(d.GuardError, "manifest_pin"):
                d.load_mutations(self.root)
        path.write_bytes(data.splitlines()[0] + b"\n")
        with patch.object(d, "MANIFEST_SHA", d.sha(path.read_bytes())):
            with self.assertRaisesRegex(d.GuardError, "manifest_count"):
                d.load_mutations(self.root)

    def exercise(self, middle):
        calls = []

        def runner(command, root, seconds):
            calls.append(command)
            if len(calls) in (1, 3):
                self.assertEqual(self.target.read_bytes(), self.original)
                return 0, baseline(), 0.01
            self.assertIn(b"MUTANT", self.target.read_bytes())
            if isinstance(middle, Exception):
                raise middle
            return middle

        buffer = io.StringIO()
        with patch.object(d, "SOURCE_SHA", d.sha(self.original)), \
                patch.object(d, "load_mutations", return_value=[self.row, self.row]), \
                contextlib.redirect_stdout(buffer):
            result = d.execute(self.root, runner)
        self.assertEqual(len(calls), 3)  # stop immediately, then final baseline
        self.assertEqual(self.target.read_bytes(), self.original)
        rows = [json.loads(line) for line in buffer.getvalue().splitlines()]
        self.assertEqual(rows[-1]["label"], "baseline_after")
        self.assertEqual(rows[-2]["status"], "restored")
        return result, rows

    def test_survivor_stops_without_loosening_and_baselines_after(self):
        result, rows = self.exercise((0, status("PASS", d.TESTS[0]), 0.01))
        self.assertEqual(result, 1)
        self.assertEqual(rows[1]["status"], "survived")

    def test_compile_failure_stops_and_restores(self):
        result, rows = self.exercise((101, "synthetic compiler failure", 0.01))
        self.assertEqual(result, 1)
        self.assertEqual(rows[1]["status"], "invalid")

    def test_deadline_restores_and_baselines_after(self):
        result, rows = self.exercise(d.GuardError("deadline"))
        self.assertEqual(result, 1)
        self.assertEqual(rows[1]["status"], "deadline")

    def test_complete_run_requires_all_eight_named_kills(self):
        rows = [{**self.row, "mutation": f"synthetic_{n}"} for n in range(8)]
        calls = []

        def runner(command, root, seconds):
            calls.append(command)
            self.assertGreater(seconds, 0)
            if len(calls) in (1, 10):
                self.assertEqual(self.target.read_bytes(), self.original)
                return 0, baseline(), 0.01
            self.assertIn(b"MUTANT", self.target.read_bytes())
            return 100, status("FAIL", d.TESTS[0]) + "error: test run failed\n", 0.01

        buffer = io.StringIO()
        with patch.object(d, "SOURCE_SHA", d.sha(self.original)), \
                patch.object(d, "load_mutations", return_value=rows), contextlib.redirect_stdout(buffer):
            self.assertEqual(d.execute(self.root, runner), 0)
        evidence = [json.loads(line) for line in buffer.getvalue().splitlines()]
        self.assertEqual(sum(row["status"] == "killed" for row in evidence), 8)
        self.assertEqual(len(calls), 10)
        self.assertEqual(self.target.read_bytes(), self.original)

    def test_bad_initial_baseline_still_owes_final_baseline(self):
        calls = []

        def runner(command, root, seconds):
            calls.append(command)
            return (101, "synthetic failure", 0.01) if len(calls) == 1 else (0, baseline(), 0.01)

        with patch.object(d, "SOURCE_SHA", d.sha(self.original)), \
                patch.object(d, "load_mutations", return_value=[self.row]), \
                contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(d.GuardError, "baseline_failed"):
            d.execute(self.root, runner)
        self.assertEqual(len(calls), 2)
        self.assertEqual(self.target.read_bytes(), self.original)

    def test_root_refused_before_source_or_runner(self):
        with patch.object(sys, "argv", ["driver.py"]), patch.object(sys, "platform", "linux"), \
                patch.object(d.os, "getuid", create=True, return_value=0), \
                patch.object(d.os, "geteuid", create=True, return_value=0), \
                contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(d.GuardError, "root_refused"):
            d.main()

    def test_filter_is_native_exact_not_scanner(self):
        cmd = d.command(d.TESTS[0])
        self.assertEqual(cmd[cmd.index("-E") + 1], "test(=" + d.TESTS[0] + ")")
        self.assertNotIn("l6_e3", " ".join(cmd))


if __name__ == "__main__":
    unittest.main()
