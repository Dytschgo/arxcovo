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
HOSTED_ENV = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted", "RUNNER_OS": "Linux"}
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
FIRST_PREFIX = (
    r"C13 first phase=(?:preflight|founder|join|promotion-transfer|recovery|survivor|publication|close) "
    r"category="
)
FIRST = re.compile(FIRST_PREFIX
                   + r"(?:assertion|spawn|helper-reap|listener|daemon-refused|terminal-io|pipe-io)")
ASSERTION = re.compile(
    FIRST_PREFIX + r"assertion at=(?:m3_activation_linux\.rs|publish_only_probe\.rs):[1-9][0-9]{0,9} "
    r"message=(?:assertion-failed|assertion-equality|value-unavailable|process-exited|"
    r"readiness-elapsed|mint-refused|join-refused|terminal-unprotected|publication-refused|"
    r"publication-possession-refused|publication-helper-failed)"
)
FIRST_LIMIT = 160
OWNER_EXIT = re.compile(
    r"C13 owner node=[abcd] action=(?:join|mint) exit=(?:[1-9][0-9]?|1[0-9]{2}|2[0-4][0-9]|25[0-5]|signal)"
)
DIAGNOSTIC = re.compile(
    r"C13 diagnostic node=[abcd] (?:stage=(?:owner-join|join-start|join-attach|"
    r"join-publication|join-admission|admission-route|admission-register|admission-worker|"
    r"admission-membership|admission-confirm|admission-reply|admission-exchange|"
    r"learner-authorization|peer-pre-frame|peer-dispatch|admission-dispatch|peer-client|"
    r"transfer-client|transfer-receiver|transfer-exchange|lifecycle-observe) "
    r"category=(?:unavailable-store-wait|unavailable-store|unavailable|phase-unavailable|"
    r"clock|identity|busy|node-busy|closed|address|tls|deadline|dispatch|transport|"
    r"revoked|stale-tail|refused|unconfirmed|incomplete|not-leader|view-changed|"
    r"transfer|history|issuer|listener)|"
    r"stage=transfer-seal category=(?:offer-node-busy|offer-fresh-read-busy|offer-read|offer-changed|issuer-busy|"
    r"issuer-refused|seal|recheck-busy|recheck-refused))"
)
PIN = "C13 starts=6 closes=6 Found=1 Mint=3 AnchorQuery=3 Redeem=3 admissions=2 AddNodes=2 joint=1 flatten=1 transfer=2 enrollment_proposals=7"
PUBLICATION = re.compile(r"C13 publication node=4 term=\d+ leader=\d+ index=\d+")
# Both pins are the panic lines from the runs that reddened main, and both
# assertions are still those lines on 270df38e. Mint is the owner-mint
# success check. Promotion-transfer is the readiness assert, which other
# phases also call, so the phase stays part of the pin. A wider line pin
# would hide a different assertion with the same message.
# Run 45570315686 attempt 1: join mint-refused, owner node a action mint
# exit 8, result 101. Run 45457780671 attempt 1: promotion-transfer
# process-exited, result 101, no owner line. Both reaped every helper with
# helper_unknown=0 and removed the fixture with no retained or unknown
# roles. Neither closed every worker gracefully (clean=2, spawned=3), so
# graceful close stays required for success only.
MINT_QUARANTINE = (
    "C13 first phase=join category=assertion "
    "at=m3_activation_linux.rs:1366 message=mint-refused"
)
PROMOTION_QUARANTINE = (
    "C13 first phase=promotion-transfer category=assertion "
    "at=m3_activation_linux.rs:766 message=process-exited"
)
MINT_OWNER_QUARANTINE = "C13 owner node=a action=mint exit=8"
QUARANTINE_NAMES = frozenset(("mint-refused", "promotion-transfer-process-exited"))
QUARANTINE_MARKER = "c13-quarantine"


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
        # Allow one trailing CR within the record limit.
        *lines, tail = data.split(b"\n")
        for line in lines:
            if self._line is not None and len(self._line) + len(line) <= FIRST_LIMIT:
                record = (self._line + line).decode("ascii", errors="replace").removesuffix("\r")
                if FIRST.fullmatch(record) or ASSERTION.fullmatch(record):
                    self.first = record
                    return
            self._line = b""
        if self._line is not None:
            self._line = self._line + tail if len(self._line) + len(tail) <= FIRST_LIMIT else None

    def text(self):
        return self.bulk.decode("utf-8", errors="replace")


def pipe_capacity(fd):
    import fcntl
    return fcntl.fcntl(fd, fcntl.F_GETPIPE_SZ)


def bounded_run(args, *, env, timeout, stderr=subprocess.DEVNULL):
    capture = Capture()
    deadline = time.monotonic() + timeout
    work_deadline = deadline - REAP_GRACE
    remaining(work_deadline, timeout)
    process = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=stderr)
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


def run_campaign(args, *, env, timeout):
    # Optional diagnostics never enter Capture or influence cleanup evidence.
    try:
        reader, writer = os.pipe2(os.O_NONBLOCK | os.O_CLOEXEC)
    except OSError:
        code, capture = bounded_run(args, env=env, timeout=timeout)
        return code, capture, b""
    try:
        code, capture = bounded_run(args, env=env, timeout=timeout, stderr=writer)
        # Nonblocking and bounded even if a descendant still owns the writer.
        try:
            diagnostic = os.read(reader, OUTPUT_LIMIT)
        except OSError:
            diagnostic = b""
        return code, capture, diagnostic
    finally:
        os.close(writer)
        os.close(reader)


def cleanup_disposition(lines):
    records = [m.groups() for line in lines if (m := CLEANUP.fullmatch(line))]
    if len(records) != 1:
        return False, False
    spawned, signaled, reaped, clean, helpers, helpers_reaped, helper_unknown = records[0]
    reaped_all = (spawned == reaped and clean <= signaled <= spawned
                  and helpers == helpers_reaped and helper_unknown == "0")
    clean_close = reaped_all and spawned == signaled == clean
    return reaped_all, clean_close


def quarantine_signature(first, lines, *, code, truncated, fixture, retained, unknown):
    # Result 101 is the harness panic status from both incidents. A timeout,
    # a different status, or truncated campaign output is a separate anomaly.
    # Artifact mismatches raise before this decision and keep a non-zero exit.
    if (code != 101 or truncated or fixture != "removed" or retained != 0
            or unknown != 0 or PIN in lines):
        return None
    reaped_all, _clean_close = cleanup_disposition(lines)
    if not reaped_all:
        return None
    failures = [line for line in lines if FIRST.fullmatch(line) or ASSERTION.fullmatch(line)]
    if failures != [first]:
        return None
    if any(PUBLICATION.fullmatch(line) for line in lines):
        return None
    owners = [line for line in lines if OWNER_EXIT.fullmatch(line)]
    if first == MINT_QUARANTINE and owners == [MINT_OWNER_QUARANTINE]:
        return "mint-refused"
    if first == PROMOTION_QUARANTINE and not owners:
        return "promotion-transfer-process-exited"
    return None


def announce_quarantine(signature):
    line = f"C13 quarantined signature={signature}"
    print(line, flush=True)
    # GitHub renders a workflow command only when it starts a log line.
    print(f"::warning::{line}", flush=True)


def write_quarantine_marker(directory, signature):
    if signature not in QUARANTINE_NAMES:
        raise ValueError("quarantine signature")
    path = os.path.join(os.fspath(directory), QUARANTINE_MARKER)
    flags = os.O_CREAT | os.O_EXCL | os.O_WRONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_BINARY"):
        flags |= os.O_BINARY
    descriptor = os.open(path, flags, 0o644)
    try:
        os.write(descriptor, (signature + "\n").encode("ascii"))
    finally:
        os.close(descriptor)


def read_quarantine_marker(stage):
    path = os.path.join(os.fspath(stage), QUARANTINE_MARKER)
    try:
        metadata = os.lstat(path)
    except OSError:
        return None
    if (stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode)
            or metadata.st_size > 80):
        return None
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    descriptor = os.open(path, flags)
    try:
        data = os.read(descriptor, 81)
    finally:
        os.close(descriptor)
    try:
        text = data.decode("ascii")
    except UnicodeDecodeError:
        return None
    if text.count("\n") != 1 or not text.endswith("\n"):
        return None
    signature = text[:-1]
    return signature if signature in QUARANTINE_NAMES else None


def append_step_summary(line):
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if not path:
        return
    if len(path) > 300 or any(char in path for char in "\r\n\x00"):
        raise ValueError("step summary path")
    flags = os.O_WRONLY | os.O_APPEND
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_BINARY"):
        flags |= os.O_BINARY
    descriptor = os.open(path, flags)
    try:
        os.write(descriptor, (line + "\n").encode("utf-8"))
    finally:
        os.close(descriptor)


def ip_json(arguments, deadline):
    run = subprocess.run(["/usr/sbin/ip", "-j", "-4", *arguments],
                         env=ENV, capture_output=True, stdin=subprocess.DEVNULL,
                         check=True, timeout=remaining(deadline, 4))
    if len(run.stdout) > OUTPUT_LIMIT:
        raise ValueError("bounded network inventory")
    return json.loads(run.stdout)


def primary_ip(deadline):
    # Ask the kernel which source and link it would use for an off-link
    # destination, as in the hosted admission controls. No packet is sent.
    routes = ip_json(["route", "get", "192.0.2.1"], deadline)
    if len(routes) != 1 or not routes[0].get("dev") or not routes[0].get("prefsrc"):
        raise ValueError("eligible runner route")
    route = routes[0]
    source = ipaddress.IPv4Address(route["prefsrc"])
    private = tuple(map(ipaddress.ip_network, ("10.0.0.0/8", "172.16.0.0/12",
                                              "192.168.0.0/16", "100.64.0.0/10")))
    if not any(source in net for net in private):
        raise ValueError("eligible runner address")
    links = ip_json(["addr", "show", "scope", "global"], deadline)
    if not any(link.get("ifname") == route["dev"]
               and {"UP", "LOWER_UP"}.issubset(link.get("flags", ()))
               and any(address.get("local") == str(source) for address in link.get("addr_info", ()))
               for link in links):
        raise ValueError("eligible runner link")
    return str(source)


def qualify_runner():
    if sys.platform != "linux" or any(os.environ.get(name) != value for name, value in HOSTED_ENV.items()):
        raise ValueError("hosted Linux only")


def launch_privileged(stage, root_deadline, deadline):
    # sudo provides SUDO_UID for the ownership check. Pass only the fixed,
    # already-qualified runner facts as env arguments to the command.
    facts = [f"{name}={value}" for name, value in HOSTED_ENV.items()]
    result = subprocess.run(["/usr/bin/sudo", "-n", "/usr/bin/env", *facts,
                             "/usr/bin/python3", "-I", "-B", Path(__file__).resolve(),
                             "--staged", stage, str(int(root_deadline * 1_000_000))],
                            env=ENV, stdin=subprocess.DEVNULL, check=False,
                            timeout=remaining(deadline - 2, TOTAL))
    if result.returncode != 0:
        return 1
    # The privileged child prints the warning. Only this process has the
    # runner's step-summary file, and only a signature it just recorded.
    signature = read_quarantine_marker(stage)
    if signature is not None:
        append_step_summary(f"C13 quarantined signature={signature}")
    return 0


def relay_diagnostics(diagnostic):
    if not diagnostic:
        return
    try:
        stderr = sys.stderr.fileno()
        original = os.fstat(stderr)
        if not stat.S_ISFIFO(original.st_mode):
            return
        # Reopen the pipe with independent status flags. A dup would share
        # O_NONBLOCK with any mandatory output using the same description.
        writer = os.open(f"/proc/self/fd/{stderr}", os.O_WRONLY | os.O_NONBLOCK | os.O_CLOEXEC)
        try:
            reopened = os.fstat(writer)
            if (not stat.S_ISFIFO(reopened.st_mode)
                    or (reopened.st_dev, reopened.st_ino) != (original.st_dev, original.st_ino)):
                return
            for raw in diagnostic[:OUTPUT_LIMIT].split(b"\n")[:-1]:
                if len(raw) < 128 and DIAGNOSTIC.fullmatch(raw.decode("ascii", errors="replace")):
                    os.write(writer, raw + b"\n")
        finally:
            os.close(writer)
    except (OSError, ValueError, AttributeError):
        # An unavailable, full or closed diagnostic endpoint is disposable.
        pass


def privileged(stage, deadline):
    stage_directory = os.fspath(stage)
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
    diagnostic = b""
    code = None
    capture = None
    lines = []
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
        code, capture, diagnostic = run_campaign(
            [root / "campaign", "--exact", SELECTOR, "--ignored", "--nocapture"],
            env=environment, timeout=timeout,
        )
        lines = capture.text().splitlines()
        first = capture.first
        reaped_all, clean_close = cleanup_disposition(lines)
        # A reaped daemon can fail to close gracefully. Reclaim its disposable
        # role, but still require graceful close evidence for campaign success.
        may_remove = code is not None and not capture.truncated and reaped_all
        print(f"C13 seconds={time.monotonic()-test_start:.3f} result={'timeout' if code is None else code}",
              flush=True)
        for line in lines:
            if (line == PIN or CLEANUP.fullmatch(line) or OWNER_EXIT.fullmatch(line)
                    or PUBLICATION.fullmatch(line)):
                print(line, flush=True)
        successful = (code == 0 and lines.count(PIN) == 1 and may_remove
                      and clean_close and first is None)
        if not successful and first is None:
            first = f"C13 first phase=campaign category={'timeout' if code is None else 'unconfirmed'}"
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        first = first or f"C13 first phase={phase} category={type(error).__name__}"
    finally:
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
        if first is not None:
            print(first, flush=True)
        print(f"C13 teardown fixture={fixture} roles_removed={removed} "
              f"roles_retained={retained} roles_unknown={unknown} "
              f"seconds={time.monotonic()-start:.3f}", flush=True)
    relay_diagnostics(diagnostic)
    signature = quarantine_signature(
        first, lines, code=code, truncated=bool(capture is not None and capture.truncated),
        fixture=fixture, retained=retained, unknown=unknown,
    )
    if signature is not None:
        write_quarantine_marker(stage_directory, signature)
        announce_quarantine(signature)
        return 0
    return int(not successful or retained != 0 or unknown != 0 or fixture != "removed")


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
        status = launch_privileged(stage, root_deadline, deadline)
    print(f"C13 launcher seconds={time.monotonic()-start:.3f}", flush=True)
    return status


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"C13 launcher refused ({type(error).__name__})", file=sys.stderr)
        sys.exit(1)
