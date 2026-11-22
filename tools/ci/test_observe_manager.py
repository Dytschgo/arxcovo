import contextlib
import io
import json
import subprocess
import sys
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import observe_manager as diagnostic


class ManagerObservationTests(unittest.TestCase):
    R2_ENV = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
              "RUNNER_OS": "Linux", "ImageOS": "ubuntu24", "ImageVersion": "20261116.133.1"}

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
                b"\tpath = private-owner\n\tstate = private-value\nFragmentPath=private-path\n",
                b"private-error")
            encoded = diagnostic.observe(platform, lambda _: result)
            self.assertNotIn("private", encoded)
            self.assertLessEqual(len(encoded), 2048)
            self.assertTrue(all(r["exit_code"] == 113
                                for r in json.loads(encoded)["observations"]))

    def test_r2_receipt_uses_fixed_read_only_queries_and_shapes(self):
        calls = []

        def run(argv, timeout=None):
            calls.append((argv, timeout))
            return diagnostic.Capture("completed", 0, b"255.4-1ubuntu8.18", b"private")

        with patch.object(diagnostic.os, "uname", return_value=SimpleNamespace(
            release="7.0.0-1014-aws"), create=True), patch.object(
            diagnostic, "runner_tool_present", side_effect=lambda path: path != "/usr/sbin/nft"
        ) as tools:
            encoded = diagnostic.observe("linux", run, self.R2_ENV)
        self.assertEqual(calls, [
            (["/usr/bin/systemctl", *diagnostic.SYSTEMD_ARGS], None),
            (["/usr/bin/dpkg-query", "-W", "-f=${Version}", "systemd"], 1),
        ])
        self.assertEqual([call.args[0] for call in tools.call_args_list], list(diagnostic.R2_TOOLS))
        self.assertLessEqual(len(encoded), 2048)
        self.assertNotIn("private", encoded)
        receipt = json.loads(encoded)["r2_runner"]
        self.assertEqual(receipt, {
            "image_os": "ubuntu24", "image_version": "20261116.133.1",
            "kernel": "7.0.0-1014-aws", "systemd_package": "255.4-1ubuntu8.18",
            "tools": {path: path != "/usr/sbin/nft" for path in diagnostic.R2_TOOLS},
        })

    def test_r2_receipt_rejects_uncorrelated_runner_or_malformed_versions(self):
        for name, value in [("GITHUB_ACTIONS", "false"), ("RUNNER_ENVIRONMENT", "self-hosted"),
                            ("RUNNER_OS", "Windows"), ("ImageOS", "ubuntu26"),
                            ("ImageVersion", "private-image")]:
            with self.subTest(field=name):
                env = {**self.R2_ENV, name: value}
                with patch.object(diagnostic, "runner_tool_present") as tools:
                    encoded = diagnostic.observe("linux", lambda _: diagnostic.Capture("unavailable"), env)
                tools.assert_not_called()
                self.assertNotIn("r2_runner", json.loads(encoded))
        with patch.object(diagnostic.os, "uname", return_value=SimpleNamespace(
            release="7.private\nvalue"), create=True), patch.object(
            diagnostic, "runner_tool_present", return_value=True
        ):
            for result in (diagnostic.Capture("completed", 0, b"255\nprivate"),
                           diagnostic.Capture("completed", 1, b"255.4-1ubuntu8.18"),
                           diagnostic.Capture("timeout")):
                receipt = diagnostic.r2_runner_receipt(self.R2_ENV, lambda *a, **k: result)
                self.assertIsNone(receipt["kernel"])
                self.assertIsNone(receipt["systemd_package"])

    def test_r2_package_query_refuses_an_unvetted_executable(self):
        with patch.object(diagnostic.os, "uname", return_value=SimpleNamespace(
            release="7.0.0-1014-aws"), create=True), patch.object(
            diagnostic, "runner_tool_present", side_effect=lambda path: path != "/usr/bin/dpkg-query"
        ):
            def unexpected_query(*args, **kwargs):
                self.fail("unvetted package executable was invoked")

            receipt = diagnostic.r2_runner_receipt(self.R2_ENV, unexpected_query)
        self.assertFalse(receipt["tools"]["/usr/bin/dpkg-query"])
        self.assertIsNone(receipt["systemd_package"])

    def test_r2_tool_metadata_refuses_unsafe_locations_and_permissions(self):
        system = diagnostic.Path("/usr/sbin/nft")
        with patch.object(diagnostic.Path, "resolve", return_value=diagnostic.Path("/tmp/nft")), \
             patch.object(diagnostic.Path, "lstat") as metadata:
            self.assertFalse(diagnostic.runner_tool_present("/usr/sbin/nft"))
            metadata.assert_not_called()
        with patch.object(diagnostic.Path, "resolve", return_value=system):
            for uid, mode, expected in [(0, 0o100755, True), (1001, 0o100755, False),
                                        (0, 0o100775, False), (0, 0o100644, False),
                                        (0, 0o100001, False), (0, 0o040755, False)]:
                ancestry = [SimpleNamespace(st_uid=0, st_mode=0o040755)] * len(system.parents)
                with patch.object(diagnostic.Path, "lstat", side_effect=[
                    *ancestry, SimpleNamespace(st_uid=uid, st_mode=mode)
                ]):
                    self.assertEqual(diagnostic.runner_tool_present("/usr/sbin/nft"), expected)
            for uid, mode in [(0, 0o040777), (1001, 0o040755), (0, 0o120755)]:
                with patch.object(diagnostic.Path, "lstat", return_value=SimpleNamespace(
                    st_uid=uid, st_mode=mode
                )) as metadata:
                    self.assertFalse(diagnostic.runner_tool_present("/usr/sbin/nft"))
                    metadata.assert_called_once()
        with patch.object(diagnostic.Path, "resolve", side_effect=FileNotFoundError):
            self.assertFalse(diagnostic.runner_tool_present("/usr/sbin/nft"))

    def test_absence_phrases_are_observations_not_guessed_success(self):
        result = diagnostic.Capture("completed", 113, b"",
            b'Could not find service "com.arxcovo.arxd" in domain for system')
        row = diagnostic.summarize("arxcovo", "system/com.arxcovo.arxd", result, "darwin")
        self.assertEqual(row["exit_code"], 113)
        self.assertTrue(row["missing_service_phrase"])
        self.assertTrue(row["missing_target_named"])
        self.assertEqual(row["state"], "absent")

    def test_system_word_alone_does_not_prove_domain_phrase(self):
        result = diagnostic.Capture("completed", 113, b"", b"system error")
        row = diagnostic.summarize("arxcovo", "system/com.arxcovo.arxd", result, "darwin")
        self.assertFalse(row["system_domain_phrase"])

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
