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
        dpkg = "/usr/bin/dpkg-query-real"

        def run(argv, timeout=None):
            calls.append((argv, timeout))
            return diagnostic.Capture("completed", 0, b"255.4-1ubuntu8.18", b"private")

        vetted = {path: path for path in diagnostic.R2_TOOLS}
        vetted.update({"/usr/sbin/nft": None, "/usr/bin/dpkg-query": dpkg})
        with patch.object(diagnostic.os, "uname", return_value=SimpleNamespace(
            release="7.0.0-1014-aws"), create=True), patch.object(
            diagnostic, "vetted_runner_tool", side_effect=vetted.__getitem__
        ) as tools:
            encoded = diagnostic.observe("linux", run, self.R2_ENV)
        self.assertEqual(calls, [
            (["/usr/bin/systemctl", *diagnostic.SYSTEMD_ARGS], None),
            ([dpkg, "-W", "-f=${Version}", "systemd"], 1),
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
                with patch.object(diagnostic, "vetted_runner_tool") as tools:
                    encoded = diagnostic.observe("linux", lambda _: diagnostic.Capture("unavailable"), env)
                tools.assert_not_called()
                self.assertNotIn("r2_runner", json.loads(encoded))
        with patch.object(diagnostic.os, "uname", return_value=SimpleNamespace(
            release="7.private\nvalue"), create=True), patch.object(
            diagnostic, "vetted_runner_tool", side_effect=lambda path: path
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
            diagnostic, "vetted_runner_tool", side_effect=lambda path: (
                None if path == "/usr/bin/dpkg-query" else path
            )
        ):
            def unexpected_query(*args, **kwargs):
                self.fail("unvetted package executable was invoked")

            receipt = diagnostic.r2_runner_receipt(self.R2_ENV, unexpected_query)
        self.assertFalse(receipt["tools"]["/usr/bin/dpkg-query"])
        self.assertIsNone(receipt["systemd_package"])

    def test_r2_tool_metadata_refuses_unsafe_locations_and_permissions(self):
        system = diagnostic.Path("/usr/sbin/nft")
        with patch.object(diagnostic.Path, "lstat") as metadata:
            self.assertIsNone(diagnostic.vetted_runner_tool("/tmp/nft"))
            metadata.assert_not_called()
        for uid, mode, expected in [(0, 0o100755, "/usr/sbin/nft"), (1001, 0o100755, None),
                                    (0, 0o100775, None), (0, 0o100644, None),
                                    (0, 0o100001, None), (0, 0o040755, None)]:
            ancestry = [SimpleNamespace(st_uid=0, st_mode=0o040755)] * len(system.parents)
            with patch.object(diagnostic.Path, "lstat", side_effect=[
                *ancestry, SimpleNamespace(st_uid=uid, st_mode=mode)
            ]):
                self.assertEqual(diagnostic.vetted_runner_tool("/usr/sbin/nft"), expected)
        for uid, mode in [(0, 0o040777), (1001, 0o040755), (0, 0o120755)]:
            with patch.object(diagnostic.Path, "lstat", return_value=SimpleNamespace(
                st_uid=uid, st_mode=mode
            )) as metadata:
                self.assertIsNone(diagnostic.vetted_runner_tool("/usr/sbin/nft"))
                metadata.assert_called_once()
        with patch.object(diagnostic.Path, "lstat", side_effect=FileNotFoundError):
            self.assertIsNone(diagnostic.vetted_runner_tool("/usr/sbin/nft"))

    def test_r2_tool_checks_every_symlink_hop_and_bounds_cycles(self):
        path = diagnostic.Path
        literal = path("/usr/sbin/nft")
        alias = path("/usr/bin/nft-alias")
        target = path("/usr/bin/nft-real")
        directory = SimpleNamespace(st_uid=0, st_mode=0o040755)
        symlink = SimpleNamespace(st_uid=0, st_mode=0o120777)
        executable = SimpleNamespace(st_uid=0, st_mode=0o100755)
        entries = dict.fromkeys((*literal.parents, *target.parents), directory)
        entries.update({literal: symlink, alias: symlink, target: executable})
        links = {literal: alias.as_posix(), alias: target.name}
        with patch.object(path, "lstat", autospec=True, side_effect=entries.__getitem__), \
             patch.object(diagnostic.os, "readlink", side_effect=links.__getitem__) as readlink:
            self.assertEqual(diagnostic.vetted_runner_tool(literal), target.as_posix())
            links[alias] = "/tmp/private-alias"
            self.assertIsNone(diagnostic.vetted_runner_tool(literal))
            links[alias] = target.name
            entries[alias] = SimpleNamespace(st_uid=1001, st_mode=0o120777)
            self.assertIsNone(diagnostic.vetted_runner_tool(literal))
            entries[alias] = symlink
            entries[alias.parent] = SimpleNamespace(st_uid=0, st_mode=0o040777)
            self.assertIsNone(diagnostic.vetted_runner_tool(literal))
            entries[alias.parent] = directory
            links[alias] = literal.as_posix()
            readlink.reset_mock()
            self.assertIsNone(diagnostic.vetted_runner_tool(literal))
            self.assertEqual(readlink.call_count, 8)

    def test_r2_package_query_refuses_a_raw_link_that_requires_a_directory(self):
        path = diagnostic.Path
        literal = path("/usr/bin/dpkg-query")
        target = path("/usr/bin/dpkg-query-real")
        directory = SimpleNamespace(st_uid=0, st_mode=0o040755)
        entries = dict.fromkeys(literal.parents, directory)
        entries[literal] = SimpleNamespace(st_uid=0, st_mode=0o120777)
        entries[target] = SimpleNamespace(st_uid=0, st_mode=0o100755)

        def metadata(candidate):
            if candidate not in entries:
                raise FileNotFoundError
            return entries[candidate]

        for suffix in ("/", "/."):
            with self.subTest(suffix=suffix), patch.object(
                path, "lstat", autospec=True, side_effect=metadata
            ), patch.object(diagnostic.os, "readlink", return_value=target.as_posix() + suffix), \
                 patch.object(diagnostic.os, "uname", return_value=SimpleNamespace(
                     release="7.0.0-1014-aws"
                 ), create=True), patch.object(diagnostic, "capture") as query:
                receipt = diagnostic.r2_runner_receipt(self.R2_ENV, query)
            self.assertFalse(receipt["tools"][literal.as_posix()])
            self.assertIsNone(receipt["systemd_package"])
            query.assert_not_called()

    def test_r2_ip_accepts_only_the_vetted_ubuntu_merged_usr_alias(self):
        path = diagnostic.Path
        literal, alias, target = map(path, ("/usr/sbin/ip", "/bin", "/usr/bin/ip"))
        directory = SimpleNamespace(st_uid=0, st_mode=0o040755)
        symlink = SimpleNamespace(st_uid=0, st_mode=0o120777)
        entries = dict.fromkeys((*literal.parents, *target.parents), directory)
        entries.update({literal: symlink, alias: symlink, target: SimpleNamespace(st_uid=0, st_mode=0o100755)})
        links = {literal: "/bin/ip", alias: "usr/bin"}
        with patch.object(path, "lstat", autospec=True, side_effect=entries.__getitem__), \
             patch.object(diagnostic.os, "readlink", side_effect=links.__getitem__):
            self.assertEqual(diagnostic.vetted_runner_tool(literal), target.as_posix())
            links[alias] = "usr/bin/."
            self.assertIsNone(diagnostic.vetted_runner_tool(literal))
            links[alias] = "usr/bin"
            entries[alias] = SimpleNamespace(st_uid=1001, st_mode=0o120777)
            self.assertIsNone(diagnostic.vetted_runner_tool(literal))

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
