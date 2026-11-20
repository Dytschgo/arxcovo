"""Pure D6 controls. Every child, Job and clock is synthetic."""
import io
import json
from pathlib import Path
from threading import Event
import time
import unittest

import observe_bound_start as d6


def transcript(*, poll="pending"):
    result = f"\nrunning 1 test\ntest {d6.CASE} ... "
    for index, (stage, values) in enumerate(d6.PROOFS.items(), 1):
        result += f"ARX5B {index} {stage} {poll if stage == 'engine_poll' else values[0]}\n"
    return (result + "ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; "
            "0 measured; 853 filtered out; finished in 0.04s\n").encode("ascii")


class FakeClock:
    def __init__(self, now=1000):
        self.now = now

    def __call__(self):
        return self.now

    def sleep(self, seconds):
        self.now += max(1, round(seconds * 1000))


class FakeChild:
    def __init__(self, clock, *, data=b"", leak=False, exit_after_go=True,
                 spawn_ms=0, interrupt_at=None, member=True, exit_early=False):
        self.clock, self.data, self.leak = clock, data, leak
        self.exit_after_go, self.spawn_ms = exit_after_go, spawn_ms
        self.interrupt_at, self.is_member = interrupt_at, member
        self.exit_early = exit_early
        self.sent = self.terminated = False
        self.events = []

    def create_suspended(self):
        self.events.append("created_in_job")
        self.clock.now += self.spawn_ms
        if self.interrupt_at == "creation":
            raise KeyboardInterrupt
        return object()

    def member(self, _):
        self.events.append("member")
        if self.interrupt_at == "membership":
            raise KeyboardInterrupt
        return self.is_member

    def resume(self, _):
        self.events.append("resume")
        if self.interrupt_at == "resume":
            raise KeyboardInterrupt

    def go(self):
        self.events.append("go")
        self.sent = True

    def poll(self, _):
        if self.terminated or self.exit_early:
            return 1
        if self.sent and self.exit_after_go:
            return 0
        return None

    def read(self, want):
        if self.data:
            data, self.data = self.data[:want], self.data[want:]
            return data
        return b"" if self.leak else None

    def active(self):
        return 1 if self.poll(None) is None else 0

    def close_input(self):
        self.events.append("close_input")

    def terminate(self):
        self.events.append("terminate")
        self.terminated = True


class LauncherTests(unittest.TestCase):
    def sample(self, clock=None, **kwargs):
        clock = clock or FakeClock()
        child = FakeChild(clock, **kwargs)
        events = []
        proof = d6.Proof(events.append)
        outcome = d6.observe(child, clock, clock.sleep, 1000, proof)
        return outcome, proof, child, events

    def test_exact_selection_and_minimal_environment(self):
        self.assertEqual(d6.ARGS, ("--ignored", "--exact", d6.CASE, "--nocapture", "--test-threads=1"))
        env = {key: str(d6.ROOT / leaf) for key, leaf in d6.SCRATCH.items()}
        env.update(SYSTEMROOT="windows", WINDIR="windows", CARGO_NET_OFFLINE="true",
                   CARGO_PROFILE_DEV_DEBUG="0", ARX_STEP5B_ENTRY_MS="1000",
                   ARX_STEP5B_PINS_SHA256="a" * 64)
        self.assertEqual(d6.validate_entry([], env, 1000), 1000)
        for args, env_case, reason in (
            (["extra"], env, "arguments"),
            ([], dict(env, PATH="unexpected"), "environment"),
            ([], dict(env, LD_PRELOAD="unexpected"), "environment"),
            ([], dict(env, TEMP="unexpected"), "scratch"),
            ([], dict(env, ARX_STEP5B_ENTRY_MS="1001"), "entry_clock"),
        ):
            with self.subTest(reason=reason), self.assertRaisesRegex(d6.Refusal, reason):
                d6.validate_entry(args, env_case, 1000)

    def test_pins_digest_duplicate_keys_and_fixed_inventory(self):
        sample = dict(head="0" * 40, test=d6.CASE, binary_bytes=123,
                      binary_sha256="a" * 64, python=r"D:\qualified\python.exe",
                      python_sha256="b" * 64, python_runtime={"python312.zip": "d" * 64},
                      sha256=dict.fromkeys(d6.FILES, "c" * 64))
        raw = json.dumps(sample).encode("ascii")
        self.assertEqual(d6.parse_pins(raw, d6.digest(raw)), sample)
        with self.assertRaisesRegex(d6.Refusal, "pins_digest"):
            d6.parse_pins(raw, "a" * 64)
        for invalid in (
            b'{"head":"x",' + raw[1:],
            json.dumps(dict(sample, test="different")).encode("ascii"),
            json.dumps(dict(sample, binary_bytes=True)).encode("ascii"),
            json.dumps(dict(sample, sha256={})).encode("ascii"),
            json.dumps(dict(sample, python_runtime={"../python.dll": "d" * 64})).encode("ascii"),
            json.dumps(dict(sample, python_runtime={"X.dll": "d" * 64, "x.dll": "e" * 64})).encode("ascii"),
        ):
            with self.assertRaisesRegex(d6.Refusal, "pins_schema"):
                d6.parse_pins(invalid, d6.digest(invalid))

    def test_verified_bytes_size_digest_and_admission_deadline(self):
        sample = b"pinned support"
        sha = d6.digest(sample)
        clock = FakeClock()
        self.assertEqual(d6.verify_file(io.BytesIO(sample), len(sample), sha, clock, 1001,
                                        size=len(sample), retain=True), sample)
        for data, cap, cutoff, reason in (
            (sample + b"!", len(sample), 1001, "file_size"),
            (b"short", len(sample), 1001, "file_size"),
            (sample.swapcase(), len(sample), 1001, "file_digest"),
            (sample, len(sample), 1000, "admission_deadline"),
        ):
            with self.subTest(reason=reason), self.assertRaisesRegex(d6.Refusal, reason):
                d6.verify_file(io.BytesIO(data), cap, sha, clock, cutoff, size=len(sample))

    def test_ordered_success_resume_and_go_follow_membership(self):
        for poll in ("pending", "ready"):
            out, proof, child, stages = self.sample(data=transcript(poll=poll))
            self.assertEqual(child.events, ["created_in_job", "member", "resume", "go", "close_input"])
            self.assertTrue(all(out[k] for k in ("member", "resumed", "go", "natural", "eof",
                                                 "reaped", "zero_active")))
            self.assertFalse(out["forced"])
            self.assertEqual((proof.selected, proof.passed, proof.failed), (1, 1, 0))
            self.assertIsNone(proof.first)
            self.assertEqual(proof.uncertainty, [])
            self.assertEqual([stage["stage"] for stage in stages], list(d6.PROOFS))

    def test_delayed_launcher_refuses_before_spawn_or_inherits_wall_cutoff(self):
        out, proof, child, _ = self.sample(FakeClock(83000))
        self.assertEqual(child.events, ["close_input"])
        self.assertFalse(out["go"])
        self.assertEqual(proof.first, "admission_deadline")
        clock = FakeClock(82990)
        out, proof, child, _ = self.sample(clock, spawn_ms=11)
        self.assertFalse(out["go"])
        self.assertNotIn("resume", child.events)
        self.assertIn("process_deadline", proof.uncertainty)

    def test_process_deadline_includes_suspended_creation_duration(self):
        clock = FakeClock()
        out, proof, child, _ = self.sample(clock, spawn_ms=25000, exit_after_go=False)
        self.assertTrue(out["go"])
        self.assertTrue(out["forced"])
        self.assertEqual(out["child_ms"], 48000)
        self.assertIn("process_deadline", proof.uncertainty)

    def test_interruption_at_creation_membership_or_resume_stops_job(self):
        for boundary in ("creation", "membership", "resume"):
            with self.subTest(boundary=boundary):
                out, proof, child, _ = self.sample(interrupt_at=boundary)
                self.assertFalse(out["go"])
                self.assertTrue(child.terminated)
                self.assertIn("interrupted", proof.uncertainty)

    def test_missing_membership_and_early_exit_do_not_release_go(self):
        for opts, expected in (({"member": False}, "job_membership"),
                               ({"exit_early": True}, "early_exit")):
            with self.subTest(expected=expected):
                out, proof, child, _ = self.sample(**opts)
                self.assertFalse(out["go"])
                self.assertTrue(child.terminated)
                self.assertEqual(proof.first, expected)

    def test_leaked_pipe_handles_prevent_clean_and_consume_only_cleanup_reserve(self):
        clock = FakeClock(80000)
        out, proof, _, _ = self.sample(clock, data=transcript(), leak=True)
        self.assertFalse(out["eof"])
        self.assertTrue(out["reaped"] and out["zero_active"] and out["forced"])
        self.assertFalse(out["natural"])
        self.assertEqual(clock(), 91000)
        self.assertIn("incomplete_cleanup", proof.uncertainty)

    def test_combined_output_overflow_probes_one_byte_and_preserves_stages(self):
        out, proof, child, stages = self.sample(data=transcript() + b"x" * (2 * d6.OUTPUT_CAP))
        self.assertTrue(proof.truncated and out["forced"])
        self.assertFalse(out["natural"])
        self.assertEqual(proof.bytes, d6.OUTPUT_CAP)
        self.assertIn("overflow", proof.uncertainty)
        self.assertEqual(len(stages), len(d6.PROOFS))
        self.assertGreater(len(child.data), 0)

    def test_malformed_missing_and_out_of_order_proofs(self):
        baseline = transcript()
        mutants = (
            baseline.replace(b"engine_poll pending", b"engine_poll unrelated"),
            baseline.replace(b"ARX5B 2 prepared 2\n", b""),
            baseline.replace(b"4 engine_poll", b"1 engine_poll"),
            baseline.replace(b"running 1 test", b"running 2 tests"),
            baseline.replace(b"test result: ok.", b"test result: FAILED."),
            baseline.replace(b"ARX5B 2 prepared 2\n", b"ARX5B 3 transferred 1 1 1\nARX5B 2 prepared 2\n"),
        )
        for mutant in mutants:
            with self.subTest(mutant=mutant):
                out, proof, _, _ = self.sample(data=mutant)
                self.assertTrue(out["natural"])
                self.assertTrue(proof.uncertainty)

    def test_duplicate_terminal_records_are_contradictory(self):
        baseline = transcript()
        status, summary = b"ok\n", baseline.splitlines(keepends=True)[-1]
        for mutant in (
            baseline.replace(b"ok\n\n", status + b"FAILED\n\n"),
            baseline.replace(b"ok\n\n", status + status + b"\n"),
            baseline + b"FAILED\n",
            baseline + summary,
            baseline.replace(b"1 passed; 0 failed", b"1 passed; 1 failed"),
            baseline.replace(b"running 1 test", b"ok\nrunning 1 test"),
        ):
            with self.subTest(mutant=mutant):
                _, proof, _, _ = self.sample(data=mutant)
                self.assertIn("malformed_proof", proof.uncertainty)

    def test_natural_exit_and_terminal_status_must_agree(self):
        child = FakeChild(clock := FakeClock(), data=transcript())
        child.poll = lambda _: 23 if child.sent else None
        proof = d6.Proof(lambda _: None)
        out = d6.observe(child, clock, clock.sleep, 1000, proof)
        self.assertEqual(out["natural_exit_code"], 23)
        self.assertEqual(proof.first, "child_exit")
        self.assertIn("exit_status_mismatch", proof.uncertainty)

    def test_outer_watchdog_arms_before_preflight_and_fences_resume(self):
        adr = (Path(__file__).resolve().parents[2] /
               "docs/adr/0039-retained-bound-start-ownership.md").read_text(encoding="utf-8")
        self.assertLess(adr.index("$custody.Arm($cutoff)"), adr.index("$worker.BeginInvoke()"))
        start = adr.index("Require(CreateProcess(exe")
        resume = adr.index("Require(ResumeThread", start)
        self.assertIn("0x08080404", adr[start:resume])
        self.assertIn("Require(IsProcessInJob", adr[start:resume])
        self.assertIn("Require(!Cancelled && Environment.TickCount64 < work)", adr[start:resume])

    def test_natural_nonzero_exit_survives_leaked_eof(self):
        child = FakeChild(clock := FakeClock(80000), leak=True)
        initial_poll = child.poll
        child.poll = lambda process: 23 if child.sent and not child.terminated else initial_poll(process)
        proof = d6.Proof(lambda _: None)
        out = d6.observe(child, clock, clock.sleep, 1000, proof)
        self.assertEqual(proof.first, "child_exit")
        self.assertEqual(out["natural_exit_code"], 23)
        self.assertEqual(out["exit_code"], 1)
        self.assertIn("incomplete_cleanup", proof.uncertainty)

    def test_failed_stdin_close_never_prevents_termination(self):
        child = FakeChild(clock := FakeClock(82990), exit_after_go=False)
        def broken_close():
            raise OSError
        child.close_input = broken_close
        proof = d6.Proof(lambda _: None)
        out = d6.observe(child, clock, clock.sleep, 1000, proof)
        self.assertTrue(child.terminated and out["reaped"])
        self.assertIn("stdin_cleanup", proof.uncertainty)

    def test_custodian_terminates_while_pipe_read_is_stalled(self):
        now = lambda: time.monotonic_ns() // 1_000_000
        child = FakeChild(FakeClock(), exit_after_go=False)
        stopped, entered = Event(), Event()
        original_terminate = child.terminate

        def terminate():
            original_terminate()
            stopped.set()

        def blocked_read(_):
            entered.set()
            self.assertTrue(stopped.wait(2), "the Job cutoff depended on pipe progress")
            return None

        child.terminate = terminate
        child.read = blocked_read
        proof = d6.Proof(lambda _: None)
        out = d6.observe(child, now, time.sleep, now() - 81800, proof)
        self.assertTrue(entered.is_set())
        self.assertTrue(out["forced"] and out["reaped"] and out["zero_active"])
        self.assertFalse(out["natural"])

    def test_stalled_stage_writer_cannot_delay_cutoff_or_earn_credit(self):
        entered, unblock, stopped = Event(), Event(), Event()

        class BlockedStream(io.BytesIO):
            def flush(self):
                entered.set()
                unblock.wait(5)

        class FakePath:
            def open(self, _):
                return BlockedStream()

        writer = d6.StageWriter(FakePath())
        now = lambda: time.monotonic_ns() // 1_000_000
        child = FakeChild(FakeClock(), data=transcript(), exit_after_go=False)
        terminate = child.terminate

        def record_stop():
            terminate()
            stopped.set()

        child.terminate = record_stop
        proof = d6.Proof(writer.emit)
        try:
            out = d6.observe(child, now, time.sleep, now() - 81800, proof)
            self.assertTrue(entered.is_set() and stopped.is_set())
            self.assertTrue(out["forced"] and out["reaped"])
            self.assertIsNone(writer.finish(now, now()))
        finally:
            unblock.set()
            writer.worker.join(2)

    def test_handle_identity_refuses_changed_directory_and_file(self):
        # Inject the two native path views and exchange their object identity.
        from contextlib import ExitStack
        from types import SimpleNamespace
        from unittest.mock import patch

        for directory in (False, True):
            with self.subTest(directory=directory), ExitStack() as stack:
                closed = []
                handles = iter((1, 2))
                api = SimpleNamespace(CreateFile=lambda *_: next(handles), CloseHandle=closed.append)
                with (patch.dict("sys.modules", _winapi=api),
                      patch.object(d6, "checked"), patch.object(d6, "handle_identity", side_effect=[1, 2])):
                    with self.assertRaisesRegex(d6.Refusal, "path"):
                        d6.checked_handle(Path("fixture"), directory, stack)
                stack.close()
                self.assertEqual(closed, [2, 1])

    def test_pre_disposal_retains_identity_refusal_and_individual_lock_results(self):
        nominal = transcript().split(b"before_disposal 1 1 1", 1)[0]
        for phase, first in ((b"failure owner_identity", "owner_identity"),
                             (b"before_disposal 1 0 1", "before_disposal")):
            _, proof, _, stages = self.sample(data=nominal + phase + b"\nFAILED\n")
            self.assertEqual(proof.first, first)
            self.assertEqual(stages[-1]["values"], phase.decode().split()[1:])
            self.assertIn("incomplete_proof", proof.uncertainty)

    def test_first_concrete_failure_and_orphaned_report_survive_cleanup(self):
        failure = (f"running 1 test\ntest {d6.CASE} ... ARX5B 1 admitted 1\n"
                   "ARX5B 2 failure before_disposal\n"
                   "ARX5B 3 orphaned 1 3 5000 0 0 0 0 0 0 0 1 1 5001\n"
                   "ARX5B 4 failure final_state\n").encode("ascii")
        _, proof, _, stages = self.sample(data=failure, leak=True)
        self.assertEqual(proof.first, "before_disposal")
        self.assertEqual(proof.orphaned["engine_ms"], 5000)
        self.assertIn("incomplete_cleanup", proof.uncertainty)
        self.assertIn("incomplete_proof", proof.uncertainty)
        self.assertEqual([stage["stage"] for stage in stages], ["admitted", "failure", "orphaned", "failure"])


if __name__ == "__main__":
    unittest.main()
