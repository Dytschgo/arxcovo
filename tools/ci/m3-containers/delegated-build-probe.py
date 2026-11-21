#!/usr/bin/python3
"""One hosted C14 run using the A10 broker and C13 runner qualification."""
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import secrets
import select
import shutil
import signal
import socket
import sqlite3
import stat
import struct
import subprocess
import sys
import tempfile
import time

ENV = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LANG": "C", "TZ": "UTC"}
OWNER, DAEMON, AGENT, BUILD = 21070, 21071, 21073, 21072
ROLES = (("arx-c14-owner", OWNER), ("arx-a10-daemon", DAEMON),
         ("arx-c14-agent", AGENT), ("arx-a10-build", BUILD))
SLOTS = ("ordinary", "lost")
REFUSED_SLOTS = ("link", "writable")
COMMAND = ["build", "--offline", "--locked", "--target-dir", "out/target"]
SELECTOR = "c14_target_authorized_build_and_lost_reply"
PASS = "C14 build admitted=2 executed=2 a_executed=0 ordinary=1 lost_reply_status=1"
PROVENANCE = ("CI", "GITHUB_ACTIONS", "RUNNER_OS", "RUNNER_ENVIRONMENT",
              "GITHUB_REPOSITORY", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_SHA",
              "ImageOS", "ImageVersion", "GITHUB_JOB")


def expect(value, reason):
    if not value:
        raise ValueError(reason)


def left(end, bound):
    return max(0.001, min(bound, end - time.monotonic()))


def read(path, limit=8192):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        m = os.fstat(fd)
        expect(stat.S_ISREG(m.st_mode) and m.st_nlink == 1, "evidence-type")
        data = stream.read(limit + 1)
    expect(len(data) <= limit, "evidence-bound")
    return data


def directory(path, uid=0, gid=0, mode=0o755):
    path.mkdir()
    os.chown(path, uid, gid)
    path.chmod(mode)


def record(path, value, uid):
    with path.open("x", encoding="ascii") as output:
        json.dump(value, output, separators=(",", ":"))
    os.chown(path, uid, uid)
    path.chmod(0o600)


def role(uid):
    return ["/usr/bin/setpriv", "--reuid", str(uid), "--regid", str(uid), "--clear-groups",
            "--no-new-privs", "--bounding-set=-all", "--"]


class Mcp:
    def __init__(self, root, config):
        self.process = subprocess.Popen(
            role(AGENT) + [root / "arxd", "mcp-stdio", "--client-config", config],
            env=ENV, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        )
        self.pending = bytearray()
        try:
            self.write({"jsonrpc":"2.0", "id":1, "method":"initialize",
                        "params":{"protocolVersion":"2025-11-25", "capabilities":{},
                                  "clientInfo":{"name":"c14", "version":"1"}}})
            expect("result" in self.read(1, 5), "mcp-initialize")
            self.write({"jsonrpc":"2.0", "method":"notifications/initialized"})
        except BaseException:
            self.close()
            raise

    def write(self, value):
        self.process.stdin.write(json.dumps(value).encode("ascii") + b"\n")
        self.process.stdin.flush()

    def read(self, id_, seconds):
        end = time.monotonic() + seconds
        fd = self.process.stdout.fileno()
        while time.monotonic() < end and len(self.pending) <= 65536:
            if b"\n" in self.pending:
                line, tail = self.pending.split(b"\n", 1)
                self.pending = bytearray(tail)
                value = json.loads(line)
                expect(value.get("id") == id_, "mcp-id")
                return value
            if select.select([fd], [], [], left(end, 1))[0]:
                chunk = os.read(fd, 4096)
                if not chunk:
                    break
                self.pending.extend(chunk)
        raise ValueError("mcp-response")

    def send(self, name, arguments):
        self.write({"jsonrpc":"2.0", "id":2, "method":"tools/call",
                    "params":{"name":name,"arguments":arguments}})

    def call(self, name, arguments, seconds=5):
        self.send(name, arguments)
        result = self.read(2, seconds)
        expect("result" in result, "mcp-call-refused")
        return json.loads(result["result"]["content"][0]["text"])

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=2)
        self.process.stdin.close()
        self.process.stdout.close()

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        self.close()


def owner_exchange(path, daemon, tag, payload):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(16)
        stream.connect(str(path))
        pid, uid, _ = struct.unpack("3i", stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        expect((pid, uid) == (daemon, DAEMON), "owner-peer")
        stream.sendall(b"ARXO" + bytes([tag]) + len(payload).to_bytes(2, "big") + payload)
        stream.shutdown(socket.SHUT_WR)
        with stream.makefile("rb") as result:
            value = result.read(8808)
        expect(len(value) >= 7 and value[:4] == b"ARXO" and
               int.from_bytes(value[5:7], "big") == len(value) - 7, "owner-frame")
        return value[4:5] + value[7:]


def owner(root, node, daemon, tag, data=b""):
    reply = subprocess.run(
        role(OWNER) + ["/usr/bin/python3", "-I", "-B", root / "probe.py", "--owner",
                      root / node / "owner-control/socket", str(daemon.pid), str(tag)],
        input=data, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        env=ENV, timeout=18, check=True,
    ).stdout
    expect(1 <= len(reply) <= 8801, "owner-reply")
    return reply


def ready(root, node, daemon, phase):
    end = time.monotonic() + 10
    while time.monotonic() < end:
        expect(daemon.poll() is None, "daemon-early-exit")
        if (root / node / "owner-control/socket").is_socket():
            evidence = owner(root, node, daemon, 4)
            expect(len(evidence) == 30 and evidence[0] == 15 and evidence[1] != 6, "daemon-refusal")
            if evidence[1] == phase:
                return evidence[1:]
        time.sleep(0.025)
    raise ValueError("daemon-readiness")


def configure(root, node, routes, issuer=None, key=None):
    base = root / node
    endpoint = routes[node]
    target = "b" if node == "a" else "a"
    agents = []
    for name in ("fixture", "foreign"):
        token = secrets.token_hex(32)
        agents.append({"uid":AGENT, "principal":name, "agent":name,
                       "token_sha256":hashlib.sha256(token.encode("ascii")).hexdigest()})
        record(base / "clients" / (name + ".json"), {
            "address":str(base / "mcp/socket"), "expected_server_identity":str(DAEMON),
            "token":token, "delegated":{"owner_uid":OWNER,"daemon_uid":DAEMON,"agent_group":AGENT},
        }, AGENT)
    grants = [{"source":1, "key":key, "agent":"fixture",
               "commands":[{"workspace":slot,"tool":"cargo","args":COMMAND,
                            "timeout_s":12,"output_bytes":1024} for slot in (*SLOTS, *REFUSED_SLOTS)]}] if issuer else []
    value = {name: str(base / name) for name in ("workspace","state","credentials","wrap","history")}
    value.update({
        "custody":str(base / "private/custody"), "owner_socket":str(base / "owner-control/socket"),
        "peer":endpoint[0], "enrollment":endpoint[1], "issuer_peer":issuer,
        "founder_cluster":[14] * 16 if issuer is None else None,
        "owner_uid":OWNER, "owner_gid":OWNER, "daemon_uid":DAEMON, "daemon_gid":DAEMON,
        "agent_uid":AGENT, "build_uid":BUILD,
        "executor_socket":str(root / "control/executor.sock") if issuer else None,
        "delegated_build":{
            "socket":str(base / "mcp/socket"), "activity":str(base / "activity"),
            "agent_group":AGENT, "peer":endpoint[2], "agents":agents, "grants":grants,
            "routes":[{"node":2 if node == "a" else 1, "address":routes[target][2]}],
        },
    })
    record(base / "install.json", value, DAEMON)


def start(root, node, intent):
    return subprocess.Popen(
        role(DAEMON) + [root / "arxd", "--cluster-config", root / node / "install.json",
                        "--cluster-intent", intent],
        env=ENV, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )


def rows(path, query):
    with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=.2) as db:
        return db.execute(query).fetchmany(65)


def account_counts(root, evidence):
    records = rows(root / "a/state/cluster-log.sqlite3", "SELECT idx,body FROM log ORDER BY idx LIMIT 65")
    expect(len(records) < 65, "log-bound")
    entries = []
    for idx, body in records:
        expect(body[:8] == b"ARXLE2\0\0" and 65 <= len(body) <= 4163
               and int.from_bytes(body[24:32], "big") == idx, "log-entry")
        expect(hashlib.sha256(body[:-32]).digest() == body[-32:], "log-checksum")
        entries.append(body)
    tags = [entry[32] for entry in entries]
    expect(tags.count(3) == 2 and all(tags.count(tag) == 1 for tag in (4, 5, 6)), "invocation-count")
    membership = [entry[8:32] for entry in entries if entry[32] == 3][-1]
    expect(evidence[3:5] == b"\1\1" and evidence[5:] == membership, "admission-apply")
    term, leader, index = struct.unpack(">QQQ", membership)
    print(f"C14 effects found=1 mint=1 query=1 redeem=1 proposals=3 add_nodes=1 applied=1"
          f" strict_learner=2 membership={term}/{leader}/{index}", flush=True)


def build(root, slot):
    base = root / "a/clients/fixture.json"
    args = {"target":2,"workspace":slot,"tool":"cargo","args":COMMAND,"timeout_s":12,"output_bytes":1024}
    with Mcp(root, base) as client:
        if slot == "ordinary":
            expect(client.call("discover_build_targets", {}) == {"targets":[2]}, "strict-route")
            result = client.call("run_command", args, 16)
        else:
            client.send("run_command", args)
            end = time.monotonic() + 8
            while not (root / "workspace/lost/out/observed").is_file():
                expect(client.process.poll() is None and time.monotonic() < end, "loss-before-admission")
                time.sleep(0.025)
            expect(not client.pending and not select.select([client.process.stdout], [], [], 0)[0],
                   "reply-before-loss")
            client.process.kill()
            expect(client.process.wait(timeout=2) == -signal.SIGKILL, "response-observer-loss")
            (root / "workspace/lost/out/release").touch(exist_ok=False)
            result = None
    if result is None:
        def last_outcome(node):
            check = ("SELECT a.outcome FROM delegated_build d JOIN activity a ON a.id=d.activity_id "
                     "ORDER BY a.id DESC LIMIT 1")
            result = rows(root / node / "activity/activity.sqlite3", check)
            expect(len(result) == 1, "lost-reply-correlation")
            return result[0][0]

        end = time.monotonic() + 8
        while last_outcome("b") == "started":
            expect(time.monotonic() < end, "lost-terminal")
            time.sleep(.04)
        expect(last_outcome("b") == "succeeded" and last_outcome("a") == "started",
               "reply-not-applied-at-source")
        with Mcp(root, base) as status:
            ids = status.call("delegated_build_status", {"target":2})["operations"]
        expect(len(ids) == 2, "forwarded-correlation")
        operation = ids[0]
        with Mcp(root, root / "a/clients/foreign.json") as outsider:
            denied = outsider.call("delegated_build_status", {"target":2,"operation_id":operation})
            expect(denied == {"outcome":"refused"}, "foreign-status")
        end = time.monotonic() + 8
        while time.monotonic() < end:
            with Mcp(root, base) as status:
                result = status.call("delegated_build_status", {"target":2,"operation_id":operation})
            if result["outcome"] != "unknown":
                break
            time.sleep(0.04)
    expect(result and result["outcome"] == "succeeded" and
           re.fullmatch("[0-9a-f]{64}", result["operation_id"]), "terminal-outcome")
    status = result["target_activity"]
    expect((status["uid"], status["gid"], status["cleanup_confirmed"], status["exit_code"]) ==
           (BUILD, BUILD, True, 0), "terminal-identity")
    artifact = root / "workspace" / slot / "out/target/debug/c14-fixture"
    expect(artifact.is_file() and not artifact.is_symlink() and artifact.stat().st_uid == BUILD, "artifact")
    observed = read(root / "workspace" / slot / "out/observed", 128).decode("ascii")
    expect(re.fullmatch(r"arx-\d+-\d+\n", observed), "cgroup-leaf")
    expect(not (Path("/sys/fs/cgroup/system.slice/arxd.service/runs") / observed.strip()).exists(),
           "cgroup-residue")
    return result["operation_id"], status["activity_id"]


def campaign(root, address):
    a10 = runpy.run_path(str(root / "a10.py"))
    owned = []
    phase = "founder"
    broker_started = False
    try:
        ports = []
        for _ in range(6):
            transport = socket.socket()
            transport.bind((address, 0))
            ports.append(transport)
        addresses = [f"{address}:{s.getsockname()[1]}" for s in ports]
        for stream in ports:
            stream.close()
        routes = {"a":addresses[:3], "b":addresses[3:]}
        configure(root, "a", routes)
        a = start(root, "a", "create-founder")
        owned.append(a)
        ready(root, "a", a, 4)
        phase = "mint"
        paste = owner(root, "a", a, 1)
        expect(paste[:1] == b"\x0b" and paste[1:].startswith(b"arxje1-"), "mint-result")
        key = json.loads(read(root / "a/credentials/identity.json"))["key"]
        expect(len(key) == 32, "source-key")
        configure(root, "b", routes, routes["a"][0], key)
        phase = "broker"
        broker_started = True
        a10["start"](root, time.monotonic() + 7)
        a10["wait_ready"](root, time.monotonic() + 8)
        phase = "join"
        b = start(root, "b", "join")
        owned.append(b)
        ready(root, "b", b, 1)
        expect(owner(root, "b", b, 2, paste[1:]) == b"\x0c", "join-result")
        del paste
        strict = ready(root, "b", b, 3)
        founder = ready(root, "a", a, 4)
        expect(founder[1:3] == b"\1\1", "enrollment-dispatch")
        account_counts(root, strict)
        phase = "ordinary"
        arguments = {"target":2,"workspace":"ordinary","tool":"cargo","args":COMMAND,
                     "timeout_s":12,"output_bytes":1024}
        with Mcp(root, root / "a/clients/foreign.json") as ungranted:
            expect(ungranted.call("run_command", arguments) == {"outcome":"refused"}, "agent-policy")
        with Mcp(root, root / "a/clients/fixture.json") as agent:
            for slot in REFUSED_SLOTS:
                expect(agent.call("run_command", {**arguments, "workspace":slot}) == {"outcome":"refused"},
                       "workspace-policy")
        first = build(root, "ordinary")
        phase = "lost"
        second = build(root, "lost")
        expect(first[0] != second[0] and first[1] != second[1], "distinct-correlations")
        query = ("SELECT hex(d.operation),a.id,a.outcome FROM delegated_build d "
                 "JOIN activity a ON a.id=d.activity_id WHERE d.forwarded=0 ORDER BY a.id LIMIT 3")
        expect(rows(root / "b/activity/activity.sqlite3", query) ==
               [(key.upper(), row, "succeeded") for key, row in (first, second)], "correlated-starts")
        expect(not rows(root / "a/activity/activity.sqlite3", query), "no-a-admission")
        print(PASS, flush=True)
        phase = "close"
    except (OSError, ValueError, subprocess.SubprocessError, sqlite3.Error, RuntimeError):
        print(f"C14 first phase={phase} category=refused", flush=True)
        raise
    finally:
        closes = 0
        deadline = time.monotonic() + 13
        for process in reversed(owned):
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
            try:
                closes += process.wait(timeout=left(deadline, 7)) == 0
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)
        broker_closed = False
        if broker_started:
            try:
                a10["stop"](root, deadline, 1, [])
                broker_closed = True
            except (ValueError, OSError, subprocess.SubprocessError, RuntimeError):
                print("C14 cleanup broker=unconfirmed", flush=True)
        print(f"C14 close starts={len(owned)} closes={closes} broker={int(broker_closed)}", flush=True)
        if phase == "close":
            expect(len(owned) == closes == 2 and broker_closed, "owned-close")


def child(root):
    a10 = runpy.run_path(str(root / "a10.py"))
    slot = Path.cwd()
    expect(slot.parent == root / "workspace" and slot.name in SLOTS and sys.argv[1:] == COMMAND,
           "build-command")
    expect(os.getuid() == BUILD and os.getgid() == BUILD and not os.getgroups(), "build-role")
    line = read(Path("/proc/self/cgroup"), 256).decode("ascii").strip()
    leaf = line.removeprefix("0::/system.slice/arxd.service/runs/")
    expect(re.fullmatch(r"arx-\d+-\d+", leaf), "build-cgroup")
    leaf_path = Path("/sys/fs/cgroup/system.slice/arxd.service/runs") / leaf
    expect(read(leaf_path / "memory.max", 40).strip() == str(a10["MEMORY"]).encode()
           and read(leaf_path / "pids.max", 40).strip() == str(a10["PIDS"]).encode()
           and read(leaf_path / "memory.swap.max", 40).strip() == b"0", "build-caps")
    a10["cgroup_denials"](leaf_path)
    a10["verify_child_fds"]()
    for path in ["broker/settings.json", "b/wrap", "b/private/custody", "b/history", "b/activity"]:
        try:
            fd = os.open(root / path, os.O_RDONLY | os.O_NOFOLLOW)
        except OSError as error:
            expect(error.errno in (errno.EACCES, errno.EPERM), "custody-denial")
        else:
            os.close(fd)
            raise ValueError("custody-exposed")
    a10["socket_denied"](root / "control/executor.sock")
    a10["socket_denied"](root / "b/mcp/socket")
    try:
        os.rename(slot, slot.with_name("replaced"))
    except OSError as error:
        expect(error.errno in (errno.EACCES, errno.EPERM), "slot-replacement-denial")
    else:
        raise ValueError("replaced-slot")
    with (slot / "out/observed").open("x", encoding="ascii") as out:
        out.write(leaf + "\n")
    if slot.name == "lost":
        end = time.monotonic() + 6
        while not (slot / "out/release").is_file():
            expect(time.monotonic() < end, "loss-observer")
            time.sleep(.025)
    os.execve(root / "toolchain/bin/cargo", ["cargo", *COMMAND], os.environ)


def stage_workspace(root, a10):
    directory(root / "broker", mode=0o700)
    directory(root / "control", gid=DAEMON, mode=0o710)
    directory(root / "workspace")
    directory(root / "tools")
    worker_roots = ["workspace","state","credentials","wrap","private","history","activity"]
    for name in ("a", "b"):
        node = root / name
        directory(node)
        for path in worker_roots:
            directory(node / path, DAEMON, DAEMON, 0o700)
        directory(node / "private/custody", DAEMON, DAEMON, 0o700)
        directory(node / "owner-control", DAEMON, OWNER, 0o2710)
        directory(node / "mcp", DAEMON, AGENT, 0o2710)
        directory(node / "clients", AGENT, AGENT, 0o700)
    for name in SLOTS:
        slot = root / "workspace" / name
        directory(slot)
        directory(slot / "src")
        directory(slot / "out", BUILD, BUILD, 0o700)
        (slot / "Cargo.toml").write_text(
            '[package]\nname="c14-fixture"\nversion="0.0.0"\nedition="2024"\n'
            '[profile.dev]\ndebug=0\nincremental=false\npanic="abort"\ncodegen-units=1\n', encoding="ascii")
        (slot / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "c14-fixture"\nversion = "0.0.0"\n',
                                        encoding="ascii")
        (slot / "src/main.rs").write_text('fn main() {}\n', encoding="ascii")
    (root / "workspace/link").symlink_to("ordinary")
    directory(root / "workspace/writable", mode=0o775)
    shutil.copyfile(root / "probe.py", root / "tools/cargo")
    (root / "tools/cargo").chmod(0o755)
    directory(root / "home", BUILD, BUILD, 0o700)
    record(root / "broker/settings.json", {
        "daemon_uid":DAEMON, "daemon_gid":DAEMON, "daemon_roots":[str(root / "b" / x) for x in worker_roots],
        "service":{"schema":"arxd-service/1", "client":{"identity":str(DAEMON)},
                   "build_account":{"name":"arx-a10-build","identity":str(BUILD),"gid":BUILD},
                   "rights":"standard", "workspace":str(root / "workspace"),
                   "environment":{"PATH":f'{root}/tools:{root}/toolchain/bin:/usr/bin:/bin',
                                  "HOME":str(root / "home"), "CARGO_HOME":str(root / "home")},
                   "limits":{"max_concurrent_runs":1, "memory_bytes":a10["MEMORY"],"pids":a10["PIDS"]},
                   "agents":[{"principal":"qualification","token_sha256":"0"*64}],
                   "granted_batch_right":False},
    }, 0)


def stage_toolchain(source, destination):
    expect(source.name.endswith("-x86_64-unknown-linux-gnu") and source.parent.name == "toolchains"
           and source.resolve() == source, "toolchain-layout")
    directory(destination)
    directory(destination / "bin")
    directory(destination / "lib")
    directory(destination / "lib/rustlib")
    for name in ("cargo", "rustc"):
        shutil.copy2(source / "bin" / name, destination / "bin" / name)
    for lib in (source / "lib").glob("*.so*"):
        shutil.copy2(lib, destination / "lib" / lib.name)
    arch = Path("lib/rustlib/x86_64-unknown-linux-gnu")
    shutil.copytree(source / arch, destination / arch)
    for executable in (destination / "bin/cargo", destination / "bin/rustc", "/usr/bin/cc"):
        subprocess.run(role(BUILD) + [executable, "--version"], env=ENV,
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=5, check=True)


def privileged(stage, sources, a10, c13, receipt):
    m = stage.lstat()
    expect(os.geteuid() == 0 and stage.parent == Path("/tmp")
           and re.fullmatch("arx-c14-[a-z0-9_]{8}", stage.name)
           and stat.S_ISDIR(m.st_mode) and stat.S_IMODE(m.st_mode) == 0o700
           and m.st_uid == int(os.environ["SUDO_UID"]), "staging")
    root = Path("/var/lib") / f"arx-c14-{receipt['run']}-{receipt['attempt']}"
    expect(all(a10["role_unused"](name, uid) for name, uid in ROLES), "account-collision")
    deadline = time.monotonic() + 40
    a10["ancestry"](root.parent)
    expect(a10["properties"](deadline)["LoadState"] == "not-found", "unit-collision")
    owned = []
    removable = True
    directory(root)
    try:
        for name, uid in ROLES:
            owned.append(("group", name, uid))
            a10["command"](["/usr/sbin/groupadd", "--gid", str(uid), name], deadline, "group")
            owned.append(("user", name, uid))
            a10["command"](a10["useradd_argv"](name, uid), deadline, "account")
            a10["role"](name, uid)
        for name in ("arxd", "campaign"):
            c13["stage_binary"](stage / name, root / name, deadline)
            (root / name).chmod(0o755)
        shutil.copyfile(sources, root / "probe.py")
        shutil.copyfile(sources.with_name("executor-probe.py"), root / "a10.py")
        stage_workspace(root, a10)
        stage_toolchain(stage / "toolchains/fixture-x86_64-unknown-linux-gnu", root / "toolchain")
        ip = c13["primary_ip"](deadline)
        removable = False
        code, capture = c13["bounded_run"](
            [root / "campaign", "--ignored", "--exact", SELECTOR, "--nocapture"],
            env={**ENV, "ARX_C14_HOSTED":"1", "ARX_C14_ROOT":str(root), "ARX_C14_IP":ip}, timeout=55)
        lines = capture.text().splitlines()
        safe = re.compile(r"C14 (?:first phase=(?:founder|mint|broker|join|ordinary|lost|close) category=refused|"
                          r"close starts=[0-2] closes=[0-2] broker=[01]|cleanup broker=unconfirmed|"
                          r"effects found=1 mint=1 query=1 redeem=1 proposals=3 add_nodes=1 applied=1 "
                          r"strict_learner=2 membership=\d+/\d+/\d+)")
        for line in lines:
            if safe.fullmatch(line) or line == PASS:
                print(line, flush=True)
        removable = (code == 0 and not capture.truncated and lines.count(PASS) == 1 and
                     lines.count("C14 close starts=2 closes=2 broker=1") == 1)
        expect(removable, "hosted-c14-result")
    finally:
        if removable:
            end = time.monotonic() + 12
            clean = [c13["remove_role"](kind, name, uid, end) for kind, name, uid in reversed(owned)]
            expect(all(value == "removed" for value in clean), "account-cleanup")
            shutil.rmtree(root)


def select_campaign(deps, c13, deadline):
    from itertools import islice
    entries = sorted(islice(deps.iterdir(), 4097))
    expect(len(entries) <= 4096, "artifact-inventory")
    matches = []
    for path in entries:
        if not re.fullmatch(r"m3_delegated_build_linux-[a-f0-9]{16}", path.name):
            continue
        c13["checked_file"](path)
        with c13["open_artifact"](path) as artifact:
            executable = f"/proc/{os.getpid()}/fd/{artifact.fileno()}"
            code, capture = c13["bounded_run"]([executable, "--list", "--exact", SELECTOR],
                                               env=ENV, timeout=left(deadline, 3))
        if code == 0 and not capture.truncated and c13["exact_listing"](capture.text(), SELECTOR):
            matches.append(path)
    expect(len(matches) == 1, "campaign-selector")
    return matches[0]


def main():
    here = Path(__file__).resolve()
    if here.name == "cargo":
        return child(here.parent.parent)
    if sys.argv[1:2] == ["--campaign"] and len(sys.argv) == 4:
        expect(os.geteuid() == 0 and here.parent == Path(sys.argv[2]) and
               here.parent.parent == Path("/var/lib"), "campaign-identity")
        return campaign(here.parent, sys.argv[3])
    if sys.argv[1:2] == ["--owner"] and len(sys.argv) == 5:
        expect(os.geteuid() == OWNER and int(sys.argv[4]) in (1, 2, 4), "owner-identity")
        data = sys.stdin.buffer.read(8801)
        expect(len(data) <= 8800, "owner-data")
        sys.stdout.buffer.write(owner_exchange(Path(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]), data))
        return
    a10 = runpy.run_path(str(here.with_name("executor-probe.py")))
    c13 = runpy.run_path(str(here.parent.parent / "m3-c13/run.py"))
    facts = [os.environ[name] for name in PROVENANCE]
    receipt = a10["hosted"](facts[5:])
    if sys.argv[1:2] == ["--staged"] and len(sys.argv) == 3:
        return privileged(Path(sys.argv[2]), here, a10, c13, receipt)
    expect(len(sys.argv) == 1 and os.geteuid() != 0, "hosted-entry")
    workspace = here.parents[3]
    deadline = time.monotonic() + 16
    head = subprocess.check_output(["/usr/bin/git", "-C", workspace, "rev-parse", "HEAD"],
                                   timeout=left(deadline, 3), env=ENV)
    expect(head.decode("ascii").strip() == receipt["head"], "checkout")
    profile = workspace / "target/debug"
    bin_test = select_campaign(profile / "deps", c13, deadline)
    rustup = shutil.which("rustup")
    expect(rustup, "toolchain")
    toolchain = Path(subprocess.check_output([rustup, "which", "rustc"], timeout=5).decode().strip()).resolve().parent.parent
    with tempfile.TemporaryDirectory(prefix="arx-c14-", dir="/tmp") as directory:
        stage = Path(directory)
        for name, source in (("campaign", bin_test), ("arxd", profile / "arxd")):
            c13["stage_binary"](source, stage / name, deadline)
        local = stage / "toolchains/fixture-x86_64-unknown-linux-gnu"
        for name in ("bin", "lib/rustlib"):
            (local / name).mkdir(parents=True)
        for binary in ("cargo", "rustc"):
            shutil.copy2(toolchain / "bin" / binary, local / "bin" / binary)
        for lib in (toolchain / "lib").glob("*.so*"):
            shutil.copy2(lib, local / "lib" / lib.name)
        arch = Path("lib/rustlib/x86_64-unknown-linux-gnu")
        shutil.copytree(toolchain / arch, local / arch)
        subprocess.run(["/usr/bin/sudo", "-n", "/usr/bin/env",
                        *[f"{k}={v}" for k, v in zip(PROVENANCE, facts)],
                        "/usr/bin/python3", "-I", "-B", here, "--staged", stage],
                       env=ENV, stdin=subprocess.DEVNULL, timeout=95, check=True)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, RuntimeError, subprocess.SubprocessError, sqlite3.Error):
        print("C14 bounded launcher refusal", file=sys.stderr, flush=True)
        sys.exit(1)
