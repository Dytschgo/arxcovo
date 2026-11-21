"""Pure A10 orchestration tests. No subprocess or provisioning."""
import errno
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from contextlib import ExitStack, redirect_stdout
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("executor_probe", Path(__file__).with_name("executor-probe.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class ExecutorProbeTests(unittest.TestCase):
    def test_unit_has_narrow_rights_and_delegation(self):
        argv = [str(arg) for arg in probe.start_argv(Path("/var/lib/arx-a10-1-1"))]
        for option in (
            "--unit=arxd.service", "--property=Delegate=memory pids",
            "--property=DelegateSubgroup=supervisor", "--property=NoNewPrivileges=yes",
            "--property=CapabilityBoundingSet=CAP_CHOWN CAP_SETGID CAP_SETUID",
            "--property=ReadOnlyPaths=/sys/fs/cgroup",
            "--property=ReadWritePaths=/sys/fs/cgroup/system.slice/arxd.service",
        ):
            self.assertIn(option, argv)
        self.assertNotIn("--property=ProtectControlGroups=yes", argv)

    def test_cargo_witness_does_not_require_python_on_runner_path(self):
        with Path(probe.__file__).open(encoding="utf-8") as source:
            self.assertEqual(source.readline().strip(), "#!/usr/bin/python3 -I")

    def test_denial_requires_access_error(self):
        def raises(code):
            raise OSError(code, "fixture")

        for code in (errno.EPERM, errno.EACCES, errno.EROFS):
            probe.denied(lambda: raises(code))
        with self.assertRaisesRegex(probe.Refusal, "unexpected-denial"):
            probe.denied(lambda: raises(errno.ENOENT))
        with self.assertRaisesRegex(probe.Refusal, "access-permitted"):
            probe.denied(lambda: None)

    def exercise(self, failing=None, cleanup_error=False):
        log = []
        out = io.StringIO()

        def record(label, value=None):
            def observed(*_args, **_kwargs):
                log.append(label)
                if failing == label:
                    raise probe.Refusal(label)
                return value
            return observed

        def stop(*_args, **_kwargs):
            occurrence = log.count("stop") + 1
            log.append("stop")
            if failing == "stop" and occurrence == 1:
                raise probe.Refusal("preflight-stop")
            if cleanup_error and occurrence > 1:
                raise probe.Refusal("cleanup-fixture")

        def command(args, *_rest, **_kwargs):
            if "--property=Version" in args:
                return "255.4\n"
            if "--identity" in args:
                return record("identity", 'A10 {"probes":1,"uid":21072,"gid":21072,"groups":[],"visible":true,"denied_controls":4,"inaccessible_directories":1}\n')()
            return ""

        replacements = {
            "ancestry": lambda _path: [],
            "role_unused": lambda *_: True,
            "properties": lambda _d: {"LoadState": "not-found"},
            "read": lambda *_a, **_k: "systemd",
            "command": command,
            "role": lambda *_: None,
            "stage_tree": record("stage"),
            "start": record("start"),
            "wait_ready": record("ready", 20),
            "peer_controls": record("peer"),
            "stop": stop,
            "journal": lambda *_: [],
            "prove_build": record("build"),
            "UNIT": Path("/nonexistent-a10-unit"),
        }
        with tempfile.TemporaryDirectory() as scratch, ExitStack() as stack:
            for name, value in replacements.items():
                stack.enter_context(patch.object(probe, name, value))
            stack.enter_context(patch.object(probe.os, "uname", return_value=type("Uname", (), {"release": "6.17.0"})(), create=True))
            stack.enter_context(redirect_stdout(out))
            result = probe.qualify(Path(scratch) / "owned", Path(scratch), None, 0)
        records = [probe.json.loads(line.removeprefix("A10 ")) for line in out.getvalue().splitlines()]
        return result, log, records

    def test_owned_stop_is_not_reissued_after_uncertain_result(self):
        with ExitStack() as stack:
            stack.enter_context(patch.object(probe, "properties", return_value={
                "LoadState": "loaded", "Description": "fixture", "ActiveState": "active",
            }))
            command = stack.enter_context(patch.object(probe, "command", side_effect=probe.Refusal("unit-stop")))
            requested = []
            with self.assertRaisesRegex(probe.Refusal, "unit-stop"):
                probe.stop(Path("fixture"), 0, 0, requested)
            with self.assertRaisesRegex(probe.Refusal, "unit-close-unconfirmed"):
                probe.stop(Path("fixture"), 0, 0, requested)
        command.assert_called_once()

    def test_failed_control_restoration_retains_primary_error(self):
        out = io.StringIO()

        def restore():
            raise probe.Refusal("restore-failed")

        with redirect_stdout(out), self.assertRaisesRegex(probe.Refusal, "primary"):
            with probe.restore_after(restore):
                raise probe.Refusal("primary")
        self.assertIn('"category": "restore-failed"', out.getvalue())

    def test_build_follows_zero_dispatch_preflight_and_drain(self):
        code, calls, records = self.exercise()
        self.assertEqual(code, 0)
        self.assertEqual(calls, ["stage", "start", "ready", "identity", "peer", "stop", "start", "ready", "build", "stop"])
        preflight = next(record for record in records if record["event"] == "preflight")
        self.assertEqual(preflight["runner_dispatches"], 0)
        self.assertEqual(preflight["identity_probes"], 1)
        self.assertEqual(records[-1]["dispatch_attempts"], 1)

    def test_preflight_failures_submit_no_build(self):
        for stage in ("stage", "start", "ready", "identity", "peer", "stop"):
            with self.subTest(stage=stage):
                code, calls, records = self.exercise(stage)
                self.assertEqual(code, 1)
                self.assertNotIn("build", calls)
                self.assertEqual(records[-1]["dispatch_attempts"], 0)

    def test_cleanup_preserves_original_refusal(self):
        code, calls, records = self.exercise("build", cleanup_error=True)
        self.assertEqual(code, 1)
        self.assertEqual(calls.count("build"), 1)
        self.assertEqual(records[-1]["first"], "build")
        self.assertEqual(records[-1]["cleanup"], "cleanup-fixture")
