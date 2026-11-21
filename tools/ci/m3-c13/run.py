"""Run the single pinned C13 campaign using disposable hosted runner roles."""
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import re
import shutil
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
CAMPAIGN_TIMEOUT = 126  # 102 s work + 17 s joined cleanup + process margin.
OUTPUT_LIMIT = 32_768
CLEANUP = re.compile(
    r"C13 cleanup spawned=([0-6]) signaled=([0-6]) reaped=([0-6]) clean=([0-6]) helper_unknown=([01])"
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


def select(deps, prefix, name):
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
            code, listing = bounded_run([executable, "--list", "--exact", name],
                                        env=ENV, timeout=3)
        if code == 0 and exact_listing(listing, name):
            matches.append(path)
    if len(matches) != 1:
        raise ValueError("selector inventory")
    return matches[0]


def command(arguments):
    subprocess.run(arguments, stdin=subprocess.DEVNULL, env=ENV,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   timeout=8, check=True)


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


def stage_binary(source, destination):
    with open_artifact(source) as input_file, destination.open("xb") as output:
        before = os.fstat(input_file.fileno())
        digest = hashlib.file_digest(input_file, "sha256").hexdigest()
        input_file.seek(0)
        remaining = before.st_size
        while remaining:
            block = input_file.read(min(remaining, 1024**2))
            if not block:
                raise ValueError("artifact truncated")
            output.write(block)
            remaining -= len(block)
        after = os.fstat(input_file.fileno())
        if (after.st_size, after.st_mtime_ns) != (before.st_size, before.st_mtime_ns):
            raise ValueError("artifact changed during stage")
    with destination.open("rb") as output:
        if hashlib.file_digest(output, "sha256").hexdigest() != digest:
            raise ValueError("artifact changed during stage")
    destination.chmod(0o755)
    return digest


def bounded_run(args, *, env, timeout):
    with tempfile.TemporaryFile() as output:
        result = subprocess.run(args, env=env, stdin=subprocess.DEVNULL,
                                stdout=output, stderr=subprocess.DEVNULL,
                                check=False, timeout=timeout)
        output.seek(0)
        data = output.read(OUTPUT_LIMIT + 1)
    if len(data) > OUTPUT_LIMIT:
        raise ValueError("bounded subprocess output")
    return result.returncode, data.decode("utf-8", errors="strict")


def cleanup_observed(lines):
    records = [m.groups() for line in lines if (m := CLEANUP.fullmatch(line))]
    if len(records) != 1:
        return False
    spawned, signaled, reaped, clean, helper_unknown = records[0]
    return spawned == signaled == reaped == clean and helper_unknown == "0"


def primary_ip():
    run = subprocess.run(["/usr/sbin/ip", "-j", "-4", "addr", "show", "scope", "global"],
                         env=ENV, capture_output=True, stdin=subprocess.DEVNULL,
                         check=True, timeout=4)
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


def privileged(stage):
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
    groups, users = [], []
    root = None
    may_remove = True
    cleanup = True
    start = time.monotonic()
    try:
        for name, id_ in ROLES:
            command(["/usr/sbin/groupadd", "--gid", str(id_), name])
            groups.append(name)
            command(["/usr/sbin/useradd", "--uid", str(id_), "--gid", name, "--no-create-home",
                     "--no-log-init", "--shell", "/usr/sbin/nologin", name])
            users.append(name)
        root = Path(tempfile.mkdtemp(prefix="arx-c13-fixture-", dir="/tmp"))
        stage_tree(root)
        for name in ("arxd", "arxd-publication", "campaign"):
            stage_binary(stage / name, root / name)
        environment = {**ENV, "ARX_C13_HOSTED": "1", "ARX_C13_BASE": str(root),
                       "ARX_C13_IP": primary_ip()}
        test_start = time.monotonic()
        # A subprocess timeout cannot run its owned cleanup. Hold everything
        # unless bounded and independently observed disposition is available.
        may_remove = False
        code, output = bounded_run([root / "campaign", "--exact", SELECTOR, "--ignored", "--nocapture"],
                                   env=environment, timeout=CAMPAIGN_TIMEOUT)
        lines = output.splitlines()
        may_remove = cleanup_observed(lines)
        print(f"C13 seconds={time.monotonic()-test_start:.3f} result={code}", flush=True)
        for line in lines:
            if (line == PIN or CLEANUP.fullmatch(line) or re.fullmatch(
                    r"C13 publication node=4 term=\d+ leader=\d+ index=\d+", line)):
                print(line, flush=True)
        if code != 0 or lines.count(PIN) != 1 or not may_remove:
            raise ValueError("C13 first execution refused")
    finally:
        if may_remove:
            if root is not None:
                shutil.rmtree(root, ignore_errors=False)
            for tool, names in (("userdel", users), ("groupdel", groups)):
                for name in reversed(names):
                    try:
                        command([f"/usr/sbin/{tool}", name])
                    except (OSError, subprocess.SubprocessError):
                        cleanup = False
        else:
            cleanup = False
        print(f"C13 worker stage seconds={time.monotonic()-start:.3f} cleanup={'joined' if cleanup else 'unknown'}",
              flush=True)
    if not cleanup:
        raise ValueError("C13 role cleanup unconfirmed")


def main():
    qualify_runner()
    if len(sys.argv) == 3 and sys.argv[1] == "--staged":
        return privileged(sys.argv[2])
    if len(sys.argv) != 1 or os.geteuid() == 0 or Path.cwd().resolve() != ROOT:
        raise ValueError("launcher entry")
    tests = unittest.defaultTestLoader.discover(str(Path(__file__).parent), pattern="test_run.py")
    if not unittest.TextTestRunner().run(tests).wasSuccessful():
        raise ValueError("launcher controls")
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    if target != ROOT / "target":
        raise ValueError("unexpected cargo target")
    profile = target / "debug"
    start = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="arx-c13-", dir="/tmp") as directory:
        stage = Path(directory)
        artifacts = (("arxd", profile / "arxd"),
                     ("arxd-publication", select(profile / "deps", "arxd-", PROBE)),
                     ("campaign", select(profile / "deps", "m3_activation_linux-", SELECTOR)))
        for name, actual in artifacts:
            digest = stage_binary(actual, stage / name)
            print(f"C13 artifact {name} sha256={digest}", flush=True)
        authorization = "GITHUB_ACTIONS,RUNNER_ENVIRONMENT,RUNNER_OS"
        subprocess.run(["/usr/bin/sudo", "-n", "--preserve-env=" + authorization,
                        "/usr/bin/python3", "-I", "-B", Path(__file__).resolve(),
                        "--staged", stage], stdin=subprocess.DEVNULL, check=True, timeout=170)
    print(f"C13 launcher seconds={time.monotonic()-start:.3f}", flush=True)


if __name__ == "__main__":
    try:
        main()
    except (KeyError, ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"C13 launcher refused ({type(error).__name__})", file=sys.stderr)
        sys.exit(1)
