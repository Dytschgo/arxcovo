#!/usr/bin/python3 -I
"""A10 disposable hosted-VM preflight and a single Standard execution."""
from contextlib import contextmanager
import errno
import itertools
import json
import os
from pathlib import Path
import re
import runpy
import shutil
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import time
import unittest

if sys.platform == "linux":
    import fcntl
    import grp
    import pwd

ROOT = Path(__file__).resolve().parents[3]
PARENT = Path("/var/lib")
PROVENANCE_FIELDS = ("run", "attempt", "head", "image", "image_version", "job")
DAEMON_OBJECTS = (
    ("daemon/wrap", stat.S_ISREG),
    ("daemon/custody/object", stat.S_ISREG),
    ("daemon/history", stat.S_ISDIR),
    ("daemon/control.sock", stat.S_ISSOCK),
)
CONTROL_LIMITS = ("memory.max", "memory.swap.max", "pids.max")
CGROUP = Path("/sys/fs/cgroup")
UNIT_NAME = "arxd.service"
UNIT = CGROUP / "system.slice" / UNIT_NAME
RUNS = UNIT / "runs"
ROLES = (("arx-a10-daemon", 21071), ("arx-a10-build", 21072))
DAEMON, BUILD = (uid for _, uid in ROLES)
FIXTURE_SHELL = "/bin/sh"
FIXTURE_HOME = "/nonexistent"
MODE_ENV = "ARX_EXEC_QUALIFICATION_MODE"
MODES = {"preflight-only", "build"}
BROKER_REFUSALS = (
    "arguments", "identity", "binary", "settings-read", "settings-decode", "layout",
    "policy-account", "owner-policy", "runner-construction", "startup-recheck",
    "build-account", "runtime", "signals", "membership", "hierarchy-writable-view",
    "pre-existing-runs", "hierarchy-setup", "hierarchy-pin", "bind", "drain",
)
BROKER_RECORD = r"^ARX_EXEC_(SETUP ready|DRAIN confirmed|REFUSED (" + "|".join(BROKER_REFUSALS) + r"))$"
ENV = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LANG": "C", "TZ": "UTC"}
SELECTOR = "service::cluster_build_executor::tests::hosted_standard_build"
BUILD_PASS = "ARX_EXEC_BUILD dispatches=1 migration=1 limits=1 identity=1 denial=1 workspace=1 terminal=1 cleanup=1"
MAX_TEXT = 4096
PLACEMENT = "0::/system.slice/arxd.service/supervisor"
MEMORY = 128 * 1024**2
PIDS = 32
CAPS = "00000000000000c1"
TOTAL = 90
CLEANUP = 18
WORKSPACE_PASS = "A10 child migration=1 limits=1 identity=1 denial=1 workspace=1"
LEAF = re.compile(r"arx-[1-9][0-9]{0,9}-(?:0|[1-9][0-9]{0,19})")
ID = re.compile(r"[1-9][0-9]{0,15}")
SAFE_VALUE = re.compile(r"[A-Za-z0-9._+-]{1,96}")


class Refusal(RuntimeError):
    pass


def require(value, code):
    if not value:
        raise Refusal(code)


def emit(event, **facts):
    print("A10 " + json.dumps({"event": event, **facts}, sort_keys=True), flush=True)


def remaining(deadline, limit):
    budget = min(deadline - time.monotonic(), limit)
    require(budget > 0, "deadline")
    return budget


def command(argv, deadline, code, *, limit=5, env=ENV):
    completed = subprocess.run(
        argv, env=env, stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=remaining(deadline, limit), check=False,
    )
    allowed = {0, 1} if code == "journal" else {0}
    require(completed.returncode in allowed and len(completed.stdout) <= 16 * 1024, code)
    return completed.stdout.decode("ascii", "strict")


def read(path, n=MAX_TEXT, expected_uid=None):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    with os.fdopen(fd, "rb") as f:
        m = os.fstat(f.fileno())
        require(stat.S_ISREG(m.st_mode), "read-type")
        if expected_uid is not None:
            require(m.st_uid == expected_uid and m.st_nlink == 1, "report-owner")
        data = f.read(n + 1)
    require(len(data) <= n, "read-bound")
    return data.decode("ascii", "strict")


def proc_status(pid):
    records = {}
    for line in read(Path("/proc") / str(pid) / "status", 8192).splitlines():
        key, _, value = line.partition(":")
        if key in {"Uid", "Gid", "Groups", "NoNewPrivs", "CapBnd", "CapEff",
                   "CapPrm", "CapInh", "CapAmb"}:
            require(key not in records, "duplicate-status")
            records[key] = value.strip().split()
    return records


def ancestry(path):
    results = []
    for part in (*reversed(path.parents), path):
        m = part.lstat()
        require(stat.S_ISDIR(m.st_mode) and m.st_uid == 0 and m.st_mode & 0o022 == 0,
                "protected-ancestry")
        results.append([str(part), m.st_uid, format(stat.S_IMODE(m.st_mode), "04o")])
    return results


def provenance(values):
    require(len(values) == len(PROVENANCE_FIELDS), "provenance")
    run, attempt, head, image, version, job = values
    require(ID.fullmatch(run) and ID.fullmatch(attempt)
            and re.fullmatch(r"[a-f0-9]{40}", head)
            and SAFE_VALUE.fullmatch(image) and SAFE_VALUE.fullmatch(version)
            and job == "rust", "provenance")
    return dict(zip(PROVENANCE_FIELDS, values))


def hosted(values):
    require(sys.platform == "linux" and all(
        os.environ.get(key) == expected for key, expected in {
            "CI": "true",
            "GITHUB_ACTIONS": "true",
            "RUNNER_OS": "Linux",
            "RUNNER_ENVIRONMENT": "github-hosted",
            "GITHUB_REPOSITORY": "Dytschgo/arxcovo",
        }.items()
    ), "hosted-runner")
    return provenance(values)


def role_unused(name, uid):
    for lookup, key in ((pwd.getpwnam, name), (pwd.getpwuid, uid),
                        (grp.getgrnam, name), (grp.getgrgid, uid)):
        try:
            lookup(key)
        except KeyError:
            continue
        return False
    return True


def useradd_argv(name, uid):
    # Direct setpriv execution needs a resolver-compatible shell. Disable
    # authentication with a password lock and an expired account.
    return ["/usr/sbin/useradd", "--uid", str(uid), "--gid", name, "--no-create-home",
            "--no-log-init", "--home-dir", FIXTURE_HOME, "--shell", FIXTURE_SHELL,
            "--password", "!", "--expiredate", "1970-01-02", name]


def role(name, uid, *, report=False):
    user, group = pwd.getpwnam(name), grp.getgrnam(name)
    shadow = [line.split(":") for line in read(Path("/etc/shadow"), 1024 * 1024).splitlines()
              if line.partition(":")[0] == name]
    local_shadow = len(shadow) == 1 and len(shadow[0]) == 9
    facts = {
        "identity_matches": (user.pw_uid, user.pw_gid) == (uid, uid),
        "shell_matches": user.pw_shell == FIXTURE_SHELL,
        "password_locked": local_shadow and shadow[0][1] == "!",
        "login_disabled": local_shadow and shadow[0][7] == "1",
        "home_absent": user.pw_dir == FIXTURE_HOME and not os.path.lexists(FIXTURE_HOME),
        "groups_match": group.gr_gid == uid and os.getgrouplist(name, uid) == [uid],
    }
    if report:
        emit("fixture-account", role="daemon" if uid == DAEMON else "build",
             uid=user.pw_uid, gid=user.pw_gid, **facts)
    require(all(facts.values()), "role-changed")


def start_argv(root):
    return [
        "/usr/bin/systemd-run", f"--unit={UNIT_NAME}", f"--description={root.name}",
        "--service-type=exec",
        "--property=User=root", "--property=Group=root",
        f"--property=SyslogIdentifier={root.name}",
        "--property=Delegate=memory pids", "--property=DelegateSubgroup=supervisor",
        "--property=NoNewPrivileges=yes", "--property=ProtectHome=yes",
        "--property=PrivateTmp=yes",
        "--property=CapabilityBoundingSet=CAP_CHOWN CAP_SETGID CAP_SETUID",
        "--property=ReadOnlyPaths=/sys/fs/cgroup",
        f"--property=ReadWritePaths={UNIT.as_posix()}",
        "--property=TimeoutStartSec=5s", "--property=TimeoutStopSec=5s",
        "--property=RuntimeMaxSec=40s",
        "--property=LimitCORE=0", "--property=UMask=0077",
        root / "arxd", "cluster-build-broker", root / "broker/settings.json",
    ]


def account_argv(uid):
    return [
        "/usr/bin/setpriv", "--reuid", str(uid), "--regid", str(uid), "--clear-groups",
        "--no-new-privs", "--bounding-set=-all", "--",
    ]


def live_pid(root, deadline):
    while time.monotonic() < deadline:
        data = properties(deadline)
        require(data["Description"] == root.name, "unit-identity")
        if data["ActiveState"] == "active" and ID.fullmatch(data["MainPID"]):
            pid = int(data["MainPID"])
            endpoint = root / "control/executor.sock"
            try:
                mode = endpoint.lstat()
            except FileNotFoundError:
                pass
            else:
                require(stat.S_ISSOCK(mode.st_mode) and (mode.st_uid, mode.st_gid) == (0, DAEMON),
                        "endpoint")
                if stat.S_IMODE(mode.st_mode) == 0o660:
                    return pid
        require(data["ActiveState"] in {"activating", "active"}, "broker-start")
        time.sleep(0.05)
    raise Refusal("broker-ready-timeout")


def properties(deadline):
    keys = ("Description", "ActiveState", "LoadState", "MainPID")
    data = command(
        ["/usr/bin/systemctl", "show", *[f"--property={key}" for key in keys], UNIT_NAME],
        deadline, "unit-properties",
    )
    result = dict(line.split("=", 1) for line in data.splitlines())
    require(set(result) == set(keys), "unit-properties")
    return result


def journal(root, deadline):
    # The program emits fixed category records only. Do not relay systemd
    # metadata, paths, subprocess output or arbitrary parser diagnostics.
    result = command([
        "/usr/bin/journalctl", "--quiet", "--no-pager", "--output=cat",
        "--lines=24", f"_SYSTEMD_UNIT={UNIT_NAME}", f"SYSLOG_IDENTIFIER={root.name}",
        f"--grep={BROKER_RECORD}",
    ], deadline, "journal")
    return [line for line in result.splitlines()[-24:] if re.fullmatch(BROKER_RECORD, line)]


def control_mode(path, owner_permission):
    m = path.lstat()
    require(stat.S_ISREG(m.st_mode) and m.st_uid == 0
            and not m.st_mode & 0o022 and m.st_mode & owner_permission,
            "cgroup-control-protection")
    return {"uid": m.st_uid, "mode": format(stat.S_IMODE(m.st_mode), "04o")}


def cgroup_facts():
    evidence = {}
    for name, group in (("unit", UNIT), ("runs", RUNS)):
        domain = read(group / "cgroup.type").strip()
        processes = read(group / "cgroup.procs").split()
        controllers = read(group / "cgroup.controllers").split()
        enabled = read(group / "cgroup.subtree_control").split()
        require(domain == "domain" and not processes, "cgroup-domain-state")
        require(all({"memory", "pids"}.issubset(tokens) for tokens in (controllers, enabled)),
                "controllers")
        files = {}
        for filename in CONTROL_LIMITS:
            path = group / filename
            facts = control_mode(path, stat.S_IRUSR)
            value = read(path, 32).strip()
            require(re.fullmatch(r"(?:max|[0-9]{1,20})", value), "cgroup-control-value")
            files[filename] = {**facts, "value": value}
        evidence[name] = {
            "type": domain, "procs": processes, "available": controllers,
            "readback": enabled, "controls": files,
        }
    evidence["runs"]["controls"]["cgroup.kill"] = control_mode(RUNS / "cgroup.kill", stat.S_IWUSR)
    require(read(RUNS / "cgroup.events").splitlines().count("populated 0") == 1, "runs-populated")
    evidence["runs"]["populated"] = 0
    return evidence


def wait_ready(root, deadline):
    pid = live_pid(root, min(deadline, time.monotonic() + 8))
    state = proc_status(pid)
    membership = read(Path("/proc") / str(pid) / "cgroup").strip()
    require(state["Uid"] == ["0"] * 4 and state["Gid"] == ["0"] * 4, "broker-identity")
    require(state["NoNewPrivs"] == ["1"] and state["CapBnd"] == [CAPS], "broker-hardening")
    require(membership == PLACEMENT, "broker-membership")
    require(read(UNIT / "supervisor/cgroup.procs").split() == [str(pid)], "broker-placement")
    groups = cgroup_facts()
    marker = os.getxattr(UNIT, "user.delegate", follow_symlinks=False)
    require(marker == b"1", "delegation-marker")
    mounts = Path("/proc") / str(pid) / "root/sys/fs/cgroup"
    require(os.statvfs(mounts).f_flag & os.ST_RDONLY
            and not os.statvfs(mounts / UNIT.relative_to(CGROUP)).f_flag & os.ST_RDONLY,
            "broker-cgroup-writability")
    emit("setup", pid=pid, uid=state["Uid"], no_new_privs=1, cap_bnd=CAPS,
         membership=membership, ancestor=ancestry(UNIT / "supervisor"),
         runs=ancestry(RUNS)[-1], groups=groups,
         delegate_raw=marker.decode("ascii"), outside_readonly=True, setup="ready")
    return pid


def denied(action):
    try:
        action()
    except OSError as error:
        require(error.errno in {errno.EACCES, errno.EPERM, errno.EROFS}, "unexpected-denial")
    else:
        raise Refusal("access-permitted")


def open_probe(path, mode):
    fd = os.open(path, mode | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC)
    os.close(fd)


def cgroup_denials(leaf=None):
    # Check every visible control on the job VM. A denied directory cannot
    # be searched to reach its controls. Never issue a write to test access.
    pending, count, controls, private = [CGROUP], 0, 0, 0
    while pending:
        directory = pending.pop()
        if directory == leaf:
            continue
        require(not os.access(directory, os.W_OK), "cgroup-directory-write")
        try:
            with os.scandir(directory) as scan:
                for entry in itertools.islice(scan, 32769 - count):
                    count += 1
                    require(count <= 32768, "cgroup-inventory-bound")
                    if entry.is_dir(follow_symlinks=False):
                        pending.append(Path(entry.path))
                    else:
                        require(entry.is_file(follow_symlinks=False), "cgroup-entry-type")
                        denied(lambda: open_probe(entry.path, os.O_WRONLY))
                        controls += 1
        except PermissionError:
            private += 1
    require(controls > 0, "cgroup-denials-empty")
    return controls, private


def socket_denied(path):
    with socket.socket(socket.AF_UNIX) as conn:
        conn.settimeout(1)
        denied(lambda: conn.connect(str(path)))


def identity(root):
    require(os.getuid() == BUILD and os.getgid() == BUILD and os.getgroups() == [], "identity")
    facts = proc_status("self")
    require(facts["NoNewPrivs"] == ["1"] and facts["CapEff"] == ["0" * 16], "identity-hardening")
    require({"memory", "pids"}.issubset(read(CGROUP / "cgroup.controllers").split()), "visibility")
    denied(lambda: open_probe(root / "broker/settings.json", os.O_RDONLY))
    denied(lambda: open_probe(root / "daemon/wrap", os.O_RDONLY))
    socket_denied(root / "control/executor.sock")
    controls, private = cgroup_denials()
    emit("identity", probes=1, uid=BUILD, gid=BUILD, groups=[], visible=True,
         denied_controls=controls, inaccessible_directories=private)


def peer_status(root, expected):
    require(os.getuid() == DAEMON and os.getgid() == DAEMON and os.getgroups() == [],
            "daemon-identity")
    with socket.socket(socket.AF_UNIX) as peer:
        peer.settimeout(2)
        peer.connect(str(root / "control/executor.sock"))
        pid, uid, gid = struct.unpack("3i", peer.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        require(pid > 0 and uid == gid == 0, "daemon-peer")
        request = b'"Status"'
        peer.sendall(struct.pack(">I", len(request)) + request)
        with peer.makefile("rb") as response:
            header = response.read(4)
            require(len(header) == 4, "daemon-header")
            size = struct.unpack(">I", header)[0]
            require(0 < size <= 1024, "daemon-frame")
            packet = response.read(size)
            require(len(packet) == size, "daemon-frame")
            reply = json.loads(packet)
    if expected == "ready":
        require(reply == {"Ready": {"uid": BUILD, "gid": BUILD, "slots": 1}}, "daemon-readiness")
    else:
        require(reply == {"Unavailable": expected}, "daemon-negative-control")


@contextmanager
def restore_after(restore):
    try:
        yield
    except (Refusal, OSError, subprocess.SubprocessError, UnicodeError, ValueError, KeyError):
        try:
            restore()
        except (Refusal, OSError, subprocess.SubprocessError, ValueError, KeyError) as cleanup:
            emit("control-cleanup", category=str(cleanup) if isinstance(cleanup, Refusal) else type(cleanup).__name__)
        raise
    else:
        restore()


def peer_controls(root, deadline, probe_peer):
    probe_peer("ready")
    settings = root / "broker/settings.json"
    settings.chmod(0o644)
    with restore_after(lambda: settings.chmod(0o600)):
        probe_peer("preflight_failed")

    name = ROLES[1][0]
    usermod = ["/usr/sbin/usermod", "--gid"]
    restore = lambda: command([*usermod, str(BUILD), name], deadline, "restore-fixture-group")
    with restore_after(restore):
        command([*usermod, str(DAEMON), name], deadline, "change-fixture-group")
        probe_peer("account_identity_changed")
    role(name, BUILD)
    probe_peer("ready")


def daemon_evidence(root, pid):
    for name, correct_kind in DAEMON_OBJECTS:
        m = (root / name).lstat()
        require(correct_kind(m.st_mode) and (m.st_uid, m.st_gid) == (DAEMON, DAEMON)
                and not m.st_mode & 0o077, "daemon-fixture-" + Path(name).name)
    fd = read(root / "daemon/wrap.fd", 16, DAEMON)
    require(ID.fullmatch(fd) and 3 <= int(fd) <= 2**31 - 1, "wrap-fd-number")
    wrapping = (root / "daemon/wrap").lstat()
    held = (Path("/proc") / str(pid) / "fd" / fd).stat()
    require(stat.S_ISREG(held.st_mode) and (held.st_dev, held.st_ino) == (wrapping.st_dev, wrapping.st_ino),
            "wrap-fd-target")
    return fd


def verify_child_fds():
    with os.scandir("/proc/self/fd") as directory:
        names = [item.name for item in itertools.islice(directory, 4097)]
    require(len(names) <= 4096 and all(re.fullmatch(r"[0-9]{1,10}", name) for name in names)
            and {"0", "1", "2"}.issubset(names), "fd-inventory")
    # The scan's own FD is now closed. A still-open numbered entry above
    # the standard streams is an inherited descriptor anywhere in the table.
    for fd in map(int, names):
        if fd <= 2:
            continue
        try:
            fcntl.fcntl(fd, fcntl.F_GETFD)
        except OSError as error:
            require(error.errno == errno.EBADF, "unexpected-fd-condition")
        else:
            raise Refusal("inherited-descriptor")


def child(root):
    require(sys.argv[1:] == ["--version"], "witness-arguments")
    require(os.getuid() == BUILD and os.getgid() == BUILD and os.getgroups() == [], "child-identity")
    membership = read(Path("/proc/self/cgroup")).strip()
    relative = membership.removeprefix("0::/system.slice/arxd.service/runs/")
    require(LEAF.fullmatch(relative), "leaf")
    leaf = RUNS / relative
    for name in ("broker/settings.json", "daemon/custody/object", "daemon/wrap", "daemon/history"):
        denied(lambda: open_probe(root / name, os.O_RDONLY))
    socket_denied(root / "control/executor.sock")
    socket_denied(root / "daemon/control.sock")
    daemon_pid = read(root / "daemon.pid", 16)
    wrap_fd = read(root / "wrap.fd", 16)
    require(ID.fullmatch(daemon_pid) and ID.fullmatch(wrap_fd) and int(wrap_fd) >= 3, "wrap-fd-oracle")
    # This is an actual procfs descriptor link. Do not use O_NOFOLLOW to
    # manufacture a denial that would also happen for an authorized opener.
    denied(lambda: (Path("/proc") / daemon_pid / "fd" / wrap_fd).open("rb").close())
    for name in ("daemon.pid", "broker.pid"):
        pid = read(root / name, 16).strip()
        require(ID.fullmatch(pid), "fixture-pid")
        denied(lambda: os.kill(int(pid), 0))
        denied(lambda: open_probe(Path("/proc") / pid / "fd/0", os.O_RDONLY))
    require(Path.cwd() == root / "workspace" and read(Path("visible"), 16) == "workspace\n", "workspace")
    cgroup_denials(leaf)
    verify_child_fds()
    descendant = subprocess.Popen(
        ["/usr/bin/sleep", "30"], env=ENV, stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    with open("observed", "x", encoding="ascii") as output:
        output.write(f"{os.getpid()} {relative} {descendant.pid}\n")
    deadline = time.monotonic() + 4
    while time.monotonic() < deadline:
        try:
            ack = read(root / "confirmed", 16)
        except FileNotFoundError:
            time.sleep(0.025)
        else:
            require(ack == str(os.getpid()), "observer")
            print(WORKSPACE_PASS, flush=True)
            os.execve(root / "cargo-real", ["cargo", "--version"], {"PATH": "/usr/bin:/bin", "LANG": "C"})
    raise Refusal("observer-timeout")


def publish_observer_ack(root, pid):
    pending = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="ascii", dir=root, prefix=".confirmed-", delete=False,
        ) as output:
            pending = Path(output.name)
            output.write(pid)
            output.flush()
            pending.chmod(0o644)
            os.fsync(output.fileno())
        os.replace(pending, root / "confirmed")
    finally:
        if pending is not None:
            pending.unlink(missing_ok=True)


def observe_child(root, process, deadline, broker_pid):
    until = min(deadline, time.monotonic() + 6)
    while time.monotonic() < until:
        require(process.poll() is None, "build-ended-before-observation")
        try:
            witness = read(root / "workspace/observed", 64, BUILD)
        except FileNotFoundError:
            time.sleep(0.025)
            continue
        if not witness:
            time.sleep(0.025)
            continue
        pid, leaf, descendant = witness.strip().split()
        require(ID.fullmatch(pid) and ID.fullmatch(descendant) and pid != descendant
                and LEAF.fullmatch(leaf) and leaf.split("-")[1] == str(broker_pid),
                "leaf-identity")
        membership = read(Path("/proc") / pid / "cgroup").strip()
        require(membership == f"0::/system.slice/{UNIT_NAME}/runs/{leaf}", "leaf-migration")
        directory = RUNS / leaf
        require({pid, descendant}.issubset(read(directory / "cgroup.procs").split()), "leaf-membership")
        require(read(Path("/proc") / descendant / "cgroup").strip() == membership, "descendant-migration")
        for name, expected in (("memory.max", MEMORY), ("memory.swap.max", 0), ("pids.max", PIDS)):
            require(read(directory / name).strip() == str(expected), "leaf-limits")
        facts = proc_status(pid)
        require(facts["Uid"] == [str(BUILD)] * 4 and facts["Gid"] == [str(BUILD)] * 4
                and facts["Groups"] == [] and facts["NoNewPrivs"] == ["1"], "leaf-credentials")
        require(all(facts[key] == ["0" * 16] for key in ("CapEff", "CapPrm", "CapInh", "CapAmb")),
                "leaf-capabilities")
        publish_observer_ack(root, pid)
        emit("leaf", membership=membership, uid=BUILD, gid=BUILD,
             memory=MEMORY, swap=0, pids=PIDS, no_new_privs=1, descendant=True)
        return descendant
    raise Refusal("leaf-timeout")


def stage_tree(root, stage, copy, deadline, mode):
    for filename in (("arxd", "libtest", "cargo-real") if mode == "build" else ("arxd",)):
        copy(stage / filename, root / filename, deadline)
        (root / filename).chmod(0o755)
    (root / "broker").mkdir(mode=0o700)
    (root / "control").mkdir(mode=0o710)
    os.chown(root / "control", 0, DAEMON)
    for directory, uid in (
        ("daemon", DAEMON), ("daemon/history", DAEMON),
        ("daemon/custody", DAEMON), ("workspace", BUILD),
    ):
        path = root / directory
        path.mkdir(mode=0o700)
        os.chown(path, uid, uid)
    for name in ("daemon/wrap", "daemon/custody/object"):
        with (root / name).open("xb") as witness:
            witness.write(b"disposable fixture\n")
        os.chown(root / name, DAEMON, DAEMON)
        (root / name).chmod(0o600)
    (root / "workspace/visible").write_text("workspace\n", encoding="ascii")
    (root / "tools").mkdir(mode=0o755)
    script = Path(__file__)
    # The installed child entry point carries the exact reviewed profile.
    with script.open(encoding="utf-8") as source:
        text = source.read(64 * 1024 + 1)
    require(len(text) <= 64 * 1024, "probe-source-bound")
    witness = root / "tools/cargo"
    witness.write_text(text, encoding="utf-8")
    witness.chmod(0o755)
    policy = {
        "daemon_uid": DAEMON, "daemon_gid": DAEMON, "daemon_roots": [str(root / "daemon")],
        "service": {
            "schema": "arxd-service/1", "client": {"identity": str(DAEMON)},
            "build_account": {"name": ROLES[1][0], "identity": str(BUILD), "gid": BUILD},
            "rights": "standard", "workspace": str(root / "workspace"),
            "environment": {"PATH": str(root / "tools"), "HOME": str(root / "workspace")},
            "limits": {"max_concurrent_runs": 1, "memory_bytes": MEMORY, "pids": PIDS},
            "agents": [{"principal": "qualification", "token_sha256": "0" * 64}],
            "granted_batch_right": False,
        },
    }
    settings = root / "broker/settings.json"
    settings.write_text(json.dumps(policy), encoding="ascii")
    settings.chmod(0o600)


def fixture_metadata(root):
    argv = start_argv(root)
    for role_name, path in (
        ("run-root", root), ("broker-dir", root / "broker"),
        ("settings", root / "broker/settings.json"), ("binary", root / "arxd"),
        ("control-dir", root / "control"), ("daemon-root", root / "daemon"),
    ):
        metadata = path.lstat()
        kind = {stat.S_IFDIR: "directory", stat.S_IFREG: "file"}.get(
            stat.S_IFMT(metadata.st_mode), "other"
        )
        facts = {}
        if role_name == "settings":
            facts = {"size_bytes": metadata.st_size, "startup_path_matches": argv[-1] == path}
        if role_name == "binary":
            facts = {"startup_path_matches": argv[-3] == path}
        emit("fixture-object", role=role_name, type=kind, uid=metadata.st_uid, gid=metadata.st_gid,
             mode=f"{stat.S_IMODE(metadata.st_mode):04o}", **facts)


def start(root, deadline):
    require(properties(deadline)["LoadState"] == "not-found" and not UNIT.exists(), "unit-collision")
    command(start_argv(root), deadline, "unit-start")


def stop(root, deadline, expected_drain, close_requested):
    data = properties(deadline)
    if data["LoadState"] != "not-found":
        require(data["Description"] == root.name, "cleanup-unit-identity")
        if not close_requested:
            close_requested.append(True)
            command(["/usr/bin/systemctl", "stop", UNIT_NAME], deadline, "unit-stop", limit=8)
        else:
            require(data["ActiveState"] in {"inactive", "failed"}, "unit-close-unconfirmed")
    messages = journal(root, deadline)
    require(not UNIT.exists() and not (root / "control/executor.sock").exists(), "unit-residue")
    require(messages.count("ARX_EXEC_DRAIN confirmed") == expected_drain, "broker-drain")
    return messages


def prove_build(root, pid, deadline):
    (root / "broker.pid").write_text(str(pid), encoding="ascii")
    args = account_argv(DAEMON) + [root / "libtest", SELECTOR, "--exact", "--ignored", "--nocapture"]
    with tempfile.TemporaryFile(dir=root) as output:
        with subprocess.Popen(
            args, env={**ENV, "ARX_EXEC_HOSTED": "1", "ARX_EXEC_BASE": str(root)},
            stdin=subprocess.DEVNULL, stdout=output, stderr=output,
        ) as process:
            def reap():
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=3)
            with restore_after(reap):
                end = min(deadline, time.monotonic() + 2)
                while not (root / "daemon/wrap.fd").is_file():
                    require(process.poll() is None and time.monotonic() < end, "daemon-evidence-timeout")
                    time.sleep(0.025)
                fd = daemon_evidence(root, process.pid)
                (root / "wrap.fd").write_text(fd, encoding="ascii")
                (root / "daemon.pid").write_text(str(process.pid), encoding="ascii")
                emit("fixtures", custody=True, history=True, daemon_control=True, wrapping_fd=int(fd))
                descendant = observe_child(root, process, deadline, pid)
                require(process.wait(timeout=remaining(deadline, 5)) == 0, "build-result")
        output.seek(0)
        data = output.read(16385)
        require(len(data) <= 16384 and data.splitlines().count(BUILD_PASS.encode("ascii")) == 1,
                "build-oracle")
    require(read(RUNS / "cgroup.events").splitlines().count("populated 0") == 1
            and not any(item.is_dir() for item in RUNS.iterdir()), "terminal-leaf-residue")
    remaining_stat = Path("/proc") / descendant / "stat"
    try:
        fields = read(remaining_stat, 4096).rsplit(")", 1)[1].split()
    except FileNotFoundError:
        pass
    else:
        require(fields[0] == "Z", "descendant-still-running")
    print(BUILD_PASS, flush=True)


def qualify(root, stage, copy, deadline, receipt, *, mode):
    """Track the first refusal independently of cleanup and dispatch count."""
    receipt = provenance(tuple(receipt.get(field, "") for field in PROVENANCE_FIELDS))
    require(mode in MODES, "qualification-mode")
    emit("provenance", mode=mode, **receipt)
    phase, owned, created, state, stop_expected = "stage", [], False, False, 0
    close_requested = []
    first, cleanup, identity_probes, daemon_probe_attempts, dispatch_attempts = None, "not-started", 0, 0, 0

    def probe_peer(expected):
        nonlocal daemon_probe_attempts
        daemon_probe_attempts += 1
        command(account_argv(DAEMON) + ["/usr/bin/python3", "-I", "-B", root / "tools/cargo",
                                      "--daemon-status", root, expected],
                deadline, f"daemon-status-{expected}")

    try:
        emit("ancestors", entries=ancestry(PARENT))
        require(all(role_unused(name, uid) for name, uid in ROLES), "role-collision")
        require(properties(deadline)["LoadState"] == "not-found" and not UNIT.exists(), "unit-collision")
        version = command(["/usr/bin/systemctl", "show", "--property=Version", "--value"],
                          deadline, "systemd-manager").strip()
        pid1 = read(Path("/proc/1/comm"), 32).strip()
        kernel = os.uname().release
        require(pid1 == "systemd" and SAFE_VALUE.fullmatch(kernel)
                and re.fullmatch(r"[0-9]{3,4}(?:[.][A-Za-z0-9._+-]{1,64})?", version)
                and int(version.split(".")[0]) >= 254, "image-contract")
        emit("platform", pid1=pid1, systemd=version, kernel=kernel)
        root.mkdir(mode=0o755)
        created = True
        for name, uid in ROLES:
            owned.append((name, uid))
            command(["/usr/sbin/groupadd", "--gid", str(uid), name], deadline, "group-create")
            command(useradd_argv(name, uid), deadline, "user-create")
            role(name, uid, report=True)
        stage_tree(root, stage, copy, deadline, mode)
        fixture_metadata(root)
        phase = "preflight-setup"
        state = True
        start(root, deadline)
        wait_ready(root, deadline)
        stop_expected = 1
        phase = "preflight-identity"
        identity_probes += 1
        output = command(account_argv(BUILD) + ["/usr/bin/python3", "-I", "-B", root / "tools/cargo", "--identity", root],
                         deadline, "identity-probe")
        require(len(output.splitlines()) == 1 and output.startswith("A10 "), "identity-evidence")
        record = json.loads(output.removeprefix("A10 "))
        require(record.get("probes") == 1 and record.get("visible") is True, "identity-evidence")
        emit("identity-observed", **{key: record[key] for key in (
            "probes", "uid", "gid", "groups", "visible", "denied_controls", "inaccessible_directories"
        )})
        phase = "preflight-peer"
        peer_controls(root, deadline, probe_peer)
        phase = "preflight-drain"
        stop(root, deadline, stop_expected, close_requested)
        state = False
        close_requested.clear()
        emit("preflight", setup="ready", drain="confirmed", identity_probes=identity_probes,
             daemon_identity_probes=daemon_probe_attempts, runner_dispatches=0, consensus_invocations=0)
        if mode == "build":
            phase = "build-setup"
            state = True
            start(root, deadline)
            pid = wait_ready(root, deadline)
            stop_expected = 2
            phase = "single-build"
            dispatch_attempts = 1
            prove_build(root, pid, deadline)
    except (Refusal, OSError, subprocess.SubprocessError, UnicodeError, ValueError, KeyError) as error:
        first = str(error) if isinstance(error, Refusal) else type(error).__name__
        emit("first", phase=phase, category=first)
    finally:
        try:
            end = time.monotonic() + CLEANUP
            if state:
                if first:
                    try:
                        emit("broker-records", records=journal(root, end))
                    except (Refusal, OSError, subprocess.SubprocessError, UnicodeError, ValueError):
                        emit("broker-records", records_unavailable=True)
                stop(root, end, stop_expected, close_requested)
            for name, uid in reversed(owned):
                if role_unused(name, uid):
                    continue
                role(name, uid)
                command(["/usr/sbin/userdel", name], end, "user-remove")
                if not role_unused(name, uid):
                    require(grp.getgrnam(name).gr_gid == uid, "group-cleanup-identity")
                    command(["/usr/sbin/groupdel", name], end, "group-remove")
                require(role_unused(name, uid), "role-residue")
            if created:
                ancestry(root)
                shutil.rmtree(root)
            cleanup = "confirmed"
        except (Refusal, OSError, subprocess.SubprocessError, ValueError, KeyError) as error:
            cleanup = str(error) if isinstance(error, Refusal) else type(error).__name__
        emit("result", first=first, cleanup=cleanup, identity_probes=identity_probes,
             daemon_identity_probes=daemon_probe_attempts, dispatch_attempts=dispatch_attempts,
             consensus_invocations=0, mode=mode, **receipt)
    return int(first is not None or cleanup != "confirmed")


def privileged(stage, values):
    receipt = hosted(values)
    root = PARENT / f"arx-a10-{receipt['run']}-{receipt['attempt']}"
    require(os.getuid() == os.geteuid() == 0, "staging-identity")
    owner = os.environ.get("SUDO_UID", "")
    m = stage.lstat()
    require(stage.parent == Path("/tmp") and re.fullmatch(r"arx-a10-[a-z0-9_]{8}", stage.name)
            and stat.S_ISDIR(m.st_mode) and stat.S_IMODE(m.st_mode) == 0o700
            and ID.fullmatch(owner) and m.st_uid == int(owner), "staging-ownership")
    precedent = runpy.run_path(str(ROOT / "tools/ci/m3-c13/run.py"))
    return qualify(root, stage, precedent["stage_binary"], time.monotonic() + TOTAL - CLEANUP,
                   receipt, mode=os.environ.get(MODE_ENV, ""))


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--identity":
        identity(Path(sys.argv[2]))
        return 0
    if len(sys.argv) == 4 and sys.argv[1] == "--daemon-status":
        peer_status(Path(sys.argv[2]), sys.argv[3])
        return 0
    if Path(sys.argv[0]).name == "cargo":
        child(Path(sys.argv[0]).parent.parent)
    if len(sys.argv) == 9 and sys.argv[1] == "--staged":
        return privileged(Path(sys.argv[2]), sys.argv[3:])
    require(len(sys.argv) == 1 and os.geteuid() != 0, "arguments")
    mode = os.environ.get(MODE_ENV, "")
    require(mode in MODES, "qualification-mode")
    values = [os.environ.get(name, "") for name in (
        "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_SHA", "ImageOS", "ImageVersion", "GITHUB_JOB"
    )]
    receipt = hosted(values)
    actual_head = command(
        ["/usr/bin/git", "-C", ROOT, "rev-parse", "HEAD"],
        time.monotonic() + 3, "checkout-head",
    ).strip()
    require(actual_head == receipt["head"], "checkout-head")
    suite = unittest.defaultTestLoader.discover(str(Path(__file__).parent), pattern="test_executor_probe.py")
    require(unittest.TextTestRunner().run(suite).wasSuccessful(), "probe-unittests")
    start_time = time.monotonic()
    precedent = runpy.run_path(str(ROOT / "tools/ci/m3-c13/run.py"))
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    require(target == ROOT / "target", "cargo-target")
    profile = target / "debug"
    binaries = [("arxd", profile / "arxd")]
    if mode == "build":
        libtest = precedent["select"](profile / "deps", "arxd-", SELECTOR, start_time + 12)
        cargo = shutil.which("rustup")
        require(cargo is not None, "rustup")
        path = command([cargo, "which", "cargo"], start_time + 12, "cargo-path", env=os.environ).strip()
        binaries.extend((("libtest", libtest), ("cargo-real", Path(path))))
    with tempfile.TemporaryDirectory(prefix="arx-a10-", dir="/tmp") as directory:
        stage = Path(directory)
        for name, source in binaries:
            precedent["stage_binary"](source, stage / name, start_time + 18)
        facts = [f"{name}={os.environ[name]}" for name in (
            "CI", "GITHUB_ACTIONS", "RUNNER_OS", "RUNNER_ENVIRONMENT", "GITHUB_REPOSITORY"
        )]
        return subprocess.run(
            ["/usr/bin/sudo", "-n", "/usr/bin/env", *facts, f"{MODE_ENV}={mode}", "/usr/bin/python3", "-I", "-B",
             Path(__file__).resolve(), "--staged", stage, *values],
            env=ENV, stdin=subprocess.DEVNULL, timeout=TOTAL + 3, check=False,
        ).returncode


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (Refusal, OSError, subprocess.SubprocessError, ValueError, KeyError) as error:
        emit("first", phase="launcher", category=str(error) if isinstance(error, Refusal) else type(error).__name__)
        sys.exit(1)
