"""Pure tests for the fixed C13 launcher."""
from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch

SPEC = importlib.util.spec_from_file_location("c13_run", Path(__file__).with_name("run.py"))
RUN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUN)


class LauncherTests(unittest.TestCase):
    def fixture_campaign(self, code, capture, diagnostics, role="removed"):
        stage = MagicMock(is_symlink=lambda: False)
        stage.stat.return_value.st_uid = 23
        output = io.StringIO()
        with (patch.object(RUN, "qualify_runner"),
              patch.object(RUN.os, "geteuid", return_value=0, create=True),
              patch.dict(RUN.os.environ, {"SUDO_UID": "23"}),
              patch.object(RUN, "Path", return_value=stage),
              patch.object(stage, "parent", stage),
              patch.object(stage, "name", "arx-c13-abcdefgh"),
              patch.object(RUN, "unused", return_value=True),
              patch.object(RUN, "command") as command,
              patch.object(RUN.tempfile, "mkdtemp", return_value="/tmp/arx-c13-fixture"),
              patch.object(RUN, "stage_tree"),
              patch.object(RUN, "stage_binary"),
              patch.object(RUN, "primary_ip", return_value="10.1.0.22"),
              patch.object(RUN, "run_campaign", return_value=(code, capture, diagnostics)),
              patch.object(RUN, "remove_role", return_value=role) as remove_role,
              patch.object(RUN.time, "monotonic", return_value=10),
              redirect_stdout(output)):
            status = RUN.privileged("/tmp/arx-c13-abcdefgh", 178)
        return status, output.getvalue(), remove_role.call_count, command.call_count

    def optional_pipe(self, write_effect=None):
        from contextlib import ExitStack

        stack = self.enterContext(ExitStack())
        stack.enter_context(redirect_stderr(MagicMock(**{"fileno.return_value": 10})))
        metadata = SimpleNamespace(st_mode=RUN.stat.S_IFIFO, st_dev=1, st_ino=2)
        stack.enter_context(patch.object(RUN.os, "O_NONBLOCK", 0x800, create=True))
        stack.enter_context(patch.object(RUN.os, "O_CLOEXEC", 0x80000, create=True))
        stack.enter_context(patch.object(RUN.os, "fstat", return_value=metadata))
        open_pipe = stack.enter_context(patch.object(RUN.os, "open", return_value=11))
        write = stack.enter_context(patch.object(RUN.os, "write", side_effect=write_effect))
        close = stack.enter_context(patch.object(RUN.os, "close"))
        return open_pipe, write, close

    def test_selection_is_exact(self):
        listed = f"{RUN.SELECTOR}: test\n\n1 test, 0 benchmarks\n"
        self.assertTrue(RUN.exact_listing(listed, RUN.SELECTOR))
        for invalid in ("", listed + listed, listed.replace(RUN.SELECTOR, RUN.PROBE)):
            self.assertFalse(RUN.exact_listing(invalid, RUN.SELECTOR))

    def test_hosted_gate_is_complete(self):
        env = dict(GITHUB_ACTIONS="true", RUNNER_ENVIRONMENT="github-hosted", RUNNER_OS="Linux")
        with patch.object(RUN.sys, "platform", "linux"):
            with patch.dict(RUN.os.environ, env, clear=True):
                RUN.qualify_runner()
            for name in env:
                with patch.dict(RUN.os.environ, {**env, name: "wrong"}, clear=True):
                    with self.assertRaises(ValueError):
                        RUN.qualify_runner()

    def test_primary_ip_uses_route_source_despite_docker_bridge(self):
        route = [{"dev": "eth0", "prefsrc": "10.1.0.22"}]
        eth0 = {"ifname": "eth0", "flags": ["UP", "LOWER_UP"],
                "addr_info": [{"local": "10.1.0.22"}]}
        docker0 = {"ifname": "docker0", "flags": ["NO-CARRIER", "UP"],
                   "addr_info": [{"local": "172.17.0.1"}]}
        replies = [RUN.subprocess.CompletedProcess([], 0, stdout=json.dumps(data).encode())
                   for data in (route, [docker0, eth0])]
        with patch.object(RUN.time, "monotonic", return_value=10):
            with patch.object(RUN.subprocess, "run", side_effect=replies) as run:
                self.assertEqual(RUN.primary_ip(20), "10.1.0.22")
        self.assertEqual([call.args[0] for call in run.call_args_list], [
            ["/usr/sbin/ip", "-j", "-4", "route", "get", "192.0.2.1"],
            ["/usr/sbin/ip", "-j", "-4", "addr", "show", "scope", "global"],
        ])

    def test_primary_ip_refuses_route_link_without_carrier(self):
        route = [{"dev": "eth0", "prefsrc": "10.1.0.22"}]
        links = [{"ifname": "eth0", "flags": ["NO-CARRIER", "UP"],
                  "addr_info": [{"local": "10.1.0.22"}]},
                 {"ifname": "eth1", "flags": ["UP", "LOWER_UP"],
                  "addr_info": [{"local": "10.1.0.22"}]}]
        with patch.object(RUN, "ip_json", side_effect=[route, links]):
            with self.assertRaisesRegex(ValueError, "eligible runner link"):
                RUN.primary_ip(20)

    def test_primary_ip_refuses_non_private_route_source(self):
        with patch.object(RUN, "ip_json", return_value=[{"dev": "eth0", "prefsrc": "8.8.4.4"}]) as ip:
            with self.assertRaisesRegex(ValueError, "eligible runner address"):
                RUN.primary_ip(20)
        ip.assert_called_once()

    def test_privileged_launch_passes_fixed_runner_facts_as_arguments(self):
        stage = Path("/tmp/arx-c13-abcdefgh")
        with (patch.dict(RUN.os.environ, {"RUNNER_OS": "untrusted"}, clear=True),
              patch.object(RUN.time, "monotonic", return_value=10),
              patch.object(RUN.subprocess, "run", return_value=MagicMock(returncode=1)) as run):
            self.assertEqual(RUN.launch_privileged(stage, 20, 30), 1)
        run.assert_called_once_with(
            ["/usr/bin/sudo", "-n", "/usr/bin/env", "GITHUB_ACTIONS=true",
             "RUNNER_ENVIRONMENT=github-hosted", "RUNNER_OS=Linux",
             "/usr/bin/python3", "-I", "-B", Path(RUN.__file__).resolve(),
             "--staged", stage, "20000000"],
            env=RUN.ENV, stdin=RUN.subprocess.DEVNULL, check=False, timeout=18,
        )

    def test_failed_campaign_reports_first_record_and_reconciles_roles(self):
        join = "C13 first phase=join category=assertion"
        publication = "C13 first phase=publication category=assertion"
        join_cleanup = "C13 cleanup spawned=3 signaled=2 reaped=3 clean=2 helpers=17 helpers_reaped=17 helper_unknown=0"
        publication_cleanup = "C13 cleanup spawned=6 signaled=6 reaped=6 clean=6 helpers=93 helpers_reaped=93 helper_unknown=0"
        diagnostic = "C13 diagnostic node=a stage=admission-dispatch category=unavailable-store-wait"
        seal = "C13 diagnostic node=a stage=transfer-seal category=recheck-busy"
        node_busy = "C13 diagnostic node=a stage=transfer-seal category=offer-node-busy"
        read_busy = "C13 diagnostic node=a stage=transfer-seal category=offer-fresh-read-busy"
        misplaced = "C13 diagnostic node=c stage=transfer-seal category=offer-read"
        owner_exit = "C13 owner node=c action=join exit=8"
        _, write, _ = self.optional_pipe()
        for first, cleanup, remove in (
            (join, join_cleanup, True),
            (publication, publication_cleanup, True),
            (join + " at=m3_activation_linux.rs:1155 message=join-refused", join_cleanup, True),
            (publication + " at=publish_only_probe.rs:34 message=publication-refused",
             publication_cleanup, True),
            (join, join_cleanup.replace("reaped=3", "reaped=2"), False),
        ):
            with self.subTest(first=first, cleanup=cleanup):
                write.reset_mock()
                capture = RUN.Capture()
                capture.feed(
                    (f"fixture-private-output\n{first}\n{cleanup}\n{misplaced}\n{owner_exit}\n"
                     "C13 owner node=c action=join exit=256\n").encode("ascii")
                )
                diagnostics = (
                    f"{diagnostic}\n{seal}\n{node_busy}\n{read_busy}\n{cleanup}\n{first}\n"
                    f"{diagnostic} secret=fixture-private-output\n"
                    f"{diagnostic.replace('unavailable-store-wait', 'fixture-private-output')}\n"
                    f"{seal.replace('transfer-seal', 'admission-dispatch')}\n"
                    "C13 diagnostic node=a stage=transfer-seal category=seal"
                ).encode("ascii")
                status, output, roles, commands = self.fixture_campaign(101, capture, diagnostics)
                self.assertEqual(status, 1)
                self.assertIn(first + "\n", output)
                self.assertNotIn("C13 diagnostic", output)
                self.assertEqual([call.args for call in write.call_args_list], [
                    (11, f"{line}\n".encode("ascii"))
                    for line in (diagnostic, seal, node_busy, read_busy)
                ])
                self.assertIn(owner_exit + "\n", output)
                self.assertNotIn("exit=256", output)
                self.assertIn(f"roles_removed={8 if remove else 0} "
                              f"roles_retained={0 if remove else 8}", output)
                self.assertNotIn("fixture-private-output", output)
                self.assertEqual(roles, 8 if remove else 0)
                self.assertEqual(commands, 9 if remove else 8)

    def test_closed_stderr_preserves_success_and_teardown(self):
        capture = RUN.Capture()
        capture.feed((RUN.PIN + "\n"
                      "C13 cleanup spawned=6 signaled=6 reaped=6 clean=6 "
                      "helpers=93 helpers_reaped=93 helper_unknown=0\n").encode("ascii"))
        diagnostic = b"C13 diagnostic node=d stage=admission-worker category=busy\n"
        _, write, close = self.optional_pipe(BrokenPipeError)
        status, output, roles, commands = self.fixture_campaign(0, capture, diagnostic)
        self.assertEqual(status, 0)
        self.assertNotIn("quarantined", output)
        self.assertNotIn("::warning::", output)
        self.assertIn("fixture=removed roles_removed=8 roles_retained=0 roles_unknown=0", output)
        self.assertEqual((roles, commands), (8, 9))
        write.assert_called_once_with(11, diagnostic)
        close.assert_called_once_with(11)

    def test_full_stderr_drops_optional_output_on_independent_nonblocking_writer(self):
        diagnostic = b"C13 diagnostic node=d stage=admission-worker category=busy\n"
        open_pipe, write, close = self.optional_pipe(BlockingIOError)
        RUN.relay_diagnostics(diagnostic * 2)
        open_pipe.assert_called_once_with("/proc/self/fd/10", RUN.os.O_WRONLY | 0x80800)
        write.assert_called_once_with(11, diagnostic)
        close.assert_called_once_with(11)

    def test_campaign_diagnostic_read_failure_preserves_mandatory_capture(self):
        capture = RUN.Capture()
        capture.feed(b"C13 first phase=join category=assertion\n")
        with (patch.object(RUN.os, "O_NONBLOCK", 0x800, create=True),
              patch.object(RUN.os, "O_CLOEXEC", 0x80000, create=True),
              patch.object(RUN.os, "pipe2", return_value=(10, 11), create=True) as pipe,
              patch.object(RUN.os, "read", side_effect=BlockingIOError) as read,
              patch.object(RUN.os, "close") as close,
              patch.object(RUN, "bounded_run", return_value=(101, capture)) as run):
            self.assertEqual(RUN.run_campaign(["fixture"], env={}, timeout=1),
                             (101, capture, b""))
        pipe.assert_called_once_with(0x80800)
        run.assert_called_once_with(["fixture"], env={}, timeout=1, stderr=11)
        read.assert_called_once_with(10, RUN.OUTPUT_LIMIT)
        self.assertEqual([call.args for call in close.call_args_list], [(11,), (10,)])

    def test_workflow_pins_one_runner_only_step(self):
        workflow = (RUN.ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        step = (
            "      - name: Hosted C13 cluster process activation\n"
            "        if: runner.os == 'Linux'\n"
            "        timeout-minutes: 3\n"
            "        shell: bash\n"
            "        run: python3 -I -B tools/ci/m3-c13/run.py\n"
        )
        self.assertEqual(workflow.count(step), 1)
        self.assertEqual(workflow.count("tools/ci/m3-c13/run.py"), 1)



    def test_fixture_cleanup_distinguishes_reap_from_graceful_close(self):
        good = "C13 cleanup spawned=6 signaled=6 reaped=6 clean=6 helpers=18 helpers_reaped=18 helper_unknown=0"
        self.assertEqual(RUN.cleanup_disposition([good]), (True, True))
        unclean = good.replace("clean=6", "clean=5")
        self.assertEqual(RUN.cleanup_disposition([unclean]), (True, False))
        for lines in (
            [], [good, good], [good.replace("signaled=6", "signaled=5")],
            [good.replace("reaped=6", "reaped=5")],
            [good.replace("helpers_reaped=18", "helpers_reaped=17")],
            [good.replace("unknown=0", "unknown=1")],
        ):
            self.assertEqual(RUN.cleanup_disposition(lines), (False, False))


    def test_teardown_reserve_and_redacted_first_failure(self):
        with patch.object(RUN.time, "monotonic", return_value=100):
            self.assertEqual(RUN.remaining(103, 9), 3)
            with self.assertRaises(TimeoutError):
                RUN.remaining(100, 9)
    def test_first_record_survives_truncation_and_chunk_boundaries(self):
        first = b"C13 first phase=join category=assertion at=m3_activation_linux.rs:1155 message=join-refused"
        next_record = b"C13 first phase=close category=assertion"
        for oversized_prefix in (b"", b"x" * (RUN.OUTPUT_LIMIT + 20)):
            with self.subTest(truncated_before_record=bool(oversized_prefix)):
                capture = RUN.Capture()
                capture.feed(oversized_prefix)
                capture.feed(b"suffix\nC13 first phase=/tmp category=secret\n" + first[:15])
                capture.feed(first[15:] + b"\r\n" + next_record + b"\n")
                capture.feed(b"x" * RUN.OUTPUT_LIMIT)
                self.assertEqual(capture.first, first.decode("ascii"))
                self.assertTrue(capture.truncated)
                self.assertEqual(len(capture.bulk), RUN.OUTPUT_LIMIT)

    def test_partial_and_oversized_lines_do_not_produce_records(self):
        capture = RUN.Capture()
        first = b"C13 first phase=join category=pipe-io"
        capture.feed(b"x" * (RUN.FIRST_LIMIT + 1))
        capture.feed(first + b"\n" + first)
        self.assertIsNone(capture.first)
        capture.feed(b"\n")
        self.assertEqual(capture.first, first.decode("ascii"))

    def test_assertion_summary_rejects_unlisted_fields_and_payloads(self):
        valid = b"C13 first phase=publication category=assertion at=publish_only_probe.rs:34 message=publication-refused"
        for invalid in (
            valid.replace(b"publish_only_probe.rs", b"/tmp/publish_only_probe.rs"),
            valid.replace(b":34", b":99999999999"),
            valid.replace(b"message=publication-refused", b"message=fixture-private-output"),
            valid + b" proof=fixture-private-output",
            valid.replace(b"category=assertion", b"category=pipe-io"),
        ):
            with self.subTest(invalid=invalid):
                capture = RUN.Capture()
                capture.feed(invalid + b"\n")
                self.assertIsNone(capture.first)
                capture.feed(valid + b"\n")
                self.assertEqual(capture.first, valid.decode("ascii"))

    def test_timeout_preserves_first_failure_when_child_cannot_be_reaped(self):
        record = b"C13 first phase=join category=pipe-io\n"
        child = MagicMock()
        child.args = ["fixture"]
        child.wait.side_effect = RUN.subprocess.TimeoutExpired(child.args, RUN.REAP_GRACE)
        selector = MagicMock()
        selector.__enter__.return_value.select.side_effect = TimeoutError
        with (patch.object(RUN.subprocess, "Popen", return_value=child),
              patch.object(RUN.selectors, "DefaultSelector", return_value=selector),
              patch.object(RUN.time, "monotonic", side_effect=[10, 10, 10, 10.8]),
              patch.object(RUN.os, "set_blocking"),
              patch.object(RUN, "pipe_capacity", return_value=4096),
              patch.object(RUN.os, "read", side_effect=[record, BlockingIOError])):
            code, capture = RUN.bounded_run(child.args, env={}, timeout=1)
        self.assertIsNone(code)
        self.assertEqual(capture.first, record.decode("ascii").strip())
        child.kill.assert_called_once_with()
        selector.__enter__.return_value.select.assert_called_once_with(1 - RUN.REAP_GRACE)
        child.wait.assert_called_once()
        self.assertAlmostEqual(child.wait.call_args.kwargs["timeout"], 0.2)
        child.__exit__.assert_not_called()
        child.stdout.__exit__.assert_called_once()

    def evidence(self, first, cleanup, owner=None, extra=()):
        lines = [line for line in (owner, cleanup, first, *extra) if line is not None]
        capture = RUN.Capture()
        capture.feed(("\n".join(lines) + "\n").encode("ascii"))
        return lines, capture

    def test_quarantine_pins_are_the_observed_failures(self):
        self.assertEqual(
            RUN.MINT_QUARANTINE,
            "C13 first phase=join category=assertion "
            "at=m3_activation_linux.rs:1366 message=mint-refused",
        )
        self.assertEqual(RUN.MINT_OWNER_QUARANTINE, "C13 owner node=a action=mint exit=8")
        self.assertEqual(
            RUN.PROMOTION_QUARANTINE,
            "C13 first phase=promotion-transfer category=assertion "
            "at=m3_activation_linux.rs:766 message=process-exited",
        )

    def test_clean_signatures_quarantine_and_mutants_stay_red(self):
        mint_cleanup = ("C13 cleanup spawned=3 signaled=3 reaped=3 clean=2 "
                        "helpers=16 helpers_reaped=16 helper_unknown=0")
        promotion_cleanup = ("C13 cleanup spawned=3 signaled=2 reaped=3 clean=2 "
                             "helpers=22 helpers_reaped=22 helper_unknown=0")
        mint_lines, _ = self.evidence(RUN.MINT_QUARANTINE, mint_cleanup, RUN.MINT_OWNER_QUARANTINE)
        promotion_lines, _ = self.evidence(RUN.PROMOTION_QUARANTINE, promotion_cleanup)
        clean = {"code": 101, "truncated": False, "fixture": "removed", "retained": 0, "unknown": 0}
        self.assertEqual(RUN.quarantine_signature(RUN.MINT_QUARANTINE, mint_lines, **clean),
                         "mint-refused")
        self.assertEqual(
            RUN.quarantine_signature(RUN.PROMOTION_QUARANTINE, promotion_lines, **clean),
            "promotion-transfer-process-exited",
        )
        success = [RUN.PIN, promotion_cleanup.replace("spawned=3 signaled=2 reaped=3 clean=2",
                                                      "spawned=6 signaled=6 reaped=6 clean=6")]
        self.assertIsNone(RUN.quarantine_signature(None, success, code=0, truncated=False,
                                                   fixture="removed", retained=0, unknown=0))
        mutants = {
            "unclean teardown": {**clean, "fixture": "retained", "retained": 8},
            "different message": None,
            "different phase": None,
            "extra anomaly": None,
            "missing owner": None,
            "parse failure": None,
            "timeout": {**clean, "code": None},
            "other status": {**clean, "code": 102},
            "truncated output": {**clean, "truncated": True},
        }
        different_message, _ = self.evidence(
            RUN.MINT_QUARANTINE.replace("message=mint-refused", "message=join-refused"),
            mint_cleanup, RUN.MINT_OWNER_QUARANTINE,
        )
        different_phase, _ = self.evidence(
            RUN.PROMOTION_QUARANTINE.replace("phase=promotion-transfer", "phase=join"),
            promotion_cleanup,
        )
        extra, _ = self.evidence(
            RUN.MINT_QUARANTINE, mint_cleanup, RUN.MINT_OWNER_QUARANTINE,
            extra=("C13 first phase=close category=assertion",),
        )
        missing_owner, _ = self.evidence(RUN.MINT_QUARANTINE, mint_cleanup)
        unparsed, _ = self.evidence(
            RUN.MINT_QUARANTINE, "C13 cleanup spawned=3 signaled=3 reaped=3",
            RUN.MINT_OWNER_QUARANTINE,
        )
        cases = (
            ("unclean teardown", RUN.MINT_QUARANTINE, mint_lines, mutants["unclean teardown"]),
            ("different message", different_message[2], different_message, clean),
            ("different phase", different_phase[1], different_phase, clean),
            ("extra anomaly", RUN.MINT_QUARANTINE, extra, clean),
            ("missing owner", RUN.MINT_QUARANTINE, missing_owner, clean),
            ("parse failure", RUN.MINT_QUARANTINE, unparsed, clean),
            ("timeout", RUN.MINT_QUARANTINE, mint_lines, mutants["timeout"]),
            ("other status", RUN.MINT_QUARANTINE, mint_lines, mutants["other status"]),
            ("truncated output", RUN.MINT_QUARANTINE, mint_lines, mutants["truncated output"]),
        )
        for name, first, lines, fields in cases:
            with self.subTest(name=name):
                self.assertIsNone(RUN.quarantine_signature(first, lines, **fields))

    def test_privileged_quarantine_warns_and_success_stays_success(self):
        mint_cleanup = ("C13 cleanup spawned=3 signaled=3 reaped=3 clean=2 "
                        "helpers=16 helpers_reaped=16 helper_unknown=0")
        promotion_cleanup = ("C13 cleanup spawned=3 signaled=2 reaped=3 clean=2 "
                             "helpers=22 helpers_reaped=22 helper_unknown=0")
        cases = (
            ("mint-refused", RUN.MINT_QUARANTINE, mint_cleanup, RUN.MINT_OWNER_QUARANTINE),
            ("promotion-transfer-process-exited", RUN.PROMOTION_QUARANTINE, promotion_cleanup, None),
        )
        for signature, first, cleanup, owner in cases:
            with self.subTest(signature=signature):
                _, capture = self.evidence(first, cleanup, owner)
                with patch.object(RUN, "write_quarantine_marker") as marker:
                    status, output, roles, commands = self.fixture_campaign(101, capture, b"")
                self.assertEqual(status, 0)
                self.assertEqual((roles, commands), (8, 9))
                line = f"C13 quarantined signature={signature}\n"
                self.assertIn(line, output)
                self.assertIn(f"::warning::{line}", output)
                marker.assert_called_once_with("/tmp/arx-c13-abcdefgh", signature)
        refused = (
            ("unclean teardown", RUN.MINT_QUARANTINE, mint_cleanup, RUN.MINT_OWNER_QUARANTINE, "retained"),
            ("different message",
             RUN.MINT_QUARANTINE.replace("message=mint-refused", "message=join-refused"),
             mint_cleanup, RUN.MINT_OWNER_QUARANTINE, "removed"),
            ("different phase",
             RUN.PROMOTION_QUARANTINE.replace("phase=promotion-transfer", "phase=founder"),
             promotion_cleanup, None, "removed"),
            ("extra anomaly", RUN.PROMOTION_QUARANTINE, promotion_cleanup, None, "removed"),
            ("missing owner", RUN.MINT_QUARANTINE, mint_cleanup, None, "removed"),
        )
        for name, first, cleanup, owner, role in refused:
            with self.subTest(privileged=name):
                extra = ("C13 publication node=4 term=1 leader=1 index=1",) if name == "extra anomaly" else ()
                _, capture = self.evidence(first, cleanup, owner, extra)
                with patch.object(RUN, "write_quarantine_marker") as marker:
                    status, output, _, _ = self.fixture_campaign(101, capture, b"", role)
                self.assertEqual(status, 1)
                self.assertNotIn("quarantined", output)
                self.assertNotIn("::warning::", output)
                marker.assert_not_called()

    def test_step_summary_follows_only_a_recorded_quarantine(self):
        stage = Path(self.enterContext(tempfile.TemporaryDirectory()))
        summary = stage / "summary.txt"
        summary.write_bytes(b"")
        marker = stage / RUN.QUARANTINE_MARKER
        env = {"GITHUB_STEP_SUMMARY": str(summary)}

        def launch(code, **kwargs):
            with (patch.dict(RUN.os.environ, env, clear=True),
                  patch.object(RUN.time, "monotonic", return_value=10),
                  patch.object(RUN.subprocess, "run", return_value=MagicMock(returncode=code))):
                return RUN.launch_privileged(stage, 20, 30)

        self.assertEqual(launch(0), 0)
        self.assertEqual(summary.read_bytes(), b"")
        marker.write_bytes(b"mint-refused\n")
        self.assertEqual(launch(0), 0)
        self.assertEqual(summary.read_bytes(), b"C13 quarantined signature=mint-refused\n")
        self.assertEqual(launch(1), 1)
        self.assertEqual(summary.read_bytes(), b"C13 quarantined signature=mint-refused\n")
        marker.write_bytes(b"promotion-transfer-process-exited\n")
        self.assertEqual(launch(0), 0)
        self.assertEqual(
            summary.read_text(encoding="utf-8"),
            "C13 quarantined signature=mint-refused\n"
            "C13 quarantined signature=promotion-transfer-process-exited\n",
        )
        summary.write_bytes(b"")
        marker.unlink()
        marker.mkdir()
        self.assertEqual(launch(0), 0)
        self.assertEqual(summary.read_bytes(), b"")
        marker.rmdir()
        marker.write_bytes(b"not-a-signature\n")
        self.assertEqual(launch(0), 0)
        self.assertEqual(summary.read_bytes(), b"")

    def test_replay_rejects_unframed_and_near_miss_records(self):
        # The six privileged replays from the #332 review. Each previously
        # reached the marker writer and returned zero.
        owner = RUN.MINT_OWNER_QUARANTINE.encode("ascii")
        cleanup = (b"C13 cleanup spawned=3 signaled=3 reaped=3 clean=2 "
                   b"helpers=16 helpers_reaped=16 helper_unknown=0")
        first = RUN.MINT_QUARANTINE.encode("ascii")
        framed = owner + b"\n" + cleanup + b"\n" + first + b"\n"
        cases = (
            ("cr-delimited cleanup",
             owner + b"\n" + b"ordinary\r" + cleanup + b"\rordinary\n" + first + b"\n"),
            ("cr-delimited owner",
             b"ordinary\r" + owner + b"\rordinary\n" + cleanup + b"\n" + first + b"\n"),
            ("unterminated cleanup", owner + b"\n" + first + b"\n" + cleanup),
            ("malformed cleanup", framed + b"C13 cleanup spawned=3 signaled=3 reaped=3\n"),
            ("owner exit out of grammar", framed + b"C13 owner node=a action=mint exit=9999\n"),
            ("unlisted first-failure",
             framed + b"C13 first phase=not-a-phase category=not-a-category\n"),
        )
        for name, payload in cases:
            with self.subTest(name=name):
                capture = RUN.Capture()
                capture.feed(payload)
                with patch.object(RUN, "write_quarantine_marker") as marker:
                    status, output, _, _ = self.fixture_campaign(101, capture, b"")
                self.assertEqual(status, 1)
                self.assertNotIn("quarantined", output)
                self.assertNotIn("::warning::", output)
                marker.assert_not_called()

    def test_launcher_timeout_is_not_quarantined(self):
        stage = Path("/tmp/arx-c13-abcdefgh")
        with (patch.object(RUN.time, "monotonic", return_value=10),
              patch.object(RUN.subprocess, "run",
                           side_effect=RUN.subprocess.TimeoutExpired("sudo", 1))):
            with self.assertRaises(RUN.subprocess.TimeoutExpired):
                RUN.launch_privileged(stage, 20, 30)
