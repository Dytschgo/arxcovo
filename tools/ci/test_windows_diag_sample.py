"""Pure tests for the Windows diagnostic sampler: fakes only.

No real process, ctypes call or cargo runs here. Files read: the sampler source,
the quarantine guard and D1 source pins.
"""
import io
import json
import os
import re
import signal
import threading
import unittest
from pathlib import Path
from unittest.mock import patch
from xml.sax.saxutils import escape

import windows_diag_sample as w

REPO = Path(__file__).resolve().parents[2]
NT = os.name == "nt"
FROOT = "C:\\fake\\repo" if NT else "/fake/repo"
FTEMP = "C:\\fake\\tmp" if NT else "/fake/tmp"
J = os.path.join
PRIV = J(FTEMP, "arxdiag-abc123")
LISTING = J(PRIV, w.LIST_NAME)
JUNIT = J(FROOT, "target", "nextest", "diag-sample", "junit.xml")
NAMES = list(w.IDS)
GUARD_SRC = (REPO / "tools/ci/test_windows_cluster_quarantine.py").read_bytes()

# --- D1 fixtures: one valid line per form (and alternatives) -----------------------
D1 = {
    "pass": "[DIAG-3n] pass t=12034 lag=57@3400 over=0 ticks=240 n1 term=2 ldr=1 seen=40 high=0 "
            "n2 ring=busy high=3 n3 term=0 ldr=0 seen=12 high=1 max Vote=12,Append=300,Apply=4,"
            "Snapshot=0,Other=1 queue=9 leader=3:18446744073709551615:7 write=2:5:3 cand=Store",
    "fail": "[DIAG-3n] v1 fail t=45001 lag=400@44000 over=3 ticks=880 cand=Both",
    "lagev": "[DIAG-3n] lagev [(44000, 400), (12, 100)]",
    "lagev_empty": "[DIAG-3n] lagev []",
    "lagev_busy": "[DIAG-3n] lagev busy",
    "helpers": "[DIAG-3n] helpers leader=3:120:80 write=4:9:5 converge=2 stop=1",
    "attr_busy": "[DIAG-3n] attr busy",
    "attr_none": "[DIAG-3n] attr none",
    "attr": "[DIAG-3n] attr helper=ConvergeWait fail=NotLeader node=2 t=30000 polls=17 retries=0",
    "s3": "[DIAG-3n] s3 before=100,4,L,1,10,10,10,5 after=200,4,?,1,-,-,-,- class=SameTermCandidate",
    "at": "[DIAG-3n] at n1=5,1,L,1,9,9,9,0 n2=- n3=7,1,F,1,9,9,9,-",
    "report": "[DIAG-3n] report ShutdownReport { first: Owners, engine: Joined, engine_ms: 12, "
              "owners_done_at_start: 1, owners_waited: 2, owners_left: 1, owners_panicked: 0, "
              "last_owner_join_ms: 65535, propose_held: 0, read_held: 0, log_strong: 1, "
              "sm_strong: 255, total_ms: 300 }",
    "ops": "[DIAG-3n] n1 ops Vote=1:2:3:4:5^1000 Append=0:0:0:0:0^0 Apply=10:0:0:0:1^70000 "
           "Snapshot=0:0:0:0:0^0 Other=1:0:0:0:0^3 queue=5:0:0:0:0^8 delay=0 seen=40 ovw=0 term=2 "
           "ldr=1 st=3",
    "ops_busy": "[DIAG-3n] n3 ops Vote=0:0:0:0:0^0 Append=0:0:0:0:0^0 Apply=0:0:0:0:0^0 "
                "Snapshot=0:0:0:0:0^0 Other=0:0:0:0:0^0 queue=0:0:0:0:0^0 delay=150 ring=busy",
    "ring": "[DIAG-3n] n2 ring 5,1,L,1,9,9,9,0 6,1,C,1,10,10,10,-",
    "ring_busy": "[DIAG-3n] n1 ring busy",
    "ring_empty": "[DIAG-3n] n3 ring",
    "end": "[DIAG-3n] end bytes=8192 trunc=1",
}
SAMPLE = "5,1,L,1,9,9,9,0"


class FakeClock:
    def __init__(self, wall=1_800_000_000.0):
        self.t, self.wall = 1000.0, wall

    def monotonic(self):
        return self.t

    def time(self):
        return self.wall + (self.t - 1000.0)

    def sleep(self, s):
        self.t += s


class FakeStdin:
    def __init__(self, proc):
        self.proc, self.data, self.closed = proc, b"", False

    def write(self, b):
        if self.closed:
            raise ValueError("closed")
        self.data += b
        self.proc.go_at = self.proc.clock.t

    def flush(self):
        pass

    def close(self):
        self.closed = True


class FakeProc:
    def __init__(self, clock, rc=0, run_s=1.0, kill_works=True, exit_on_terminate=True,
                 on_exit=None, on_kill=None):
        self.clock, self.rc, self.run_s = clock, rc, run_s
        self.kill_works, self.exit_on_terminate = kill_works, exit_on_terminate
        self.on_exit, self.on_kill = on_exit, on_kill
        self.stdin = FakeStdin(self)
        self.go_at, self.returncode, self.kills = None, None, 0
        self._handle = 1234

    def poll(self):
        if (self.returncode is None and self.go_at is not None and self.run_s is not None
                and self.clock.t >= self.go_at + self.run_s):
            self.returncode = self.rc
            if self.on_exit:
                self.on_exit()
        return self.returncode

    def kill(self):
        self.kills += 1
        if self.kill_works and self.returncode is None:
            self.returncode = 1

    def job_terminated(self):
        if self.exit_on_terminate and self.returncode is None:
            self.returncode = 1
            if self.on_kill:
                self.on_kill()


class FakeSpawner:
    def __init__(self, log, makers):
        self.log, self.makers, self.procs, self.fail = log, makers, [], False

    def spawn(self, phase, privdir):
        self.log.append("spawn:" + phase)
        if self.fail:
            raise OSError("no spawn")
        p = self.makers[phase]()
        self.procs.append(p)
        return p


class FakeJobApi:
    def __init__(self, log, assign_ok=True, in_job_ok=True, create_fail=False, active=None,
                 on_assign=None):
        self.log, self.assign_ok, self.in_job_ok = log, assign_ok, in_job_ok
        self.create_fail, self.active_seq, self.on_assign = create_fail, active, on_assign
        self.members, self.terminated, self.closed, self.created = [], 0, 0, 0

    def create(self):
        self.log.append("create")
        if self.create_fail:
            raise w.JobError
        self.created += 1
        return "job%d" % self.created

    def assign(self, job, proc):
        self.log.append("assign")
        if self.on_assign:
            self.on_assign()
        if self.assign_ok:
            self.members.append(proc)
        return self.assign_ok

    def in_job(self, job, proc):
        self.log.append("in_job")
        return self.in_job_ok

    def terminate(self, job):
        self.log.append("terminate")
        self.terminated += 1
        for p in self.members:
            p.job_terminated()
        return True

    def active(self, job):
        self.log.append("active")
        if self.active_seq is not None:
            return self.active_seq.pop(0) if len(self.active_seq) > 1 else self.active_seq[0]
        return sum(1 for p in self.members if p.returncode is None)

    def close(self, job):
        self.log.append("close")
        self.closed += 1


class FakeFs:
    def __init__(self):
        self.kinds, self.files, self.reads, self.unlinked, self.removed = {}, {}, [], [], []
        self.unreal = set()
        for d in (FROOT, J(FROOT, "tools"), J(FROOT, "tools", "ci"), FTEMP, J(FROOT, "target"),
                  J(FROOT, "target", "nextest")):
            self.kinds[d] = "dir"
        self.files[J(FROOT, *w.GUARD_REL)] = GUARD_SRC
        self.files[J(FROOT, *w.CONFIG_REL)] = b"[profile.diag-sample]\n"

    def kind(self, p):
        if p in self.kinds:
            return self.kinds[p]
        return "file" if p in self.files else "missing"

    def realpath_ok(self, p):
        return p not in self.unreal

    def read_capped(self, p, cap):
        self.reads.append(p)
        return self.files[p][:cap + 1]

    def unlink(self, p):
        self.unlinked.append(p)
        self.files.pop(p, None)

    def mkdtemp(self, base):
        self.kinds[PRIV] = "dir"
        return PRIV

    def rmdir(self, p):
        self.removed.append(p)
        self.kinds.pop(p, None)


def listing(names=None, ignored=True, binary="arxd", kind="lib", target=None, extra=None):
    names = NAMES if names is None else names
    cases = {"cluster::consensus::three_node::" + n: {"kind": "test", "ignored": ignored,
                                                      "filter-match": {"status": "matches"}}
             for n in names}
    cases["cluster::consensus::tests::other"] = {
        "kind": "test", "ignored": False,
        "filter-match": {"status": "mismatch", "reason": "expression"}}
    suite = {"package-name": "arxd", "binary-id": binary, "binary-name": binary,
             "package-id": "path+file:///x#arxd@0.1.0", "kind": kind,
             "binary-path": "x", "build-platform": "target", "cwd": "x", "status": "listed",
             "testcases": cases}
    doc = {"rust-build-meta": {"target-directory": target or J(FROOT, "target"),
                               "base-output-directories": ["debug"]},
           "test-count": len(cases), "rust-suites": {binary: suite}}
    if extra:
        doc["rust-suites"].update(extra)
    return json.dumps(doc).encode()


def case(name, status="", err="", time="1.250", classname="arxd"):
    full = "cluster::consensus::three_node::" + name
    return ('<testcase name="%s" classname="%s" timestamp="2026-11-16T00:00:00.000+00:00" '
            'time="%s">%s<system-out></system-out><system-err>%s</system-err></testcase>'
            % (full, classname, time, status, escape(err)))


def junit(cases, suite="arxd"):
    return ('<?xml version="1.0" encoding="UTF-8"?>\n<testsuites name="nextest-run" tests="8" '
            'failures="0" errors="0" uuid="00000000-0000-0000-0000-000000000000" '
            'timestamp="2026-11-16T00:00:00.000+00:00" time="12.000">'
            '<testsuite name="%s" tests="8" disabled="0" errors="0" failures="0">%s'
            '</testsuite></testsuites>\n' % (suite, "".join(cases))).encode()


def all_pass_junit(err=""):
    return junit([case(n, err=err) for n in NAMES])


def failure(ty, tag="failure"):
    return '<%s type="%s">panicked at C:\\Users\\secret\\x.rs</%s>' % (tag, escape(ty), tag)


class Harness:
    def __init__(self, env=None, list_rc=0, run_rc=0, run_s=2.0, junit_bytes=None,
                 listing_bytes=None, api_kwargs=None, run_kwargs=None, list_kwargs=None):
        self.log, self.clock, self.fs = [], FakeClock(), FakeFs()
        self.cancel = threading.Event()
        self.env = {"GITHUB_EVENT_NAME": "push", "RUNNER_TEMP": FTEMP,
                    "ARX_DIAG_T0": str(int(self.clock.time()) - 300)}
        self.env.update(env or {})
        self.api = FakeJobApi(self.log, **(api_kwargs or {}))
        lb = listing() if listing_bytes is None else listing_bytes
        jb = all_pass_junit() if junit_bytes is None else junit_bytes

        def wrote_listing():
            self.fs.files[LISTING] = lb

        def wrote_junit():
            self.fs.files[JUNIT] = jb
        lk = dict(rc=list_rc, run_s=1.0, on_exit=wrote_listing)
        lk.update(list_kwargs or {})
        rk = dict(rc=run_rc, run_s=run_s, on_exit=wrote_junit)
        rk.update(run_kwargs or {})
        self.spawner = FakeSpawner(self.log, {"list": lambda: FakeProc(self.clock, **lk),
                                              "run": lambda: FakeProc(self.clock, **rk)})
        self.ctx = w.Ctx(env=self.env, is_windows=True, in_venv=False,
                         api_factory=lambda: self.api, spawner=self.spawner, clock=self.clock,
                         fs=self.fs, cancel=self.cancel, root=FROOT)

    def run(self):
        code, lines = w.run(self.ctx)
        self.lines = lines
        return code, lines

    def field(self, name):
        m = re.search(r" %s=(\S+)" % name, self.lines[0])
        return m.group(1)


class BudgetTests(unittest.TestCase):
    def test_admission_edge_600_runs_601_skips(self):
        self.assertEqual(w.admission(585), (600, True))
        self.assertEqual(w.admission(586), (601, False))
        self.assertEqual(w.ADMIT_E_MAX, 600)

    def test_anchor_cases(self):
        now = 1_800_000_000
        for raw in (None, "", "0", "abc", "012", "1.5", "-5", " 1", "1\n", "\u0661\u0662",
                    "123456789012", str(now + 1)):
            with self.subTest(raw=raw), self.assertRaises(w.Guard) as g:
                w.check_anchor(raw, now)
            self.assertEqual(g.exception.reason, "anchor")
        self.assertEqual(w.check_anchor(str(now), now), 0)
        self.assertEqual(w.check_anchor(str(now - 720), now), 720)
        self.assertEqual(w.check_anchor(str(now - 721), now), 721)
        self.assertEqual(w.check_anchor(str(now - 3600), now), 3600)

    def test_budget_skip_spawns_nothing_and_exits_zero(self):
        h = Harness()
        h.env["ARX_DIAG_T0"] = str(int(h.clock.time()) - 586)
        self.assertEqual(h.run()[0], 0)
        self.assertEqual(h.field("outcome"), "skipped_budget")
        self.assertEqual(h.field("e_s"), "601")
        self.assertEqual(h.log, [])
        h = Harness()
        h.env["ARX_DIAG_T0"] = str(int(h.clock.time()) - 585)
        self.assertEqual(h.run()[0], 0)
        self.assertEqual(h.field("outcome"), "all_pass")
        self.assertEqual(h.field("e_s"), "600")

    def test_missing_anchor_gates(self):
        h = Harness()
        del h.env["ARX_DIAG_T0"]
        self.assertEqual(h.run(), (1, [h.lines[0], "ARXDIAG v1 guard reason=anchor"]))
        self.assertEqual(h.field("e_s"), "-")

    def test_anchor_past_cap_skips_budget_without_diagnostic_work(self):
        for age in (w.CAP_S + 1, 3600):
            with self.subTest(age=age):
                h = Harness()
                h.env["ARX_DIAG_T0"] = str(int(h.clock.time()) - age)
                code, lines = h.run()
                self.assertEqual(code, 0)
                self.assertEqual(h.field("outcome"), "skipped_budget")
                self.assertEqual(h.field("e_s"), str(age + w.ANCHOR_PRE_S))
                self.assertEqual(lines, [lines[0]])
                self.assertNotIn("guard", lines[0])
                self.assertEqual(h.log, [])

    def test_negative_age_still_guards(self):
        h = Harness()
        h.env["ARX_DIAG_T0"] = str(int(h.clock.time()) + 1)
        self.assertEqual(h.run(), (1, [h.lines[0], "ARXDIAG v1 guard reason=anchor"]))
        self.assertEqual(h.field("e_s"), "-")
        self.assertEqual(h.log, [])

    def _shift_later_wall_read(self, clock, delta):
        original = clock.time
        seen = {"n": 0}

        def time():
            seen["n"] += 1
            now = original()
            return now + delta if seen["n"] > 1 else now

        clock.time = time
        return seen

    def test_backward_wall_step_cannot_admit_after_a_late_anchor(self):
        # Review replay: first age 750 s, then the wall clock steps back 165 s
        # so a second read alone would be 585 s and E=600.
        h = Harness()
        h.env["ARX_DIAG_T0"] = str(int(h.clock.time()) - 750)
        seen = self._shift_later_wall_read(h.clock, -165)
        code, lines = h.run()
        self.assertGreaterEqual(seen["n"], 2)
        self.assertEqual(code, 0)
        self.assertEqual(h.field("outcome"), "skipped_budget")
        self.assertEqual(h.field("e_s"), str(750 + w.ANCHOR_PRE_S))
        self.assertEqual(lines, [lines[0]])
        self.assertNotIn("guard", lines[0])
        self.assertEqual(h.log, [])

    def test_forward_wall_step_uses_the_later_age(self):
        # First age would admit (585 s, E=600). A forward step to 750 s must skip.
        h = Harness()
        h.env["ARX_DIAG_T0"] = str(int(h.clock.time()) - 585)
        seen = self._shift_later_wall_read(h.clock, 165)
        code, lines = h.run()
        self.assertGreaterEqual(seen["n"], 2)
        self.assertEqual(code, 0)
        self.assertEqual(h.field("outcome"), "skipped_budget")
        self.assertEqual(h.field("e_s"), str(750 + w.ANCHOR_PRE_S))
        self.assertEqual(lines, [lines[0]])
        self.assertEqual(h.log, [])


class InventoryTests(unittest.TestCase):
    def check(self, data, reason):
        with self.assertRaises(w.Guard) as g:
            w.check_inventory(data, NAMES, FROOT)
        self.assertEqual(g.exception.reason, reason)

    def test_exact_eight_ignored_pass(self):
        w.check_inventory(listing(), NAMES, FROOT)

    def test_unsampled_cases_mismatch_and_unexpected_match_gates(self):
        doc = json.loads(listing())
        cases = doc["rust-suites"]["arxd"]["testcases"]
        names = ["r1_apply_wait_deadline", "r2_isolated_read_refusal",
                 "r3_store_wait_deadline_and_heartbeat", "r4_read_admission_and_caller_abort",
                 "r5_shutdown_read_owners"]
        for name in names:
            cases[w.TEST_PREFIX + name] = {
                "kind": "test", "ignored": True,
                "filter-match": {"status": "mismatch", "reason": "expression"}}
        doc["test-count"] = len(cases)
        w.check_inventory(json.dumps(doc).encode(), NAMES, FROOT)
        for name in names:
            with self.subTest(name=name):
                case = cases[w.TEST_PREFIX + name]
                case["filter-match"] = {"status": "matches"}
                self.check(json.dumps(doc).encode(), "inventory")
                case["filter-match"] = {"status": "mismatch", "reason": "expression"}

    def test_unsampled_filter_matches_verified_guard_inventory(self):
        # Capture the namespace executed by the real verified-bytes loader.
        with patch("builtins.exec", wraps=exec) as execute:
            self.assertEqual(sorted(w.load_expected(w.RealFs(), str(REPO))), sorted(NAMES))
        execute.assert_called_once()
        namespace = execute.call_args.args[1]
        excluded = re.findall(r" & not test\(=cluster::consensus::three_node::([a-z0-9_]+)\)", w.FILTER)
        self.assertEqual(excluded, namespace["WINDOWS_UNSAMPLED"])
        self.assertEqual(excluded, ['r1_apply_wait_deadline', 'r2_isolated_read_refusal', 'r3_store_wait_deadline_and_heartbeat', 'r4_read_admission_and_caller_abort', 'r5_shutdown_read_owners'])
        self.assertTrue(set(excluded).isdisjoint(NAMES))

    def test_ignored_unsampled_expression_mismatch_passes_but_match_gates(self):
        doc = json.loads(listing())
        cases = doc["rust-suites"]["arxd"]["testcases"]
        for name in ['r1_apply_wait_deadline', 'r2_isolated_read_refusal', 'r3_store_wait_deadline_and_heartbeat', 'r4_read_admission_and_caller_abort', 'r5_shutdown_read_owners']:
            with self.subTest(name=name):
                case = {"kind": "test", "ignored": True,
                        "filter-match": {"status": "mismatch", "reason": "expression"}}
                cases[w.TEST_PREFIX + name] = case
                doc["test-count"] = len(cases)
                w.check_inventory(json.dumps(doc).encode(), NAMES, FROOT)
                case["filter-match"] = {"status": "matches"}
                self.check(json.dumps(doc).encode(), "inventory")
                case["filter-match"] = {"status": "mismatch", "reason": "expression"}

    def test_rejections(self):
        self.check(listing(NAMES[:7]), "inventory")
        self.check(listing(NAMES + ["t9_extra_fixture"]), "inventory")
        self.check(listing(NAMES[:7] + ["d1_lag_control_positive"]), "inventory")
        self.check(listing(binary="arxd-other"), "inventory")
        self.check(listing(kind="test"), "inventory")
        self.check(listing(ignored=False), "inventory")
        self.check(listing(target=J(FTEMP, "elsewhere")), "inventory")
        other = json.loads(listing())["rust-suites"]["arxd"]
        self.check(listing(extra={"arxd::bin/arxd": dict(other, **{"binary-id": "arxd::bin/arxd"})}),
                   "inventory")
        self.check(b"{not json", "list_parse")
        self.check(b"[]", "list_parse")
        self.check(b"x" * (w.READ_CAP + 1), "list_parse")

    def test_expected_comes_from_the_fixed_guard_file(self):
        self.assertEqual(sorted(w.load_expected(w.RealFs(), str(REPO))), sorted(NAMES))
        fs = FakeFs()
        self.assertEqual(sorted(w.load_expected(fs, FROOT)), sorted(NAMES))
        fs.files[J(FROOT, *w.GUARD_REL)] = GUARD_SRC.replace(
            b"'r6_shutdown_report_store_only', ", b"")
        with self.assertRaises(w.Guard) as g:
            w.load_expected(fs, FROOT)
        self.assertEqual(g.exception.reason, "inventory")
        fs = FakeFs()
        fs.unreal.add(J(FROOT, *w.GUARD_REL))
        with self.assertRaises(w.Guard) as g:
            w.load_expected(fs, FROOT)
        self.assertEqual(g.exception.reason, "reparse")
        fs = FakeFs()
        fs.kinds[J(FROOT, "tools", "ci")] = "reparse"
        with self.assertRaises(w.Guard) as g:
            w.load_expected(fs, FROOT)
        self.assertEqual(g.exception.reason, "reparse")

    def test_id_map_matches_guard(self):
        self.assertEqual(sorted(w.IDS.values()), sorted(w.ID_ORDER))


class JunitTests(unittest.TestCase):
    TYPES = [("test failure with exit code 101", "failure", "fail"),
             ("test failure with exit code 1 (leaked handles)", "failure", "fail"),
             ("test abort", "failure", "abort"),
             ("test abort (leaked handles)", "failure", "abort"),
             ("test timeout", "failure", "timeout"),
             ("execution failure", "error", "exec_fail"),
             ("test exited with code 0, but leaked handles so was marked failed", "error", "leak"),
             ("test passed but leaked handles", "error", "leak")]

    def test_every_kind_from_nextest_strings(self):
        for ty, tag, kind in self.TYPES:
            with self.subTest(ty=ty):
                cases = [case(NAMES[0], failure(ty, tag))] + [case(n) for n in NAMES[1:]]
                outcome, results = w.parse_junit(junit(cases), 100)
                self.assertEqual(outcome, "diag_fail")
                self.assertEqual(results[w.ID_ORDER.index(w.IDS[NAMES[0]])].kind, kind)

    def test_unknown_type_and_tag_mismatch_gate(self):
        for status in (failure("flaky failure"), failure("test failure with exit code 0"),
                       failure("test failure with exit code -1"), failure("test timeout", "error"),
                       '<skipped message="x"/>', failure("test abort") + failure("test abort")):
            with self.subTest(status=status), self.assertRaises(w.Guard) as g:
                w.parse_junit(junit([case(NAMES[0], status)] + [case(n) for n in NAMES[1:]]), 100)
            self.assertEqual(g.exception.reason, "junit_type")

    def test_inventory_breaks_gate(self):
        dup = [case(n) for n in NAMES] + [case(NAMES[0])]
        extra = [case(n) for n in NAMES] + [case("t9_unexpected")]
        wrong = [case(NAMES[0], classname="arxd::bin/arxd")] + [case(n) for n in NAMES[1:]]
        for data in (junit(dup), junit(extra), junit(wrong), junit([], suite="other"),
                     b"<root/>", junit([case(n, time="1,5") for n in NAMES])):
            with self.subTest(data=data[:60]), self.assertRaises(w.Guard) as g:
                w.parse_junit(data, 100)
            self.assertEqual(g.exception.reason, "junit_inventory")

    def test_not_run_only_at_exit_100(self):
        part = junit([case(n) for n in NAMES[:5]])
        outcome, results = w.parse_junit(part, 100)
        self.assertEqual(outcome, "diag_fail")
        self.assertEqual([r.kind for r in results].count("not_run"), 3)
        with self.assertRaises(w.Guard) as g:
            w.parse_junit(part, 0)
        self.assertEqual(g.exception.reason, "junit_inventory")

    def test_all_pass_only_at_exit_zero(self):
        self.assertEqual(w.parse_junit(all_pass_junit(), 0)[0], "all_pass")
        self.assertEqual(w.parse_junit(all_pass_junit(), 100)[0], "diag_fail")

    def test_oversize_and_malformed_are_diagnostic(self):
        big = all_pass_junit() + b" " * (w.READ_CAP + 1)
        self.assertEqual(w.parse_junit(big, 100), ("diag_unparsed", []))
        self.assertEqual(w.parse_junit(all_pass_junit()[:200], 100), ("diag_unparsed", []))
        self.assertEqual(w.parse_junit(b'<!DOCTYPE x [<!ENTITY a "b">]><testsuites/>', 100),
                         ("diag_unparsed", []))


class GrammarTests(unittest.TestCase):
    def test_each_form_rerenders_byte_identical(self):
        for key, line in D1.items():
            with self.subTest(form=key):
                body = w.parse_d1(line)
                self.assertEqual(body, line[len("[DIAG-3n] "):])
                r = w.TestResult("T1", "fail", 1)
                w.extract_diag(r, line, [100, 100000])
                self.assertEqual(r.diag, ["ARXDIAG v1 diag id=T1 " + body])

    def test_ring_and_lagev_bounds(self):
        pairs = ", ".join("(%d, 100)" % i for i in range(8))
        self.assertIsNotNone(w.parse_d1("[DIAG-3n] lagev [%s]" % pairs))
        self.assertIsNone(w.parse_d1("[DIAG-3n] lagev [%s, (9, 100)]" % pairs))
        self.assertIsNotNone(w.parse_d1("[DIAG-3n] n1 ring" + (" " + SAMPLE) * 32))
        r128 = "[DIAG-3n] n1 ring" + (" " + "1,1,L,1,1,1,1,1") * 128
        r129 = "[DIAG-3n] n1 ring" + (" " + "1,1,L,1,1,1,1,1") * 129
        self.assertIsNotNone(w.parse_d1(r128))
        self.assertIsNone(w.parse_d1(r129))

    def test_rejects(self):
        bad = [
            D1["fail"].replace("lag=400", "lag=0400"),           # leading zero
            D1["attr"].replace("node=2", "node=256"),             # u8 overflow
            D1["fail"].replace("t=45001", "t=4294967296"),       # u32 overflow
            D1["fail"].replace("cand=Both", "cand=Huge"),        # unknown enum
            D1["report"].replace("first: Owners", "first: Disk"),
            D1["report"].replace("engine_ms: 12", "engine_ms: 65536"),
            D1["s3"].replace(",L,", ",X,"),
            "[DIAG-3n] attr C:\\Users\\secret\\file.rs",          # a path
            "[DIAG-3n] attr n\u00f6ne",                            # non-ASCII
            "[DIAG-3n] end bytes=8193 trunc=0",
            "[DIAG-3n] end bytes=10 trunc=2",
            D1["fail"] + " ",
            D1["fail"] + " extra=1",
            "[DIAG-3n] ring n1",
            "[DIAG-3n] n4 ring",
            "[DIAG-3n] " + "pass " + "x" * 9000,
            D1["helpers"].replace("leader=3:120:80", "leader=3:18446744073709551616:80"),
        ]
        for line in bad:
            with self.subTest(line=line[:70]):
                self.assertIsNone(w.parse_d1(line))
        r = w.TestResult("R6c", "fail", 1)
        w.extract_diag(r, "\n".join(bad), [100, 100000])
        self.assertEqual((r.diag, r.rejects, r.dropped_lines), ([], len(bad), 0))

    def test_indented_and_midline_copies_are_ignored_not_counted(self):
        r = w.TestResult("T2", "fail", 1)
        text = "  " + D1["attr_none"] + "\nnoise " + D1["attr_none"] + "\n" + D1["attr_none"]
        w.extract_diag(r, text, [100, 100000])
        self.assertEqual((len(r.diag), r.rejects, r.dropped_lines), (1, 0, 0))

    def test_seventeenth_line_and_bytes_are_truncated_with_exact_counts(self):
        r = w.TestResult("T3", "fail", 1)
        w.extract_diag(r, "\n".join([D1["attr_none"]] * 17 + ["[DIAG-3n] bogus"]), [100, 100000])
        self.assertEqual(len(r.diag), 16)
        n = len("ARXDIAG v1 diag id=T3 attr none") + 1
        self.assertEqual((r.dropped_lines, r.dropped_bytes, r.rejects), (1, n, 1))
        wide = "4294967295,18446744073709551615,L,18446744073709551615,-,-,-,-"
        ring = "[DIAG-3n] n1 ring" + (" " + wide) * 60
        self.assertGreater(len(ring), 3600)
        r = w.TestResult("T4", "fail", 1)
        w.extract_diag(r, "\n".join([ring] * 3), [100, 100000])
        self.assertEqual((len(r.diag), r.dropped_lines), (2, 1))
        self.assertLessEqual(sum(len(x) + 1 for x in r.diag), w.DIAG_BYTES_PER_TEST)

    def test_step_line_cap_holds_with_full_dumps(self):
        dump = "\n".join([D1["fail"]] * 16 + ["[DIAG-3n] x"])
        data = junit([case(n, failure("test timeout"), dump) for n in NAMES])
        outcome, results = w.parse_junit(data, 100)
        rep = w.Report(event="push", outcome=outcome, nextest="100", e_s=315, tests=results)
        lines = w.render(rep)
        self.assertLessEqual(len(lines), w.STEP_LINES)
        self.assertLessEqual(sum(len(x) + 1 for x in lines), w.STEP_BYTES)
        self.assertEqual(sum(r.dropped_lines for r in results), 8 * 16 - 125)
        self.assertTrue(all(x.startswith("ARXDIAG v1 ") for x in lines))

    def test_released_lines_never_carry_raw_output(self):
        err = "C:\\Users\\secret\\target\\debug\\x.exe\nthread panicked\nRPC counters 1\n" + D1["pass"]
        data = junit([case(NAMES[0], failure("test failure with exit code 101"), err)]
                     + [case(n) for n in NAMES[1:]])
        outcome, results = w.parse_junit(data, 100)
        lines = w.render(w.Report(event="pull_request", outcome=outcome, nextest="100", e_s=1,
                                  tests=results))
        blob = "\n".join(lines)
        for leak in ("secret", "\\", "panicked", "RPC", "x.exe"):
            self.assertNotIn(leak, blob)

    def test_d1_format_literals_still_in_source(self):
        src = (REPO / "crates/arxd/src/cluster/consensus/diagnostics.rs").read_text(encoding="utf-8")
        for lit in ('"[DIAG-3n] pass"', '" t={} lag={}@{} over={} ticks={}"', '" n{}"',
                    '" term={t} ldr={l} seen={seen}"', '" ring=busy"', '" high={}"',
                    '"{op:?}={}"', '" max {} queue={} leader={}:{}:{} write={}:{}:{} cand={:?}"',
                    '"[DIAG-3n] v1 fail t={} lag={}@{} over={} ticks={} cand={:?}"',
                    '"[DIAG-3n] lagev {:?}"', '.take(8)', '"[DIAG-3n] lagev busy"',
                    '"[DIAG-3n] helpers leader={}:{}:{} write={}:{}:{} converge={} stop={}"',
                    '"[DIAG-3n] attr busy"', '"[DIAG-3n] attr none"',
                    '"[DIAG-3n] attr helper={:?} fail={:?} node={} t={} polls={} retries={}"',
                    '"[DIAG-3n] s3 before="', '" after="', '" class={c:?}"',
                    '"[DIAG-3n] report {r:?}"', '"[DIAG-3n] at"', '" n{}="',
                    '"[DIAG-3n] n{} ops"', '" {op:?}="', '" queue="', '" delay={}"',
                    '" seen={a} ovw={b} term={c} ldr={d} st={e}"', '"[DIAG-3n] n{} ring"',
                    '" busy"', '"[DIAG-3n] end bytes={} trunc={trunc}\\n"',
                    '"{}:{}:{}:{}:{}^{}"', '"{},{},{},{},{},{},{},{}"',
                    "pub(super) const RING: usize = 128;", "pub(super) const OUT_MAX: usize = 8192;",
                    "b'L'", "b'F'", "b'C'", "b'l'", "b'S'"):
            with self.subTest(lit=lit):
                self.assertIn(lit, src)
        enums = {"Op": w.OPS, "Helper": w.HELPER, "Fail": w.FAIL, "S3": w.S3, "Candidate": w.CAND}
        for name, want in enums.items():
            body = re.search(r"pub\(super\) enum %s \{([^}]*)\}" % name, src).group(1)
            self.assertEqual(tuple(re.findall(r"(\w+),", body)), want, name)
        core = (REPO / "crates/arxd/src/cluster/consensus.rs").read_text(encoding="utf-8")
        for name, want in {"Phase": w.PHASE, "EngineEnd": w.ENGINE}.items():
            body = re.search(r"pub\(super\) enum %s \{([^}]*)\}" % name, core).group(1)
            self.assertEqual(tuple(re.findall(r"(\w+),", body)), want, name)
        body = re.search(r"pub\(super\) struct ShutdownReport \{([^}]*)\}", core).group(1)
        fields = re.findall(r"pub (\w+): (\w+),", body)
        types = {w.U8: "u8", w.U16: "u16", w.PHASE: "Phase", w.ENGINE: "EngineEnd"}
        self.assertEqual(fields, [(n, types[t]) for n, t in w.REPORT_FIELDS])


class OwnershipTests(unittest.TestCase):
    def phase(self, api_kwargs=None, proc_kwargs=None, deadline=60.0, cancel=False):
        log, clock = [], FakeClock()
        api = FakeJobApi(log, **(api_kwargs or {}))
        spawner = FakeSpawner(log, {"run": lambda: FakeProc(clock, **(proc_kwargs or {}))})
        ev = threading.Event()
        if cancel:
            ev.set()
        return log, clock, api, spawner, ev, lambda: w.run_phase(
            api, spawner, clock, ev, "run", PRIV, clock.t + deadline)

    def test_order_spawn_assign_check_cancel_then_go(self):
        log, clock, api, sp, ev, go = self.phase()
        res = go()
        self.assertEqual(log[:4], ["create", "spawn:run", "assign", "in_job"])
        self.assertEqual(sp.procs[0].stdin.data, b"G")
        self.assertEqual((res.status, res.rc, res.orphans), ("exit", 0, 0))
        self.assertEqual(api.terminated, 1)
        self.assertEqual(log.count("terminate"), 1)
        self.assertEqual(log[-1], "close")

    def test_assign_failure_kills_owned_child_without_go(self):
        for kwargs in ({"assign_ok": False}, {"in_job_ok": False}):
            with self.subTest(kwargs=kwargs):
                log, clock, api, sp, ev, go = self.phase(kwargs, {"run_s": None})
                with self.assertRaises(w.Guard) as g:
                    go()
                self.assertEqual(g.exception.reason, "job")
                p = sp.procs[0]
                self.assertEqual((p.stdin.data, p.kills, p.returncode), (b"", 1, 1))
                self.assertTrue(p.stdin.closed)
                self.assertNotIn("terminate", log)
                self.assertEqual(log[-1], "close")

    def test_unkillable_unassigned_child_gates_cleanup_within_two_seconds(self):
        log, clock, api, sp, ev, go = self.phase({"assign_ok": False},
                                                {"run_s": None, "kill_works": False})
        t = clock.t
        with self.assertRaises(w.Guard) as g:
            go()
        self.assertEqual(g.exception.reason, "cleanup")
        self.assertLessEqual(clock.t - t, w.UNASSIGNED_REAP_S + 0.2)

    def test_cancel_between_spawn_and_assign_kills_child_without_go(self):
        log, clock, api, sp, ev, go = self.phase(proc_kwargs={"run_s": None})
        api.on_assign = ev.set
        res = go()
        self.assertEqual(res.status, "cancelled")
        self.assertEqual((sp.procs[0].stdin.data, sp.procs[0].kills), (b"", 1))

    def test_create_and_spawn_failures(self):
        log, clock, api, sp, ev, go = self.phase({"create_fail": True})
        with self.assertRaises(w.Guard) as g:
            go()
        self.assertEqual((g.exception.reason, log), ("job", ["create"]))
        log, clock, api, sp, ev, go = self.phase()
        sp.fail = True
        with self.assertRaises(w.Guard) as g:
            go()
        self.assertEqual((g.exception.reason, log[-1]), ("spawn", "close"))

    def test_deadline_terminates_once_and_polls_active(self):
        log, clock, api, sp, ev, go = self.phase(proc_kwargs={"run_s": None}, deadline=5.0,
                                                 api_kwargs={"active": [3, 2, 0]})
        res = go()
        self.assertEqual((res.status, res.rc, res.orphans), ("killed", None, 3))
        self.assertEqual(log.count("terminate"), 1)
        self.assertGreaterEqual(log.count("active"), 3)
        self.assertAlmostEqual(res.ms, 5000, delta=150)

    def test_cleanup_gates_when_active_never_reaches_zero(self):
        log, clock, api, sp, ev, go = self.phase(api_kwargs={"active": [1]})
        t = clock.t
        with self.assertRaises(w.Guard) as g:
            go()
        self.assertEqual(g.exception.reason, "cleanup")
        self.assertEqual(log.count("terminate"), 1)
        self.assertLessEqual(clock.t - t, 1.0 + w.ACTIVE_POLL_S + 0.3)
        self.assertEqual(log[-1], "close")

    def test_cleanup_gates_when_child_survives_terminate(self):
        log, clock, api, sp, ev, go = self.phase(proc_kwargs={"run_s": None,
                                                              "exit_on_terminate": False},
                                                 deadline=1.0)
        with self.assertRaises(w.Guard) as g:
            go()
        self.assertEqual(g.exception.reason, "cleanup")

    def test_cancel_during_wait(self):
        log, clock, api, sp, ev, go = self.phase(proc_kwargs={"run_s": None})
        orig = clock.sleep

        def sleep(s):
            orig(s)
            if clock.t > 1002:
                ev.set()
        clock.sleep = sleep
        res = go()
        self.assertEqual(res.status, "cancelled")
        self.assertEqual(log.count("terminate"), 1)


class OutcomeTests(unittest.TestCase):
    def test_pre_go_expiry_inside_step_budget_is_integrity_failure(self):
        for seam in ("create", "spawn", "assign", "stale_junit"):
            with self.subTest(seam=seam):
                h = Harness()
                if seam == "create":
                    original = h.api.create
                    def create():
                        job = original()
                        if h.api.created == 2:
                            h.clock.sleep(65)
                        return job
                    h.api.create = create
                elif seam == "spawn":
                    original = h.spawner.spawn
                    def spawn(phase, privdir):
                        proc = original(phase, privdir)
                        if phase == "run":
                            h.clock.sleep(65)
                        return proc
                    h.spawner.spawn = spawn
                elif seam == "assign":
                    def assign():
                        if h.api.created == 2:
                            h.clock.sleep(65)
                    h.api.on_assign = assign
                else:
                    h.fs.files[JUNIT] = b"stale"
                    original = h.fs.unlink
                    def unlink(p):
                        original(p)
                        if p == JUNIT and "spawn:run" not in h.log:
                            h.clock.sleep(82)
                    h.fs.unlink = unlink
                code, lines = h.run()
                self.assertLess(h.clock.t - 1000, w.STEP_S)
                self.assertEqual(code, 1)
                self.assertIn("guard reason=deadline", "\n".join(lines))
                self.assertEqual(h.field("outcome"), "guard")
                if len(h.spawner.procs) > 1:
                    self.assertIsNone(h.spawner.procs[1].go_at)
                    self.assertEqual(h.spawner.procs[1].kills, 1)

    def test_unexpected_cleanup_kind_is_never_removed_and_gates(self):
        h = Harness()
        read = h.fs.read_capped
        def changed(p, cap):
            data = read(p, cap)
            if p == JUNIT:
                h.fs.kinds[LISTING] = "reparse"
            return data
        h.fs.read_capped = changed
        self.assertEqual(h.run()[0], 1)
        self.assertNotIn(LISTING, h.fs.unlinked)
        self.assertIn(PRIV, h.fs.removed)
        self.assertIn("guard reason=cleanup", "\n".join(h.lines))

    def test_expired_launch_never_sends_go(self):
        for phase in ("list", "run"):
            h = Harness()
            spawn = h.spawner.spawn
            def delayed(p, d):
                proc = spawn(p, d)
                if p == phase:
                    h.clock.sleep(95)
                return proc
            h.spawner.spawn = delayed
            self.assertEqual(h.run()[0], 1)
            self.assertIsNone(h.spawner.procs[-1].go_at)
            self.assertEqual(h.spawner.procs[-1].kills, 1)

    def test_expired_assignment_reaps_without_go(self):
        h = Harness()
        h.api.on_assign = lambda: h.clock.sleep(95)
        self.assertEqual(h.run()[0], 1)
        self.assertIsNone(h.spawner.procs[0].go_at)
        self.assertEqual(h.spawner.procs[0].kills, 1)

    def test_phase_cleanup_poll_uses_common_deadline(self):
        h = Harness(run_s=None, api_kwargs={"active": [1]})
        # A live owned child still must be killed, even with no wait budget.
        h.spawner.makers["run"] = lambda: FakeProc(h.clock, run_s=None)
        with self.assertRaises(w.Guard) as caught:
            w.run_phase(h.api, h.spawner, h.clock, h.cancel, "run", PRIV,
                        h.clock.t + 1, h.clock.t + 2)
        self.assertEqual(caught.exception.reason, "cleanup")
        self.assertLessEqual(h.clock.t, 1002.0001)
        self.assertEqual(h.api.terminated, 1)
        self.assertEqual(h.api.closed, 1)

    def test_expired_inventory_never_starts_run(self):
        h = Harness()
        read = h.fs.read_capped
        def delayed(p, cap):
            data = read(p, cap)
            if p == LISTING:
                h.clock.sleep(82)
            return data
        h.fs.read_capped = delayed
        self.assertEqual(h.run()[0], 1)
        self.assertNotIn("spawn:run", h.log)

    def test_delayed_validation_final_read_and_cleanup_cannot_pass(self):
        for where in ("validation", "junit", "cleanup"):
            h = Harness()
            read, remove = h.fs.read_capped, h.fs.rmdir
            def delayed(p, cap):
                data = read(p, cap)
                if ((where == "validation" and p.endswith(w.GUARD_REL[-1]))
                        or (where == "junit" and p == JUNIT)):
                    h.clock.sleep(95 if where == "junit" else 3)
                return data
            def delayed_remove(p):
                remove(p)
                h.clock.sleep(95)
            h.fs.read_capped = delayed
            if where == "cleanup":
                h.fs.rmdir = delayed_remove
            self.assertEqual(h.run()[0], 1)
            self.assertEqual(h.field("outcome"), "guard")
            if where == "validation":
                self.assertFalse(h.spawner.procs)

    def test_failed_final_cleanup_gates_and_attempts_remaining_paths(self):
        for failed in (LISTING, JUNIT, PRIV):
            h = Harness()
            unlink, rmdir = h.fs.unlink, h.fs.rmdir
            attempts = []
            def remove(p):
                attempts.append(p)
                if p == failed:
                    raise PermissionError("private")
                (rmdir if p == PRIV else unlink)(p)
            h.fs.unlink = h.fs.rmdir = remove
            self.assertEqual(h.run()[0], 1)
            self.assertEqual(h.field("outcome"), "guard")
            self.assertEqual(attempts[-3:], [LISTING, JUNIT, PRIV])

    def test_all_pass_exit_zero_and_order(self):
        h = Harness()
        code, lines = h.run()
        self.assertEqual(code, 0)
        self.assertEqual(h.field("outcome"), "all_pass")
        self.assertEqual(h.field("nextest"), "0")
        self.assertEqual(h.field("orphans"), "0")
        self.assertEqual([l.split()[3] for l in lines[1:]], ["id=" + i for i in w.ID_ORDER])
        self.assertEqual(h.log.count("create"), 2)
        self.assertEqual(h.log.count("terminate"), 2)
        self.assertIn(JUNIT, h.fs.unlinked)
        self.assertIn(PRIV, h.fs.removed)

    def test_pass_line_from_tests_is_released_typed(self):
        h = Harness(junit_bytes=all_pass_junit(err=D1["pass"] + "\nT3 elapsed=1s"))
        code, lines = h.run()
        self.assertEqual(code, 0)
        self.assertIn("ARXDIAG v1 diag id=T1 " + D1["pass"][10:], lines)

    def test_diag_fail_exit_zero(self):
        cases = [case(NAMES[0], failure("test timeout"), D1["fail"])] + [case(n) for n in NAMES[1:]]
        h = Harness(run_rc=100, junit_bytes=junit(cases))
        self.assertEqual(h.run()[0], 0)
        self.assertEqual((h.field("outcome"), h.field("nextest")), ("diag_fail", "100"))

    def test_global_timeout_partial_report_is_diagnostic(self):
        cases = [case(n, failure("test abort")) for n in NAMES[:3]]
        h = Harness(run_rc=100, junit_bytes=junit(cases))
        code, lines = h.run()
        self.assertEqual((code, h.field("outcome")), (0, "diag_fail"))
        self.assertEqual(sum("kind=not_run" in l for l in lines), 5)

    def test_unparsed_report_exit_zero_without_test_lines(self):
        for data in (all_pass_junit()[:150], b"x" * (w.READ_CAP + 5)):
            h = Harness(run_rc=100, junit_bytes=data)
            code, lines = h.run()
            self.assertEqual((code, h.field("outcome"), len(lines)), (0, "diag_unparsed", 1))

    def test_external_deadline_kill_is_incomplete_and_not_parsed(self):
        for leave in (None, all_pass_junit()[:100]):
            with self.subTest(leave=leave is not None):
                h = Harness(run_s=None)
                if leave is not None:
                    h.spawner.makers["run"] = lambda: FakeProc(
                        h.clock, run_s=None, on_kill=lambda: h.fs.files.__setitem__(JUNIT, leave))
                code, lines = h.run()
                self.assertEqual((code, len(lines)), (0, 1))
                self.assertEqual((h.field("outcome"), h.field("nextest")),
                                 ("diag_incomplete", "killed"))
                self.assertNotIn(JUNIT, h.fs.reads)
                self.assertLessEqual(int(h.field("wall_ms")), 90_000)
                self.assertLessEqual(int(h.field("run_ms")), 60_000)

    def test_run_deadline_is_clipped_by_absolute_step_budget(self):
        # 10 s inventory processing: D - 8 s binds before run + 60 s.
        h = Harness(run_s=None, list_kwargs={"run_s": 19.0})
        read = h.fs.read_capped
        def delayed(p, cap):
            data = read(p, cap)
            if p == LISTING:
                h.clock.sleep(10)
            return data
        h.fs.read_capped = delayed
        code, h.lines = w.run(h.ctx)
        self.assertEqual((code, h.field("outcome")), (0, "diag_incomplete"))
        self.assertLessEqual(int(h.field("wall_ms")), 90_000)
        self.assertAlmostEqual(int(h.field("run_ms")), 72_000 - 19_000, delta=300)

    def test_gating_outcomes(self):
        cases = [
            ("list_exit", dict(list_rc=101)),
            ("deadline_list", dict(list_kwargs={"run_s": None})),
            ("inventory", dict(listing_bytes=listing(NAMES[:7]))),
            ("list_parse", dict(listing_bytes=b"{")),
            ("nextest_exit", dict(run_rc=101)),
            ("nextest_exit", dict(run_rc=110)),
            ("nextest_exit", dict(run_rc=4)),
            ("job", dict(api_kwargs={"assign_ok": False})),
            ("cleanup", dict(api_kwargs={"active": [1]})),
        ]
        for reason, kwargs in cases:
            with self.subTest(reason=reason, kwargs=kwargs):
                h = Harness(**kwargs)
                code, lines = h.run()
                self.assertEqual((code, lines[-1]), (1, "ARXDIAG v1 guard reason=" + reason))
                self.assertEqual(h.field("outcome"), "guard")
                self.assertEqual(len(lines), 2)

    def test_junit_missing_at_exit_100_gates(self):
        h = Harness(run_rc=100)
        h.spawner.makers["run"] = lambda: FakeProc(h.clock, rc=100, run_s=1.0)
        self.assertEqual(h.run()[1][-1], "ARXDIAG v1 guard reason=junit_missing")

    def test_platform_reparse_and_event(self):
        h = Harness()
        h.ctx.in_venv = True
        self.assertEqual(h.run()[1][-1], "ARXDIAG v1 guard reason=platform")
        h = Harness()
        h.ctx.is_windows = False
        self.assertEqual(h.run()[1][-1], "ARXDIAG v1 guard reason=platform")
        h = Harness()
        del h.env["RUNNER_TEMP"]
        self.assertEqual(h.run()[1][-1], "ARXDIAG v1 guard reason=platform")
        for path in (J(FROOT, "target"), J(FROOT, "target", "nextest"), FTEMP, JUNIT):
            with self.subTest(path=path):
                h = Harness()
                h.fs.kinds[path] = "reparse"
                self.assertEqual(h.run()[1][-1], "ARXDIAG v1 guard reason=reparse")
                self.assertEqual(h.log, [])
        h = Harness(env={"GITHUB_EVENT_NAME": "workflow_dispatch"})
        code, lines = h.run()
        self.assertEqual((code, h.field("outcome"), h.field("event"), h.log),
                         (0, "skipped_event", "-", []))

    def test_stale_junit_removed_before_run(self):
        h = Harness()
        h.fs.files[JUNIT] = b"stale"
        h.run()
        self.assertEqual(h.fs.unlinked.count(JUNIT), 2)
        self.assertEqual(h.field("outcome"), "all_pass")

    def test_cancel_before_spawn(self):
        h = Harness()
        h.cancel.set()
        self.assertEqual(h.run()[0], 0)
        self.assertEqual((h.field("outcome"), h.log), ("cancelled", []))

    def test_internal_without_traceback(self):
        h = Harness()

        def boom(p):
            raise RuntimeError("C:\\secret path")
        h.fs.kind = boom
        self.assertEqual(h.run(), (1, ["ARXDIAG v1 guard reason=internal"]))

    def test_main_never_prints_traceback_and_rejects_argv(self):
        saved = {s: signal.getsignal(s) for s in (signal.SIGINT,)}
        try:
            for argv, factory in ((["x", "extra"], None),
                                  (["x"], lambda c: (_ for _ in ()).throw(RuntimeError("x")))):
                out = io.StringIO()
                self.assertEqual(w.main(argv, make_ctx=factory, out=out), 1)
                self.assertEqual(out.getvalue(), "ARXDIAG v1 guard reason=internal\n")
        finally:
            for s, hnd in saved.items():
                signal.signal(s, hnd)


class SourceTests(unittest.TestCase):
    SRC = (REPO / "tools/ci/windows_diag_sample.py").read_text(encoding="utf-8")

    def test_no_unowned_process_control(self):
        for token in ("taskkill", "psutil", "os.kill", "shell=True", "pid", "CreateToolhelp32",
                      "OpenProcess"):
            self.assertNotIn(token, self.SRC)

    def test_fixed_argv_matches_brief(self):
        head = ["--workspace", "--locked", "--cargo-quiet", "--user-config-file", "none",
                "--config-file", "tools/ci/windows-diag-sample.nextest.toml", "-P", "diag-sample",
                "--run-ignored", "only"]
        expr = ["-E", "package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/) & not test(=cluster::consensus::three_node::r1_apply_wait_deadline) & not test(=cluster::consensus::three_node::r2_isolated_read_refusal) & not test(=cluster::consensus::three_node::r3_store_wait_deadline_and_heartbeat) & not test(=cluster::consensus::three_node::r4_read_admission_and_caller_abort) & not test(=cluster::consensus::three_node::r5_shutdown_read_owners)"]
        # `list` has no --no-tests flag in nextest 0.9.151 (usage error, exit 2).
        self.assertEqual(w.cargo_argv("list"),
                         ["cargo", "nextest", "list", "-T", "json"] + head + expr)
        self.assertEqual(w.cargo_argv("run"),
                         ["cargo", "nextest", "run"] + head + ["--no-tests", "fail"] + expr)
        with self.assertRaises(w.Guard):
            w.cargo_argv("anything")

    def test_profile_bounds(self):
        cfg = (REPO / "tools/ci/windows-diag-sample.nextest.toml").read_text(encoding="utf-8")
        for line in ('retries = 0', 'fail-fast = false', 'test-threads = 4',
                     'global-timeout = "50s"',
                     'slow-timeout = { period = "24s", terminate-after = 2, grace-period = "2s" }',
                     '[profile.diag-sample.junit]', 'path = "junit.xml"',
                     'store-success-output = true', 'store-failure-output = true'):
            self.assertIn(line + "\n", cfg)
        self.assertEqual((w.STEP_S, w.LIST_S, w.RUN_S, w.CLEANUP_RESERVE_S), (90.0, 20.0, 60.0, 8.0))
        self.assertEqual(w.CHILD_EXIT_S + w.ACTIVE_POLL_S, w.CLEANUP_RESERVE_S)

    def test_child_privdir_validation(self):
        fs = FakeFs()
        fs.kinds[PRIV] = "dir"
        env = {"RUNNER_TEMP": FTEMP}
        self.assertTrue(w.valid_privdir(PRIV, env, fs))
        for bad in (J(FTEMP, "other-abc"), J(FROOT, "arxdiag-abc123"), "arxdiag-abc123",
                    J(FTEMP, "arxdiag-ab..c"), J(FTEMP, "arxdiag-missing1")):
            self.assertFalse(w.valid_privdir(bad, env, fs), bad)
        fs.kinds[PRIV] = "reparse"
        self.assertFalse(w.valid_privdir(PRIV, env, fs))


if __name__ == "__main__":
    unittest.main()
