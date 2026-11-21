"""Pure tests for the fixed C13 launcher."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

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
