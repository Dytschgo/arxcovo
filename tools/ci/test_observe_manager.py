import contextlib
import io
import json
import subprocess
import sys
import unittest
from unittest.mock import patch

import observe_manager as diagnostic


class ManagerObservationTests(unittest.TestCase):
    def test_only_fixed_read_queries(self):
        calls = []

        def run(argv):
            calls.append(argv)
            return diagnostic.Capture("completed", 1)

        diagnostic.observe("darwin", run)
        diagnostic.observe("linux", run)
        self.assertEqual(calls, [
            ["/bin/launchctl", "print", "system/com.arxcovo.arxd"],
            ["/bin/launchctl", "print", "system/com.apple.syslogd"],
            ["/usr/bin/systemctl", "show",
             "--property=LoadState,ActiveState,FragmentPath,DropInPaths", "arxd.service"],
        ])

    def test_no_raw_output_or_error_values(self):
        for platform in ("darwin", "linux"):
            result = diagnostic.Capture("completed", 113,
                b"path = private-owner\nstate = private-value\nFragmentPath=private-path\n",
                b"private-error")
            encoded = diagnostic.observe(platform, lambda _: result)
            self.assertNotIn("private", encoded)
            self.assertLessEqual(len(encoded), 2048)
            self.assertTrue(all(r["exit_code"] == 113
                                for r in json.loads(encoded)["observations"]))

    def test_absence_phrases_are_observations_not_guessed_success(self):
        result = diagnostic.Capture("completed", 113, b"",
            b'Could not find service "com.arxcovo.arxd" in domain for system')
        row = diagnostic.summarize("arxcovo", "system/com.arxcovo.arxd", result, "darwin")
        self.assertEqual(row["exit_code"], 113)
        self.assertTrue(row["missing_service_phrase"])
        self.assertTrue(row["missing_target_named"])
        self.assertEqual(row["state"], "absent")

    def test_duplicate_fields_and_unknown_values_stay_explicit(self):
        result = diagnostic.Capture("completed", 0,
            b"LoadState=loaded\nLoadState=not-found\nActiveState=private\nFragmentPath=\nDropInPaths=\n")
        row = diagnostic.summarize("arxcovo", "arxd.service", result, "linux")
        self.assertEqual(row["load"], "duplicate")
        self.assertEqual(row["active"], "other")
        self.assertEqual(row["fragment"], "empty")

    def test_known_loaded_control_has_only_fixed_shapes(self):
        result = diagnostic.Capture("completed", 0,
            b"system/com.apple.syslogd = {\n\tstate = running\n"
            b"\tpath = /System/Library/LaunchDaemons/com.apple.syslogd.plist\n"
            b"\tpid = 12345\n\tendpoints = {\n\t\tstate = private\n"
            b"\t\tpath = private\n\t\tpid = 67890\n\t}\n}")
        row = diagnostic.summarize("apple_syslog_control", "system/com.apple.syslogd",
                                   result, "darwin")
        self.assertTrue(row["target_header"])
        self.assertTrue(row["pid_field"])
        self.assertEqual(row["state"], "running")
        self.assertEqual(row["path"], "apple_syslog")
        self.assertNotIn("12345", json.dumps(row))

    def test_nested_fields_cannot_stand_in_for_outer_job_fields(self):
        result = diagnostic.Capture("completed", 0,
            b"system/com.apple.syslogd = {\n\tendpoints = {\n\t\tstate = running\n"
            b"\t\tpath = /System/Library/LaunchDaemons/com.apple.syslogd.plist\n"
            b"\t\tpid = 12345\n\t}\n}")
        row = diagnostic.summarize("apple_syslog_control", "system/com.apple.syslogd",
                                   result, "darwin")
        self.assertEqual(row["state"], "absent")
        self.assertEqual(row["path"], "absent")
        self.assertFalse(row["pid_field"])

    def test_encoded_output_bound_is_enforced(self):
        with patch.object(diagnostic.json, "dumps", return_value="x" * 2049):
            with self.assertRaisesRegex(RuntimeError, "manager-observation-bound"):
                diagnostic.observe("unsupported")

    def test_capture_has_fixed_environment_and_no_shell(self):
        with patch.object(diagnostic.subprocess, "Popen", side_effect=OSError("private")) as popen:
            self.assertEqual(diagnostic.capture(["/bin/launchctl", "print", "fixed"]).status,
                             "unavailable")
        options = popen.call_args.kwargs
        self.assertIs(options["shell"], False)
        self.assertEqual(options["stdin"], subprocess.DEVNULL)
        self.assertEqual(options["cwd"], "/")
        self.assertEqual(options["env"], {"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C",
                                          "SYSTEMD_PAGER": "cat", "SYSTEMD_COLORS": "0"})

    def test_bad_arguments_never_query_or_echo(self):
        with patch.object(diagnostic.sys, "argv", ["observe_manager.py", "private"]), \
             patch.object(diagnostic, "observe") as observe, \
             contextlib.redirect_stderr(io.StringIO()) as output:
            self.assertEqual(diagnostic.main(), 2)
        observe.assert_not_called()
        self.assertEqual(output.getvalue(), "manager-observation-arguments\n")

    @unittest.skipUnless(sys.platform in ("linux", "darwin"), "Unix pipe polling")
    def test_real_ordinary_child_timeout(self):
        result = diagnostic.capture([sys.executable, "-c", "import time; time.sleep(10)"], timeout=0.1)
        self.assertEqual(result.status, "timeout")
        self.assertEqual(result.stdout, b"")

    @unittest.skipUnless(sys.platform in ("linux", "darwin"), "Unix pipe polling")
    def test_real_ordinary_child_combined_output_cap(self):
        result = diagnostic.capture([sys.executable, "-c",
            "import os; os.write(1,b'x'*8000); os.write(2,b'y'*9000)"])
        self.assertEqual(result.status, "overflow")
        self.assertEqual(result.stdout, b"")

    @unittest.skipUnless(sys.platform in ("linux", "darwin"), "Unix pipe polling")
    def test_real_ordinary_child_exit_and_eof(self):
        result = diagnostic.capture([sys.executable, "-c",
            "import sys; assert sys.stdin.read()==''; print('fixed'); sys.exit(3)"])
        self.assertEqual(result.status, "completed")
        self.assertEqual(result.code, 3)
        self.assertEqual(result.stdout, b"fixed\n")


if __name__ == "__main__":
    unittest.main()
