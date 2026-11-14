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

spec = importlib.util.spec_from_file_location("s2b_driver", Path(__file__).with_name("driver.py"))
d = importlib.util.module_from_spec(spec)
spec.loader.exec_module(d)



class HarnessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.original = b"unique original"
        for target in d.TARGETS:
            path = self.root / target
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(self.original)
        self.target = self.root / d.TARGETS[0]

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

    def test_windows_entry_point_never_reaches_native_runner(self):
        with patch.object(sys, "argv", ["driver.py"]), patch.object(sys, "platform", "win32"), \
                patch.object(d, "execute", side_effect=AssertionError("native forbidden")), \
                self.assertRaisesRegex(d.GuardError, "ubuntu_only"):
            d.main()

    def test_real_nextest_format_and_fail_closed(self):
        for counter in ("", "(1/1) ", "( 1/11) "):
            passed = "    PASS [   0.028s] " + counter + "arxd " + d.TEST + "\n"
            failed = passed.replace("PASS", "FAIL") + "error: test run failed\n"
            self.assertEqual(d.classify(0, passed), "survived")
            self.assertEqual(d.classify(100, failed), "killed")
            for code, text in ((101, failed), (4, "error: no tests to run"), (100, failed + "SKIP"), (100, failed + "error[E0308]: mismatch"), (100, failed + "error: test run failed\n"), (100, failed.replace(d.TEST, d.TEST+"x")), (100, failed.replace("arxd ", "arxd::other ")), (100, failed+"FAIL [ 0.1s] arxd other\n"), (100, failed.replace("error: test run failed\n", ""))):
                self.assertEqual(d.classify(code, text), "invalid")

    def test_exact_matches_and_restore_exception(self):
        row = dict(target=d.TARGETS[0], old="unique original", new="mutated")
        originals = {p:self.original for p in d.TARGETS}
        before = d.fingerprint(self.root)
        for data in (b"absent", self.original*2):
            with self.assertRaises(d.GuardError):
                d.apply(data, row)
        with self.assertRaisesRegex(d.GuardError, "deadline"):
            with d.mutated(self.root, originals, row):
                self.assertEqual(self.target.read_bytes(), b"mutated")
                raise d.GuardError("deadline")
        self.assertEqual(d.fingerprint(self.root), before)

    def test_pinned_manifest_applicability_in_memory_only(self):
        root = Path(__file__).absolute().parents[3]
        rows = d.load_mutations(root)
        self.assertEqual(len(rows), 2)
        for path, pin in d.PINS.items():
            self.assertEqual(d.sha(d.bounded_read(d.regular(root,path),262144)),pin)
        for row in rows:
            original = d.bounded_read(d.regular(root,row['target']),262144)
            self.assertNotEqual(d.apply(original,row), original)
        self.assertIn('std::mem::forget(_group);',rows[1]['new'])
        self.assertEqual(d.sha(d.bounded_read(d.regular(root,d.WORKFLOW),32768)),d.WORKFLOW_SHA)
        cmd = d.command()
        self.assertEqual(cmd[cmd.index('-E')+1], 'test(='+d.TEST+')')
        self.assertIn('--success-output',cmd)
        self.assertIn('--lib',cmd)

    def test_whole_run_restores_and_requires_after_baseline(self):
        row = dict(mutation="synthetic", target=d.TARGETS[0], old="unique original", new="mutated")
        for p in (d.MANIFEST, d.WORKFLOW):
            path = self.root/p
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixed")
        for middle in ("killed", "survived", "compile", "timeout"):
            calls = []
            def runner(cmd, root, seconds, limit=d.LIMIT):
                self.assertGreater(seconds,0)
                if cmd[:2] == ["git", "ls-files"]:
                    return 0, ''.join('100644 '+ 'a'*40+' 0\t'+p+'\0' for p in (*d.TARGETS,d.MANIFEST,d.WORKFLOW)),0
                if cmd[:2] == ["git", "status"]:
                    return 0,"",0
                if cmd[:2] == ["git", "rev-parse"]:
                    return 0, ('a'*40 if cmd[-1]=='HEAD' else str(root))+'\n',0
                calls.append(cmd)
                if len(calls)==2:
                    self.assertEqual(self.target.read_bytes(),b'mutated')
                    if middle=='timeout':
                        raise d.GuardError('deadline')
                    if middle=='survived':
                        return 0,'PASS [ 0.1s] arxd '+d.TEST+'\n',0
                    if middle=='compile':
                        return 101,'error[E0308]: mismatch',0
                    if middle=='killed':
                        return 100,'FAIL [ 0.1s] arxd '+d.TEST+'\nerror: test run failed\n',0
                self.assertEqual(self.target.read_bytes(),self.original)
                return 0,'PASS [ 0.1s] arxd '+d.TEST+'\n',0
            with patch.object(d,'PINS',{p:d.sha(self.original) for p in d.TARGETS}), patch.object(d,'WORKFLOW_SHA',d.sha(b'fixed')), patch.object(d,'load_mutations',return_value=[row]), contextlib.redirect_stdout(io.StringIO()):
                if middle=='timeout':
                    with self.assertRaisesRegex(d.GuardError,'deadline'):
                        d.execute(self.root,runner)
                else:
                    self.assertEqual(d.execute(self.root,runner),0 if middle=='killed' else 1)
            self.assertEqual(self.target.read_bytes(),self.original)
            self.assertEqual(len(calls),3)

if __name__ == '__main__':
    unittest.main()
