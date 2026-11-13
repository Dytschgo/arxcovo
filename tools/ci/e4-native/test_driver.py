"""Portable owned-temp tests. Children are Python only; native runner is synthetic."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("e4_driver", Path(__file__).with_name("driver.py"))
d = importlib.util.module_from_spec(spec)
spec.loader.exec_module(d)


def status(kind, name, binary="arxd"):
    return f"    {kind} [   0.015s] ( 1/11) {binary} {name}\n"


class HarnessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.original = b"fn synthetic() { SAFE; }\n#[cfg(test)]\nmod tests { SAFE; }\n"
        self.originals = dict.fromkeys(d.TARGETS, self.original)
        for target in d.TARGETS:
            path = self.root / target
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(self.original)
        self.target = self.root / d.TARGETS[0]
        self.change = {"target": d.TARGETS[0], "old": "fn synthetic() { SAFE; }",
                       "new": "fn synthetic() { MUTANT; }"}
        self.row = {"mutation": "S8_synthetic", "changes": [self.change],
                    "assertion": d.TESTS[0], "binary": "arxd", "blocker": None}

    def test_safe_regular_targets_only(self):
        for name in ("../x", "/x", "Cargo.toml", d.TARGETS[0] + "/../x"):
            with self.subTest(name=name), self.assertRaises(d.GuardError):
                d.regular(self.root, name)
        os.link(self.target, self.root / "alias")
        with self.assertRaises(d.GuardError):
            d.regular(self.root, d.TARGETS[0])

    def test_reparse_and_symlink_guards(self):
        from types import SimpleNamespace
        real = Path.lstat
        for mode, attributes in ((0o120777, 0), (0o40700, 0x400)):
            def fake(path, *args, **kwargs):
                return (SimpleNamespace(st_mode=mode, st_file_attributes=attributes)
                        if path == self.target.parent else real(path, *args, **kwargs))
            with patch.object(Path, "lstat", fake), self.assertRaises(d.GuardError):
                d.regular(self.root, d.TARGETS[0])

    def test_read_bound(self):
        with self.assertRaisesRegex(d.GuardError, "read_bound"):
            d.bounded_read(self.target, 4)

    def test_exact_once_before_test_boundary(self):
        self.assertIn(b"MUTANT", d.apply(self.original, self.change))
        for data in (self.original + self.original, self.original.replace(b"synthetic", b"other")):
            with self.assertRaises(d.GuardError):
                d.apply(data, self.change)
        with self.assertRaises(d.GuardError):
            d.apply(self.original, {**self.change, "old": "mod tests { SAFE; }"})

    def test_restore_after_exception_and_changed_source_refusal(self):
        with self.assertRaises(RuntimeError):
            with d.mutated(self.root, self.originals, self.row):
                self.assertIn(b"MUTANT", self.target.read_bytes())
                raise RuntimeError("synthetic")
        self.assertEqual(self.target.read_bytes(), self.original)
        self.target.write_bytes(b"changed")
        with self.assertRaisesRegex(d.GuardError, "source_changed"):
            with d.mutated(self.root, self.originals, self.row):
                self.fail()
        self.assertEqual(self.target.read_bytes(), b"changed")

    def test_nextest_padded_counter_footer_and_exact_name(self):
        for binary in ("arxd", "unix_shim"):
            name = d.TESTS[0] if binary == "arxd" else d.S6
            failed = status("FAIL", name, binary) + "error: test run failed\n"
            self.assertEqual(d.classify(100, failed, name, binary), "killed")
            self.assertEqual(d.classify(0, status("PASS", name, binary), name, binary), "survived")
            self.assertEqual(d.statuses(status("PASS", name, binary).replace("( 1/11) ", ""), name, binary), ["PASS"])
            for code, output in ((101, failed), (100, failed + "error[E0308]: mismatch\n"),
                                 (100, failed + "error: test run failed\n"), (100, failed + "SKIP\n"),
                                 (100, failed.replace("error: test run failed\n", "")),
                                 (100, failed.replace(name, name + "_suffix")),
                                 (100, failed.replace("( 1/11)", "(1/x)")),
                                 (100, failed + status("PASS", name, binary)), (100, "")):
                with self.subTest(code=code, output=output):
                    self.assertEqual(d.classify(code, output, name, binary), "invalid")

    def test_baseline_visible_skips_and_refusals(self):
        text = "".join(status("PASS", n) for n in d.TESTS)
        self.assertEqual(d.baseline_result(0, text)["inherited_nnp"], "passed")
        self.assertEqual(d.baseline_result(0, text + "E4_SKIP_INHERITED_NNP\n")["inherited_nnp"], "skipped")
        self.assertEqual(d.baseline_result(0, status("PASS", d.L8) + "SKIP L8: unexpected Standard hardening\n", (d.L8,))["L8"], "skipped")
        for code, output in ((101, text), (0, text + "E4_SKIP_ROOT\n"),
                             (0, text.replace(d.TESTS[0], "other")), (0, text + "SKIP unknown\n")):
            with self.assertRaises(d.GuardError):
                d.baseline_result(code, output)

    def child(self, source, seconds=3, limit=4096):
        return d.run_bounded([sys.executable, "-I", "-B", "-c", source], self.root, seconds, limit)

    def test_actual_python_command_result(self):
        code, output, elapsed = self.child("print('fixed'); raise SystemExit(7)")
        self.assertEqual((code, output.strip()), (7, "fixed"))
        self.assertLess(elapsed, 3)

    def test_actual_deadline_and_output_bounds(self):
        with self.assertRaisesRegex(d.GuardError, "deadline"):
            self.child("import time; time.sleep(10)", seconds=0.05)
        with self.assertRaisesRegex(d.GuardError, "output_bound"):
            self.child("print('x'*100000)", limit=32)
        with self.assertRaisesRegex(d.GuardError, "deadline"):
            self.child("raise SystemExit(0)", seconds=0)

    def test_manifest_pin_schema_and_count(self):
        path = self.root / d.MANIFEST
        path.parent.mkdir(parents=True)
        rows = [{**self.row, "mutation": f"S8_synthetic_{n}"} for n in range(11)]
        def load(rows):
            data = b"".join(json.dumps(r).encode() + b"\n" for r in rows)
            path.write_bytes(data)
            with patch.object(d, "MANIFEST_SHA", d.sha(data)):
                return d.load_mutations(self.root)
        self.assertEqual(len(load(rows)), 11)
        for invalid in (rows[:8], [self.row]*11,
                        [{**r, "binary": "other"} for r in rows],
                        [{**r, "blocker": "arbitrary"} for r in rows]):
            with self.assertRaises(d.GuardError):
                load(invalid)
        path.write_bytes(b"changed")
        with self.assertRaisesRegex(d.GuardError, "manifest_pin"):
            d.load_mutations(self.root)

    def test_fixed_manifest_and_production_fragments_read_only(self):
        root = Path(__file__).absolute().parents[3]
        rows = d.load_mutations(root)
        self.assertEqual(len(rows), 11)
        self.assertEqual(sum(r["blocker"] is None for r in rows), 11)
        for row in rows:
            change = row["changes"][0]
            original = d.bounded_read(d.regular(root, change["target"]), 262144)
            self.assertEqual(d.sha(original), d.PINS[change["target"]])
            d.apply(original, change)  # in memory only; no native execution
        path = next(r for r in rows if r["mutation"] == "S9c_path_reopen")
        self.assertEqual(path["assertion"], d.PREFIX + "s9c_bound_object_survives_file_and_directory_swap")
        self.assertNotIn('/usr/bin/sudo', path["changes"][0]["new"])

    def exercise(self, middle, blocked=False):
        calls = []
        def runner(cmd, root, seconds, limit=d.LIMIT):
            self.assertEqual(root, self.root)
            self.assertGreater(seconds, 0)
            if cmd[0] == "git":
                return 0, "".join(f"100644 {'a'*40} 0\t{p}\0" for p in (*d.TARGETS, d.MANIFEST)), 0.01
            calls.append(cmd)
            if len(calls) == 4 and not blocked:
                self.assertIn(b"MUTANT", self.target.read_bytes())
                if isinstance(middle, Exception):
                    raise middle
                return middle
            self.assertEqual(self.target.read_bytes(), self.original)
            selection = cmd[cmd.index("-E") + 1]
            if selection == f"test(={d.S6})":
                return 0, status("PASS", d.S6, "unix_shim"), 0.01
            names = (d.L8,) if selection == f"test(={d.L8})" else d.TESTS
            return 0, "".join(status("PASS", n) for n in names), 0.01
        row = {**self.row, "blocker": "nnp_self_skip"} if blocked else self.row
        output = io.StringIO()
        with patch.object(d, "PINS", {p: d.sha(self.original) for p in d.TARGETS}), \
                patch.object(d, "load_mutations", return_value=[row]), contextlib.redirect_stdout(output):
            result = d.execute(self.root, runner)
        self.assertEqual(self.target.read_bytes(), self.original)
        self.assertEqual(len(calls), 6 if blocked else 7)
        return result, [json.loads(line) for line in output.getvalue().splitlines()]

    def test_actual_kill_is_required_and_restoration_baselines_run(self):
        self.assertEqual(self.exercise((100, status("FAIL", d.TESTS[0]) + "error: test run failed\n", 0.01))[0], 0)
        for middle in ((0, status("PASS", d.TESTS[0]), 0.01),
                       (101, "error: compiler failed", 0.01), d.GuardError("deadline")):
            result, rows = self.exercise(middle)
            self.assertEqual(result, 1)
            self.assertEqual(rows[-1]["label"], "baseline_after_S6_NNP_fenced")
            self.assertEqual(rows[-4]["status"], "restored")

    def test_known_fixture_gap_never_runs_mutant_or_claims_kill(self):
        result, rows = self.exercise(None, blocked=True)
        self.assertEqual(result, 1)
        self.assertEqual(rows[3]["status"], "blocked")
        self.assertFalse(any(r["status"] == "killed" for r in rows))

    def test_tracking_source_pin_and_fingerprint(self):
        before = d.fingerprint(self.root)
        self.target.write_bytes(b"changed")
        self.assertNotEqual(d.fingerprint(self.root), before)
        with self.assertRaisesRegex(d.GuardError, "source_pin"):
            d.execute(self.root, lambda *a: self.fail("no subprocess before pin"))
        for code, text in ((0, ""), (1, ""), (0, "120000 " + "a"*40 + " 0\tx\0")):
            with self.assertRaisesRegex(d.GuardError, "tracked_guard"):
                d.tracked(self.root, lambda *a: (code, text, 0.01))

    def test_root_euid_and_hosted_guards(self):
        for uid, euid, hosted, expected in ((0, 1001, "true", "root_refused"),
                                           (1001, 0, "true", "root_refused"),
                                           (1001, 1001, "false", "hosted_only")):
            with patch.object(sys, "argv", ["driver.py"]), patch.object(sys, "platform", "linux"), \
                    patch.object(d.os, "getuid", create=True, return_value=uid), \
                    patch.object(d.os, "geteuid", create=True, return_value=euid), \
                    patch.dict(os.environ, {"GITHUB_ACTIONS": hosted, "RUNNER_OS": "Linux"}), \
                    contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(d.GuardError, expected):
                d.main()

    def test_commands_have_exact_scopes_and_no_retries(self):
        cmd = d.command(d.TESTS[0])
        self.assertEqual(cmd[cmd.index("-E") + 1], "test(=" + d.TESTS[0] + ")")
        self.assertIn("--lib", cmd)
        self.assertEqual(cmd[cmd.index("--retries") + 1], "0")
        self.assertIn("--test", d.command(d.S6, "unix_shim"))
        self.assertNotIn("--run-ignored", " ".join(d.command()))

    def test_ten_kills_then_blocked_eleventh_restores_every_target(self):
        rows = [{**self.row, "mutation": f"S8_synthetic_{n}"} for n in range(10)]
        rows.append({**self.row, "mutation": "S11_nnp_true", "blocker": "nnp_self_skip"})
        native_calls = []
        def runner(cmd, root, seconds, limit=d.LIMIT):
            self.assertGreater(seconds, 0)
            if cmd[0] == "git":
                return 0, "".join(f"100644 {'b'*40} 0\t{p}\0" for p in (*d.TARGETS, d.MANIFEST)), 0.01
            native_calls.append(cmd)
            if 4 <= len(native_calls) <= 13:
                self.assertIn(b"MUTANT", self.target.read_bytes())
                return 100, status("FAIL", d.TESTS[0]) + "error: test run failed\n", 0.01
            for target in d.TARGETS:
                self.assertEqual((self.root / target).read_bytes(), self.original)
            selection = cmd[cmd.index("-E") + 1]
            if selection == f"test(={d.S6})":
                return 0, status("PASS", d.S6, "unix_shim"), 0.01
            names = (d.L8,) if selection == f"test(={d.L8})" else d.TESTS
            return 0, "".join(status("PASS", n) for n in names), 0.01
        output = io.StringIO()
        with patch.object(d, "PINS", {p: d.sha(self.original) for p in d.TARGETS}), \
                patch.object(d, "load_mutations", return_value=rows), contextlib.redirect_stdout(output):
            self.assertEqual(d.execute(self.root, runner), 1)
        evidence = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(sum(r["status"] == "killed" for r in evidence), 10)
        self.assertEqual(sum(r["status"] == "blocked" for r in evidence), 1)
        self.assertEqual(len(native_calls), 16)

    def test_all_eleven_kills_require_final_restored_baselines(self):
        rows = [{**self.row, "mutation": f"S8_synthetic_{n}"} for n in range(10)]
        rows.append({**self.row, "mutation": "S11_nnp_true", "blocker": None})
        native_calls = []
        def runner(cmd, root, seconds, limit=d.LIMIT):
            self.assertGreater(seconds, 0)
            if cmd[0] == "git":
                return 0, "".join(f"100644 {'b'*40} 0\t{p}\0" for p in (*d.TARGETS, d.MANIFEST)), 0.01
            native_calls.append(cmd)
            if 4 <= len(native_calls) <= 14:
                self.assertIn(b"MUTANT", self.target.read_bytes())
                return 100, status("FAIL", d.TESTS[0]) + "error: test run failed\n", 0.01
            for target in d.TARGETS:
                self.assertEqual((self.root / target).read_bytes(), self.original)
            selection = cmd[cmd.index("-E") + 1]
            if selection == f"test(={d.S6})":
                return 0, status("PASS", d.S6, "unix_shim"), 0.01
            names = (d.L8,) if selection == f"test(={d.L8})" else d.TESTS
            return 0, "".join(status("PASS", n) for n in names), 0.01
        output = io.StringIO()
        with patch.object(d, "PINS", {p: d.sha(self.original) for p in d.TARGETS}), \
                patch.object(d, "load_mutations", return_value=rows), contextlib.redirect_stdout(output):
            self.assertEqual(d.execute(self.root, runner), 0)
        evidence = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(sum(r["status"] == "killed" for r in evidence), 11)
        self.assertEqual(sum(r["status"] == "blocked" for r in evidence), 0)
        self.assertEqual(len(native_calls), 17)

    def test_workflow_is_separate_read_only_plain_pr_with_fixed_actions(self):
        root = Path(__file__).absolute().parents[3]
        workflow = (root / ".github/workflows/e4-native-evidence.yml").read_text()
        reference = (root / ".github/workflows/e3-native-evidence.yml").read_text()
        self.assertEqual(__import__("re").findall(r"uses: (.+)", workflow),
                         __import__("re").findall(r"uses: (.+)", reference))
        for fragment in ("  pull_request:\n", "  contents: read\n", "timeout-minutes: 10",
                         "CARGO_PROFILE_DEV_DEBUG: '0'", "CARGO_INCREMENTAL: '1'",
                         "cargo-nextest@0.9.151", "persist-credentials: false",
                         "docs/evidence/linux-rights-probe-e4-native.md"):
            self.assertIn(fragment, workflow)
        for fragment in ("pull_request_target", "workflow_dispatch", "sudo ", "write-all"):
            self.assertNotIn(fragment, workflow)

    def test_windows_entry_point_never_reaches_native_runner(self):
        with patch.object(sys, "argv", ["driver.py"]), patch.object(sys, "platform", "win32"), \
                patch.object(d, "execute", side_effect=AssertionError("native forbidden")), \
                self.assertRaisesRegex(d.GuardError, "ubuntu_only"):
            d.main()

    def test_failed_initial_baseline_restores_all_targets_before_final_baseline(self):
        calls = []
        def runner(cmd, root, seconds, limit=d.LIMIT):
            if cmd[0] == "git":
                return 0, "".join(f"100644 {'c'*40} 0\t{p}\0" for p in (*d.TARGETS, d.MANIFEST)), 0.01
            calls.append(cmd)
            if len(calls) == 1:
                (self.root / d.TARGETS[1]).write_bytes(b"synthetic damage")
                return 101, "synthetic baseline failure", 0.01
            for target in d.TARGETS:
                self.assertEqual((self.root / target).read_bytes(), self.original)
            selection = cmd[cmd.index("-E") + 1]
            if selection == f"test(={d.S6})":
                return 0, status("PASS", d.S6, "unix_shim"), 0.01
            names = (d.L8,) if selection == f"test(={d.L8})" else d.TESTS
            return 0, "".join(status("PASS", n) for n in names), 0.01
        with patch.object(d, "PINS", {p: d.sha(self.original) for p in d.TARGETS}), \
                patch.object(d, "load_mutations", return_value=[self.row]), \
                contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(d.GuardError, "baseline_failed"):
            d.execute(self.root, runner)
        self.assertEqual(len(calls), 4)


if __name__ == "__main__":
    unittest.main()
