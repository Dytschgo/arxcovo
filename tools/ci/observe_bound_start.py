"""Fixed D6 Windows observation launcher. See ADR0039 for qualified entry."""
import datetime
import hashlib
import json
import os
from pathlib import Path
from queue import Empty, Full, Queue
import re
import stat
import subprocess
import sys
import time
from threading import Event, Thread
from contextlib import ExitStack
from types import ModuleType

ROOT = Path("D:/Temp/arx-m3-step5b/observation")
CASE = "cluster::consensus::bound_start::nominal_observation::nominal_real_engine_start_close"
ARGS = ("--ignored", "--exact", CASE, "--nocapture", "--test-threads=1")
EXE = "bin/arxd-step5b.exe"
LAUNCHER = "source/tools/ci/observe_bound_start.py"
SUPPORT = "source/tools/ci/windows_diag_sample.py"
FILES = {
    "launcher": (LAUNCHER, 128 * 1024),
    "support": (SUPPORT, 128 * 1024),
    "manifest": ("source-manifest.json", 16 * 1024 * 1024),
    "loader": ("loader.json", 64 * 1024),
    "lockfile": ("source/Cargo.lock", 1024 * 1024),
    "driver": ("source/crates/arxd/src/cluster/consensus/bound_start.rs", 64 * 1024),
    "case": ("source/crates/arxd/src/cluster/consensus/bound_start/nominal_observation.rs", 64 * 1024),
}
SCRATCH = {
    "TMP": "tmp", "TEMP": "tmp", "CARGO_TARGET_DIR": "target",
    "CARGO_HOME": "cargo-home", "RUSTUP_HOME": "rustup-home",
    "ARX_SCRATCH_ROOT": ".", "ARX_OBSERVER_ROOT": "evidence",
}
WINDOWS_ENV = {"SYSTEMROOT", "WINDIR"}
CONTROL = {"ARX_STEP5B_ENTRY_MS", "ARX_STEP5B_PINS_SHA256"}
OUTPUT_CAP = 16 * 1024
PROOFS = {
    "admitted": ("1",),
    "prepared": ("2",),
    "transferred": ("1 1 1",),
    "engine_poll": ("pending", "ready"),
    "detached": ("1 1 1",),
    "running": ("1 1",),
    "live": ("1 1 1",),
    "close_requested": ("1",),
    "close_dispatched": ("1",),
    "close_joined": ("1",),
    "engine_joined": ("1",),
    "released": ("1 1 1 1 1",),
    "before_disposal": ("1 1 1",),
    "outer_released": ("1",),
    "finished": ("1 1 0",),
}
FAILURES = frozenset("""
preparation preparation_join bound_intent bound_layout bound_inspection
bound_unavailable bound_identity bound_open bound_init bound_unwind open_count
config transfer transfer_identity owner_identity engine_start handshake
first_poll_missing observer_count detached running retained_node live peer_client
missing_engine store_release before_disposal close_join driver_join
supervisor_join final_state outer_release startup_join test_admission
fixture_privacy fixture_create fixture_layout fixture_config fixture_plan
""".split())
CHECKS = {"detached": 3, "live": 3, "close_joined": 1, "released": 5,
          "release_observed": 5, "before_disposal": 3, "outer_released": 1}
CHECK_FAILURE = {"released": "store_release", "outer_released": "outer_release"}
ORPHANED_FIELDS = (
    "first", "engine", "engine_ms", "owners_done_at_start", "owners_waited",
    "owners_left", "owners_panicked", "last_owner_join_ms", "propose_held",
    "read_held", "log_strong", "sm_strong", "total_ms",
)
RESULT_LINE = re.compile(
    r"test result: (ok|FAILED)\. ([01]) passed; ([01]) failed; "
    r"0 ignored; 0 measured; ([0-9]{1,6}) filtered out; finished in [0-9]{1,3}\.[0-9]{2}s"
)


class Refusal(Exception):
    """Only fixed codes are emitted."""


def need(condition, code):
    if not condition:
        raise Refusal(code)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def hex_digest(value):
    return isinstance(value, str) and re.fullmatch("[0-9a-f]{64}", value)


def unique_object(pairs):
    result = dict(pairs)
    need(len(result) == len(pairs), "pins_schema")
    return result


def parse_pins(data, trusted):
    need(hex_digest(trusted) and digest(data) == trusted, "pins_digest")
    pins = json.loads(data, object_pairs_hook=unique_object)
    need(isinstance(pins, dict) and set(pins) == {
        "head", "test", "binary_bytes", "binary_sha256", "python", "python_sha256",
        "python_runtime", "sha256",
    }, "pins_schema")
    need(isinstance(pins["head"], str) and re.fullmatch("[0-9a-f]{40}", pins["head"])
         and pins["test"] == CASE and type(pins["binary_bytes"]) is int
         and 0 < pins["binary_bytes"] <= 512 * 1024 * 1024
         and hex_digest(pins["binary_sha256"]) and hex_digest(pins["python_sha256"])
         and isinstance(pins["python"], str) and 0 < len(pins["python"]) <= 512
         and isinstance(pins["sha256"], dict) and pins["sha256"].keys() == FILES.keys()
         and all(hex_digest(h) for h in pins["sha256"].values()), "pins_schema")
    runtime = pins["python_runtime"]
    need(isinstance(runtime, dict) and 0 < len(runtime) <= 64
         and all(re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.-]{0,95}", name)
                 and not name.endswith(".") and hex_digest(sha) for name, sha in runtime.items())
         and len({name.lower() for name in runtime}) == len(runtime)
         and Path(pins["python"]).name.lower() not in {name.lower() for name in runtime},
         "pins_schema")
    return pins


def validate_entry(argv, environment, now):
    need(not argv, "arguments")
    need(set(environment) == WINDOWS_ENV | CONTROL | SCRATCH.keys()
         | {"CARGO_NET_OFFLINE", "CARGO_PROFILE_DEV_DEBUG"}, "environment")
    need(all(environment[key] == str(ROOT / leaf) for key, leaf in SCRATCH.items())
         and environment["CARGO_NET_OFFLINE"] == "true"
         and environment["CARGO_PROFILE_DEV_DEBUG"] == "0", "scratch")
    stamp = environment["ARX_STEP5B_ENTRY_MS"]
    need(re.fullmatch("[0-9]{1,16}", stamp), "entry_clock")
    start = int(stamp)
    need(start <= now < start + 82000, "entry_clock")
    return start


def verify_file(stream, cap, expected, clock, deadline, size=None, retain=False):
    total, sha = 0, hashlib.sha256()
    held = bytearray()
    while True:
        need(clock() < deadline, "admission_deadline")
        chunk = stream.read(min(128 * 1024, cap + 1 - total))
        if not chunk:
            break
        total += len(chunk)
        need(total <= cap, "file_size")
        sha.update(chunk)
        if retain:
            held.extend(chunk)
    need(size is None or total == size, "file_size")
    need(sha.hexdigest() == expected, "file_digest")
    return bytes(held)


class Proof:
    """Incremental allowlist. Raw output is held only in bounded memory."""
    def __init__(self, emit):
        self.emit = emit
        self.buf = b""
        self.bytes = 0
        self.count = 0
        self.last_us = 0
        self.selected = self.passed = self.failed = 0
        self.summary = self.summary_seen = False
        self.test_status = None
        self.test_line = False
        self.first = None
        self.orphaned = None
        self.uncertainty = []
        self.truncated = False

    def fail(self, code):
        if self.first is None:
            self.first = code

    def uncertain(self, code):
        if code not in self.uncertainty:
            self.uncertainty.append(code)

    def feed(self, data):
        available = OUTPUT_CAP - self.bytes
        self.bytes += min(len(data), available)
        self.buf += data[:available]
        lines = self.buf.split(b"\n")
        self.buf = lines.pop()
        for line in lines:
            self.line(line.rstrip(b"\r").decode("ascii", errors="replace"))
        if len(data) > available:
            self.truncated = True
            self.uncertain("overflow")

    def line(self, line):
        prefix = f"test {CASE} ... "
        if line.startswith(prefix):
            if self.test_line or self.selected != 1:
                self.uncertain("malformed_proof")
            self.test_line = True
            line = line[len(prefix):]
        if line.startswith("running "):
            if line == "running 1 test" and not self.selected and not self.test_line:
                self.selected = 1
            else:
                self.uncertain("malformed_proof")
        elif line in ("ok", "FAILED"):
            if not self.test_line or self.test_status is not None or self.summary:
                self.uncertain("malformed_proof")
                return
            self.test_status = line
            if line == "FAILED":
                self.fail("test_failure")
        elif line.startswith("ARX5B "):
            self.stage(line.split(" ", 3))
        elif line.startswith("test result:"):
            match = RESULT_LINE.fullmatch(line)
            duplicate, self.summary_seen = self.summary_seen, True
            if (duplicate or not match or self.test_status != match[1]
                    or (match[2], match[3]) != (("1", "0") if match[1] == "ok" else ("0", "1"))):
                self.uncertain("malformed_proof")
                return
            self.summary = True
            self.passed, self.failed = int(match[2]), int(match[3])
            if self.failed == 1:
                self.fail("test_failure")

    def stage(self, parts):
        if len(parts) != 4 or not re.fullmatch("[0-9]{1,8}", parts[1]):
            self.uncertain("malformed_proof")
            return
        _, at, stage, fields = parts
        at = int(at)
        if not self.test_line or self.test_status is not None or not self.last_us <= at <= 48_000_000:
            self.uncertain("malformed_proof")
            return
        if stage == "failure" and fields in FAILURES:
            self.fail(fields)
        elif stage == "owner_refusal" and re.fullmatch("[0-7]", fields):
            self.fail(stage)
        elif stage == "orphaned":
            nums = fields.split(" ")
            if (self.orphaned is not None or len(nums) != len(ORPHANED_FIELDS)
                    or not all(re.fullmatch("[0-9]{1,5}", x) for x in nums)):
                self.uncertain("malformed_proof")
                return
            values = list(map(int, nums))
            limits = (3, 3, 65535, 255, 255, 255, 255, 65535, 255, 255, 255, 255, 65535)
            if any(n > limit for n, limit in zip(values, limits)):
                self.uncertain("malformed_proof")
                return
            self.orphaned = dict(zip(ORPHANED_FIELDS, values))
            self.fail(stage)
        elif (stage in PROOFS and self.count < len(PROOFS)
              and stage == tuple(PROOFS)[self.count] and fields in PROOFS[stage]
              and self.first is None):
            self.count += 1
        elif stage in CHECKS and re.fullmatch(r"[01]( [01]){" + str(CHECKS[stage] - 1) + "}", fields):
            if stage == "release_observed" and self.orphaned is not None:
                pass
            elif (self.count < len(PROOFS) and stage == tuple(PROOFS)[self.count]
                    and self.first is None and "0" in fields):
                if stage == "close_joined":
                    # Preserve the concrete Orphaned report as the first failure.
                    self.uncertain("close_unproven")
                else:
                    self.fail(CHECK_FAILURE.get(stage, stage))
            else:
                self.uncertain("malformed_proof")
                return
        else:
            self.uncertain("malformed_proof")
            return
        self.emit({"stage": stage, "us": at, "values": fields.split(" ")})
        self.last_us = at

    def finish(self):
        if self.buf or self.count != len(PROOFS) or not self.summary or (
            self.selected, self.passed, self.failed, self.test_status
        ) != (1, 1, 0, "ok"):
            self.uncertain("incomplete_proof")


class Cutoff:
    """A Job custodian independent of creation, pipe I/O and evidence writes."""
    def __init__(self, backend, clock, deadline):
        self.backend, self.clock, self.deadline = backend, clock, deadline
        self.wake = Event()
        self.natural = False
        self.expired = False
        self.failed = False
        self.worker = Thread(target=self.run, daemon=True)
        self.worker.start()

    def run(self):
        while not self.wake.is_set():
            left = self.deadline - self.clock()
            if left <= 0:
                self.expired = True
                break
            self.wake.wait(left / 1000)
        if not self.natural or self.expired:
            try:
                self.backend.terminate()
            except Exception:
                self.failed = True

    def finish(self, natural, wall):
        self.natural = natural
        self.wake.set()
        self.worker.join(max(0, wall - self.clock()) / 1000)
        return not self.worker.is_alive() and not self.failed


class StageWriter:
    """Single ordered consumer, with a hard queue cap even if storage stalls."""
    def __init__(self, path):
        self.events = Queue(maxsize=64)
        self.hexdigest = None
        self.incomplete = False
        self.worker = Thread(target=self.write, args=(path,), daemon=True)
        self.worker.start()

    def write(self, path):
        sha = hashlib.sha256()
        try:
            with path.open("xb") as stream:
                while (event := self.events.get()) is not None:
                    data = (json.dumps(event) + "\n").encode("ascii")
                    stream.write(data)
                    stream.flush()
                    sha.update(data)
            self.hexdigest = sha.hexdigest()
        except Exception:
            self.incomplete = True

    def emit(self, event):
        if not self.incomplete:
            try:
                self.events.put_nowait(event)
            except Full:
                self.incomplete = True

    def finish(self, clock, deadline):
        try:
            self.events.put(None, timeout=max(0, deadline - clock()) / 1000)
            self.worker.join(max(0, deadline - clock()) / 1000)
        except Full:
            self.incomplete = True
        if self.incomplete or self.worker.is_alive():
            return None
        return self.hexdigest


def observe(backend, clock, sleep, start, proof, custodian=Cutoff):
    """Own one suspended child. Clock values share the outer Windows tick epoch."""
    work, wall = start + 82000, start + 90000
    process = cutoff = natural_code = code = None
    member = resumed = sent = eof = natural = zero = reaped = forced = False

    def read():
        nonlocal eof
        if not eof and not proof.truncated:
            data = backend.read(max(1, OUTPUT_CAP + 1 - proof.bytes))
            eof = data is None
            if data:
                proof.feed(data)

    try:
        spawned = clock()
        need(spawned < work, "admission_deadline")
        stop = min(work, spawned + 48000)
        cutoff = custodian(backend, clock, stop)

        def before_stop():
            need(clock() < stop and not cutoff.expired, "process_deadline")

        before_stop()
        process = backend.create_suspended()
        before_stop()
        member = backend.member(process)
        need(member, "job_membership")
        before_stop()
        backend.resume(process)
        resumed = True
        before_stop()
        early_code = backend.poll(process)
        if early_code is not None and clock() < stop and not cutoff.expired:
            natural_code = early_code
        need(early_code is None, "early_exit")
        before_stop()
        backend.go()
        sent = True
        while clock() < stop and not cutoff.expired:
            code = backend.poll(process)
            if code is not None and clock() < stop and not cutoff.expired and natural_code is None:
                natural_code = code
            read()
            if proof.truncated:
                break
            if natural_code is not None and eof and backend.active() == 0:
                if clock() < stop and not cutoff.expired:
                    natural = zero = reaped = True
                break
            sleep(min(0.01, max(0, stop - clock()) / 1000))
        if not natural and not proof.truncated:
            proof.uncertain("process_deadline")
    except Refusal as error:
        if error.args[0] == "process_deadline":
            proof.uncertain("process_deadline")
        else:
            proof.fail(error.args[0])
    except KeyboardInterrupt:
        proof.uncertain("interrupted")
    except Exception:
        proof.uncertain("process_io")
    finally:
        if process is None:
            process = getattr(backend, "process", None)
        forced = cutoff is not None and not natural
        try:
            if cutoff is not None and not cutoff.finish(natural, wall):
                proof.uncertain("job_termination")
            if cutoff is not None and cutoff.expired:
                forced = True
                natural = False
                proof.uncertain("process_deadline")
        except (Exception, KeyboardInterrupt):
            proof.uncertain("job_termination")
        if not natural:
            pipe_failed = False
            while process is not None and clock() < wall:
                if not pipe_failed:
                    try:
                        read()
                    except (Exception, KeyboardInterrupt):
                        pipe_failed = True
                        proof.uncertain("pipe_cleanup")
                try:
                    code = backend.poll(process)
                    reaped = code is not None
                    zero = backend.active() == 0
                except (Exception, KeyboardInterrupt):
                    proof.uncertain("reap_io")
                    break
                if reaped and zero and (eof or proof.truncated or pipe_failed):
                    break
                sleep(min(0.01, max(0, wall - clock()) / 1000))
            if process is not None and not (reaped and zero and eof):
                proof.uncertain("incomplete_cleanup")
        try:
            backend.close_input()
        except (Exception, KeyboardInterrupt):
            proof.uncertain("stdin_cleanup")
    # A witness emitted before exit may only reach the reader during cleanup.
    if natural_code not in (None, 0):
        proof.fail("child_exit")
    if natural_code is not None and proof.test_status is not None:
        if (natural_code == 0) != (proof.test_status == "ok"):
            proof.uncertain("exit_status_mismatch")
    if clock() >= wall:
        proof.uncertain("wall_deadline")
    proof.finish()
    return {
        "member": member, "resumed": resumed, "go": sent, "natural": natural,
        "forced": forced, "eof": eof, "reaped": reaped, "zero_active": zero,
        "exit_code": code, "natural_exit_code": natural_code,
        "child_ms": None if process is None else clock() - spawned,
    }


class WindowsChild:
    """CreateProcessW inserts the suspended process into the Job atomically."""
    def __init__(self, support, root, env, stack):
        import _winapi
        import ctypes as c
        from ctypes import wintypes as w
        from types import SimpleNamespace

        self.api = support.WinJobApi()
        self.job = self.api.create()
        self.custodian = None
        stack.callback(self.close_job)
        self.wapi, self.c, self.w = _winapi, c, w
        self.namespace = SimpleNamespace
        self.root, self.env, self.stack = root, env, stack
        self.in_r, self.in_w = _winapi.CreatePipe(None, 0)
        stack.callback(self.close_input)
        stack.callback(self.close, "in_r")
        self.out_r, self.out_w = _winapi.CreatePipe(None, 0)
        self.pipe = Queue(maxsize=32)
        self.reader = self.process = None
        stack.callback(self.close_reader)
        stack.callback(self.close, "out_w")
        os.set_handle_inheritable(self.in_r, True)
        os.set_handle_inheritable(self.out_w, True)

    def close(self, name):
        handle = getattr(self, name)
        if handle is not None:
            setattr(self, name, None)
            self.wapi.CloseHandle(handle)

    def close_input(self):
        self.close("in_w")

    def close_job(self):
        # A stuck termination worker retains this handle until process exit.
        if self.custodian is None or not self.custodian.worker.is_alive():
            self.api.close(self.job)

    def close_reader(self):
        if self.reader is None or not self.reader.is_alive():
            self.close("out_r")

    def create_suspended(self):
        c, w, k = self.c, self.w, self.api.k
        H, D, P = w.HANDLE, w.DWORD, c.c_void_p

        class Startup(c.Structure):
            _fields_ = [
                ("cb", D), ("reserved", w.LPWSTR), ("desktop", w.LPWSTR), ("title", w.LPWSTR),
                *((s, D) for s in ("x", "y", "xs", "ys", "xc", "yc", "fill", "flags")),
                ("show", w.WORD), ("reserved_size", w.WORD), ("reserved2", P),
                ("stdin", H), ("stdout", H), ("stderr", H), ("attributes", P),
            ]

        class Process(c.Structure):
            _fields_ = [("process", H), ("thread", H), ("pid", D), ("tid", D)]

        for name, args, ret in (
            ("InitializeProcThreadAttributeList", [P, D, D, c.POINTER(c.c_size_t)], w.BOOL),
            ("UpdateProcThreadAttribute", [P, D, c.c_size_t, P, c.c_size_t, P, P], w.BOOL),
            ("DeleteProcThreadAttributeList", [P], None),
            ("CreateProcessW", [w.LPCWSTR, w.LPWSTR, P, P, w.BOOL, D, P, w.LPCWSTR,
                                c.POINTER(Startup), c.POINTER(Process)], w.BOOL),
            ("ResumeThread", [H], D),
        ):
            function = getattr(k, name)
            function.argtypes, function.restype = args, ret

        size = c.c_size_t()
        k.InitializeProcThreadAttributeList(None, 2, 0, c.byref(size))
        need(0 < size.value <= 4096, "job_attributes")
        attrs = c.create_string_buffer(size.value)
        need(k.InitializeProcThreadAttributeList(attrs, 2, 0, c.byref(size)), "job_attributes")
        info = Process()
        try:
            handles = (H * 2)(self.in_r, self.out_w)
            jobs = (H * 1)(self.job)
            # HANDLE_LIST and JOB_LIST. Failing either refuses the single spawn.
            for attribute, values in ((0x20002, handles), (0x2000D, jobs)):
                need(k.UpdateProcThreadAttribute(attrs, 0, attribute, c.byref(values),
                                                  c.sizeof(values), None, None), "job_attributes")
            startup = Startup(cb=c.sizeof(Startup), flags=0x100, stdin=self.in_r,
                              stdout=self.out_w, stderr=self.out_w, attributes=c.addressof(attrs))
            exe = str(self.root / EXE)
            argv = c.create_unicode_buffer(subprocess.list2cmdline([exe, *ARGS]))
            env = c.create_unicode_buffer("".join(f"{k}={v}\0" for k, v in sorted(self.env.items())) + "\0")
            # EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW |
            # CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED
            need(k.CreateProcessW(exe, argv, None, None, True, 0x08080404, env,
                                  str(self.root), c.byref(startup), c.byref(info)), "spawn")
        finally:
            k.DeleteProcThreadAttributeList(attrs)
            if info.process:
                self.process = self.namespace(_handle=info.process, thread=info.thread)
                self.stack.callback(self.wapi.CloseHandle, info.process)
            if info.thread:
                self.stack.callback(self.wapi.CloseHandle, info.thread)
        self.close("in_r")
        self.close("out_w")
        self.reader = Thread(target=self.drain, daemon=True)
        self.reader.start()
        return self.process

    def member(self, process):
        return self.api.in_job(self.job, process)

    def resume(self, process):
        need(self.api.k.ResumeThread(process.thread) == 1, "resume")

    def poll(self, process):
        if self.wapi.WaitForSingleObject(process._handle, 0) != self.wapi.WAIT_OBJECT_0:
            return None
        return self.wapi.GetExitCodeProcess(process._handle)

    def go(self):
        need(self.wapi.WriteFile(self.in_w, b"G") == (1, 0), "go")

    def drain(self):
        left = OUTPUT_CAP + 1
        try:
            while left:
                data, error = self.wapi.ReadFile(self.out_r, min(left, 4096))
                need(error == 0 and data, "pipe")
                left -= len(data)
                self.pipe.put(data)
        except OSError as error:
            self.pipe.put(None if error.winerror in (109, 232) else Refusal("pipe"))
        except Exception:
            self.pipe.put(Refusal("pipe"))

    def read(self, _want):
        # The producer will read at most OUTPUT_CAP + 1 bytes in total.
        try:
            data = self.pipe.get_nowait()
        except Empty:
            return b""
        if isinstance(data, Refusal):
            raise data
        return data

    def terminate(self):
        need(self.api.terminate(self.job), "terminate")

    def active(self):
        return self.api.active(self.job)


def checked(path, directory):
    kind = stat.S_ISDIR if directory else stat.S_ISREG
    before = path.lstat()
    need(kind(before.st_mode) and not before.st_file_attributes & 0x400
         and (directory or before.st_nlink == 1)
         and path.resolve(strict=True) == path, "path")
    return before


def handle_identity(handle, path, directory):
    import ctypes as c
    from ctypes import wintypes as w

    class Info(c.Structure):
        _fields_ = [("attr", w.DWORD), ("created", w.FILETIME), ("accessed", w.FILETIME),
                    ("written", w.FILETIME), ("volume", w.DWORD), ("size_high", w.DWORD),
                    ("size_low", w.DWORD), ("links", w.DWORD), ("index_high", w.DWORD),
                    ("index_low", w.DWORD)]

    k = c.WinDLL("kernel32", use_last_error=True)
    k.GetFileInformationByHandle.argtypes = [w.HANDLE, c.POINTER(Info)]
    k.GetFinalPathNameByHandleW.argtypes = [w.HANDLE, w.LPWSTR, w.DWORD, w.DWORD]
    k.GetFinalPathNameByHandleW.restype = w.DWORD
    info, final = Info(), c.create_unicode_buffer(1024)
    need(k.GetFileInformationByHandle(handle, c.byref(info))
         and not info.attr & 0x400 and bool(info.attr & 0x10) == directory
         and (directory or info.links == 1), "path")
    size = k.GetFinalPathNameByHandleW(handle, final, len(final), 0)
    need(0 < size < len(final) and final.value == "\\\\?\\" + str(path), "path")
    return info.volume, info.index_high, info.index_low


def checked_handle(path, directory, stack):
    import _winapi

    checked(path, directory)
    flags = 0x00200000 | (0x02000000 if directory else 0)
    share = 3 if directory else 1
    access = 0x80 if directory else 0x80000000
    handle = _winapi.CreateFile(str(path), access, share, 0, 3, flags, 0)
    stack.callback(_winapi.CloseHandle, handle)
    actual = handle_identity(handle, path, directory)
    checked(path, directory)
    other = _winapi.CreateFile(str(path), 0x80, share, 0, 3, flags, 0)
    try:
        need(handle_identity(other, path, directory) == actual, "path")
    finally:
        _winapi.CloseHandle(other)
    return handle


def hold_tree(path, stack, held):
    if path in held:
        return
    if path != path.parent:
        hold_tree(path.parent, stack, held)
    checked_handle(path, True, stack)
    held.add(path)


def stable_input(path, stack, held):
    import msvcrt
    hold_tree(path.parent, stack, held)
    with ExitStack() as ownership:
        handle = checked_handle(path, False, ownership)
        fd = msvcrt.open_osfhandle(handle, os.O_RDONLY | os.O_BINARY | os.O_NOINHERIT)
        ownership.pop_all()
    try:
        return stack.enter_context(os.fdopen(fd, "rb"))
    except BaseException:
        os.close(fd)
        raise


def system_clock():
    import ctypes as c
    k = c.WinDLL("kernel32", use_last_error=True)
    k.GetTickCount64.restype = c.c_ulonglong
    return k.GetTickCount64


def require_ntfs():
    import ctypes as c
    from ctypes import wintypes as w
    k = c.WinDLL("kernel32", use_last_error=True)
    k.GetDriveTypeW.argtypes = [w.LPCWSTR]
    k.GetVolumeInformationW.argtypes = [
        w.LPCWSTR, w.LPWSTR, w.DWORD, c.POINTER(w.DWORD), c.POINTER(w.DWORD),
        c.POINTER(w.DWORD), w.LPWSTR, w.DWORD,
    ]
    fs = c.create_unicode_buffer(16)
    need(k.GetDriveTypeW(ROOT.anchor) == 3 and k.GetVolumeInformationW(
        ROOT.anchor, None, 0, None, None, None, fs, len(fs)
    ) and fs.value == "NTFS", "volume")


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def verified_support(source):
    module = ModuleType("_arx_step5b_pinned_winjob")
    sys.modules[module.__name__] = module
    module.__file__ = str(ROOT / SUPPORT)
    exec(compile(source, "<verified windows_diag_sample>", "exec", dont_inherit=True), module.__dict__)
    return module


def launch():
    need(os.name == "nt", "platform")
    clock = system_clock()
    launched, started_utc = clock(), utc()
    start = validate_entry(sys.argv[1:], os.environ, launched)
    until = start + 82000
    need(sys.flags.isolated and sys.flags.no_site and sys.flags.dont_write_bytecode
         and Path.cwd() == ROOT and Path(__file__) == ROOT / LAUNCHER, "entry")
    require_ntfs()
    with ExitStack() as stack:
        held = set()
        hold_tree(ROOT, stack, held)
        entry = stable_input(ROOT / "entry-claimed", stack, held)
        need(not entry.read(1), "entry_claim")
        with (ROOT / "claimed").open("xb"):
            pass
        for leaf in set(SCRATCH.values()) | {"bin"}:
            hold_tree(ROOT / leaf, stack, held)
        evidence = ROOT / "evidence"
        result_file = stack.enter_context((evidence / "result.json").open("xb"))
        hashes_file = stack.enter_context((evidence / "hashes.json").open("xb"))

        writer = StageWriter(evidence / "stages.jsonl")
        proof = Proof(writer.emit)
        hashes, process = {}, {}
        try:
            pins_stream = stable_input(ROOT / "pins.json", stack, held)
            data = pins_stream.read(16 * 1024 + 1)
            need(len(data) <= 16 * 1024, "pins_size")
            pins = parse_pins(data, os.environ["ARX_STEP5B_PINS_SHA256"])
            python = Path(pins["python"])
            need(python.is_absolute() and Path(sys.executable) == python, "interpreter")
            interpreter = stable_input(python, stack, held)
            verify_file(interpreter, 32 * 1024 * 1024, pins["python_sha256"], clock, until)
            for leaf, sha in pins["python_runtime"].items():
                runtime = stable_input(python.parent / leaf, stack, held)
                verify_file(runtime, 64 * 1024 * 1024, sha, clock, until)
            source = None
            for key, (leaf, cap) in FILES.items():
                stream = stable_input(ROOT / leaf, stack, held)
                verified = verify_file(stream, cap, pins["sha256"][key], clock, until,
                                       retain=key == "support")
                if key == "support":
                    source = verified
            stream = stable_input(ROOT / EXE, stack, held)
            verify_file(stream, pins["binary_bytes"], pins["binary_sha256"], clock, until,
                        size=pins["binary_bytes"])
            hashes.update(pins["sha256"], head=pins["head"], pins=digest(data),
                          binary=pins["binary_sha256"], interpreter=pins["python_sha256"])
            child_env = {k: v for k, v in os.environ.items() if k not in CONTROL}
            with ExitStack() as custody:
                backend = WindowsChild(verified_support(source), ROOT, child_env, custody)
                def start_cutoff(backend, clock, deadline):
                    backend.custodian = Cutoff(backend, clock, deadline)
                    return backend.custodian

                process = observe(backend, clock, time.sleep, start, proof, start_cutoff)
        except Refusal as error:
            proof.fail(error.args[0])
        except FileExistsError:
            proof.fail("evidence_exists")
        except KeyboardInterrupt:
            proof.uncertain("interrupted")
        except Exception:
            proof.fail("admission_io")

        stages_digest = writer.finish(clock, start + 88750)
        if stages_digest is None:
            proof.uncertain("stage_flush")
        else:
            hashes["stages"] = stages_digest
        if clock() >= start + 90000:
            proof.uncertain("wall_deadline")
        if not proof.first and not proof.uncertainty and process.get("natural"):
            disposition = "success"
        else:
            disposition = "failure" if proof.first else "uncertain"
        result = {
            **process, "case": CASE, "disposition": disposition,
            "launcher_started_utc": started_utc, "ended_utc": utc(),
            "wall_ms": clock() - start, "launcher_ms": clock() - launched,
            "selected": proof.selected, "passed": proof.passed, "failed": proof.failed,
            "first_failure": proof.first, "orphaned": proof.orphaned, "truncated": proof.truncated,
            "uncertainty": proof.uncertainty,
        }
        result_bytes = (json.dumps(result, sort_keys=True) + "\n").encode("ascii")
        result_file.write(result_bytes)
        result_file.flush()
        hashes["result"] = digest(result_bytes)
        hashes_file.write((json.dumps(hashes, sort_keys=True) + "\n").encode("ascii"))
        hashes_file.flush()
        return 0 if disposition == "success" else 1


if __name__ == "__main__":
    try:
        sys.exit(launch())
    except (Exception, KeyboardInterrupt):
        # No traceback, OS paths, or untrusted input on the console.
        print("Step 5b launcher refused or interrupted.", file=sys.stderr)
        sys.exit(1)
