"""Ten safe D7h tests: all package reads, privilege and Linux ports are fake."""

import ast
from contextlib import ExitStack, redirect_stdout
import hashlib
import importlib.util
import io
import json
from pathlib import Path, PurePosixPath
import stat
import types
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).absolute().parents[2]
SPEC = importlib.util.spec_from_file_location("d7h", ROOT / "tools/ci/sudo-package-readonly.py")
W = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(W)
LOAD_COMPARATOR = W._load_comparator
SHA = "a" * 40  # Synthetic gate value, never an actual candidate claim.
ENV = {
    "CI": "true", "GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
    "RUNNER_OS": "Linux", "RUNNER_ARCH": "X64", "ImageOS": "ubuntu24",
    "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REPOSITORY": "Dytschgo/arxcovo",
    "GITHUB_ACTOR": "Dytschgo", "GITHUB_TRIGGERING_ACTOR": "Dytschgo",
    "GITHUB_REF": "refs/heads/main", "GITHUB_RUN_ATTEMPT": "1",
    "GITHUB_WORKFLOW_REF": "Dytschgo/arxcovo/.github/workflows/sudo-package-readonly.yml@refs/heads/main",
    "GITHUB_SHA": SHA, "SUDO_PACKAGE_REVIEWED_SHA": SHA,
    "GITHUB_WORKFLOW_SHA": SHA, "GITHUB_RUN_ID": "123", "GITHUB_WORKSPACE": "/repo",
}


def result(match=True):
    return {"schema": 1, "status": "candidate_match" if match else "refused",
            "candidate_match": match, "qualified": False, "revision": W._REVISION,
            "architecture": "amd64",
            "digests": dict(W._DIGESTS) if match else dict.fromkeys(W._DIGESTS)}


class Tests(unittest.TestCase):
    def setUp(self):
        self.stack = ExitStack()
        self.addCleanup(self.stack.close)
        for target, value in (("Path", PurePosixPath),
                              ("__file__", "/repo/tools/ci/sudo-package-readonly.py")):
            self.stack.enter_context(patch.object(W, target, value))
        self.stack.enter_context(patch.dict(W.os.environ, ENV, clear=True))
        self.stack.enter_context(patch.object(W.sys, "platform", "linux"))
        self.stack.enter_context(patch.object(W.sys, "argv", [W.__file__]))
        for name in ("getuid", "geteuid"):
            self.stack.enter_context(patch.object(W.os, name, return_value=0, create=True))
        self.reader = Mock(return_value=result())
        self.loader = self.stack.enter_context(patch.object(
            W, "_load_comparator", return_value=types.SimpleNamespace(_read_linux=self.reader)))

    def invoke(self):
        out = io.StringIO()
        with redirect_stdout(out):
            code = W._main()
        self.assertEqual(len(out.getvalue().splitlines()), 1)
        return code, json.loads(out.getvalue())

    def denied(self):
        self.loader.reset_mock()
        self.reader.reset_mock()
        self.assertEqual(self.invoke(), (2, {"schema": 1, "status": "refused"}))
        self.loader.assert_not_called()
        self.reader.assert_not_called()

    def test_01_gate_every_required_environment_value(self):
        for key in ENV:
            with self.subTest(key=key), patch.dict(W.os.environ, {key: "PRIVATE"}):
                self.denied()

    def test_02_triple_and_old_variable_has_no_authority(self):
        for key in ("GITHUB_SHA", "GITHUB_WORKFLOW_SHA", "SUDO_PACKAGE_REVIEWED_SHA"):
            with patch.dict(W.os.environ, {key: "b" * 40}):
                self.denied()
        with patch.dict(W.os.environ, {"UNIX_EXECUTION_REVIEWED_SHA": SHA}):
            del W.os.environ["SUDO_PACKAGE_REVIEWED_SHA"]
            self.denied()

    def test_03_fresh_host_attempt_and_run_bounds(self):
        for key, values in {
            "GITHUB_RUN_ATTEMPT": ("2", "01", ""),
            "GITHUB_RUN_ID": ("0", "01", "1\n", "1" * 21, ""),
            "RUNNER_ENVIRONMENT": ("self-hosted", ""),
        }.items():
            for value in values:
                with self.subTest(key=key, value=value), patch.dict(W.os.environ, {key: value}):
                    self.denied()

    def test_04_os_uid_argv_and_script_binding(self):
        for name in ("getuid", "geteuid"):
            with patch.object(W.os, name, return_value=1001):
                self.denied()
        with patch.object(W.sys, "platform", "win32"):
            self.denied()
        with patch.object(W.sys, "argv", ["script", "override"]):
            self.denied()
        with patch.object(W, "__file__", "/other/tools/ci/sudo-package-readonly.py"):
            self.denied()
        for workspace in ("repo", "/repo/../repo", "/repo/", "//repo"):
            with patch.dict(W.os.environ, {"GITHUB_WORKSPACE": workspace}):
                self.denied()

    def test_05_success_is_bound_and_not_qualified(self):
        code, payload = self.invoke()
        self.assertEqual(code, 0)
        self.assertEqual(payload, {"schema": 1, "binding": {
            "source_sha": SHA, "workflow_sha": SHA, "run": "123", "attempt": "1"},
            "comparison": result()})
        self.loader.assert_called_once_with()
        self.reader.assert_called_once_with()

    def test_06_known_refusal_is_bound_without_hash_oracle(self):
        self.reader.return_value = result(False)
        code, payload = self.invoke()
        self.assertEqual(code, 2)
        self.assertEqual(payload["comparison"], result(False))

    def test_07_complete_schema_rejects_private_or_partial_data(self):
        mutants = [None, [], {"PRIVATE": "secret"}]
        for key in result():
            value = result()
            del value[key]
            mutants.append(value)
        for key, replacement in (("schema", True), ("status", "PRIVATE"),
                                 ("qualified", True), ("candidate_match", 1),
                                 ("architecture", "arm64"), ("revision", "PRIVATE"),
                                 ("digests", {"sudo": "PRIVATE"})):
            value = result()
            value[key] = replacement
            mutants.append(value)
        value = result()
        value["extra"] = "PRIVATE"
        mutants.append(value)
        value = result(False)
        value["digests"] = dict(W._DIGESTS)
        mutants.append(value)
        for value in mutants:
            self.reader.return_value = value
            self.assertEqual(self.invoke(), (2, {"schema": 1, "status": "refused"}))

    def test_08_errors_never_print_partial_or_exception_payload(self):
        self.loader.side_effect = RuntimeError("PRIVATE import")
        self.assertEqual(self.invoke(), (2, {"schema": 1, "status": "refused"}))
        self.reader.assert_not_called()
        self.loader.side_effect = None
        self.reader.side_effect = RuntimeError("PRIVATE read")
        self.assertEqual(self.invoke(), (2, {"schema": 1, "status": "refused"}))

    def test_09_bounded_fixed_source_ports_and_hash_before_exec(self):
        source = (ROOT / "tools/unix-exec-validation/sudo_package_profile.py").read_bytes()
        self.assertEqual((len(source), hashlib.sha256(source).hexdigest()),
                         (W._MODULE_SIZE, W._MODULE_SHA256))
        with patch.object(W, "_module_bytes", return_value=source):
            comparator = LOAD_COMPARATOR()
        self.assertTrue(callable(comparator._read_linux))  # Never invoked.
        self.assertEqual(comparator.compare(None), result(False))
        info = types.SimpleNamespace(st_mode=stat.S_IFREG | 0o644, st_nlink=1,
                                     st_size=len(source), st_dev=1, st_ino=2,
                                     st_uid=1001, st_gid=1001,
                                     st_mtime_ns=10, st_ctime_ns=20, st_atime_ns=30)
        for data, metadata, expected in ((source, info, True),
                                        (b"X" + source[1:], info, False),
                                        (source[:-1], info, False),
                                        (source + b"X", info, False),
                                        (source, types.SimpleNamespace(
                                            st_mode=stat.S_IFLNK, st_nlink=1,
                                            st_size=len(source)), False)):
            stream = io.BytesIO(data)
            def read(fd, count):
                self.assertLessEqual(count, 4096)
                self.assertGreater(count, 0)
                return stream.read(count)
            with ExitStack() as ports:
                for name, flag in (("O_NOFOLLOW", 131072), ("O_DIRECTORY", 65536),
                                   ("O_CLOEXEC", 524288), ("O_NONBLOCK", 2048)):
                    ports.enter_context(patch.object(W.os, name, flag, create=True))
                opened = ports.enter_context(patch.object(W.os, "open", side_effect=range(10, 30)))
                ports.enter_context(patch.object(W.os, "fstat", return_value=metadata))
                ports.enter_context(patch.object(W.os, "read", side_effect=read))
                closed = ports.enter_context(patch.object(W.os, "close"))
                if expected:
                    self.assertEqual(W._module_bytes(), source)
                else:
                    with self.assertRaises(ValueError):
                        W._module_bytes()
                self.assertEqual(closed.call_count, opened.call_count)
                self.assertEqual(opened.call_args.args[0], "sudo_package_profile.py")
                for call in opened.call_args_list:
                    self.assertTrue(call.args[1] & W.os.O_NOFOLLOW)
                self.assertEqual(closed.call_args_list[0].args[0],
                                 10 + opened.call_count - 1)
                # Read/metadata/close errors cannot leak descriptors or bytes.
                for fault in ("read", "drift", "close"):
                    stream.seek(0)
                    opened.reset_mock(side_effect=True)
                    opened.side_effect = iter(range(10, 30))
                    closed.reset_mock(side_effect=True)
                    changed = types.SimpleNamespace(st_mode=0, st_nlink=0, st_size=0)
                    with patch.object(W.os, "fstat", side_effect=[info, changed]
                                      if fault == "drift" else None, return_value=info), \
                         patch.object(W.os, "read", side_effect=RuntimeError("PRIVATE")
                                      if fault == "read" else read):
                        if fault == "close":
                            closed.side_effect = [OSError("PRIVATE")] + [None] * 19
                        with self.assertRaises(Exception):
                            W._module_bytes()
                    self.assertEqual(closed.call_count, opened.call_count)

    def test_11_module_read_atime_only_and_identity_drift(self):
        source = (ROOT / "tools/unix-exec-validation/sudo_package_profile.py").read_bytes()
        values = dict(st_mode=stat.S_IFREG | 0o644, st_nlink=1, st_size=len(source),
                      st_dev=1, st_ino=2, st_uid=1001, st_gid=1001,
                      st_mtime_ns=10, st_ctime_ns=20, st_atime_ns=30)
        for field in ("st_atime_ns", "st_dev", "st_ino", "st_uid", "st_gid",
                      "st_mode", "st_nlink", "st_size", "st_mtime_ns", "st_ctime_ns"):
            before = types.SimpleNamespace(**values)
            after = types.SimpleNamespace(**{**values, field: values[field] + 1})
            stream = io.BytesIO(source)
            with self.subTest(field=field), ExitStack() as ports:
                for name, flag in (("O_NOFOLLOW", 131072), ("O_DIRECTORY", 65536),
                                   ("O_CLOEXEC", 524288), ("O_NONBLOCK", 2048)):
                    ports.enter_context(patch.object(W.os, name, flag, create=True))
                opened = ports.enter_context(patch.object(W.os, "open", side_effect=range(10, 30)))
                ports.enter_context(patch.object(W.os, "fstat", side_effect=[before, after]))
                ports.enter_context(patch.object(W.os, "read", side_effect=lambda fd, n: stream.read(n)))
                closed = ports.enter_context(patch.object(W.os, "close"))
                if field == "st_atime_ns":
                    self.assertEqual(W._module_bytes(), source)
                else:
                    with self.assertRaises(ValueError):
                        W._module_bytes()
                self.assertEqual(closed.call_count, opened.call_count)

    def test_10_workflow_and_callgraph_fences(self):
        source = (ROOT / "tools/ci/sudo-package-readonly.py").read_text(encoding="utf-8")
        tree = ast.parse(source)
        main = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "_main")
        calls = [n.func.id for n in ast.walk(main) if isinstance(n, ast.Call)
                 and isinstance(n.func, ast.Name)]
        self.assertLess(calls.index("_gate"), calls.index("_load_comparator"))
        self.assertEqual(source.count("comparator._read_linux()"), 1)
        self.assertNotIn("UNIX_EXECUTION_REVIEWED_SHA", source)
        workflow = (ROOT / ".github/workflows/sudo-package-readonly.yml").read_text(encoding="utf-8")
        for forbidden in ("UNIX_EXECUTION_REVIEWED_SHA", "secrets.", "upload-artifact", "inputs:",
                          "pull_request:", "push:", "schedule:", "sudo -V", "driver.py"):
            self.assertNotIn(forbidden, workflow)
        for required in ("timeout-minutes: 3", "runs-on: ubuntu-24.04", "contents: read",
                         "persist-credentials: false", "sudo -n /usr/bin/env -i",
                         "actions/checkout@11d5960a326750d5838078e36cf38b85af677262"):
            self.assertIn(required, workflow)
        self.assertLess(workflow.index("test-sudo-package-readonly.py"), workflow.index("sudo -n"))


if __name__ == "__main__":
    unittest.main()
