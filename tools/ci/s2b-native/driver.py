"""Bounded S2b decomposition evidence; native entry is hosted nonroot Linux only."""
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

PINS = {'crates/arxd/src/exec/cgroup.rs': '64d4cb8c48e516d5fdcd92f85fe2c9c9abb028ee55251929e95824c16211d121', 'crates/arxd/src/exec/mod.rs': 'f8d425f7a57a309b7e10c29b4609f2c0c75002a33bc1eb0c00e04cd47dd3f9b4', 'crates/arxd/src/service/executor.rs': '45da7396dc926ad40b18a5fab1ff2fd898c3d0577b8cd13d91eba1a7205cf5ed', 'crates/arxd/src/service/unix_bridge/tests.rs': 'b2e0c0efc24feb46555fee12413edf79c8afe22d0221908fde59238a597a780d', 'crates/arxd/src/service/unix_bridge/tests/e6.rs': 'f26d1ed0778c5b24495f46bde26b8248f046aedfbcae07440857453bf17dabb1', 'crates/arxd/src/service/unix_bridge/tests/s2b.rs': '6eb93098dfda74ae52e751a07f69fd9252417184d65d6064cb6a26270643e089', 'crates/arxd/src/service/unix_bridge.rs': '323fa965a041c4ad53c99d149f810e031d886cc09e83089d26c82d27b49026d1'}
TARGETS = tuple(PINS)
MANIFEST = "tools/ci/s2b-native/mutations.jsonl"
MANIFEST_SHA = '28f05948ca4aace6944fbe8c77676e8f702575042cf77d9c9c176096ac5b0f94'
WORKFLOW = ".github/workflows/s2b-native-evidence.yml"
WORKFLOW_SHA = '3097257cbc463e1b85a4845fc529f3df8af5c0db768db00d1b3a259aa81ad418'
TEST = "service::unix_bridge::tests::s2b::s2b_real_group_unwinds_before_bridge_returns"
LIMIT = 2 * 1024 * 1024
class GuardError(Exception):
    """Fixed failure category; exception details are never evidence."""


def sha(data):
    return hashlib.sha256(data).hexdigest()


def regular(root, relative):
    """No traversal, symlink or Windows junction in any component."""
    if relative not in (*TARGETS, MANIFEST, WORKFLOW):
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



def statuses(output):
    return re.findall(r"^\s*(PASS|FAIL|SKIP)\s+\[[^\]\r\n]+\]\s+(?:\(\s*\d+/\d+\)\s+)?arxd\s+" + re.escape(TEST) + r"\s*$", output, re.MULTILINE)


def classify(code, output):
    errors = [l.strip() for l in output.splitlines() if re.match(r"^error(?::|\[)", l.strip())]
    # Count every status, including any unexpected other test.
    all_marks = re.findall(r"^\s*(PASS|FAIL|SKIP)\s+\[", output, re.MULTILINE)
    if "SKIP" in output or all_marks != statuses(output):
        return "invalid"
    if code == 100 and statuses(output) == ["FAIL"] and errors == ["error: test run failed"]:
        return "killed"
    if code == 0 and statuses(output) == ["PASS"] and not errors:
        return "survived"
    return "invalid"


def apply(original, row):
    old, new = (row[k].encode() for k in ("old", "new"))
    if not old or old == new or original.count(old) != 1:
        raise GuardError("exact_count")
    return original.replace(old, new, 1)


def load_mutations(root):
    data = bounded_read(regular(root, MANIFEST), 32768)
    if sha(data) != MANIFEST_SHA:
        raise GuardError("manifest_pin")
    rows = [json.loads(line) for line in data.splitlines()]
    if [r.get("mutation") for r in rows] != ["drop_skips_store", "group_forgotten"]:
        raise GuardError("manifest_schema")
    for row, target in zip(rows, ("crates/arxd/src/exec/cgroup.rs",
                                  "crates/arxd/src/service/unix_bridge/tests/s2b.rs")):
        if set(row) != {"mutation", "target", "old", "new"} or row["target"] != target or not all(isinstance(v, str) for v in row.values()):
            raise GuardError("manifest_schema")
    return rows


@contextlib.contextmanager
def mutated(root, originals, row):
    path = regular(root, row["target"])
    original = originals[row["target"]]
    if bounded_read(path, 262144) != original:
        raise GuardError("source_changed")
    try:
        path.write_bytes(apply(original, row))
        yield
    finally:
        regular(root, row["target"]).write_bytes(original)
        if bounded_read(path, 262144) != original:
            raise GuardError("restore_failed")


def command():
    return ["cargo", "nextest", "run", "--locked", "-p", "arxd", "--lib", "-E", "test(=" + TEST + ")", "--success-output", "immediate", "--failure-output", "immediate", "--no-fail-fast", "--color", "never", "--status-level", "pass", "--final-status-level", "none", "--retries", "0", "--no-tests", "fail"]


def fingerprint(root):
    return sha(b"".join(p.encode() + b"\0" + bytes.fromhex(sha(bounded_read(regular(root, p), 262144))) for p in TARGETS))


def emit(label, status, **values):
    print(json.dumps(dict(label=label, status=status, **values), sort_keys=True), flush=True)


def execute(root, runner=run_bounded):
    deadline = time.monotonic() + 480
    def run(cmd, cap=120, reserve=0, limit=LIMIT):
        return runner(cmd, root, min(cap, deadline-time.monotonic()-reserve), limit)
    paths = [*TARGETS, MANIFEST, WORKFLOW]
    code, output, _ = run(["git", "ls-files", "--stage", "-z", "--", *paths], 10, limit=8192)
    records = output.rstrip("\0").split("\0")
    if code or len(records) != len(paths):
        raise GuardError("tracked_guard")
    if any(not re.fullmatch(r"100644 [0-9a-f]{40} 0\t" + re.escape(p), r) for r,p in zip(sorted(records, key=lambda r:r.split("\t")[-1]), sorted(paths))):
        raise GuardError("tracked_guard")
    for cmd, expected in ((["git", "rev-parse", "--show-toplevel"], str(root)), (["git", "status", "--porcelain"], "")):
        code, text, _ = run(cmd, 10, limit=8192)
        if code or text.strip().replace("\\", "/") != expected.replace("\\", "/"):
            raise GuardError("checkout_guard")
    code, head, _ = run(["git", "rev-parse", "HEAD"], 10, limit=128)
    if code or not re.fullmatch(r"[0-9a-f]{40}\n?", head):
        raise GuardError("head_guard")
    emit("checkout", "clean", head=head.strip())
    if sha(bounded_read(regular(root, WORKFLOW), 32768)) != WORKFLOW_SHA:
        raise GuardError("workflow_pin")
    originals = {p: bounded_read(regular(root, p), 262144) for p in TARGETS}
    if any(sha(originals[p]) != PINS[p] for p in TARGETS):
        raise GuardError("source_pin")
    rows = load_mutations(root)
    for row in rows:
        apply(originals[row["target"]], row)
    before = fingerprint(root)
    def baseline(label, first=False):
        code, text, elapsed = run(command(), 210 if first else 90)
        if classify(code, text) != "survived" or fingerprint(root) != before:
            raise GuardError("baseline_failed")
        emit(label, "passed", seconds=elapsed, source_sha256=before)
    complete = False
    try:
        baseline("baseline_before", True)
        for row in rows:
            with mutated(root, originals, row):
                code, text, elapsed = run(command(), 85, 95)
                result = classify(code, text)
            emit(row["mutation"], result, seconds=elapsed, exit=code)
            if result != "killed":
                return 1
        complete = True
    finally:
        for p, data in originals.items():
            path = regular(root, p)
            if bounded_read(path, 262144) != data:
                path.write_bytes(data)
        if fingerprint(root) != before:
            raise GuardError("restore_failed")
        emit("restored_sources", "restored", sha256=before)
        baseline("baseline_after")
    return 0 if complete else 1

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
