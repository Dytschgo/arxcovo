"""Pure A10 orchestration tests. No subprocess or provisioning."""
import errno
import importlib.util
import io
import stat
from types import SimpleNamespace
from pathlib import Path
import tempfile
import unittest
from contextlib import ExitStack, redirect_stdout
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("executor_probe", Path(__file__).with_name("executor-probe.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

RECEIPT = dict(zip(probe.PROVENANCE_FIELDS, ("42", "1", "a" * 40, "ubuntu26", "20261121", "rust")))


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

    def test_each_required_provenance_field_refuses_before_staging(self):
        for field in probe.PROVENANCE_FIELDS:
            with self.subTest(field=field), patch.object(probe, "stage_tree") as stage:
                receipt = {key: value for key, value in RECEIPT.items() if key != field}
                with self.assertRaisesRegex(probe.Refusal, "provenance"):
                    probe.qualify(Path("unused"), Path("unused"), None, 0, receipt)
                stage.assert_not_called()

    def test_cgroup_setup_requires_each_control_and_group_observation(self):
        values = {}
        for group in (probe.UNIT, probe.RUNS):
            values.update({
                group / "cgroup.type": "domain\n", group / "cgroup.procs": "",
                group / "cgroup.controllers": "memory pids\n",
                group / "cgroup.subtree_control": "memory pids\n",
                **{group / item: "max\n" for item in probe.CONTROL_LIMITS},
            })
        values[probe.RUNS / "cgroup.events"] = "populated 0\n"
        controls = {group / item for group in (probe.UNIT, probe.RUNS) for item in probe.CONTROL_LIMITS}
        controls.add(probe.RUNS / "cgroup.kill")

        def read(path, *_args):
            if path not in values:
                raise FileNotFoundError
            return values[path]

        def show_facts(missing_control=None):
            def control_mode(path, *_args):
                self.assertIn(path, controls)
                if path == missing_control:
                    raise FileNotFoundError
                return {"uid": 0, "mode": "0644"}

            with patch.object(probe, "read", side_effect=read), patch.object(probe, "control_mode", side_effect=control_mode):
                return probe.cgroup_facts()

        facts = show_facts()
        self.assertEqual(facts["unit"]["procs"], [])
        self.assertEqual(facts["runs"]["populated"], 0)
        self.assertIn("cgroup.kill", facts["runs"]["controls"])
        for path in list(values):
            with self.subTest(missing=str(path)):
                original = values.pop(path)
                with self.assertRaises(FileNotFoundError):
                    show_facts()
                values[path] = original
        for path in controls:
            with self.subTest(missing_control=str(path)), self.assertRaises(FileNotFoundError):
                show_facts(path)
        for file, value in (("cgroup.type", "threaded"), ("cgroup.procs", "123\n")):
            with self.subTest(invalid=file), patch.dict(values, {probe.UNIT / file: value}):
                with self.assertRaisesRegex(probe.Refusal, "cgroup-domain-state"):
                    show_facts()

    def test_missing_daemon_objects_and_wrap_descriptor_refuse(self):
        root = Path("/fixture")
        file = SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_uid=probe.DAEMON,
                               st_gid=probe.DAEMON, st_dev=2, st_ino=3)
        objects = {root / name: SimpleNamespace(
            st_uid=probe.DAEMON, st_gid=probe.DAEMON, st_dev=file.st_dev, st_ino=file.st_ino,
            st_mode=mode | 0o600,
        ) for (name, _), mode in zip(probe.DAEMON_OBJECTS, (
            stat.S_IFREG, stat.S_IFREG, stat.S_IFDIR, stat.S_IFSOCK,
        ))}
        def lstat(path):
            if path not in objects:
                raise FileNotFoundError
            return objects[path]

        with patch.object(Path, "lstat", lstat), patch.object(Path, "stat", autospec=True, return_value=file) as follow:
            with patch.object(probe, "read", return_value="200"):
                self.assertEqual(probe.daemon_evidence(root, 42), "200")
                follow.assert_called_once_with(Path("/proc") / "42/fd/200")
                for path in list(objects):
                    with self.subTest(missing=str(path)):
                        original = objects.pop(path)
                        with self.assertRaises(FileNotFoundError):
                            probe.daemon_evidence(root, 42)
                        objects[path] = original
                follow.return_value = SimpleNamespace(st_mode=stat.S_IFREG, st_dev=2, st_ino=99)
                with self.assertRaisesRegex(probe.Refusal, "wrap-fd-target"):
                    probe.daemon_evidence(root, 42)
                follow.side_effect = FileNotFoundError
                with self.assertRaises(FileNotFoundError):
                    probe.daemon_evidence(root, 42)
            with patch.object(probe, "read", side_effect=FileNotFoundError), self.assertRaises(FileNotFoundError):
                probe.daemon_evidence(root, 42)

    def test_whole_fd_inventory_requires_standard_streams_and_checks_high_fds(self):
        from contextlib import nullcontext
        for names, failure in (("0 1", "fd-inventory"), ("0 1 2 200", "inherited-descriptor")):
            with self.subTest(names=names):
                scan = nullcontext(iter(SimpleNamespace(name=name) for name in names.split()))
                fcntl = SimpleNamespace(F_GETFD=1, fcntl=lambda *_: 0)
                with patch.object(probe.os, "scandir", return_value=scan), patch.object(probe, "fcntl", fcntl, create=True):
                    with self.assertRaisesRegex(probe.Refusal, failure):
                        probe.verify_child_fds()

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
            result = probe.qualify(Path(scratch) / "owned", Path(scratch), None, 0, RECEIPT)
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
        for field, value in RECEIPT.items():
            self.assertEqual(records[-1][field], value)

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
