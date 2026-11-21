"""Pure tests for the fixed C13 launcher."""
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import MagicMock, patch

SPEC = importlib.util.spec_from_file_location("c13_run", Path(__file__).with_name("run.py"))
RUN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUN)


class LauncherTests(unittest.TestCase):
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
              patch.object(RUN.subprocess, "run") as run):
            RUN.launch_privileged(stage, 20, 30)
        run.assert_called_once_with(
            ["/usr/bin/sudo", "-n", "/usr/bin/env", "GITHUB_ACTIONS=true",
             "RUNNER_ENVIRONMENT=github-hosted", "RUNNER_OS=Linux",
             "/usr/bin/python3", "-I", "-B", Path(RUN.__file__).resolve(),
             "--staged", stage, "20000000"],
            env=RUN.ENV, stdin=RUN.subprocess.DEVNULL, check=True, timeout=18,
        )

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



    def test_fixture_cleanup_requires_all_owned_children_observed(self):
        good = "C13 cleanup spawned=6 signaled=6 reaped=6 clean=6 helpers=18 helpers_reaped=18 helper_unknown=0"
        self.assertTrue(RUN.cleanup_observed([good]))
        for lines in (
            [], [good, good], [good.replace("clean=6", "clean=5")],
            [good.replace("reaped=6", "reaped=5")], [good.replace("helpers_reaped=18", "helpers_reaped=17")], [good.replace("unknown=0", "unknown=1")],
        ):
            self.assertFalse(RUN.cleanup_observed(lines))


    def test_teardown_reserve_and_redacted_first_failure(self):
        with patch.object(RUN.time, "monotonic", return_value=100):
            self.assertEqual(RUN.remaining(103, 9), 3)
            with self.assertRaises(TimeoutError):
                RUN.remaining(100, 9)
    def test_first_record_survives_truncation_and_chunk_boundaries(self):
        first = b"C13 first phase=join category=pipe-io"
        next_record = b"C13 first phase=close category=assertion"
        for oversized_prefix in (b"", b"x" * (RUN.OUTPUT_LIMIT + 20)):
            with self.subTest(truncated_before_record=bool(oversized_prefix)):
                capture = RUN.Capture()
                capture.feed(oversized_prefix)
                capture.feed(b"suffix\nC13 first phase=/tmp category=secret\n" + first[:15])
                capture.feed(first[15:] + b"\n" + next_record + b"\n")
                capture.feed(b"x" * RUN.OUTPUT_LIMIT)
                self.assertEqual(capture.first, first.decode("ascii"))
                self.assertTrue(capture.truncated)
                self.assertEqual(len(capture.bulk), RUN.OUTPUT_LIMIT)

    def test_partial_and_oversized_lines_do_not_produce_records(self):
        capture = RUN.Capture()
        first = b"C13 first phase=join category=pipe-io"
        capture.feed(b"x" * 97)
        capture.feed(first + b"\n" + first)
        self.assertIsNone(capture.first)
        capture.feed(b"\n")
        self.assertEqual(capture.first, first.decode("ascii"))

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
