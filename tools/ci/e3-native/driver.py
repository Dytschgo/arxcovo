"""Bounded, fixed E3 native evidence; execute only in reviewed Ubuntu CI."""
import contextlib
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import threading
import time

TARGET = "crates/arxd/src/exec/hierarchy/linux.rs"
SOURCE_SHA = "e5b6ac15d01092a96ea23488f23938f1b897717efe19b5bd3f71e898ef54eaff"
MANIFEST_SHA = "c824820588a406800124352931597e3a220a32b945333e85d3a186612bca3d93"
PREFIX = "exec::hierarchy::linux::tests::"
NAMES = (
    "l1_read_limit_leaves_excess_pipe_bytes_unread", "l1_bounds_and_bounded_writes",
    "l2_nofollow_and_nonblocking_fifo", "l3_raw_budget_includes_dots",
    "l4_entry_kinds", "l5_system_constants_and_shared_mode_trust",
    "l7_xattr_bounded", "l8_unprivileged_hardening_zero_changes",
    "l9_fresh_descriptions_and_owned_lifetime", "l10_real_entry_kinds",
)
TESTS = tuple(PREFIX + name for name in NAMES)
LIMIT = 2 * 1024 * 1024


class GuardError(Exception):
    """Fixed failure category; exception details are never evidence."""


def sha(data):
    return hashlib.sha256(data).hexdigest()


def regular(root, relative):
    """No traversal, symlink or Windows junction in any component."""
    if relative not in (TARGET, "tools/ci/e3-native/mutations.jsonl"):
        raise GuardError("target_guard")
    root = Path(root)
    for path in (*reversed(root.parents), root, *[root.joinpath(*Path(relative).parts[:n])
                  for n in range(1, len(Path(relative).parts) + 1)]):
        info = path.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise GuardError("target_guard")
    path = root / relative
    if not stat.S_ISREG(path.lstat().st_mode) or path.stat().st_nlink != 1:
        raise GuardError("target_guard")
    return path


def bounded_read(path, limit):
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise GuardError("read_bound")
    return data


def load_mutations(root):
    data = bounded_read(regular(root, "tools/ci/e3-native/mutations.jsonl"), 32768)
    if sha(data) != MANIFEST_SHA:
        raise GuardError("manifest_pin")
    rows = [json.loads(line) for line in data.splitlines()]
    if len(rows) != 8:
        raise GuardError("manifest_count")
    for row in rows:
        if set(row) != {"mutation", "changes", "assertions"}:
            raise GuardError("manifest_schema")
        if len(row["changes"]) != 1 or len(row["assertions"]) != 1:
            raise GuardError("manifest_schema")
        if row["assertions"][0] not in TESTS:
            raise GuardError("manifest_schema")
    return rows


def apply(original, row):
    old, new = (value.encode("utf-8") for value in row["changes"][0])
    # Mutations must occur once, entirely before the native test module.
    boundary = original.index(b"#[cfg(test)]\nmod tests {")
    if not old or old == new or original.count(old) != 1 or original[:boundary].count(old) != 1:
        raise GuardError("exact_count")
    return original.replace(old, new, 1)


@contextlib.contextmanager
def mutated(root, original, row):
    path = regular(root, TARGET)
    if bounded_read(path, 65536) != original:
        raise GuardError("source_changed")
    replacement = apply(original, row)
    try:
        path.write_bytes(replacement)
        yield
    finally:
        regular(root, TARGET).write_bytes(original)
        if sha(bounded_read(path, 65536)) != sha(original):
            raise GuardError("restore_failed")


def run_bounded(command, cwd, seconds, limit=LIMIT):
    """Own process group, bounded capture and wall time, no shell or raw logging."""
    if seconds <= 0:
        raise GuardError("deadline")
    started = time.monotonic()
    proc = subprocess.Popen(command, cwd=cwd, stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            start_new_session=(os.name == "posix"))
    output = bytearray()
    overflow = threading.Event()
    read_error = threading.Event()
    finished = threading.Event()

    def read():
        try:
            while True:
                chunk = proc.stdout.read(4096)
                if not chunk:
                    break
                if len(output) + len(chunk) > limit:
                    overflow.set()
                    break
                output.extend(chunk)
        except OSError:
            read_error.set()
        finally:
            finished.set()

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    failure = None
    try:
        while proc.poll() is None or not finished.is_set():
            if overflow.is_set():
                failure = "output_bound"
                break
            if time.monotonic() - started >= seconds:
                failure = "deadline"
                break
            finished.wait(0.01) if not finished.is_set() else time.sleep(0.01)
    finally:
        # Also closes descendants holding the capture pipe after the parent exits.
        if os.name == "posix":
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        elif proc.poll() is None:
            proc.kill()
        proc.wait(timeout=5)
        reader.join(timeout=5)
        proc.stdout.close()
    if failure or overflow.is_set() or reader.is_alive() or read_error.is_set():
        raise GuardError(failure or ("capture_failed" if read_error.is_set() else "output_bound"))
    return proc.returncode, output.decode("utf-8", errors="replace"), round(time.monotonic() - started, 3)


def statuses(output, name):
    pattern = r"^\s*(PASS|FAIL|SKIP)\s+\[[^\]\r\n]+\]\s+(?:\(\s*\d+/\d+\)\s+)?arxd\s+" + re.escape(name) + r"\s*$"
    return re.findall(pattern, output, re.MULTILINE)


def error_lines(output):
    return [line.strip() for line in output.splitlines()
            if re.match(r"^error(?::|\[)", line.strip())]


def classify(code, output, name):
    if "SKIP" in output:
        return "invalid"
    errors = error_lines(output)
    marks = statuses(output, name)
    if code == 100 and marks == ["FAIL"] and errors == ["error: test run failed"]:
        return "killed"
    if code == 0 and marks == ["PASS"] and not errors:
        return "survived"
    return "invalid"


def baseline_result(code, output):
    if code != 0 or "SKIP E3" in output or error_lines(output):
        raise GuardError("baseline_failed")
    for name in TESTS:
        if statuses(output, name) != ["PASS"]:
            raise GuardError("baseline_missing_test")
    allowed = ("SKIP L7: user xattrs refused", "SKIP L8: unexpected Standard hardening")
    for line in output.splitlines():
        if "SKIP" in line and line.strip() not in allowed:
            raise GuardError("unexpected_skip")
    return {"L7": "skipped" if allowed[0] in output else "passed",
            "L8": "skipped" if allowed[1] in output else "passed"}


def command(name=None):
    selection = "test(=" + name + ")" if name else "test(exec::hierarchy::linux::tests::)"
    return ["cargo", "nextest", "run", "--locked", "-p", "arxd", "--lib", "-E", selection,
            "--success-output", "immediate", "--no-fail-fast", "--color", "never",
            "--failure-output", "immediate", "--status-level", "pass",
            "--final-status-level", "none", "--retries", "0", "--no-tests", "fail"]


def tree_hash(root):
    """Content hash of the hierarchy tree, including relative filenames."""
    base = root / "crates/arxd/src/exec/hierarchy"
    digest = hashlib.sha256()
    # Do not materialize an unbounded directory inventory before checking the cap.
    paths = []
    for path in base.rglob("*"):
        paths.append(path)
        if len(paths) > 64:
            raise GuardError("tree_bound")
    for path in sorted(paths):
        info = path.lstat()
        if path.is_symlink() or getattr(info, "st_file_attributes", 0) & 0x400:
            raise GuardError("tree_guard")
        if path.is_file():
            digest.update(path.relative_to(base).as_posix().encode() + b"\0")
            digest.update(bytes.fromhex(sha(bounded_read(path, 262144))))
    return digest.hexdigest()


def emit(label, status, **values):
    print(json.dumps({"label": label, "status": status, **values}, sort_keys=True), flush=True)


def execute(root, runner=run_bounded):
    original = bounded_read(regular(root, TARGET), 65536)
    if sha(original) != SOURCE_SHA:
        raise GuardError("source_pin")
    rows = load_mutations(root)
    for row in rows:
        apply(original, row)
    before = tree_hash(root)
    deadline = time.monotonic() + 480
    outcome = "complete"

    def baseline(label, max_seconds):
        code, output, elapsed = runner(command(), root, min(max_seconds, deadline - time.monotonic()))
        results = baseline_result(code, output)
        emit(label, "passed", seconds=elapsed, **results)

    try:
        baseline("baseline_before", 180)
        for row in rows:
            try:
                with mutated(root, original, row):
                    code, output, elapsed = runner(command(row["assertions"][0]), root,
                                                  min(40, deadline - time.monotonic() - 60))
                    result = classify(code, output, row["assertions"][0])
                    emit(row["mutation"], result, seconds=elapsed, exit=code)
                if result != "killed":
                    outcome = result
                    break
            except GuardError as error:
                emit(row["mutation"], str(error))
                outcome = "incomplete"
                break
    finally:
        # Even a failed baseline/mutant owes a restored-source check and a final baseline.
        if sha(bounded_read(regular(root, TARGET), 65536)) != sha(original):
            raise GuardError("restore_failed")
        after = tree_hash(root)
        emit("restored_tree", "restored" if after == before else "changed",
             source_sha256=sha(original), tree_sha256=after)
        if after != before:
            raise GuardError("tree_changed")
        baseline("baseline_after", 60)
    return 0 if outcome == "complete" else 1


def main():
    if len(sys.argv) != 1 or sys.platform != "linux":
        raise GuardError("ubuntu_only")
    uid, euid = os.getuid(), os.geteuid()
    emit("identity", "ordinary" if uid != 0 and euid != 0 else "refused", uid=uid, euid=euid)
    if uid == 0 or euid == 0:
        raise GuardError("root_refused")
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_OS") != "Linux":
        raise GuardError("hosted_only")
    os.umask(0o077)
    root = Path(__file__).absolute().parents[3]
    if Path.cwd().absolute() != root:
        raise GuardError("cwd_guard")
    return execute(root)


if __name__ == "__main__":
    def interrupted(_signum, _frame):
        raise GuardError("interrupted")

    signal.signal(signal.SIGTERM, interrupted)
    try:
        sys.exit(main())
    except GuardError as error:
        emit("harness", "failed_closed", reason=str(error))
        sys.exit(1)
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        reason = "reap_timeout" if isinstance(error, subprocess.TimeoutExpired) else (
            "os_error" if isinstance(error, OSError) else "value_error")
        emit("harness", "failed_closed", reason=reason)
        sys.exit(1)
