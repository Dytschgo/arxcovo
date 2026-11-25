"""Bounded, non-gating Windows three-node diagnostic sample (hosted CI only).

Contract: docs/briefs/m3-windows-hosted-diagnostic-sample.md and the Sol recheck in
docs/reviews/m3-hosted-sample-sol-contract-review.md. Test behavior never gates:
only selection, ownership and wrapper integrity exit 1. Every released line is
re-rendered from typed, range-checked values; raw test output, paths, list JSON,
failure exit codes and exceptions stay private. Takes no input that selects argv,
paths, test IDs or module code. Ownership uses only this wrapper's own child
handle and its own kill-on-close Job Object; it never walks or signals unowned
processes. A green sample is not re-enable evidence and authorizes no fix.
"""
import importlib.util
import json
import os
import re
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field

SCRIPT = os.path.abspath(__file__)
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(SCRIPT)))

# --- Fixed bounds (seconds) -------------------------------------------------------
STEP_S = 90.0          # one absolute wrapper deadline D = start + 90 s
VALIDATION_S = 2.0
LIST_S = 20.0
RUN_S = 60.0
CLEANUP_RESERVE_S = 8.0
CHILD_EXIT_S = 5.0     # after TerminateJobObject
ACTIVE_POLL_S = 3.0    # ActiveProcesses must reach 0
UNASSIGNED_REAP_S = 2.0
WATCHDOG_S = 5.0       # child waits at most this long for the go byte
POLL_S = 0.1
READ_CAP = 4 * 1024 * 1024
GUARD_SOURCE_CAP = 64 * 1024
# Conservative, measured admission (not a guarantee): E = now - T0 + 15 s, and
# E + 90 s step + 30 s post steps must fit the unchanged 720 s job cap.
ANCHOR_PRE_S = 15
POST_S = 30
CAP_S = 720
ADMIT_E_MAX = CAP_S - int(STEP_S) - POST_S  # 600

EVENTS = ("push", "pull_request")
PHASES = ("list", "run")
PROFILE = "diag-sample"
CONFIG_REL = ("tools", "ci", "windows-diag-sample.nextest.toml")
GUARD_REL = ("tools", "ci", "test_windows_cluster_quarantine.py")
TARGET_REL = ("target", "nextest", PROFILE)
JUNIT_NAME = "junit.xml"
LIST_NAME = "list.json"
PRIV_PREFIX = "arxdiag-"
FILTER = "package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/) & not test(=cluster::consensus::three_node::r1_apply_wait_deadline) & not test(=cluster::consensus::three_node::r2_isolated_read_refusal) & not test(=cluster::consensus::three_node::r3_store_wait_deadline_and_heartbeat) & not test(=cluster::consensus::three_node::r4_read_admission_and_caller_abort) & not test(=cluster::consensus::three_node::r5_shutdown_read_owners)"
TEST_PREFIX = "cluster::consensus::three_node::"
BINARY_ID = "arxd"
IDS = {
    "t1_follower_offline_enrollment_and_reopen": "T1",
    "t2_leader_offline_reads_and_unknown_retry": "T2",
    "t3_snapshot_catchup_then_consecutive_append": "T3",
    "t4_registry_revocation_snapshot_and_prune": "T4",
    "u6_cancelled_callers_keep_engine_permits_until_resolution": "U6",
    "r6_shutdown_report_cancelled_proposal": "R6c",
    "r6_shutdown_report_store_only": "R6s",
    "r6_shutdown_report_normal_and_reaped_panic": "R6p",
}
ID_ORDER = ("T1", "T2", "T3", "T4", "U6", "R6c", "R6s", "R6p")

# Child exit codes outside nextest's table; any of them gates via *_exit.
CHILD_NO_GO, CHILD_WATCHDOG, CHILD_ARGS, CHILD_SPAWN, CHILD_RANGE = 2, 3, 6, 7, 8

OUTCOMES = ("all_pass", "diag_fail", "diag_unparsed", "diag_incomplete",
            "skipped_budget", "skipped_event", "cancelled", "guard")
GUARDS = ("anchor", "platform", "reparse", "job", "spawn", "cleanup", "list_exit",
          "list_parse", "inventory", "deadline_list", "nextest_exit", "junit_missing",
          "junit_type", "junit_inventory", "deadline", "internal")
KINDS = ("pass", "fail", "abort", "timeout", "leak", "exec_fail", "not_run")

U8, U16, U32, U64 = 2**8 - 1, 2**16 - 1, 2**32 - 1, 2**64 - 1
LINE_PREFIX = "ARXDIAG v1 "
D1_PREFIX = "[DIAG-3n] "
D1_LINE_MAX = 8192
DIAG_PER_TEST = 16
DIAG_BYTES_PER_TEST = 8448
STEP_LINES = 150
STEP_BYTES = 72 * 1024


class Guard(Exception):
    """Selection, ownership or wrapper-integrity failure: exit 1."""

    def __init__(self, reason):
        if reason not in GUARDS:
            reason = "internal"
        super().__init__(reason)
        self.reason = reason


def cargo_argv(phase):
    """Fixed argv from module constants only; the same cargo args as the gate.

    nextest 0.9.151 accepts `--no-tests` only on `run`; `list` rejects it with a
    usage error, and the inventory gate already refuses an empty match.
    """
    if phase not in PHASES:
        raise Guard("internal")
    verb = ["list", "-T", "json"] if phase == "list" else ["run"]
    no_tests = [] if phase == "list" else ["--no-tests", "fail"]
    return ["cargo", "nextest", *verb, "--workspace", "--locked", "--cargo-quiet",
            "--user-config-file", "none", "--config-file", "/".join(CONFIG_REL),
            "-P", PROFILE, "--run-ignored", "only", *no_tests, "-E", FILTER]


# --- Budget ------------------------------------------------------------------------
def check_anchor(raw, now):
    """Return the anchor age in whole seconds, or gate a bad anchor.

    A missing, malformed, or non-positive timestamp, and a future anchor,
    are misconfiguration and gate. Age past CAP_S is a slow runner: return
    it so admission records skipped_budget instead of failing the job.
    """
    if not isinstance(raw, str) or not re.fullmatch(r"[1-9][0-9]{0,10}", raw, re.ASCII):
        raise Guard("anchor")
    age = int(now) - int(raw)
    if age < 0:
        raise Guard("anchor")
    return age


def admission(age):
    """Return (E, admitted). Admitted only if E <= 600 s."""
    e = age + ANCHOR_PRE_S
    return e, e <= ADMIT_E_MAX


# --- Typed D1 grammar -----------------------------------------------------------
class _NoMatch(Exception):
    pass


CAND = ("None", "Cpu", "Store", "Both")
STATES = ("L", "F", "C", "l", "S", "?")
HELPER = ("Leader", "Write", "Converge", "ConvergeWait", "Stop")
FAIL = ("Elapsed", "NotLeader", "NoQuorum", "Unavailable", "Busy", "Stopped",
        "Unknown", "Orphaned", "Wait")
S3 = ("Churn", "SameTermCandidate")
PHASE = ("Lock", "Engine", "Owners", "StoreRelease")
ENGINE = ("Absent", "Joined", "JoinFailed", "Deadline")
NODE = ("1", "2", "3")
OPS = ("Vote", "Append", "Apply", "Snapshot", "Other")
REPORT_FIELDS = (("first", PHASE), ("engine", ENGINE), ("engine_ms", U16),
                 ("owners_done_at_start", U8), ("owners_waited", U8),
                 ("owners_left", U8), ("owners_panicked", U8),
                 ("last_owner_join_ms", U16), ("propose_held", U8), ("read_held", U8),
                 ("log_strong", U8), ("sm_strong", U8), ("total_ms", U16))
LAGEV_MAX = 8
RING_MAX = 128


class _P:
    """Cursor parser; output is built only from module constants and str(int)."""

    def __init__(self, s):
        self.s, self.i, self.out = s, 0, []

    def lit(self, t):
        if not self.s.startswith(t, self.i):
            raise _NoMatch
        self.i += len(t)
        self.out.append(t)

    def peek(self, t):
        return self.s.startswith(t, self.i)

    def num(self, mx):
        j = self.i
        while j < len(self.s) and j - self.i <= 20 and self.s[j] in "0123456789":
            j += 1
        tok = self.s[self.i:j]
        if not tok or len(tok) > 20 or (len(tok) > 1 and tok[0] == "0"):
            raise _NoMatch
        v = int(tok)
        if v > mx:
            raise _NoMatch
        self.i = j
        self.out.append(str(v))
        return v

    def enum(self, choices):
        for c in sorted(choices, key=len, reverse=True):
            if self.s.startswith(c, self.i):
                self.i += len(c)
                self.out.append(c)
                return c
        raise _NoMatch

    def opt(self):
        if self.peek("-"):
            self.lit("-")
        else:
            self.num(U64)

    def seq(self, *items):
        for it in items:
            if isinstance(it, str):
                self.lit(it)
            elif isinstance(it, int):
                self.num(it)
            elif it is _O:
                self.opt()
            else:
                self.enum(it)

    def alt(self, *fns):
        for fn in fns:
            i, n = self.i, len(self.out)
            try:
                fn()
                return
            except _NoMatch:
                self.i = i
                del self.out[n:]
        raise _NoMatch

    def end(self):
        if self.i != len(self.s):
            raise _NoMatch
        return "".join(self.out)


_O = object()  # u64 or "-"


def _sample(p):
    p.seq(U32, ",", U64, ",", STATES, ",", _O, ",", _O, ",", _O, ",", _O, ",", _O)


def _buckets(p):
    p.seq(U32, ":", U32, ":", U32, ":", U32, ":", U32, "^", U32)


def _f_pass(p):
    p.seq("pass t=", U32, " lag=", U32, "@", U32, " over=", U32, " ticks=", U32)
    for k in NODE:
        p.lit(" n" + k)
        p.alt(lambda: p.lit(" ring=busy"),
              lambda: p.seq(" term=", U32, " ldr=", U32, " seen=", U32))
        p.seq(" high=", U32)
    p.seq(" max Vote=", U32, ",Append=", U32, ",Apply=", U32, ",Snapshot=", U32,
          ",Other=", U32, " queue=", U32, " leader=", U32, ":", U64, ":", U32,
          " write=", U32, ":", U32, ":", U32, " cand=", CAND)


def _f_fail(p):
    p.seq("v1 fail t=", U32, " lag=", U32, "@", U32, " over=", U32, " ticks=", U32,
          " cand=", CAND)


def _f_lagev(p):
    p.lit("lagev ")

    def pairs():
        p.lit("[")
        n = 0
        while not p.peek("]"):
            if n:
                p.lit(", ")
            p.seq("(", U32, ", ", U32, ")")
            n += 1
            if n > LAGEV_MAX:
                raise _NoMatch
        p.lit("]")
    p.alt(lambda: p.lit("busy"), pairs)


def _f_helpers(p):
    p.seq("helpers leader=", U32, ":", U64, ":", U32, " write=", U32, ":", U32, ":", U32,
          " converge=", U32, " stop=", U32)


def _f_attr(p):
    p.lit("attr ")
    p.alt(lambda: p.lit("busy"), lambda: p.lit("none"),
          lambda: p.seq("helper=", HELPER, " fail=", FAIL, " node=", U8, " t=", U32,
                        " polls=", U32, " retries=", U32))


def _f_s3(p):
    p.lit("s3 before=")
    _sample(p)
    p.lit(" after=")
    _sample(p)
    p.seq(" class=", S3)


def _f_at(p):
    p.lit("at")
    for k in NODE:
        p.lit(" n" + k + "=")
        p.alt(lambda: _sample(p), lambda: p.lit("-"))


def _f_report(p):
    p.lit("report ShutdownReport { ")
    for idx, (name, typ) in enumerate(REPORT_FIELDS):
        p.lit(("" if idx == 0 else ", ") + name + ": ")
        p.seq(typ)
    p.lit(" }")


def _f_ops(p):
    p.seq("n", NODE, " ops")
    for op in OPS:
        p.lit(" " + op + "=")
        _buckets(p)
    p.lit(" queue=")
    _buckets(p)
    p.seq(" delay=", U32)
    p.alt(lambda: p.lit(" ring=busy"),
          lambda: p.seq(" seen=", U32, " ovw=", U32, " term=", U32, " ldr=", U32,
                        " st=", U32))


def _f_ring(p):
    p.seq("n", NODE, " ring")

    def samples():
        n = 0
        while p.i < len(p.s):
            p.lit(" ")
            _sample(p)
            n += 1
            if n > RING_MAX:
                raise _NoMatch
    p.alt(lambda: (p.lit(" busy"), p.end()), samples)


def _f_end(p):
    p.seq("end bytes=", U16)
    if int(p.out[-1]) > D1_LINE_MAX:
        raise _NoMatch
    p.seq(" trunc=", ("0", "1"))


FORMS = (_f_pass, _f_fail, _f_lagev, _f_helpers, _f_attr, _f_s3, _f_at, _f_report,
         _f_ops, _f_ring, _f_end)


def parse_d1(line):
    """Return the canonical re-rendered body of one stored D1 line, or None."""
    if not isinstance(line, str) or not line.startswith(D1_PREFIX):
        return None
    if not line.isascii() or len(line) > D1_LINE_MAX:
        return None
    body = line[len(D1_PREFIX):]
    for form in FORMS:
        p = _P(body)
        try:
            form(p)
            return p.end()
        except _NoMatch:
            continue
    return None


@dataclass
class TestResult:
    id: str
    kind: str
    ms: int
    diag: list = field(default_factory=list)
    dropped_lines: int = 0
    dropped_bytes: int = 0
    rejects: int = 0


def extract_diag(result, stderr_text, budget):
    """Collect typed D1 lines of one test; `budget` is a shared [lines, bytes] left."""
    used = 0
    for raw in stderr_text.split("\n"):
        if not raw.startswith(D1_PREFIX):
            continue  # indented or mid-line copies are ignored, never counted
        body = parse_d1(raw)
        if body is None:
            result.rejects += 1
            continue
        out = "%sdiag id=%s %s" % (LINE_PREFIX, result.id, body)
        n = len(out) + 1
        if (len(result.diag) >= DIAG_PER_TEST or used + n > DIAG_BYTES_PER_TEST
                or budget[0] < 1 or budget[1] < n):
            result.dropped_lines += 1
            result.dropped_bytes += n
            continue
        result.diag.append(out)
        used += n
        budget[0] -= 1
        budget[1] -= n


# --- Inventory -----------------------------------------------------------------------
def _same_path(a, b):
    return os.path.normcase(os.path.normpath(a)) == os.path.normcase(os.path.normpath(b))


def check_inventory(data, expected, root):
    """Gate unless exactly the eight expected ignored tests of arxd lib match."""
    if len(data) > READ_CAP:
        raise Guard("list_parse")
    try:
        doc = json.loads(data.decode("utf-8"))
        suites = doc["rust-suites"]
        target = doc["rust-build-meta"]["target-directory"]
        if not isinstance(suites, dict) or not isinstance(target, str):
            raise TypeError
        matches = []
        for bid, suite in suites.items():
            cases = suite["testcases"]
            if not isinstance(cases, dict):
                raise TypeError
            for name, case in cases.items():
                status = case["filter-match"]["status"]
                if status not in ("matches", "mismatch"):
                    raise TypeError
                if status == "matches":
                    matches.append((bid, suite.get("binary-id"), suite.get("kind"),
                                    suite.get("status", "listed"), name, case.get("ignored")))
    except (ValueError, KeyError, TypeError, AttributeError, RecursionError):
        raise Guard("list_parse") from None
    if not _same_path(target, os.path.join(root, "target")):
        raise Guard("inventory")
    want = sorted(TEST_PREFIX + n for n in expected)
    for bid, binary_id, kind, status, _, ignored in matches:
        if (bid, binary_id, kind, status) != (BINARY_ID, BINARY_ID, "lib", "listed"):
            raise Guard("inventory")
        if ignored is not True:
            raise Guard("inventory")
    if sorted(m[4] for m in matches) != want:
        raise Guard("inventory")


def check_expected(expected):
    if (not isinstance(expected, list) or len(expected) != len(IDS)
            or any(not isinstance(n, str) for n in expected)
            or sorted(expected) != sorted(IDS)):
        raise Guard("inventory")
    return list(expected)


# --- JUnit -------------------------------------------------------------------------
_EXIT_TYPE = re.compile(r"test failure with exit code [1-9][0-9]{0,9}( \(leaked handles\))?",
                        re.ASCII)
JUNIT_TYPES = {
    "test abort": ("failure", "abort"),
    "test abort (leaked handles)": ("failure", "abort"),
    "test timeout": ("failure", "timeout"),
    "execution failure": ("error", "exec_fail"),
    "test exited with code 0, but leaked handles so was marked failed": ("error", "leak"),
    "test passed but leaked handles": ("error", "leak"),
}
_STATUS_TAGS = ("failure", "error", "skipped", "flakyFailure", "flakyError",
                "rerunFailure", "rerunError")
_TIME = re.compile(r"(0|[1-9][0-9]{0,6})(\.[0-9]{1,9})?", re.ASCII)


def junit_kind(case):
    status = [c for c in case if c.tag in _STATUS_TAGS]
    for c in case:
        if c.tag not in _STATUS_TAGS and c.tag not in ("system-out", "system-err", "properties"):
            raise Guard("junit_inventory")
    if not status:
        return "pass"
    if len(status) != 1 or status[0].tag not in ("failure", "error"):
        raise Guard("junit_type")
    ty = status[0].get("type")
    if isinstance(ty, str) and _EXIT_TYPE.fullmatch(ty):
        want = ("failure", "fail")
    elif ty in JUNIT_TYPES:
        want = JUNIT_TYPES[ty]
    else:
        raise Guard("junit_type")
    if status[0].tag != want[0]:
        raise Guard("junit_type")
    return want[1]


def parse_junit(data, rc, budget=None):
    """Return (outcome, [TestResult]). Unparsed input is diagnostic, never gating."""
    if len(data) > READ_CAP or b"<!DOCTYPE" in data or b"<!ENTITY" in data:
        return "diag_unparsed", []
    try:
        root = ET.fromstring(data)
    except (ET.ParseError, ValueError):
        return "diag_unparsed", []
    if root.tag != "testsuites":
        raise Guard("junit_inventory")
    found = {}
    for suite in root:
        if suite.tag != "testsuite" or suite.get("name") != BINARY_ID:
            raise Guard("junit_inventory")
        for case in suite:
            if case.tag in ("properties", "system-out", "system-err"):
                continue
            name, classname = case.get("name"), case.get("classname")
            if (case.tag != "testcase" or classname != BINARY_ID or not isinstance(name, str)
                    or not name.startswith(TEST_PREFIX)):
                raise Guard("junit_inventory")
            tid = IDS.get(name[len(TEST_PREFIX):])
            if tid is None or tid in found:
                raise Guard("junit_inventory")
            kind = junit_kind(case)
            t = case.get("time")
            if not isinstance(t, str) or not _TIME.fullmatch(t):
                raise Guard("junit_inventory")
            errs = [c for c in case if c.tag == "system-err"]
            if len(errs) > 1:
                raise Guard("junit_inventory")
            text = (errs[0].text or "") if errs else ""
            found[tid] = (kind, min(round(float(t) * 1000), U32), text)
    results = []
    budget = budget if budget is not None else [STEP_LINES - 1 - 3 * len(ID_ORDER),
                                                STEP_BYTES - 8192]
    for tid in ID_ORDER:
        if tid not in found:
            if rc != 100:
                raise Guard("junit_inventory")
            results.append(TestResult(tid, "not_run", 0))
            continue
        kind, ms, text = found[tid]
        r = TestResult(tid, kind, ms)
        extract_diag(r, text, budget)
        results.append(r)
    ok = rc == 0 and all(r.kind == "pass" for r in results)
    return ("all_pass" if ok else "diag_fail"), results


# --- Rendering ------------------------------------------------------------------------
@dataclass
class Report:
    event: str = "-"
    outcome: str = "guard"
    nextest: str = "none"
    wall_ms: int = 0
    list_ms: int = 0
    run_ms: int = 0
    cleanup_ms: int = 0
    orphans: int = 0
    e_s: object = None
    tests: list = field(default_factory=list)
    guard: object = None


def _u(v, mx):
    if not isinstance(v, int) or isinstance(v, bool) or v < 0:
        raise Guard("internal")
    return str(min(v, mx))


def _enum(v, choices):
    if v not in choices:
        raise Guard("internal")
    return v


def render(rep):
    lines = [LINE_PREFIX + "sample event=%s outcome=%s nextest=%s wall_ms=%s list_ms=%s "
             "run_ms=%s cleanup_ms=%s orphans=%s e_s=%s" % (
                 _enum(rep.event, EVENTS + ("-",)), _enum(rep.outcome, OUTCOMES),
                 _enum(rep.nextest, ("0", "100", "killed", "none")),
                 _u(rep.wall_ms, U32), _u(rep.list_ms, U32), _u(rep.run_ms, U32),
                 _u(rep.cleanup_ms, U32), _u(rep.orphans, U8),
                 "-" if rep.e_s is None else _u(rep.e_s, U16))]
    if rep.outcome in ("all_pass", "diag_fail"):
        for t in rep.tests:
            lines.append(LINE_PREFIX + "test id=%s kind=%s ms=%s" % (
                _enum(t.id, ID_ORDER), _enum(t.kind, KINDS), _u(t.ms, U32)))
            for d in t.diag:
                if not d.startswith(LINE_PREFIX + "diag id=" + t.id + " "):
                    raise Guard("internal")
                lines.append(d)
            if t.dropped_lines:
                lines.append(LINE_PREFIX + "trunc id=%s dropped_lines=%s dropped_bytes=%s" % (
                    t.id, _u(t.dropped_lines, U16), _u(t.dropped_bytes, U32)))
            if t.rejects:
                lines.append(LINE_PREFIX + "reject id=%s n=%s" % (t.id, _u(t.rejects, U16)))
    if rep.guard is not None:
        lines.append(LINE_PREFIX + "guard reason=" + _enum(rep.guard, GUARDS))
    total = sum(len(x) + 1 for x in lines)
    if (len(lines) > STEP_LINES or total > STEP_BYTES
            or any(not x.isascii() or "\n" in x or "\r" in x for x in lines)):
        raise Guard("internal")
    return lines


# --- Filesystem seam ------------------------------------------------------------------
class RealFs:
    def kind(self, path):
        try:
            st = os.lstat(path)
        except FileNotFoundError:
            return "missing"
        if getattr(st, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400):
            return "reparse"
        if stat.S_ISLNK(st.st_mode):
            return "reparse"
        if stat.S_ISDIR(st.st_mode):
            return "dir"
        if stat.S_ISREG(st.st_mode):
            return "file"
        return "other"

    def realpath_ok(self, path):
        return _same_path(os.path.realpath(path), os.path.abspath(path))

    def read_capped(self, path, cap):
        """Read at most cap+1 bytes from an ordinary file that is not swapped."""
        before = os.lstat(path)
        fd = os.open(path, os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOINHERIT", 0))
        try:
            after = os.fstat(fd)
            if (before.st_ino, before.st_dev) != (after.st_ino, after.st_dev):
                raise Guard("reparse")
            chunks, n = [], 0
            while n <= cap:
                b = os.read(fd, min(65536, cap + 1 - n))
                if not b:
                    break
                chunks.append(b)
                n += len(b)
            return b"".join(chunks)
        finally:
            os.close(fd)

    def unlink(self, path):
        os.unlink(path)

    def mkdtemp(self, base):
        return tempfile.mkdtemp(prefix=PRIV_PREFIX, dir=base)

    def rmdir(self, path):
        os.rmdir(path)


def require(fs, path, want):
    """Gate `reparse` unless path is the wanted ordinary kind (or allowed missing)."""
    k = fs.kind(path)
    if k == "missing" and "missing" in want:
        return k
    if k not in want:
        raise Guard("reparse")
    return k


def load_expected(fs, root):
    """Import EXPECTED from the fixed guard path via importlib, from verified bytes."""
    path = os.path.join(root, *GUARD_REL)
    for p in (root, os.path.join(root, "tools"), os.path.join(root, "tools", "ci")):
        require(fs, p, ("dir",))
    require(fs, path, ("file",))
    if not fs.realpath_ok(path):
        raise Guard("reparse")
    src = fs.read_capped(path, GUARD_SOURCE_CAP)
    if len(src) > GUARD_SOURCE_CAP:
        raise Guard("inventory")
    spec = importlib.util.spec_from_file_location("arx_windows_quarantine_pin", path)
    if spec is None:
        raise Guard("inventory")
    module = importlib.util.module_from_spec(spec)
    # Execute exactly the verified bytes; never a cached .pyc or an ambient module.
    try:
        exec(compile(src, path, "exec", dont_inherit=True), module.__dict__)
    except Exception:
        raise Guard("inventory") from None
    return check_expected(getattr(module, "EXPECTED", None))


# --- Ownership ----------------------------------------------------------------------
class JobError(Exception):
    pass


class WinJobApi:
    """Kernel32 Job Object calls via stdlib ctypes. Only this wrapper's handles."""

    KILL_ON_JOB_CLOSE = 0x2000
    BREAKAWAY_MASK = 0x0800 | 0x1000  # BREAKAWAY_OK | SILENT_BREAKAWAY_OK
    EXTENDED_LIMIT = 9
    BASIC_ACCOUNTING = 1
    HANDLE_FLAG_INHERIT = 0x1

    def __init__(self):
        import ctypes
        from ctypes import wintypes as w
        self.c, self.w = ctypes, w
        k = ctypes.WinDLL("kernel32", use_last_error=True)
        H, B, D = w.HANDLE, w.BOOL, w.DWORD
        sig = {
            "CreateJobObjectW": ([ctypes.c_void_p, w.LPCWSTR], H),
            "SetInformationJobObject": ([H, ctypes.c_int, ctypes.c_void_p, D], B),
            "QueryInformationJobObject": ([H, ctypes.c_int, ctypes.c_void_p, D,
                                           ctypes.POINTER(D)], B),
            "AssignProcessToJobObject": ([H, H], B),
            "IsProcessInJob": ([H, H, ctypes.POINTER(B)], B),
            "TerminateJobObject": ([H, w.UINT], B),
            "GetHandleInformation": ([H, ctypes.POINTER(D)], B),
            "CloseHandle": ([H], B),
        }
        for name, (args, res) in sig.items():
            fn = getattr(k, name)
            fn.argtypes, fn.restype = args, res
        self.k = k

        class Basic(ctypes.Structure):
            _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64),
                        ("PerJobUserTimeLimit", ctypes.c_int64), ("LimitFlags", D),
                        ("MinimumWorkingSetSize", ctypes.c_size_t),
                        ("MaximumWorkingSetSize", ctypes.c_size_t),
                        ("ActiveProcessLimit", D), ("Affinity", ctypes.c_size_t),
                        ("PriorityClass", D), ("SchedulingClass", D)]

        class Io(ctypes.Structure):
            _fields_ = [(n, ctypes.c_ulonglong) for n in (
                "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
                "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

        class Extended(ctypes.Structure):
            _fields_ = [("BasicLimitInformation", Basic), ("IoInfo", Io),
                        ("ProcessMemoryLimit", ctypes.c_size_t),
                        ("JobMemoryLimit", ctypes.c_size_t),
                        ("PeakProcessMemoryUsed", ctypes.c_size_t),
                        ("PeakJobMemoryUsed", ctypes.c_size_t)]

        class Accounting(ctypes.Structure):
            _fields_ = [("TotalUserTime", ctypes.c_int64), ("TotalKernelTime", ctypes.c_int64),
                        ("ThisPeriodTotalUserTime", ctypes.c_int64),
                        ("ThisPeriodTotalKernelTime", ctypes.c_int64),
                        ("TotalPageFaultCount", D), ("TotalProcesses", D),
                        ("ActiveProcesses", D), ("TotalTerminatedProcesses", D)]

        self.Extended, self.Accounting = Extended, Accounting

    def create(self):
        c, k = self.c, self.k
        h = k.CreateJobObjectW(None, None)  # unnamed; NULL attributes: not inheritable
        if not h:
            raise JobError
        try:
            info = self.Extended()
            info.BasicLimitInformation.LimitFlags = self.KILL_ON_JOB_CLOSE
            if not k.SetInformationJobObject(h, self.EXTENDED_LIMIT, c.byref(info), c.sizeof(info)):
                raise JobError
            back = self.Extended()
            if not k.QueryInformationJobObject(h, self.EXTENDED_LIMIT, c.byref(back),
                                               c.sizeof(back), None):
                raise JobError
            flags = back.BasicLimitInformation.LimitFlags
            if not flags & self.KILL_ON_JOB_CLOSE or flags & self.BREAKAWAY_MASK:
                raise JobError
            hf = self.w.DWORD()
            if not k.GetHandleInformation(h, c.byref(hf)) or hf.value & self.HANDLE_FLAG_INHERIT:
                raise JobError
        except BaseException:
            k.CloseHandle(h)
            raise
        return h

    def assign(self, job, proc):
        return bool(self.k.AssignProcessToJobObject(job, int(proc._handle)))

    def in_job(self, job, proc):
        r = self.w.BOOL()
        if not self.k.IsProcessInJob(int(proc._handle), job, self.c.byref(r)):
            return False
        return bool(r.value)

    def terminate(self, job):
        return bool(self.k.TerminateJobObject(job, 1))

    def active(self, job):
        info = self.Accounting()
        if not self.k.QueryInformationJobObject(job, self.BASIC_ACCOUNTING, self.c.byref(info),
                                                self.c.sizeof(info), None):
            raise JobError
        return int(info.ActiveProcesses)

    def close(self, job):
        self.k.CloseHandle(job)


class PopenSpawner:
    def spawn(self, phase, privdir):
        return subprocess.Popen([sys.executable, "-I", "-B", SCRIPT, "--child", phase, privdir],
                                stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, cwd=ROOT, close_fds=True)


class Clock:
    def monotonic(self):
        return time.monotonic()

    def time(self):
        return time.time()

    def sleep(self, seconds):
        time.sleep(seconds)


@dataclass
class Phase:
    status: str          # exit | killed (after go) | expired (before go) | cancelled
    rc: object = None
    ms: int = 0
    cleanup_ms: int = 0
    orphans: int = 0


def _ms(a, b):
    return max(0, int(round((b - a) * 1000)))


def _nap(clock, until):
    clock.sleep(max(0.0, min(POLL_S, until - clock.monotonic())))


def _wait_exit(proc, clock, seconds, deadline=float("inf")):
    end = min(clock.monotonic() + seconds, deadline)
    while True:
        if proc.poll() is not None:
            return True
        if clock.monotonic() >= end:
            return False
        _nap(clock, end)


def _reap_unassigned(proc, clock, deadline=float("inf")):
    """Kill our own blocked child (never in the job) and wait <= 2 s."""
    try:
        if proc.stdin is not None:
            proc.stdin.close()
    except OSError:
        pass
    try:
        proc.kill()  # TerminateProcess on this wrapper's own Popen handle
    except OSError:
        pass
    if not _wait_exit(proc, clock, UNASSIGNED_REAP_S, deadline):
        raise Guard("cleanup")


def run_phase(api, spawner, clock, cancel, phase, privdir, deadline, cleanup_deadline=None):
    """Spawn -> assign -> IsProcessInJob -> cancel check -> go; then bounded cleanup."""
    cleanup_deadline = deadline + CLEANUP_RESERVE_S if cleanup_deadline is None else cleanup_deadline
    if clock.monotonic() >= deadline:
        return Phase("expired")
    try:
        job = api.create()
    except Exception:
        raise Guard("job") from None
    try:
        start = clock.monotonic()
        if start >= deadline:
            return Phase("expired")
        try:
            proc = spawner.spawn(phase, privdir)
        except Exception:
            raise Guard("spawn") from None
        reason = None
        try:
            if not api.assign(job, proc):
                reason = "job"
            elif not api.in_job(job, proc):
                reason = "job"
            elif proc.poll() is not None:
                reason = "job"  # child already reaped before go
            elif cancel.is_set():
                reason = "cancelled"
            elif clock.monotonic() >= deadline:
                reason = "expired"
        except Exception:
            reason = "job"
        if reason is not None:
            _reap_unassigned(proc, clock, cleanup_deadline)
            if reason in ("cancelled", "expired"):
                return Phase(reason, ms=_ms(start, clock.monotonic()))
            raise Guard(reason)
        try:
            proc.stdin.write(b"G")
            proc.stdin.flush()
            proc.stdin.close()
        except OSError:
            pass  # child gone; its exit code gates below
        status, rc = "exit", None
        while True:
            rc = proc.poll()
            if rc is not None:
                break
            if cancel.is_set():
                status = "cancelled"
                break
            if clock.monotonic() >= deadline:
                status = "killed"
                break
            _nap(clock, deadline)
        ended = clock.monotonic()
        try:
            orphans = api.active(job)
        except Exception:
            orphans = 0
        try:
            api.terminate(job)  # exactly once per phase
        except Exception:
            pass
        if not _wait_exit(proc, clock, CHILD_EXIT_S, cleanup_deadline):
            raise Guard("cleanup")
        until = min(clock.monotonic() + ACTIVE_POLL_S, cleanup_deadline)
        while True:
            try:
                left = api.active(job)
            except Exception:
                raise Guard("cleanup") from None
            if left == 0:
                break
            if clock.monotonic() >= until:
                raise Guard("cleanup")
            _nap(clock, until)
        return Phase(status, rc if status == "exit" else None, _ms(start, ended),
                     _ms(ended, clock.monotonic()), orphans)
    finally:
        try:
            api.close(job)  # kill-on-close reaps anything still in the job
        except Exception:
            pass


# --- Orchestration ------------------------------------------------------------------
@dataclass
class Ctx:
    env: dict
    is_windows: bool
    in_venv: bool
    api_factory: object
    spawner: object
    clock: object
    fs: object
    cancel: object
    root: str = ROOT


def sample(ctx, rep, start):
    """Fill `rep` and return it; raise Guard for integrity failures."""
    clock, fs, env = ctx.clock, ctx.fs, ctx.env
    deadline = start + STEP_S
    ev = env.get("GITHUB_EVENT_NAME")
    rep.event = ev if ev in EVENTS else "-"
    if not ctx.is_windows or ctx.in_venv:
        raise Guard("platform")
    # Keep this age. A later wall-clock step backward must not admit work
    # that this reading already showed does not fit the job cap.
    opened = clock.monotonic()
    first_age = check_anchor(env.get("ARX_DIAG_T0"), clock.time())
    if rep.event == "-":
        rep.outcome = "skipped_event"
        return rep
    temp = env.get("RUNNER_TEMP")
    if not isinstance(temp, str) or not temp or not os.path.isabs(temp):
        raise Guard("platform")
    require(fs, temp, ("dir",))
    root = ctx.root
    expected = load_expected(fs, root)
    require(fs, os.path.join(root, *CONFIG_REL), ("file",))
    tdirs = [os.path.join(root, *TARGET_REL[:i]) for i in range(1, len(TARGET_REL) + 1)]
    require(fs, tdirs[0], ("dir",))
    for d in tdirs[1:]:
        require(fs, d, ("dir", "missing"))
    junit = os.path.join(tdirs[-1], JUNIT_NAME)
    require(fs, junit, ("file", "missing"))
    check_deadline(clock, start + VALIDATION_S)
    second_age = check_anchor(env.get("ARX_DIAG_T0"), clock.time())
    elapsed = clock.monotonic() - opened
    grown = first_age + (int(elapsed) if elapsed > 0 else 0)
    e, admitted = admission(max(first_age, second_age, grown))
    rep.e_s = e
    if not admitted:
        rep.outcome = "skipped_budget"
        return rep
    if ctx.cancel.is_set():
        rep.outcome = "cancelled"
        return rep
    try:
        api = ctx.api_factory()
    except Exception:
        raise Guard("job") from None
    privdir = fs.mkdtemp(temp)
    try:
        require(fs, privdir, ("dir",))
        now = clock.monotonic()
        lp = run_phase(api, ctx.spawner, clock, ctx.cancel, "list", privdir,
                       min(now + LIST_S, deadline - CLEANUP_RESERVE_S), deadline)
        rep.list_ms, rep.cleanup_ms, rep.orphans = lp.ms, lp.cleanup_ms, lp.orphans
        if lp.status == "cancelled":
            rep.outcome = "cancelled"
            return rep
        if lp.status == "expired":
            raise Guard("deadline")
        if lp.status == "killed":
            raise Guard("deadline_list")
        if lp.rc != 0:
            raise Guard("list_exit")
        listing = os.path.join(privdir, LIST_NAME)
        if require(fs, listing, ("file", "missing")) == "missing":
            raise Guard("list_parse")
        check_inventory(fs.read_capped(listing, READ_CAP), expected, root)
        check_deadline(clock, deadline - CLEANUP_RESERVE_S)
        if require(fs, junit, ("file", "missing")) == "file":
            fs.unlink(junit)
        if ctx.cancel.is_set():
            rep.outcome = "cancelled"
            return rep
        now = clock.monotonic()
        rp = run_phase(api, ctx.spawner, clock, ctx.cancel, "run", privdir,
                       min(now + RUN_S, deadline - CLEANUP_RESERVE_S), deadline)
        rep.run_ms = rp.ms
        rep.cleanup_ms += rp.cleanup_ms
        rep.orphans += rp.orphans
        if rp.status == "cancelled":
            rep.outcome = "cancelled"
            return rep
        if rp.status == "expired":
            raise Guard("deadline")
        if rp.status == "killed":
            rep.nextest = "killed"
            rep.outcome = "diag_incomplete"  # JUnit missing or truncated: not parsed
            return rep
        if rp.rc not in (0, 100):
            raise Guard("nextest_exit")
        rep.nextest = str(rp.rc)
        for d in tdirs:
            require(fs, d, ("dir", "missing"))
        if require(fs, junit, ("file", "missing")) == "missing":
            raise Guard("junit_missing")
        rep.outcome, rep.tests = parse_junit(fs.read_capped(junit, READ_CAP), rp.rc)
        return rep
    finally:
        failed = False
        for p, kind, rm in ((os.path.join(privdir, LIST_NAME), "file", fs.unlink),
                            (junit, "file", fs.unlink), (privdir, "dir", fs.rmdir)):
            try:
                actual = fs.kind(p)
                if actual == kind:
                    rm(p)
                elif actual != "missing":
                    failed = True
            except Exception:
                failed = True
        if failed:
            raise Guard("cleanup")
        check_deadline(clock, deadline)


def check_deadline(clock, deadline):
    # Synchronous OS/interpreter stalls cannot be preempted. Refuse subsequent
    # work and successful reporting once control returns after the deadline.
    if clock.monotonic() >= deadline:
        raise Guard("deadline")


def run(ctx, start=None):
    """Return (exit_code, lines). Never raises; never prints a traceback."""
    start = ctx.clock.monotonic() if start is None else start
    rep = Report()
    try:
        try:
            sample(ctx, rep, start)
            check_deadline(ctx.clock, start + STEP_S)
        except Guard as g:
            rep.outcome, rep.guard = "guard", g.reason
            rep.tests = []
        rep.wall_ms = _ms(start, ctx.clock.monotonic())
        lines = render(rep)
        if not rep.guard and ctx.clock.monotonic() >= start + STEP_S:
            rep.outcome, rep.guard, rep.tests = "guard", "deadline", []
            rep.wall_ms = _ms(start, ctx.clock.monotonic())
            lines = render(rep)
        return (1 if rep.guard else 0), lines
    except BaseException:
        return 1, [LINE_PREFIX + "guard reason=internal"]


def default_ctx(cancel):
    return Ctx(env=dict(os.environ), is_windows=os.name == "nt",
               in_venv=sys.prefix != sys.base_prefix, api_factory=WinJobApi,
               spawner=PopenSpawner(), clock=Clock(), fs=RealFs(), cancel=cancel)


def valid_privdir(path, env, fs=None):
    fs = fs or RealFs()
    temp = env.get("RUNNER_TEMP")
    if not isinstance(path, str) or not isinstance(temp, str) or not os.path.isabs(path):
        return False
    if not _same_path(os.path.dirname(path), temp):
        return False
    if not re.fullmatch(re.escape(PRIV_PREFIX) + r"[a-z0-9_]{1,32}", os.path.basename(path), re.ASCII):
        return False
    return fs.kind(path) == "dir"


def child_main(args):
    """Owned child: no descendants before the go byte; bounded wait; EOF exits."""
    for name in ("SIGINT", "SIGBREAK"):
        if hasattr(signal, name):
            signal.signal(getattr(signal, name), signal.SIG_IGN)
    timer = threading.Timer(WATCHDOG_S, os._exit, (CHILD_WATCHDOG,))
    timer.daemon = True
    timer.start()
    try:
        go = sys.stdin.buffer.read(1)
    except Exception:
        os._exit(CHILD_NO_GO)
    if go != b"G":
        os._exit(CHILD_NO_GO)
    timer.cancel()
    if len(args) != 2 or args[0] not in PHASES or not valid_privdir(args[1], os.environ):
        os._exit(CHILD_ARGS)
    try:
        if args[0] == "list":
            out = open(os.path.join(args[1], LIST_NAME), "xb")
        else:
            out = subprocess.DEVNULL
        rc = subprocess.call(cargo_argv(args[0]), cwd=ROOT, stdin=subprocess.DEVNULL,
                             stdout=out, stderr=subprocess.DEVNULL)
    except Exception:
        os._exit(CHILD_SPAWN)
    os._exit(rc if 0 <= rc <= 255 else CHILD_RANGE)


def main(argv=None, make_ctx=default_ctx, out=None):
    argv = sys.argv if argv is None else argv
    if argv[1:2] == ["--child"]:
        child_main(argv[2:])
        return CHILD_ARGS
    out = out or sys.stdout
    cancel = threading.Event()
    try:
        for name in ("SIGINT", "SIGBREAK"):  # flag only; installed before any spawn
            if hasattr(signal, name):
                signal.signal(getattr(signal, name), lambda *_: cancel.set())
        if len(argv) != 1:
            raise Guard("internal")
        ctx = make_ctx(cancel)
        code, lines = run(ctx)
    except BaseException:
        code, lines = 1, [LINE_PREFIX + "guard reason=internal"]
    out.write("".join(x + "\n" for x in lines))
    out.flush()
    return code


if __name__ == "__main__":
    sys.exit(main())
