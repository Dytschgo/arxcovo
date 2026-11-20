"""Fixed D6 Windows observation launcher. See ADR0039 for qualified entry."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import time
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
supervisor_join final_state outer_release
""".split())
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
        "head", "test", "binary_bytes", "binary_sha256", "python", "python_sha256", "sha256"
    }, "pins_schema")
    need(isinstance(pins["head"], str) and re.fullmatch("[0-9a-f]{40}", pins["head"])
         and pins["test"] == CASE and type(pins["binary_bytes"]) is int
         and 0 < pins["binary_bytes"] <= 512 * 1024 * 1024
         and hex_digest(pins["binary_sha256"]) and hex_digest(pins["python_sha256"])
         and isinstance(pins["python"], str) and 0 < len(pins["python"]) <= 512
         and isinstance(pins["sha256"], dict) and pins["sha256"].keys() == FILES.keys()
         and all(hex_digest(h) for h in pins["sha256"].values()), "pins_schema")
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
        self.summary = False
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
        elif line in ("ok", "FAILED") and self.test_line and self.test_status is None:
            self.test_status = line
            if line == "FAILED":
                self.fail("test_failure")
        elif line.startswith("ARX5B "):
            self.stage(line.split(" ", 3))
        elif line.startswith("test result:"):
            match = RESULT_LINE.fullmatch(line)
            if self.summary or not match or self.test_status != match[1]:
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


def observe(backend, clock, sleep, start, proof):
    """Own one suspended child. Clock values share the outer Windows tick epoch."""
    work, wall = start + 82000, start + 90000
    process, spawned, code = None, None, None
    member = resumed = sent = eof = natural = zero = reaped = forced = False
    try:
        spawned = clock()
        need(spawned < work, "admission_deadline")
        stop = min(work, spawned + 48000)
        process = backend.create_suspended()
        need(clock() < stop, "process_deadline")
        member = backend.member(process)
        need(member, "job_membership")
        need(clock() < stop, "process_deadline")
        backend.resume(process)
        resumed = True
        need(clock() < stop, "process_deadline")
        need(backend.poll(process) is None, "early_exit")
        backend.go()
        sent = True
        while clock() < stop:
            if not eof:
                data = backend.read(max(1, OUTPUT_CAP + 1 - proof.bytes))
                eof = data is None
                if data:
                    proof.feed(data)
            code = backend.poll(process)
            if proof.truncated:
                break
            if code is not None and eof and backend.active() == 0 and clock() < stop:
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
        try:
            backend.close_input()
            if not natural:
                forced = process is not None
                backend.terminate()
                while process is not None and clock() < wall:
                    if not eof and not proof.truncated:
                        data = backend.read(max(1, OUTPUT_CAP + 1 - proof.bytes))
                        eof = data is None
                        if data:
                            proof.feed(data)
                    code = backend.poll(process)
                    reaped = code is not None
                    zero = backend.active() == 0
                    if reaped and zero and (eof or proof.truncated):
                        break
                    sleep(min(0.01, max(0, wall - clock()) / 1000))
                if process is not None and not (reaped and zero and eof):
                    proof.uncertain("incomplete_cleanup")
        except (Exception, KeyboardInterrupt):
            proof.uncertain("cleanup_io")
    if code not in (None, 0) and not forced:
        proof.fail("child_exit")
    if clock() >= wall:
        proof.uncertain("wall_deadline")
    proof.finish()
    return {
        "member": member, "resumed": resumed, "go": sent, "natural": natural,
        "forced": forced, "eof": eof, "reaped": reaped, "zero_active": zero,
        "exit_code": code, "child_ms": None if process is None else clock() - spawned,
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
        stack.callback(self.api.close, self.job)
        self.wapi, self.c, self.w = _winapi, c, w
        self.namespace = SimpleNamespace
        self.root, self.env, self.stack = root, env, stack
        self.in_r, self.in_w = _winapi.CreatePipe(None, 0)
        stack.callback(self.close_input)
        stack.callback(self.close, "in_r")
        self.out_r, self.out_w = _winapi.CreatePipe(None, 0)
        stack.callback(self.close, "out_r")
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
                self.stack.callback(self.wapi.CloseHandle, info.process)
            if info.thread:
                self.stack.callback(self.wapi.CloseHandle, info.thread)
        self.close("in_r")
        self.close("out_w")
        return self.namespace(_handle=info.process, thread=info.thread)

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
        self.close_input()

    def read(self, want):
        try:
            available, _ = self.wapi.PeekNamedPipe(self.out_r)
            if available:
                data, error = self.wapi.ReadFile(self.out_r, min(available, want))
                need(error == 0, "pipe")
                return data
        except OSError as error:
            if error.winerror in (109, 232):  # broken pipe / no data
                return None
            raise
        return b""

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


def hold_tree(path, stack, held):
    import _winapi
    if path in held:
        return
    if path != path.parent:
        hold_tree(path.parent, stack, held)
    checked(path, True)
    # Read attributes, deny deletion/rename, open the directory object itself.
    handle = _winapi.CreateFile(str(path), 0x80, 3, 0, 3, 0x02200000, 0)
    stack.callback(_winapi.CloseHandle, handle)
    checked(path, True)
    held.add(path)


def stable_input(path, stack, held):
    import _winapi
    import msvcrt
    hold_tree(path.parent, stack, held)
    before = checked(path, False)
    handle = _winapi.CreateFile(str(path), 0x80000000, 1, 0, 3, 0x00200000, 0)
    try:
        fd = msvcrt.open_osfhandle(handle, os.O_RDONLY | os.O_BINARY | os.O_NOINHERIT)
    except BaseException:
        _winapi.CloseHandle(handle)
        raise
    stream = stack.enter_context(os.fdopen(fd, "rb"))
    after = os.fstat(fd)
    need((after.st_dev, after.st_ino) == (before.st_dev, before.st_ino)
         and after.st_nlink == 1 and not after.st_file_attributes & 0x400, "path")
    return stream


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
        stages = stack.enter_context((evidence / "stages.jsonl").open("xb"))
        result_file = stack.enter_context((evidence / "result.json").open("xb"))
        hashes_file = stack.enter_context((evidence / "hashes.json").open("xb"))

        def emit(event):
            stages.write((json.dumps(event) + "\n").encode("ascii"))
            stages.flush()

        proof = Proof(emit)
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
                process = observe(backend, clock, time.sleep, start, proof)
        except Refusal as error:
            proof.fail(error.args[0])
        except FileExistsError:
            proof.fail("evidence_exists")
        except KeyboardInterrupt:
            proof.uncertain("interrupted")
        except Exception:
            proof.fail("admission_io")

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
        with (evidence / "stages.jsonl").open("rb") as stream:
            hashes["stages"] = digest(stream.read(64 * 1024))
        hashes["result"] = digest(result_bytes)
        hashes_file.write((json.dumps(hashes, sort_keys=True) + "\n").encode("ascii"))
        return 0 if disposition == "success" else 1


if __name__ == "__main__":
    try:
        sys.exit(launch())
    except (Exception, KeyboardInterrupt):
        # No traceback, OS paths, or untrusted input on the console.
        print("Step 5b launcher refused or interrupted.", file=sys.stderr)
        sys.exit(1)

