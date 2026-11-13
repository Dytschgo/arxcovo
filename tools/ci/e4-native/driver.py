"""Fixed E4 evidence. Native entry point is reviewed ordinary Ubuntu CI only."""
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

PINS = {'crates/arxd/src/unix_shim_rights_linux.rs': '2eb9036e2d6800d1376e3fe7bfce9f88cb1d8e38e69ddca8bd2a7de1164e3efd', 'crates/arxd/src/unix_shim.rs': '5e7a01094733e882b1ac67376cba5e24cbb1ad1db771d06a944806b747361fd4', 'crates/arxd/src/unix_shim_rights.rs': '17310f10a8035d2a732c7b2e87a3be4dae1d4caabdee8dade8c6cea8a7017928', 'crates/arxd/src/unix_shim_rights_linux_tests.rs': '102ec5720cd1ecfd1236ebea4237025e155bcef974de27ad876b550525e059fd', 'crates/arxd/tests/unix_shim.rs': '20d0f61736d7d57c973ad19a9b027fa7d38bd05e45857c3dcee199570d905b7a'}
TARGETS = tuple(PINS)
MANIFEST = "tools/ci/e4-native/mutations.jsonl"
MANIFEST_SHA = '15980053ce88457578407686d309af95a6dd18cb175fd777d82ac0e70b850671'
PREFIX = 'unix_shim::rights::linux::tests::'
NAMES = ('s8_outcomes_and_reap', 's9_all_candidates_before_exec', 's9b_exact_alias', 's9c_bound_object_survives_file_and_directory_swap', 's10_path_is_irrelevant', 's12_production_constants', 's11_nnp_parser', 's11_nnp_observed_in_child', 's9_final_object_owner_check')
TESTS = tuple(PREFIX + name for name in NAMES)
S6 = "s6_probe_bounded_order_root_cwd_and_deadline"
L8 = "exec::hierarchy::linux::tests::l8_unprivileged_hardening_zero_changes"
LIMIT = 2 * 1024 * 1024

class GuardError(Exception):
    """Fixed failure category; exception details are never evidence."""


def sha(data):
    return hashlib.sha256(data).hexdigest()


def regular(root, relative):
    """No traversal, symlink or Windows junction in any component."""
    if relative not in (*TARGETS, MANIFEST):
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


def statuses(output, name, binary="arxd"):
    pattern = r"^\s*(PASS|FAIL|SKIP)\s+\[[^\]\r\n]+\]\s+(?:\(\s*\d+/\d+\)\s+)?" + re.escape(binary) + r"\s+" + re.escape(name) + r"\s*$"
    return re.findall(pattern, output, re.MULTILINE)


def error_lines(output):
    return [line.strip() for line in output.splitlines()
            if re.match(r"^error(?::|\[)", line.strip())]


def classify(code, output, name, binary="arxd"):
    if "SKIP" in output:
        return "invalid"
    errors = error_lines(output)
    marks = statuses(output, name, binary)
    if code == 100 and marks == ["FAIL"] and errors == ["error: test run failed"]:
        return "killed"
    if code == 0 and marks == ["PASS"] and not errors:
        return "survived"
    return "invalid"




def load_mutations(root):
    data = bounded_read(regular(root, MANIFEST), 32768)
    if sha(data) != MANIFEST_SHA:
        raise GuardError("manifest_pin")
    rows = [json.loads(line) for line in data.splitlines()]
    if len(rows) != 11:
        raise GuardError("manifest_count")
    ids = set()
    for row in rows:
        if set(row) != {"mutation", "changes", "assertion", "binary", "blocker"}:
            raise GuardError("manifest_schema")
        if not re.fullmatch(r"S[0-9A-Za-z_]{1,48}", row["mutation"]) or row["mutation"] in ids:
            raise GuardError("manifest_schema")
        ids.add(row["mutation"])
        if row["blocker"] is not None:
            raise GuardError("manifest_schema")
        if row["binary"] == "arxd":
            valid = row["assertion"] in TESTS
        else:
            valid = row["binary"] == "unix_shim" and row["assertion"] == S6
        if not valid or len(row["changes"]) != 1:
            raise GuardError("manifest_schema")
        for change in row["changes"]:
            if set(change) != {"target", "old", "new"} or change["target"] not in TARGETS[:2]:
                raise GuardError("manifest_schema")
            if not all(isinstance(change[k], str) for k in ("old", "new")):
                raise GuardError("manifest_schema")
    return rows


def apply(original, change):
    old, new = (change[k].encode("utf-8") for k in ("old", "new"))
    # Linux tests are out-of-line; the shim has an ordinary inline test module.
    boundary = original.rfind(b"\n#[cfg(test)]\n")
    if boundary < 0 or not old or old == new or original.count(old) != 1 or original[:boundary].count(old) != 1:
        raise GuardError("exact_count")
    return original.replace(old, new, 1)


@contextlib.contextmanager
def mutated(root, originals, row):
    change = row["changes"][0]
    target = change["target"]
    path = regular(root, target)
    original = originals[target]
    if bounded_read(path, 262144) != original:
        raise GuardError("source_changed")
    replacement = apply(original, change)
    try:
        path.write_bytes(replacement)
        yield
    finally:
        regular(root, target).write_bytes(original)
        if bounded_read(path, 262144) != original:
            raise GuardError("restore_failed")


def command(name=None, binary="arxd"):
    selection = "test(=" + name + ")" if name else " | ".join(
        "test(=" + n + ")" for n in TESTS)
    target = ["--lib"] if binary == "arxd" else ["--test", "unix_shim"]
    return ["cargo", "nextest", "run", "--locked", "-p", "arxd", *target, "-E", selection,
            "--success-output", "immediate", "--no-fail-fast", "--color", "never",
            "--failure-output", "immediate", "--status-level", "pass",
            "--final-status-level", "none", "--retries", "0", "--no-tests", "fail"]


def baseline_result(code, output, names=TESTS, binary="arxd"):
    if code != 0 or error_lines(output):
        raise GuardError("baseline_failed")
    for name in names:
        if statuses(output, name, binary) != ["PASS"]:
            raise GuardError("baseline_missing_test")
    allowed = {"E4_SKIP_INHERITED_NNP", "SKIP L8: unexpected Standard hardening"}
    for line in output.splitlines():
        if "SKIP" in line and line.strip() not in allowed:
            raise GuardError("unexpected_skip")
    return {"inherited_nnp": "skipped" if "E4_SKIP_INHERITED_NNP" in output else "passed",
            "L8": "skipped" if "SKIP L8: unexpected Standard hardening" in output else "passed"}


def fingerprint(root):
    digest = hashlib.sha256()
    for target in TARGETS:
        digest.update(target.encode() + b"\0")
        digest.update(bytes.fromhex(sha(bounded_read(regular(root, target), 262144))))
    return digest.hexdigest()


def emit(label, status, **values):
    print(json.dumps({"label": label, "status": status, **values}, sort_keys=True), flush=True)


def tracked(root, runner):
    paths = [*TARGETS, MANIFEST]
    code, output, _ = runner(["git", "ls-files", "--stage", "-z", "--", *paths], root, 10, 8192)
    expected = set(paths)
    for record in output.rstrip("\0").split("\0"):
        match = re.fullmatch(r"100644 [0-9a-f]{40} 0\t([^\0]+)", record)
        if not match or match[1] not in expected:
            raise GuardError("tracked_guard")
        expected.remove(match[1])
    if code != 0 or expected:
        raise GuardError("tracked_guard")


def execute(root, runner=run_bounded):
    originals = {p: bounded_read(regular(root, p), 262144) for p in TARGETS}
    if any(sha(originals[p]) != PINS[p] for p in TARGETS):
        raise GuardError("source_pin")
    rows = load_mutations(root)
    for row in rows:
        change = row["changes"][0]
        apply(originals[change["target"]], change)
    tracked(root, runner)
    before = fingerprint(root)
    deadline = time.monotonic() + 480
    outcome = "complete"

    def budget(seconds, reserve=0):
        return min(seconds, deadline - time.monotonic() - reserve)

    def baseline(label, first=False):
        if fingerprint(root) != before:
            raise GuardError("source_changed")
        code, output, elapsed = runner(command(), root, budget(180 if first else 50))
        observed = baseline_result(code, output)
        emit(label + "_library", "passed", seconds=elapsed,
             inherited_nnp=observed["inherited_nnp"], source_sha256=before)
        code, output, elapsed = runner(command(L8), root, budget(15))
        observed = baseline_result(code, output, (L8,))
        emit(label + "_L8", observed["L8"], seconds=elapsed)
        code, output, elapsed = runner(command(S6, "unix_shim"), root, budget(60 if first else 20))
        baseline_result(code, output, (S6,), "unix_shim")
        if fingerprint(root) != before:
            raise GuardError("source_changed")
        emit(label + "_S6_NNP_fenced", "passed", seconds=elapsed, source_sha256=before)

    try:
        baseline("baseline_before", True)
        for row in rows:
            if row["blocker"]:
                emit(row["mutation"], "blocked", reason="fixture_gap_nnp_self_skip")
                outcome = "incomplete"
                break
            try:
                with mutated(root, originals, row):
                    code, output, elapsed = runner(command(row["assertion"], row["binary"]), root, budget(35, 85))
                    result = classify(code, output, row["assertion"], row["binary"])
                emit(row["mutation"], result, seconds=elapsed, exit=code)
                if result != "killed":
                    outcome = result
                    break
            except GuardError as error:
                emit(row["mutation"], "failed_closed", reason=str(error))
                outcome = "incomplete"
                break
    finally:
        for target, original in originals.items():
            path = regular(root, target)
            if bounded_read(path, 262144) != original:
                path.write_bytes(original)
        after = fingerprint(root)
        if after != before:
            raise GuardError("restore_failed")
        emit("restored_sources", "restored", sha256=after)
        baseline("baseline_after")
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
    except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired) as error:
        reason = "reap_timeout" if isinstance(error, subprocess.TimeoutExpired) else (
            "os_error" if isinstance(error, OSError) else "invalid_data")
        emit("harness", "failed_closed", reason=reason)
        sys.exit(1)
