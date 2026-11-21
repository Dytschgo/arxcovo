"""Run the single pinned C13 campaign using disposable hosted runner roles."""
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import re
import selectors
import stat
import subprocess
import sys
import tempfile
import time
import unittest

SELECTOR = "c13_activation_restarts_original_nodes_and_issues_after_founder_close"
PROBE = ("cluster::consensus::bound_start::runtime::tests::"
         "publish_only_probe::c13_final_publication_only")
ROLES = (("arxc13-owner", 21901), ("arxc13-daemon", 21902),
         ("arxc13-agent", 21903), ("arxc13-build", 21904))
DAEMON = 21902
OWNER = 21901
ARTIFACT = re.compile(r"(?:arxd|m3_activation_linux)-[a-f0-9]{16}")
ROOT = Path(__file__).resolve().parents[3]
ENV = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LC_ALL": "C"}
TOTAL = 174
ROOT_TEARDOWN = 14
OUTER_MARGIN = 6
CAMPAIGN_TIMEOUT = 124  # 102 s work + 17 s joined cleanup + process margin.
OUTPUT_LIMIT = 32_768
REAP_GRACE = 0.25
CLEANUP = re.compile(
    r"C13 cleanup spawned=([0-6]) signaled=([0-6]) reaped=([0-6]) clean=([0-6]) "
    r"helpers=(\d{1,4}) helpers_reaped=(\d{1,4}) helper_unknown=([01])"
)
FIRST = re.compile(
    r"C13 first phase=(preflight|founder|join|promotion-transfer|recovery|survivor|publication|close) "
    r"category=(assertion|spawn|helper-reap|listener|daemon-refused|terminal-io|pipe-io)"
)
PIN = "C13 starts=6 closes=6 Found=1 Mint=3 AnchorQuery=3 Redeem=3 admissions=2 AddNodes=2 joint=1 flatten=1 transfer=2 enrollment_proposals=7"


def exact_listing(data, name):
    return data == f"{name}: test\n\n1 test, 0 benchmarks\n"


def checked_file(path):
    path = Path(path)
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink < 1 or not metadata.st_mode & 0o100:
        raise ValueError("artifact type")
    if not 1 <= metadata.st_size <= 256 * 1024**2:
        raise ValueError("artifact size")
    return path


def select(deps, prefix, name, deadline):
    matches = []
    from itertools import islice
    entries = sorted(islice(deps.iterdir(), 4097))
    if len(entries) > 4096:
        raise ValueError("artifact inventory")
    for path in entries:
        if not ARTIFACT.fullmatch(path.name) or not path.name.startswith(prefix):
            continue
        checked_file(path)
        with open_artifact(path) as artifact:
            executable = f"/proc/{os.getpid()}/fd/{artifact.fileno()}"
            code, capture = bounded_run([executable, "--list", "--exact", name],
                                        env=ENV, timeout=remaining(deadline, 3))
        if code == 0 and not capture.truncated and exact_listing(capture.text(), name):
            matches.append(path)
    if len(matches) != 1:
        raise ValueError("selector inventory")
    return matches[0]


def remaining(deadline, limit):
    available = deadline - time.monotonic()
    if available <= 0:
        raise TimeoutError("phase budget elapsed")
    return min(limit, available)


def command(arguments, deadline, limit=3):
    subprocess.run(arguments, stdin=subprocess.DEVNULL, env=ENV,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   timeout=remaining(deadline, limit), check=True)


def unused(name, id_):
    import grp
    import pwd
    for lookup, key in ((pwd.getpwnam, name), (pwd.getpwuid, id_),
                        (grp.getgrnam, name), (grp.getgrgid, id_)):
        try:
            lookup(key)
        except KeyError:
            continue
        return False
    return True


def stage_tree(root):
    root.chmod(0o755)
    for name in ("a", "b", "c", "d"):
        node = root / name
        node.mkdir(mode=0o755)
        for directory in ("state", "credentials", "wrap", "private", "private/custody", "history", "workspace"):
            path = node / directory
            path.mkdir(mode=0o700)
            os.chown(path, DAEMON, DAEMON)
        control = node / "control"
        control.mkdir(mode=0o700)
        os.chown(control, DAEMON, OWNER)
        control.chmod(0o2710)
        config = node / "install.json"
        with config.open("xb"):
            pass
        os.chown(config, DAEMON, DAEMON)
        config.chmod(0o600)


def open_artifact(source):
    metadata = checked_file(source).lstat()
    fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    file = os.fdopen(fd, "rb")
    current = os.fstat(fd)
    if (not stat.S_ISREG(current.st_mode)
            or (current.st_dev, current.st_ino, current.st_size, current.st_mode)
            != (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mode)):
        file.close()
        raise ValueError("artifact substituted")
    return file


def stage_binary(source, destination, deadline):
    with open_artifact(source) as input_file, destination.open("xb") as output:
        before = os.fstat(input_file.fileno())
        digest = hashlib.sha256()
        unread = before.st_size
        while unread:
            remaining(deadline, TOTAL)
            block = input_file.read(min(unread, 1024**2))
            if not block:
                raise ValueError("artifact truncated")
            output.write(block)
            digest.update(block)
            unread -= len(block)
        after = os.fstat(input_file.fileno())
        if (after.st_size, after.st_mtime_ns) != (before.st_size, before.st_mtime_ns):
            raise ValueError("artifact changed during stage")
    readback = hashlib.sha256()
    with destination.open("rb") as output:
        for block in iter(lambda: output.read(1024**2), b""):
            remaining(deadline, TOTAL)
            readback.update(block)
    if readback.digest() != digest.digest():
        raise ValueError("artifact changed during stage")
    destination.chmod(0o755)
    return digest.hexdigest()


class Capture:
    def __init__(self):
        self.bulk = bytearray()
        self.truncated = False
        self.first = None
        self._line = b""

    def feed(self, data):
        room = OUTPUT_LIMIT - len(self.bulk)
        self.bulk.extend(data[:room])
        self.truncated |= len(data) > room
        if self.first is not None:
            return
        # Track one candidate independently of the retained bulk prefix.
        # Every allowed record fits in 96 bytes, including a Windows CR.
        *lines, tail = data.split(b"\n")
        for line in lines:
            if self._line is not None and len(self._line) + len(line) <= 96:
                record = (self._line + line).decode("ascii", errors="replace").removesuffix("\r")
                if FIRST.fullmatch(record):
                    self.first = record
                    return
            self._line = b""
        if self._line is not None:
            self._line = self._line + tail if len(self._line) + len(tail) <= 96 else None

    def text(self):
        return self.bulk.decode("utf-8", errors="replace")


def pipe_capacity(fd):
    import fcntl
    return fcntl.fcntl(fd, fcntl.F_GETPIPE_SZ)


def bounded_run(args, *, env, timeout):
    capture = Capture()
    deadline = time.monotonic() + timeout
    work_deadline = deadline - REAP_GRACE
    remaining(work_deadline, timeout)
    process = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.DEVNULL)
    # Popen's context exit waits implicitly. Own the pipe separately so an
    # unobserved child cannot extend either exit path.
    with process.stdout, selectors.DefaultSelector() as sel:
        fd = process.stdout.fileno()
        os.set_blocking(fd, False)
        sel.register(fd, selectors.EVENT_READ)
        try:
            while sel.select(remaining(work_deadline, timeout)):
                data = os.read(fd, 4096)
                if not data:
                    break
                capture.feed(data)
            code = process.wait(timeout=remaining(work_deadline, timeout))
        except (TimeoutError, subprocess.TimeoutExpired):
            process.kill()
            try:
                process.wait(timeout=remaining(deadline, REAP_GRACE))
            except (TimeoutError, subprocess.TimeoutExpired):
                pass  # Disposition stays unknown whether reap succeeded or timed out.
            code = None
            # Retain already-written records even at the deadline. Bound this
            # drain to one pipe capacity in case a descendant kept stdout.
            pending = pipe_capacity(fd)
            while pending:
                try:
                    data = os.read(fd, min(pending, 4096))
                except BlockingIOError:
                    break
                if not data:
                    break
                capture.feed(data)
                pending -= len(data)
    return code, capture


def cleanup_observed(lines):
    records = [m.groups() for line in lines if (m := CLEANUP.fullmatch(line))]
    if len(records) != 1:
        return False
    spawned, signaled, reaped, clean, helpers, helpers_reaped, helper_unknown = records[0]
    return (spawned == signaled == reaped == clean and helpers == helpers_reaped
            and helper_unknown == "0")


def primary_ip(deadline):
    run = subprocess.run(["/usr/sbin/ip", "-j", "-4", "addr", "show", "scope", "global"],
                         env=ENV, capture_output=True, stdin=subprocess.DEVNULL,
                         check=True, timeout=remaining(deadline, 4))
    if len(run.stdout) > 32_768:
        raise ValueError("bounded interface inventory")
    private = tuple(map(ipaddress.ip_network, ("10.0.0.0/8", "172.16.0.0/12",
                                              "192.168.0.0/16", "100.64.0.0/10")))
    found = {str(ip) for link in json.loads(run.stdout) if "UP" in link.get("flags", ())
             for address in link.get("addr_info", ())
             if any((ip := ipaddress.IPv4Address(address["local"])) in net for net in private)}
    if len(found) != 1:
        raise ValueError("eligible runner address")
    return found.pop()


def qualify_runner():
    if (sys.platform != "linux" or os.environ.get("GITHUB_ACTIONS") != "true"
            or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"
            or os.environ.get("RUNNER_OS") != "Linux"):
        raise ValueError("hosted Linux only")


def privileged(stage, deadline):
    qualify_runner()
    if os.geteuid() != 0:
        raise ValueError("runner staging identity")
    stage = Path(stage)
    if stage.parent != Path("/tmp") or not re.fullmatch(r"arx-c13-[a-z0-9_]{8}", stage.name):
        raise ValueError("staging path")
    if not stage.is_dir() or stage.is_symlink() or stage.stat().st_uid != int(os.environ["SUDO_UID"]):
        raise ValueError("staging ownership")
    if not all(unused(name, id_) for name, id_ in ROLES):
        raise ValueError("role collision")
    deadline = min(deadline, time.monotonic() + TOTAL - OUTER_MARGIN)
    setup = deadline - ROOT_TEARDOWN - CAMPAIGN_TIMEOUT
    roles = []
    root = None
    may_remove = True
    successful = False
    first = None
    start = time.monotonic()
    phase = "staging"
    try:
        for name, id_ in ROLES:
            # Track before dispatch so a timeout with a partial effect can
            # still be reconciled against the fixed account identity.
            roles.append(("group", name, id_))
            command(["/usr/sbin/groupadd", "--gid", str(id_), name], setup)
            roles.append(("user", name, id_))
            command(["/usr/sbin/useradd", "--uid", str(id_), "--gid", name, "--no-create-home",
                     "--no-log-init", "--shell", "/usr/sbin/nologin", name], setup)
        root = Path(tempfile.mkdtemp(prefix="arx-c13-fixture-", dir="/tmp"))
        stage_tree(root)
        for name in ("arxd", "arxd-publication", "campaign"):
            stage_binary(stage / name, root / name, setup)
        environment = {**ENV, "ARX_C13_HOSTED": "1", "ARX_C13_BASE": str(root),
                       "ARX_C13_IP": primary_ip(setup)}
        phase = "campaign"
        test_start = time.monotonic()
        timeout = remaining(deadline - ROOT_TEARDOWN, CAMPAIGN_TIMEOUT)
        if timeout < CAMPAIGN_TIMEOUT - 1:
            raise TimeoutError("campaign cleanup reserve unavailable")
        may_remove = False
        code, capture = bounded_run([root / "campaign", "--exact", SELECTOR, "--ignored", "--nocapture"],
                                    env=environment, timeout=timeout)
        lines = capture.text().splitlines()
        first = capture.first
        may_remove = code is not None and not capture.truncated and cleanup_observed(lines)
        print(f"C13 seconds={time.monotonic()-test_start:.3f} result={'timeout' if code is None else code}",
              flush=True)
        for line in lines:
            if (line == PIN or CLEANUP.fullmatch(line) or re.fullmatch(
                    r"C13 publication node=4 term=\d+ leader=\d+ index=\d+", line)):
                print(line, flush=True)
        successful = code == 0 and lines.count(PIN) == 1 and may_remove and first is None
        if not successful and first is None:
            first = f"C13 first phase=campaign category={'timeout' if code is None else 'unconfirmed'}"
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        first = first or f"C13 first phase={phase} category={type(error).__name__}"
    finally:
        if first is not None:
            print(first, flush=True)
        removed, retained, unknown = 0, 0, 0
        if may_remove:
            for kind, name, id_ in reversed(roles):
                result = remove_role(kind, name, id_, deadline)
                removed += result == "removed"
                retained += result == "retained"
                unknown += result == "unknown"
        else:
            # Never remove a role while a process disposition is unconfirmed.
            retained = len(roles)
        fixture = "absent" if root is None else "retained"
        if root is not None and may_remove and retained == unknown == 0:
            try:
                command(["/usr/bin/python3", "-I", "-B", "-c",
                         "import shutil,sys; shutil.rmtree(sys.argv[1])", root], deadline)
                fixture = "removed"
            except (OSError, subprocess.SubprocessError):
                fixture = "unknown"
        print(f"C13 teardown fixture={fixture} roles_removed={removed} "
              f"roles_retained={retained} roles_unknown={unknown} "
              f"seconds={time.monotonic()-start:.3f}", flush=True)
    if not successful or retained or unknown or fixture not in ("removed", "absent"):
        raise ValueError("C13 execution or teardown unconfirmed")


def role_identity(kind, name):
    import grp
    import pwd
    try:
        if kind == "user":
            return pwd.getpwnam(name).pw_uid
        return grp.getgrnam(name).gr_gid
    except KeyError:
        return None


def remove_role(kind, name, id_, deadline):
    try:
        current = role_identity(kind, name)
        if current is None:
            return "removed"
        if current == id_:
            command([f"/usr/sbin/{kind}del", name], deadline, limit=1.25)
        return "removed" if role_identity(kind, name) is None else "retained"
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def main():
    start = time.monotonic()
    qualify_runner()
    if len(sys.argv) == 4 and sys.argv[1] == "--staged":
        if not re.fullmatch(r"[0-9]{1,20}", sys.argv[3]):
            raise ValueError("staging deadline")
        return privileged(sys.argv[2], int(sys.argv[3]) / 1_000_000)
    if len(sys.argv) != 1 or os.geteuid() == 0 or Path.cwd().resolve() != ROOT:
        raise ValueError("launcher entry")
    tests = unittest.defaultTestLoader.discover(str(Path(__file__).parent), pattern="test_run.py")
    if not unittest.TextTestRunner().run(tests).wasSuccessful():
        raise ValueError("launcher controls")
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    if target != ROOT / "target":
        raise ValueError("unexpected cargo target")
    profile = target / "debug"
    deadline = start + TOTAL
    root_deadline = deadline - OUTER_MARGIN
    setup = root_deadline - ROOT_TEARDOWN - CAMPAIGN_TIMEOUT
    with tempfile.TemporaryDirectory(prefix="arx-c13-", dir="/tmp") as directory:
        stage = Path(directory)
        artifacts = (("arxd", profile / "arxd"),
                     ("arxd-publication", select(profile / "deps", "arxd-", PROBE, setup)),
                     ("campaign", select(profile / "deps", "m3_activation_linux-", SELECTOR, setup)))
        for name, actual in artifacts:
            digest = stage_binary(actual, stage / name, setup)
            print(f"C13 artifact {name} sha256={digest}", flush=True)
        authorization = "GITHUB_ACTIONS,RUNNER_ENVIRONMENT,RUNNER_OS"
        subprocess.run(["/usr/bin/sudo", "-n", "--preserve-env=" + authorization,
                        "/usr/bin/python3", "-I", "-B", Path(__file__).resolve(),
                        "--staged", stage, str(int(root_deadline * 1_000_000))],
                       stdin=subprocess.DEVNULL, check=True, timeout=remaining(deadline - 2, TOTAL))
    print(f"C13 launcher seconds={time.monotonic()-start:.3f}", flush=True)


if __name__ == "__main__":
    try:
        main()
    except (KeyError, ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"C13 launcher refused ({type(error).__name__})", file=sys.stderr)
        sys.exit(1)
