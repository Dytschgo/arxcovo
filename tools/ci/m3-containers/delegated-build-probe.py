#!/usr/bin/python3
"""One hosted C14 run using the A10 broker and C13 runner qualification."""
from collections import Counter
from contextlib import contextmanager, redirect_stdout
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
WRAPPER_READY = b"C14 wrapper-ready\n"
PROBE_ERRORS = (ValueError, OSError, KeyError, IndexError, RuntimeError,
                TypeError, subprocess.SubprocessError, sqlite3.Error)
OUTCOMES = ("refused", "failed", "unknown", "rejected", "interrupted", "succeeded")
OPERATION = re.compile(r"[0-9a-f]{64}")
MEMBERSHIP = "0::/system.slice/arxd.service/runs/"
PASS = "C14 build admitted=2 executed=2 a_executed=0 ordinary=1 lost_reply_status=1"
PROVENANCE = ("CI", "GITHUB_ACTIONS", "RUNNER_OS", "RUNNER_ENVIRONMENT",
              "GITHUB_REPOSITORY", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_SHA",
              "ImageOS", "ImageVersion", "GITHUB_JOB")


def expect(value, reason):
    if not value:
        raise ValueError(reason)


class Fault(ValueError):
    def __init__(self, checkpoint, category):
        super().__init__(checkpoint, category)
        self.checkpoint = checkpoint
        self.category = category


@contextmanager
def checking(checkpoint, category="invalid"):
    try:
        yield
    except Fault:
        raise
    except (TimeoutError, subprocess.TimeoutExpired):
        raise Fault(checkpoint, "timeout") from None
    except PROBE_ERRORS:
        raise Fault(checkpoint, category) from None


class Evidence:
    def __init__(self):
        self.failed = False
        self.confirmed_close = True
        self.close_reported = False
        self.confirmed_ownership = True

    def first(self, fault):
        if not self.failed:
            print(f"C14 first checkpoint={fault.checkpoint} category={fault.category}", flush=True)
            self.failed = True

    def report_close(self):
        print(f"C14 process closure={'confirmed' if self.confirmed_close else 'unconfirmed'}", flush=True)
        self.close_reported = True


def left(end, bound):
    return max(0.001, min(bound, end - time.monotonic()))


def phase_end(end, seconds):
    return min(end, time.monotonic() + seconds)


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
    def __init__(self, root, config, deadline, clients):
        self.deadline = deadline
        self.process = subprocess.Popen(
            role(AGENT) + [root / "arxd", "mcp-stdio", "--client-config", config],
            env=ENV, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        )
        clients.append(self)
        self.pending = bytearray()
        try:
            self.write({"jsonrpc":"2.0", "id":1, "method":"initialize",
                        "params":{"protocolVersion":"2025-11-25", "capabilities":{},
                                  "clientInfo":{"name":"c14", "version":"1"}}})
            expect("result" in self.read(1, phase_end(deadline, 5)), "mcp-initialize")
            self.write({"jsonrpc":"2.0", "method":"notifications/initialized"})
        except PROBE_ERRORS:
            try:
                self.close()
            except PROBE_ERRORS:
                pass
            raise

    def write(self, value):
        self.process.stdin.write(json.dumps(value).encode("ascii") + b"\n")
        self.process.stdin.flush()

    def ready(self):
        return bool(self.pending or select.select([self.process.stdout], [], [], 0)[0])

    def read(self, id_, end):
        fd = self.process.stdout.fileno()
        while len(self.pending) <= 65536:
            if b"\n" in self.pending:
                line, tail = self.pending.split(b"\n", 1)
                self.pending = bytearray(tail)
                with checking("rpc", "malformed"):
                    value = json.loads(line)
                    expect(isinstance(value, dict) and value.get("jsonrpc") == "2.0"
                           and value.get("id") == id_, "mcp-envelope")
                    expect(("error" in value) != ("result" in value), "mcp-variant")
                if "error" in value:
                    raise Fault("rpc", "rpc-error")
                return value
            if time.monotonic() >= end:
                raise Fault("rpc", "timeout")
            if select.select([fd], [], [], left(end, 1))[0]:
                chunk = os.read(fd, 4096)
                if not chunk:
                    raise Fault("rpc", "eof")
                self.pending.extend(chunk)
        raise Fault("rpc", "malformed")

    def send(self, name, arguments):
        self.write({"jsonrpc":"2.0", "id":2, "method":"tools/call",
                    "params":{"name":name,"arguments":arguments}})

    def call(self, name, arguments, seconds=5):
        self.send(name, arguments)
        return self.result(phase_end(self.deadline, seconds))

    def result(self, end):
        reply = self.read(2, end)
        with checking("rpc", "malformed"):
            result = json.loads(reply["result"]["content"][0]["text"])
            expect(isinstance(result, dict), "mcp-result")
        return result

    def close(self, deadline=None):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=left(self.deadline if deadline is None else deadline, 2))
        self.process.stdin.close()
        self.process.stdout.close()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, *_args):
        try:
            self.close()
        except PROBE_ERRORS:
            if exc_type is None:
                raise Fault("cleanup", "unconfirmed") from None


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


def owner(root, node, daemon, tag, deadline, data=b""):
    reply = subprocess.run(
        role(OWNER) + ["/usr/bin/python3", "-I", "-B", root / "probe.py", "--owner",
                      root / node / "owner-control/socket", str(daemon.pid), str(tag)],
        input=data, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        env=ENV, timeout=left(deadline, 18), check=True,
    ).stdout
    expect(1 <= len(reply) <= 8801, "owner-reply")
    return reply


def ready(root, node, daemon, phase, deadline):
    end = phase_end(deadline, 10)
    while time.monotonic() < end:
        expect(daemon.poll() is None, "daemon-early-exit")
        if (root / node / "owner-control/socket").is_socket():
            evidence = owner(root, node, daemon, 4, end)
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


def frozen_kinds(workspace):
    source = workspace / "crates/arxd/src/cluster"

    def encoder(path, begin, end):
        with path.open("rb") as stream:
            raw = stream.read(65_537)
        expect(len(raw) <= 65_536, "codec-source-bound")
        return raw.decode("utf-8").split(begin, 1)[1].split(end, 1)[0]

    enrollment = encoder(source / "enrollment/codec.rs", "fn encode_command(", "fn decode_command(")
    log = encoder(source / "log_store.rs", "fn encode_row(", "fn decode_row(")
    names = re.findall(r"Command::(\w+)\s*\{[^}]*\}\s*=>\s*\{\s*b\.push\((\d+)\);",
                       enrollment)
    names += re.findall(r"Entry::(\w+)(?:\(\w+\))?\s*=>\s*(?:\{\s*)?b\.push\((\d+)\)", log)
    tags = {name: int(tag) for name, tag in names}
    expect(len(names) == len(tags) == len(set(tags.values())) and
           set(tags) == {"Blank", "Membership", "Found", "Mint", "Remove", "Redeem"}, "codec-kinds")
    return tags


def admission_membership(records, tags):
    expect(len(records) < 65, "log-bound")
    counts = Counter()
    membership = None
    for idx, body in records:
        expect(body[:8] == b"ARXLE2\0\0" and 65 <= len(body) <= 4163
               and int.from_bytes(body[24:32], "big") == idx, "log-entry")
        expect(hashlib.sha256(body[:-32]).digest() == body[-32:], "log-checksum")
        tag = body[32]
        if tag != tags["Blank"]:
            counts[tag] += 1
        if tag == tags["Membership"]:
            membership = body[8:32]
    expect(counts == {tags["Found"]: 1, tags["Mint"]: 1, tags["Redeem"]: 1,
                      tags["Membership"]: 2}, "invocation-count")
    return membership


def account_counts(root, evidence, tags):
    records = rows(root / "a/state/cluster-log.sqlite3", "SELECT idx,body FROM log ORDER BY idx LIMIT 65")
    membership = admission_membership(records, tags)
    expect(evidence[3:5] == b"\1\1" and evidence[5:] == membership, "admission-apply")
    term, leader, index = struct.unpack(">QQQ", membership)
    print(f"C14 effects found=1 mint=1 query=1 redeem=1 proposals=3 add_nodes=1 applied=1"
          f" strict_learner=2 membership={term}/{leader}/{index}", flush=True)


def outcome(result):
    value = result.get("outcome")
    return value if value in OUTCOMES else "malformed"


def negative_control(client, tool, arguments, checkpoint):
    with checking(checkpoint):
        result = client.call(tool, arguments)
        if result != {"outcome":"refused"}:
            raise Fault(checkpoint, outcome(result))


def private_cgroup(path):
    mode = path.lstat()
    expect(stat.S_ISDIR(mode.st_mode) and (mode.st_uid, mode.st_gid) == (0, 0)
           and stat.S_IMODE(mode.st_mode) == 0o700, "private-cgroup")


def observe(root, slot, broker_pid, a10):
    read_live = a10["read"]
    with checking("witness"):
        try:
            value = read_live(root / "workspace" / slot / "out/observed", 64, BUILD)
        except FileNotFoundError:
            return None
        if not value:
            return None
        match = re.fullmatch(r"([1-9][0-9]{0,9}) (arx-[^\s]+)\n", value)
        expect(match, "witness-frame")
        pid, leaf = match.groups()
        expect(int(pid) <= 2**31 - 1 and a10["LEAF"].fullmatch(leaf)
               and leaf.split("-")[1] == str(broker_pid), "witness-identity")
        path = a10["RUNS"] / leaf
        private_cgroup(a10["RUNS"])
        private_cgroup(path)
        expect(read_live(Path("/proc") / pid / "cgroup", 256).strip() == MEMBERSHIP + leaf
               and pid in read_live(path / "cgroup.procs").split(), "witness-membership")
    with checking("limits"):
        for control, value in (("memory.max", a10["MEMORY"]), ("pids.max", a10["PIDS"]),
                               ("memory.swap.max", 0)):
            expect(read_live(path / control, 40).strip() == str(value), "controller-value")
    with checking("role"):
        facts = a10["proc_status"](pid)
        expect(facts["Uid"] == facts["Gid"] == [str(BUILD)] * 4 and facts["Groups"] == []
               and facts["NoNewPrivs"] == ["1"] and facts["CapBnd"] == [a10["CAPS"]], "credential")
        expect(all(facts[key] == ["0" * 16] for key in ("CapEff", "CapPrm", "CapInh", "CapAmb")),
               "capabilities")
    return pid, leaf


def reject_early_reply(client, end):
    if client.ready():
        result = client.result(end)
        category = outcome(result)
        raise Fault("terminal" if category in ("failed", "rejected", "interrupted") else "admission",
                    "premature" if category == "succeeded" else category)


def gate(root, slot, client, broker_pid, a10, end, deadline):
    while time.monotonic() < end:
        reject_early_reply(client, end)
        witness = observe(root, slot, broker_pid, a10)
        if witness is None:
            time.sleep(.025)
            continue
        pid, leaf = witness
        reject_early_reply(client, end)
        if time.monotonic() >= end:
            break
        if slot == "lost":
            with checking("observer-loss"):
                client.process.kill()
                expect(client.process.wait(timeout=left(deadline, 2)) == -signal.SIGKILL,
                       "response-observer-loss")
        with checking("witness-release"):
            a10["publish_observer_ack"](root / "observer" / slot, pid)
        print(f"C14 observed slot={slot} private=0700 uid={BUILD} gid={BUILD} "
              f"memory={a10['MEMORY']} pids={a10['PIDS']} swap=0 no_new_privs=1", flush=True)
        return leaf
    raise Fault("witness", "timeout")


def admissions(root, node, forwarded):
    # Both call sites pass a fixed flag. Bound the read to one row beyond the expected two.
    query = ("SELECT hex(d.operation),a.id,a.outcome FROM delegated_build d "
             f"JOIN activity a ON a.id=d.activity_id WHERE d.forwarded={int(forwarded)} "
             "ORDER BY a.id LIMIT 3")
    return rows(root / node / "activity/activity.sqlite3", query)


def recover_lost(root, connect, deadline, first):
    with checking("terminal"):
        end = phase_end(deadline, 8)
        while time.monotonic() < end:
            before = admissions(root, "b", False)
            expect(len(before) == 2 and before[0] == (first[0].upper(), first[1], "succeeded"),
                   "lost-target-admission")
            if before[1][2] != "started":
                break
            time.sleep(.04)
        else:
            raise Fault("terminal", "timeout")
        key, _, state = before[1]
        if state != "succeeded":
            raise Fault("terminal", outcome({"outcome":state}))
        operation = key.lower()
        expect(OPERATION.fullmatch(operation) and operation != first[0], "distinct-operation")
        source = admissions(root, "a", True)
        expect([(key, state) for key, _, state in source] ==
               [(first[0].upper(), "succeeded"), (key, "started")], "source-pending-before-status")

    with checking("rpc"):
        with connect("fixture", deadline) as status:
            listed = status.call("delegated_build_status", {"target":2})
    with checking("forwarded-list"):
        # A can commit the lost result while the list RPC is in flight.
        terminal_source = [source[0], (*source[1][:2], "succeeded")]
        after_list = admissions(root, "a", True)
        expect(after_list in (source, terminal_source), "forwarded-source-correlation")
        expect(listed == {"operations":[operation]}
               or (listed == {"operations":[]} and after_list == terminal_source),
               "forwarded-correlation")

    with checking("rpc"):
        arguments = {"target":2,"operation_id":operation}
        with connect("foreign", deadline) as outsider:
            negative_control(outsider, "delegated_build_status", arguments, "foreign-status")
        end = phase_end(deadline, 8)
        while time.monotonic() < end:
            with connect("fixture", end) as status:
                result = status.call("delegated_build_status", arguments)
            if outcome(result) != "unknown":
                break
            time.sleep(.04)
        else:
            raise Fault("terminal", "unknown")
    with checking("terminal"):
        expect(admissions(root, "b", False) == before, "status-repeated-execution")
        if outcome(result) == "succeeded":
            expect(result.get("operation_id") == operation, "status-correlation")
            expect(admissions(root, "a", True) == terminal_source, "status-source-terminal")
    return result


def build(root, slot, deadline, connect, broker_pid, a10, first=None):
    args = {"target":2,"workspace":slot,"tool":"cargo","args":COMMAND,"timeout_s":12,"output_bytes":1024}
    with checking("rpc"):
        with connect("fixture", deadline) as client:
            if slot == "ordinary":
                with checking("route"):
                    expect(client.call("discover_build_targets", {}) == {"targets":[2]}, "strict-route")
            client.send("run_command", args)
            reply_end = phase_end(deadline, 16)
            leaf = gate(root, slot, client, broker_pid, a10, phase_end(reply_end, 8), deadline)
            result = client.result(reply_end) if slot == "ordinary" else None
    if slot == "lost":
        result = recover_lost(root, connect, deadline, first)
    with checking("terminal", "malformed"):
        category = outcome(result)
        if category != "succeeded":
            raise Fault("terminal", category)
        expect(OPERATION.fullmatch(result["operation_id"]), "operation")
        status = result["target_activity"]
        expect((status["uid"], status["gid"]) == (BUILD, BUILD)
               and status["cleanup_confirmed"] is True
               and type(status["exit_code"]) is int and status["exit_code"] == 0, "terminal-identity")
        expect(type(status["activity_id"]) is int and status["activity_id"] > 0, "activity-id")
    with checking("artifact"):
        out = root / "workspace" / slot / "out"
        for path in (out / "target", out / "target/debug"):
            expect(stat.S_ISDIR(path.lstat().st_mode), "artifact-directory")
        artifact = (out / "target/debug/c14-fixture").lstat()
        expect(stat.S_ISREG(artifact.st_mode) and artifact.st_uid == BUILD, "artifact")
    with checking("leaf-cleanup"):
        expect(not (a10["RUNS"] / leaf).exists(), "cgroup-residue")
    return result["operation_id"], status["activity_id"]


def wrapper_ready(wrapper, deadline):
    end = phase_end(deadline, 3)
    fd = wrapper.stderr.fileno()
    message = bytearray()
    while len(message) < len(WRAPPER_READY) and time.monotonic() < end:
        if not select.select([fd], [], [], left(end, 1))[0]:
            continue
        chunk = os.read(fd, len(WRAPPER_READY) + 1 - len(message))
        if not chunk:
            break
        message.extend(chunk)
    expect(message == WRAPPER_READY, "wrapper-readiness")


def reap_wrapper(wrapper, deadline):
    try:
        if wrapper.poll() is None:
            wrapper.terminate()
            try:
                wrapper.wait(timeout=left(deadline, .5))
            except subprocess.TimeoutExpired:
                wrapper.kill()
                wrapper.wait(timeout=left(deadline, 1))
        return True
    except PROBE_ERRORS:
        return False
    finally:
        for stream in (wrapper.stdin, wrapper.stderr):
            try:
                stream.close()
            except OSError:
                pass


def supervise(wrapper, work, cleanup, evidence, deadline, cleanup_deadline):
    evidence.confirmed_close = False
    try:
        with checking("campaign"):
            work(deadline)
            if time.monotonic() >= deadline:
                raise Fault("campaign", "timeout")
            wrapper.stdin.write(b"\1")
            wrapper.stdin.close()
            expect(wrapper.wait(timeout=left(deadline, 3)) == 0, "wrapper-result")
    except Fault as fault:
        evidence.first(fault)
    finally:
        reaped = reap_wrapper(wrapper, cleanup_deadline)
        try:
            closed = cleanup(cleanup_deadline)
        except PROBE_ERRORS:
            closed = False
        evidence.confirmed_close = reaped and closed
        if not evidence.confirmed_close:
            evidence.first(Fault("cleanup", "unconfirmed"))
        print(f"C14 cleanup wrapper_reaped={int(reaped)}", flush=True)
        evidence.report_close()


def campaign(root, address, tags, a10, cleanup_deadline, evidence):
    deadline = cleanup_deadline - 13
    if time.monotonic() >= deadline:
        raise Fault("campaign", "timeout")
    owned = []
    clients = []
    phase = "founder"
    broker_started = False
    wrapper = subprocess.Popen(
        [root / "campaign", "--ignored", "--exact", SELECTOR, "--nocapture"],
        env={**ENV, "ARX_C14_HOSTED":"1", "ARX_C14_ROOT":str(root)},
        stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
    )

    def connect(principal, deadline):
        return Mcp(root, root / "a/clients" / (principal + ".json"), deadline, clients)

    def setup(deadline):
        nonlocal phase, broker_started
        wrapper_ready(wrapper, deadline)
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
        expect(time.monotonic() < deadline, "founder-deadline")
        a = start(root, "a", "create-founder")
        owned.append(a)
        ready(root, "a", a, 4, deadline)
        phase = "mint"
        paste = owner(root, "a", a, 1, deadline)
        expect(paste[:1] == b"\x0b" and paste[1:].startswith(b"arxje1-"), "mint-result")
        key = json.loads(read(root / "a/credentials/identity.json"))["key"]
        expect(len(key) == 32, "source-key")
        configure(root, "b", routes, routes["a"][0], key)
        phase = "broker"
        broker_started = True
        a10["start"](root, phase_end(deadline, 7))
        # The A10 setup validates ancestry and delegation. Its report includes
        # paths and PID, so C14 emits only the fixed qualification facts.
        with open(os.devnull, "w") as sink, redirect_stdout(sink):
            broker_pid = a10["wait_ready"](root, phase_end(deadline, 8))
        private_cgroup(a10["RUNS"])
        print("C14 broker setup=ready runs_owner=0 runs_mode=0700", flush=True)
        phase = "join"
        expect(time.monotonic() < deadline, "join-deadline")
        b = start(root, "b", "join")
        owned.append(b)
        ready(root, "b", b, 1, deadline)
        expect(owner(root, "b", b, 2, deadline, paste[1:]) == b"\x0c", "join-result")
        del paste
        strict = ready(root, "b", b, 3, deadline)
        founder = ready(root, "a", a, 4, deadline)
        expect(founder[1:3] == b"\1\1", "enrollment-dispatch")
        account_counts(root, strict, tags)
        return broker_pid

    def work(deadline):
        try:
            broker_pid = setup(deadline)
        except (TimeoutError, subprocess.TimeoutExpired):
            raise Fault(phase, "timeout") from None
        except PROBE_ERRORS:
            raise Fault(phase, "invalid") from None
        arguments = {"target":2,"workspace":"ordinary","tool":"cargo","args":COMMAND,
                     "timeout_s":12,"output_bytes":1024}
        with checking("principal-control"):
            with connect("foreign", deadline) as ungranted:
                negative_control(ungranted, "run_command", arguments, "principal-control")
        with checking("workspace-control"):
            with connect("fixture", deadline) as agent:
                for slot in REFUSED_SLOTS:
                    negative_control(agent, "run_command", {**arguments, "workspace":slot},
                                     "workspace-control")
        first = build(root, "ordinary", deadline, connect, broker_pid, a10)
        second = build(root, "lost", deadline, connect, broker_pid, a10, first)
        with checking("admission"):
            expect(first[0] != second[0] and first[1] != second[1], "distinct-correlations")
            expect(admissions(root, "b", False) ==
                   [(key.upper(), row, "succeeded") for key, row in (first, second)], "correlated-starts")
            expect(not admissions(root, "a", False), "no-a-admission")

    def cleanup(end):
        mcp_closed = True
        for client in reversed(clients):
            try:
                client.close(end)
            except PROBE_ERRORS:
                mcp_closed = False
        closes = 0
        for process in reversed(owned):
            try:
                if process.poll() is None:
                    process.send_signal(signal.SIGTERM)
                try:
                    closes += process.wait(timeout=left(end - 2, 7)) == 0
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=left(end, 1))
            except PROBE_ERRORS:
                print("C14 cleanup daemon=unconfirmed", flush=True)
        broker_closed = not broker_started
        if broker_started:
            try:
                a10["stop"](root, end, 1, [])
                broker_closed = True
            except PROBE_ERRORS:
                print("C14 cleanup broker=unconfirmed", flush=True)
        print(f"C14 close starts={len(owned)} closes={closes} broker={int(broker_closed)} "
              f"mcp={int(mcp_closed)}", flush=True)
        return len(owned) == closes and broker_closed and mcp_closed

    supervise(wrapper, work, cleanup, evidence, deadline, cleanup_deadline)


def child(root):
    a10 = runpy.run_path(str(root / "a10.py"))
    slot = Path.cwd()
    expect(slot.parent == root / "workspace" and slot.name in SLOTS and sys.argv[1:] == COMMAND,
           "build-command")
    expect(os.getuid() == BUILD and os.getgid() == BUILD and not os.getgroups(), "build-role")
    line = read(Path("/proc/self/cgroup"), 256).decode("ascii").strip()
    leaf = line.removeprefix(MEMBERSHIP)
    expect(a10["LEAF"].fullmatch(leaf) and line == MEMBERSHIP + leaf, "build-cgroup")
    a10["cgroup_denials"](a10["RUNS"] / leaf)
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
    pid = str(os.getpid())
    with (slot / "out/observed").open("x", encoding="ascii") as out:
        out.write(f"{pid} {leaf}\n")
    wait_for_ack(root / "observer" / slot.name / "confirmed", pid, a10["read"],
                 time.monotonic() + 6)
    os.execve(root / "toolchain/bin/cargo", ["cargo", *COMMAND], os.environ)


def wait_for_ack(path, pid, read_ack, end):
    while time.monotonic() < end:
        try:
            ack = read_ack(path, 16, 0)
        except FileNotFoundError:
            time.sleep(.025)
        else:
            expect(ack == pid, "observer-ack")
            return
    raise ValueError("observer-timeout")


def stage_child(root, sources):
    for source, relative, mode in (
        (sources, "probe.py", 0o644),
        (sources.with_name("executor-probe.py"), "a10.py", 0o644),
        (sources, "tools/cargo", 0o755),
    ):
        destination = root / relative
        shutil.copyfile(source, destination)
        destination.chmod(mode)


def stage_workspace(root, a10):
    directory(root / "broker", mode=0o700)
    directory(root / "control", gid=DAEMON, mode=0o710)
    directory(root / "workspace")
    directory(root / "observer")
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
        directory(root / "observer" / name)
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


def staged_scope(root, prepare, remove_role, evidence, end):
    owned = []
    identity = None
    try:
        with checking("staging"):
            root.mkdir(mode=0o755)
            identity = root.lstat()
            root.chmod(0o755)
            prepare(owned)
    except Fault as fault:
        evidence.first(fault)
    if not evidence.close_reported:
        evidence.report_close()
    if identity is None or not (evidence.confirmed_close and evidence.confirmed_ownership):
        print("C14 staging cleanup=retained", flush=True)
        return
    try:
        with checking("cleanup", "unconfirmed"):
            current = root.lstat()
            expect(stat.S_ISDIR(current.st_mode) and
                   (current.st_dev, current.st_ino, current.st_uid) ==
                   (identity.st_dev, identity.st_ino, identity.st_uid), "fixture-ownership")
            deadline = phase_end(end, 12)
            clean = [remove_role(kind, name, uid, deadline) for kind, name, uid in reversed(owned)]
            expect(all(value == "removed" for value in clean), "account-cleanup")
            shutil.rmtree(root)
    except Fault as fault:
        evidence.first(fault)
        print("C14 staging cleanup=unconfirmed", flush=True)
    else:
        print("C14 staging cleanup=removed", flush=True)


def privileged(stage, sources, a10, c13, receipt, end):
    evidence = Evidence()
    try:
        with checking("staging"):
            m = stage.lstat()
            expect(os.geteuid() == 0 and stage.parent == Path("/tmp")
                   and re.fullmatch("arx-c14-[a-z0-9_]{8}", stage.name)
                   and stat.S_ISDIR(m.st_mode) and stat.S_IMODE(m.st_mode) == 0o700
                   and m.st_uid == int(os.environ["SUDO_UID"]), "staging")
            root = Path("/var/lib") / f"arx-c14-{receipt['run']}-{receipt['attempt']}"
            expect(all(a10["role_unused"](name, uid) for name, uid in ROLES), "account-collision")
            deadline = phase_end(end - 26, 40)
            tags = frozen_kinds(sources.parents[3])
            a10["ancestry"](root.parent)
            expect(a10["properties"](deadline)["LoadState"] == "not-found", "unit-collision")
    except Fault as fault:
        evidence.first(fault)
        evidence.report_close()
        return 1

    def prepare(owned):
        for name, uid in ROLES:
            # A failed creation can leave a partial account. Only claim a
            # successfully created object and retain the fixture if uncertain.
            evidence.confirmed_ownership = False
            a10["command"](["/usr/sbin/groupadd", "--gid", str(uid), name], deadline, "group")
            owned.append(("group", name, uid))
            a10["command"](a10["useradd_argv"](name, uid), deadline, "account")
            owned.append(("user", name, uid))
            a10["role"](name, uid)
            evidence.confirmed_ownership = True
        for name in ("arxd", "campaign"):
            c13["stage_binary"](stage / name, root / name, deadline)
            (root / name).chmod(0o755)
        stage_workspace(root, a10)
        stage_child(root, sources)
        stage_toolchain(stage / "toolchains/fixture-x86_64-unknown-linux-gnu", root / "toolchain")
        ip = c13["primary_ip"](deadline)
        campaign(root, ip, tags, a10, phase_end(end - 13, 55), evidence)

    staged_scope(root, prepare, c13["remove_role"], evidence, end)
    if evidence.failed:
        return 1
    print(PASS, flush=True)
    return 0


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


def self_test(workspace):
    from io import BytesIO, StringIO
    from types import SimpleNamespace
    import unittest
    from unittest.mock import MagicMock, Mock, patch

    a10 = runpy.run_path(str(Path(__file__).with_name("executor-probe.py")))

    class ProbeTest(unittest.TestCase):
        def test_staged_children_require_protected_ack_before_cargo(self):
            enter = """
import errno, os, runpy, sys
from pathlib import Path
from unittest.mock import Mock, patch

sys.argv = sys.argv[1:]
probe = runpy.run_path(sys.argv[0])
root = Path(sys.argv[0]).parent.parent
slot = Path.cwd()
helper = runpy.run_path(str(root / 'a10.py'))
leaf = 'arx-42-0'
confirmed = root / 'observer' / slot.name / 'confirmed'

def evidence(path, size, uid):
    assert (path, size, uid) == (confirmed, 16, 0)
    (slot / 'out/waiting').write_bytes(b'')
    return path.read_text(encoding='ascii')

def proc_only(path, size):
    assert path == Path('/proc/self/cgroup'), 'unexpected privileged read'
    return (probe['MEMBERSHIP'] + leaf + '\\n').encode('ascii')

def denied_open(path, flags):
    assert path.is_relative_to(root) and not path.is_relative_to(root / 'workspace')
    raise PermissionError(errno.EACCES, 'private')

def cargo(path, argv, env):
    assert path == root / 'toolchain/bin/cargo' and argv == ['cargo', *probe['COMMAND']]
    (slot / 'out/cargo-called').write_bytes(b'')

denials = Mock()
fds = Mock()
sockets = Mock()
helper.update(read=evidence, cgroup_denials=denials, verify_child_fds=fds, socket_denied=sockets)
with patch.dict(probe['child'].__globals__, {'read': proc_only}), \\
     patch.object(runpy, 'run_path', return_value=helper), \\
     patch.object(os, 'getuid', return_value=probe['BUILD'], create=True), \\
     patch.object(os, 'getgid', return_value=probe['BUILD'], create=True), \\
     patch.object(os, 'getgroups', return_value=[], create=True), \\
     patch.object(os, 'O_NOFOLLOW', 0x20000, create=True), \\
     patch.object(os, 'open', side_effect=denied_open), \\
     patch.object(os, 'rename', side_effect=PermissionError(errno.EACCES, 'private')), \\
     patch.object(os, 'execve', side_effect=cargo):
    probe['main']()
denials.assert_called_once_with(helper['RUNS'] / leaf)
fds.assert_called_once_with()
assert sockets.call_count == 2
"""
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve() / "var/lib/arx-c14-fixture"
                (root / "tools").mkdir(parents=True)
                stage_child(root, Path(__file__).resolve())
                wrapper = [sys.executable, "-I", "-B", "-c", enter, root / "tools/cargo"]
                for name in SLOTS:
                    with self.subTest(slot=name):
                        slot = root / "workspace" / name
                        out = slot / "out"
                        out.mkdir(parents=True)
                        ack = root / "observer" / name
                        ack.mkdir(parents=True)
                        with subprocess.Popen(wrapper + COMMAND, cwd=slot, stderr=subprocess.DEVNULL) as proc:
                            try:
                                end = time.monotonic() + 5
                                while not (out / "waiting").exists() and proc.poll() is None:
                                    self.assertLess(time.monotonic(), end)
                                    time.sleep(.01)
                                self.assertIsNone(proc.poll())
                                self.assertEqual((out / "observed").read_text(encoding="ascii"),
                                                 f"{proc.pid} arx-42-0\n")
                                # An old build-owned release file has no effect.
                                (out / "release").touch()
                                self.assertFalse((out / "cargo-called").exists())
                                a10["publish_observer_ack"](ack, str(proc.pid))
                                self.assertEqual(proc.wait(timeout=5), 0)
                                self.assertTrue((out / "cargo-called").is_file())
                            finally:
                                if proc.poll() is None:
                                    proc.kill()

        def test_missing_or_wrong_ack_fails_closed(self):
            path = Path("confirmed")
            with patch.object(time, "monotonic", side_effect=[0, 2]), patch.object(time, "sleep"):
                missing = Mock(side_effect=FileNotFoundError)
                with self.assertRaisesRegex(ValueError, "observer-timeout"):
                    wait_for_ack(path, "25", missing, 1)
                missing.assert_called_once_with(path, 16, 0)
            with self.assertRaisesRegex(ValueError, "observer-ack"):
                wait_for_ack(path, "25", Mock(return_value="26"), time.monotonic() + 1)

        def test_observer_validates_live_membership_limits_modes_and_credentials(self):
            root = Path("fixture")
            leaf = "arx-42-0"
            runs = a10["RUNS"]
            path = runs / leaf
            witness = root / "workspace/ordinary/out/observed"
            proc = Path("/proc/25/cgroup")
            records = {
                witness: f"25 {leaf}\n", proc: MEMBERSHIP + leaf + "\n",
                path / "cgroup.procs": "25\n", path / "memory.max": str(a10["MEMORY"]),
                path / "pids.max": str(a10["PIDS"]), path / "memory.swap.max": "0",
            }
            credentials = {"Uid":[str(BUILD)] * 4, "Gid":[str(BUILD)] * 4, "Groups":[],
                           "NoNewPrivs":["1"], "CapBnd":[a10["CAPS"]],
                           **{key:["0" * 16] for key in ("CapEff", "CapPrm", "CapInh", "CapAmb")}}
            modes = {directory: SimpleNamespace(st_mode=stat.S_IFDIR | 0o700, st_uid=0, st_gid=0)
                     for directory in (runs, path)}

            def host_read(file, limit=4096, owner=None):
                if file == witness:
                    self.assertEqual((limit, owner), (64, BUILD))
                return records[file]

            helper = {**a10, "read":host_read, "proc_status":Mock(return_value=credentials)}
            with patch.object(type(runs), "lstat", autospec=True, side_effect=modes.__getitem__):
                self.assertEqual(observe(root, "ordinary", 42, helper), ("25", leaf))
                helper["proc_status"].assert_called_once_with("25")
                for file, wrong in (
                    (witness, f"0 {leaf}\n"), (witness, f"2147483648 {leaf}\n"),
                    (witness, f"26 {leaf}\n"), (witness, f"{leaf}\n"),
                    (witness, "25 arx-43-0\n"), (witness, "25 arx-42-../0\n"),
                    (proc, MEMBERSHIP + "arx-42-1"), (path / "cgroup.procs", "125\n"),
                    (path / "memory.max", "max"), (path / "pids.max", "31"),
                    (path / "memory.swap.max", "1"),
                ):
                    with self.subTest(file=file.name, wrong=wrong), patch.dict(records, {file:wrong}):
                        with self.assertRaises(Fault):
                            observe(root, "ordinary", 42, helper)
                for key, correct in credentials.copy().items():
                    with self.subTest(credential=key):
                        for wrong in (["unexpected"], None):
                            with patch.dict(credentials, {key:wrong}):
                                if wrong is None:
                                    del credentials[key]
                                with self.assertRaisesRegex(Fault, "role"):
                                    observe(root, "ordinary", 42, helper)
                        self.assertEqual(credentials[key], correct)
                for directory in modes:
                    for info in ({"st_mode":stat.S_IFDIR | 0o755, "st_uid":0},
                                 {"st_mode":stat.S_IFDIR | 0o700, "st_uid":BUILD},
                                 {"st_mode":stat.S_IFLNK | 0o700, "st_uid":0}):
                        with self.subTest(directory=directory.name, info=info):
                            with patch.dict(modes, {directory:SimpleNamespace(st_gid=0, **info)}):
                                with self.assertRaisesRegex(Fault, "witness"):
                                    observe(root, "ordinary", 42, helper)

        def test_witness_reader_rejects_unsafe_files(self):
            # Check the A10 reader directly so a later observation fault
            # cannot mask an unsafe read. Inject POSIX file operations on Windows.
            frame = b"25 arx-42-0\n"
            path = Path("fixture/workspace/ordinary/out/observed")
            cases = (
                ("valid", None),
                ("oversized", "read-bound"),
                ("unowned", "report-owner"),
                ("hardlink", "report-owner"),
                ("fifo", "read-type"),
                ("symlink", None),
            )
            for kind, refusal in cases:
                with self.subTest(kind=kind):
                    data = frame.ljust(65, b" ") if kind == "oversized" else frame
                    metadata = SimpleNamespace(st_uid=0 if kind == "unowned" else BUILD,
                                               st_nlink=2 if kind == "hardlink" else 1,
                                               st_mode=stat.S_IFIFO if kind == "fifo" else stat.S_IFREG)

                    def open_witness(_path, flags):
                        if kind == "symlink" and flags & os.O_NOFOLLOW:
                            raise OSError(errno.ELOOP, "refused symlink")
                        return 7

                    stream_read = Mock(wraps=BytesIO(data).read)
                    with patch.object(os, "O_NOFOLLOW", 0x20000, create=True), \
                         patch.object(os, "O_NONBLOCK", 0x800, create=True), \
                         patch.object(os, "O_CLOEXEC", 0x80000, create=True), \
                         patch.object(os, "open", side_effect=open_witness), \
                         patch.object(os, "fdopen", return_value=MagicMock()) as wrapped, \
                         patch.object(os, "fstat", return_value=metadata) as fstat:
                        wrapped.return_value.__enter__.return_value.read = stream_read
                        if kind == "symlink":
                            with self.assertRaises(OSError) as caught:
                                a10["read"](path, 64, BUILD)
                            self.assertEqual(caught.exception.errno, errno.ELOOP)
                            fstat.assert_not_called()
                        elif refusal:
                            with self.assertRaisesRegex(a10["Refusal"], f"^{refusal}$"):
                                a10["read"](path, 64, BUILD)
                        else:
                            self.assertEqual(a10["read"](path, 64, BUILD), frame.decode("ascii"))
                        if kind in ("valid", "oversized"):
                            stream_read.assert_called_once_with(65)
                        else:
                            stream_read.assert_not_called()

        def test_both_gates_reject_early_pipe_results_before_witness(self):
            sentinel = "redaction-sentinel"

            def response(result):
                return {"result":{"content":[{"text":json.dumps(result)}]}}

            cases = (
                ({"error":{"message":sentinel}}, ("rpc", "rpc-error")),
                (response({"outcome":"refused"}), ("admission", "refused")),
                (response({"outcome":"failed", "target_activity":{"exit_code":1},
                           "stderr":sentinel}), ("terminal", "failed")),
                (response({"outcome":"unknown"}), ("admission", "unknown")),
                (response({"outcome":"succeeded"}), ("admission", "premature")),
                (response({"outcome":sentinel}), ("admission", "malformed")),
                (b"{", ("rpc", "malformed")),
                (b"", ("rpc", "eof")),
            )
            for slot in SLOTS:
                for frame, expected in cases:
                    with self.subTest(slot=slot, reason=expected):
                        client = Mcp.__new__(Mcp)
                        client.pending = bytearray()
                        client.process = Mock()
                        data = frame if isinstance(frame, bytes) else json.dumps(
                            {"jsonrpc":"2.0", "id":2, **frame}).encode("ascii")
                        if data:
                            data += b"\n"
                        with patch.object(select, "select", return_value=([7], [], [])), \
                             patch.object(os, "read", return_value=data), \
                             patch.dict(gate.__globals__, {"observe":Mock()}) as functions:
                            end = time.monotonic() + 1
                            with self.assertRaises(Fault) as caught:
                                gate(Path("fixture"), slot, client, 42, a10, end, end)
                            self.assertEqual((caught.exception.checkpoint, caught.exception.category),
                                             expected)
                            output = StringIO()
                            with redirect_stdout(output):
                                Evidence().first(caught.exception)
                            self.assertNotIn(sentinel, output.getvalue())
                            functions["observe"].assert_not_called()
                            client.process.kill.assert_not_called()

        def test_negative_control_requires_a_json_refusal(self):
            client = Mock()
            for checkpoint in ("principal-control", "workspace-control", "foreign-status"):
                with self.subTest(checkpoint=checkpoint):
                    client.call.return_value = {"outcome":"refused"}
                    negative_control(client, "probe", {}, checkpoint)
                    client.call.return_value = {"outcome":"failed"}
                    with self.assertRaisesRegex(Fault, checkpoint):
                        negative_control(client, "probe", {}, checkpoint)
            client.call.side_effect = Fault("rpc", "rpc-error")
            with self.assertRaisesRegex(Fault, "rpc-error"):
                negative_control(client, "probe", {}, "principal-control")

        def test_gate_order_and_witness_timeout(self):
            root = Path("fixture")
            for slot in SLOTS:
                with self.subTest(slot=slot):
                    calls = Mock()
                    calls.observe.return_value = ("25", "arx-42-0")
                    calls.client.ready.return_value = False
                    calls.client.process.wait.return_value = -signal.SIGKILL if os.name != "nt" else -9
                    with patch.dict(gate.__globals__, {"observe":calls.observe}), \
                         patch.object(signal, "SIGKILL", 9, create=True), redirect_stdout(StringIO()):
                        end = time.monotonic() + 1
                        leaf = gate(root, slot, calls.client, 42, {"publish_observer_ack":calls.publish,
                                    "MEMORY":a10["MEMORY"], "PIDS":a10["PIDS"]}, end, end)
                    self.assertEqual(leaf, "arx-42-0")
                    self.assertEqual([call[0] for call in calls.mock_calls],
                                     ["client.ready", "observe", "client.ready"] +
                                     (["client.process.kill", "client.process.wait"] if slot == "lost" else []) +
                                     ["publish"])
                    calls.publish.assert_called_once_with(root / "observer" / slot, "25")

                    calls.reset_mock()
                    calls.client.ready.side_effect = [False, True]
                    calls.client.result.return_value = {"outcome":"succeeded"}
                    with patch.dict(gate.__globals__, {"observe":calls.observe}):
                        end = time.monotonic() + 1
                        with self.assertRaisesRegex(Fault, "premature"):
                            gate(root, slot, calls.client, 42, a10, end, end)
                    calls.client.process.kill.assert_not_called()
                    calls.publish.assert_not_called()

                    calls.reset_mock(side_effect=True)
                    calls.observe.return_value = None
                    with patch.object(time, "monotonic", side_effect=[0, 2]), \
                         patch.object(time, "sleep"), patch.dict(gate.__globals__, {"observe":calls.observe}):
                        with self.assertRaisesRegex(Fault, "witness.*timeout"):
                            gate(root, slot, calls.client, 42, a10, 1, 1)
                    calls.client.ready.assert_called_once()
                    calls.observe.assert_called_once()
                    calls.client.process.kill.assert_not_called()

        def test_build_checks_terminal_identity_artifact_and_leaf_removal(self):
            operation = "a" * 64
            leaf = "arx-42-0"
            result = {"outcome":"succeeded", "operation_id":operation, "target_activity":{
                "uid":BUILD, "gid":BUILD, "exit_code":0, "cleanup_confirmed":True, "activity_id":3,
            }}
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                artifact = root / "workspace/ordinary/out/target/debug/c14-fixture"
                artifact.parent.mkdir(parents=True)
                artifact.write_bytes(b"fixture")
                helper = {**a10, "RUNS":root / "runs"}
                client = MagicMock()
                client.__enter__.return_value = client
                client.call.return_value = {"targets":[2]}
                client.result.return_value = result
                lstat = type(root).lstat

                def observed_stat(path):
                    metadata = lstat(path)
                    if path == artifact:
                        return SimpleNamespace(st_mode=metadata.st_mode, st_uid=BUILD)
                    return metadata

                end = time.monotonic() + 5
                with patch.dict(build.__globals__, {"gate":Mock(return_value=leaf)}), \
                     patch.object(type(root), "lstat", observed_stat):
                    self.assertEqual(build(root, "ordinary", end, Mock(return_value=client), 42, helper),
                                     (operation, 3))
                    with patch.dict(result["target_activity"], {"exit_code":1}):
                        with self.assertRaisesRegex(Fault, "terminal"):
                            build(root, "ordinary", end, Mock(return_value=client), 42, helper)
                    (helper["RUNS"] / leaf).mkdir(parents=True)
                    with self.assertRaisesRegex(Fault, "leaf-cleanup"):
                        build(root, "ordinary", end, Mock(return_value=client), 42, helper)
                    artifact.unlink()
                    with self.assertRaisesRegex(Fault, "artifact"):
                        build(root, "ordinary", end, Mock(return_value=client), 42, helper)

        ordinary_operation, lost_operation = "a" * 64, "b" * 64
        spent = [(ordinary_operation.upper(), 10, "succeeded"), (lost_operation.upper(), 11, "succeeded")]
        pending_source = [(ordinary_operation.upper(), 20, "succeeded"), (lost_operation.upper(), 21, "started")]
        completed_source = [pending_source[0], (lost_operation.upper(), 21, "succeeded")]
        pending_list = {"operations":[lost_operation]}
        scoped_result = {"operation_id":lost_operation, "outcome":"succeeded"}
        read_source, read_target = ("a", True), ("b", False)
        list_call = ("fixture", {"target":2})
        by_id = ("fixture", {"target":2,"operation_id":lost_operation})
        foreign_by_id = ("foreign", by_id[1])

        def check_lost_script(self, script, fault=None):
            remaining = iter(script)

            def reply(request):
                step = next(remaining, None)
                self.assertIsNotNone(step, f"unexpected request: {request}")
                expected, value = step
                self.assertEqual(request, expected)
                if isinstance(value, Fault):
                    raise value
                return value

            def admitted(_root, node, forwarded):
                return reply((node, forwarded))

            @contextmanager
            def connect(principal, _deadline):
                def status(tool, args):
                    self.assertEqual(tool, "delegated_build_status")
                    return reply((principal, args))

                yield SimpleNamespace(call=status)

            with patch.dict(recover_lost.__globals__, {"admissions":admitted}), \
                 patch.object(time, "monotonic", return_value=0):
                def recover():
                    return recover_lost(Path("fixture"), connect, 5, (self.ordinary_operation, 10))

                if fault:
                    with self.assertRaises(Fault) as caught:
                        recover()
                    self.assertEqual((caught.exception.checkpoint, caught.exception.category), fault)
                else:
                    self.assertEqual(recover(), self.scoped_result)
            self.assertIsNone(next(remaining, None), "recovery stopped before the script completed")

        def test_lost_oracle_recovers_pending_source_and_list_time_completion(self):
            for after_list, listed in ((self.pending_source, self.pending_list),
                                       (self.completed_source, self.pending_list),
                                       (self.completed_source, {"operations":[]})):
                with self.subTest(after_list=after_list, listed=listed):
                    self.check_lost_script([
                        (self.read_target, self.spent), (self.read_source, self.pending_source),
                        (self.list_call, listed), (self.read_source, after_list),
                        (self.foreign_by_id, {"outcome":"refused"}), (self.by_id, self.scoped_result),
                        (self.read_target, self.spent), (self.read_source, self.completed_source),
                    ])

        def test_lost_oracle_rejects_invalid_forwarded_lists_before_status(self):
            stale_list = {"operations":[self.lost_operation, self.ordinary_operation]}
            invalid_lists = (
                stale_list,
                {"operations":[self.lost_operation, self.lost_operation]},
                {"operations":["c" * 64]},
                {"operations":self.lost_operation},
                {"operations":[self.lost_operation], "extra":True},
                {}, {"outcome":"unknown"}, {"outcome":"refused"}, {"operations":[]},
            )
            ordinary, lost = self.pending_source
            invalid_sources = (
                [ordinary], [ordinary, lost, ("C" * 64, 22, "started")], [lost, ordinary],
                [("C" * 64, 20, "succeeded"), lost], [(ordinary[0], 22, "succeeded"), lost],
                [ordinary, ("C" * 64, 21, "started")], [ordinary, (lost[0], 22, "started")],
                [(*ordinary[:2], "started"), lost], [ordinary, (*lost[:2], "interrupted")],
            )
            cases = [(listed, self.pending_source) for listed in invalid_lists]
            cases.extend((self.pending_list, source) for source in invalid_sources)
            cases.extend([
                (stale_list, self.completed_source),
                ({"outcome":"unknown"}, self.completed_source),
                ({"operations":[]}, [ordinary, (lost[0], 22, "succeeded")]),
            ])
            for listed, after_list in cases:
                with self.subTest(listed=listed, after_list=after_list):
                    self.check_lost_script([
                        (self.read_target, self.spent), (self.read_source, self.pending_source),
                        (self.list_call, listed), (self.read_source, after_list),
                    ], ("forwarded-list", "invalid"))

        def test_lost_oracle_preserves_forwarded_list_wire_faults(self):
            for category in ("malformed", "eof", "rpc-error", "timeout"):
                with self.subTest(category=category):
                    self.check_lost_script([
                        (self.read_target, self.spent), (self.read_source, self.pending_source),
                        (self.list_call, Fault("rpc", category)),
                    ], ("rpc", category))

        def test_lost_oracle_rejects_precompleted_source_and_invalid_status_evidence(self):
            extra = self.spent + [(self.lost_operation.upper(), 12, "succeeded")]
            changed = [self.spent[0], (self.lost_operation.upper(), 12, "succeeded")]
            before_list = [(self.read_target, self.spent), (self.read_source, self.pending_source),
                           (self.list_call, self.pending_list), (self.read_source, self.pending_source)]
            after_foreign = before_list + [(self.foreign_by_id, {"outcome":"refused"})]
            after_status = after_foreign + [(self.by_id, self.scoped_result)]
            ordinary, lost = self.completed_source
            changed_ordinary = [(*ordinary[:2], "started"), lost]
            cases = (
                ("prior-spend", [(self.read_target, extra)], ("terminal", "invalid")),
                ("precompleted-source", [(self.read_target, self.spent), (self.read_source, self.completed_source)],
                 ("terminal", "invalid")),
                ("foreign-success", before_list + [(self.foreign_by_id, self.scoped_result)],
                 ("foreign-status", "succeeded")),
                ("extra-b", after_status + [(self.read_target, extra)], ("terminal", "invalid")),
                ("changed-b", after_status + [(self.read_target, changed)], ("terminal", "invalid")),
                ("wrong-status-id", after_foreign + [(self.by_id, dict(self.scoped_result,
                    operation_id=self.ordinary_operation)), (self.read_target, self.spent)], ("terminal", "invalid")),
                ("missing-a-terminal", after_status + [(self.read_target, self.spent),
                    (self.read_source, self.pending_source)], ("terminal", "invalid")),
                ("changed-a-terminal", after_status + [(self.read_target, self.spent),
                    (self.read_source, [self.completed_source[0], (self.lost_operation.upper(), 22, "succeeded")])],
                 ("terminal", "invalid")),
                ("changed-a-ordinary", after_status + [(self.read_target, self.spent),
                    (self.read_source, changed_ordinary)], ("terminal", "invalid")),
            )
            for name, script, fault in cases:
                with self.subTest(name=name):
                    self.check_lost_script(script, fault)

        def test_frozen_tag_kinds_and_admission_sequence(self):
            tags = frozen_kinds(workspace)
            self.assertEqual(tags, {"Blank": 0, "Mint": 1, "Membership": 3,
                                    "Found": 4, "Remove": 5, "Redeem": 6})

            def seal(index, tag):
                position = struct.pack(">QQQ", 1, 1, index)
                body = b"ARXLE2\0\0" + position + bytes([tag])
                return index, body + hashlib.sha256(body).digest()

            kinds = ("Membership", "Blank", "Found", "Mint", "Redeem", "Membership")
            records = [seal(index, tags[kind]) for index, kind in enumerate(kinds, 1)]
            self.assertEqual(admission_membership(records, tags), struct.pack(">QQQ", 1, 1, 6))
            for refused in (records[:3] + [seal(4, tags["Remove"])] + records[4:],
                            records + [seal(7, tags["Remove"])]):
                with self.assertRaisesRegex(ValueError, "invocation-count"):
                    admission_membership(refused, tags)

        def test_timeout_reaps_wrapper_before_cleanup_and_preserves_first_failure(self):
            # This child performs no campaign work. It wedges after the success byte.
            wrapper = subprocess.Popen(
                [sys.executable, "-I", "-B", "-c",
                 "import sys,time; sys.stdin.buffer.read(1); time.sleep(30)"],
                stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            )
            events = []
            evidence = Evidence()
            output = StringIO()

            def cleanup(_deadline):
                self.assertIsNotNone(wrapper.returncode)
                events.append("cleanup")
                raise ValueError("redaction-sentinel")

            try:
                end = time.monotonic() + .2
                with redirect_stdout(output):
                    supervise(wrapper, lambda _: None, cleanup, evidence, end, end + 2)
                self.assertEqual(events, ["cleanup"])
                self.assertEqual(output.getvalue().count("C14 first"), 1)
                self.assertIn("checkpoint=campaign category=timeout", output.getvalue())
                self.assertIn("process closure=unconfirmed", output.getvalue())
                self.assertNotIn("redaction-sentinel", output.getvalue())
                self.assertTrue(evidence.failed)
                self.assertFalse(evidence.confirmed_close)
                self.assertNotEqual(wrapper.returncode, 0)
            finally:
                reap_wrapper(wrapper, time.monotonic() + 2)

        def test_staging_uses_confirmed_closure_even_after_campaign_failure(self):
            for closed, ownership, removal in ((True, True, True), (False, True, False),
                                               (True, False, False), (True, True, False)):
                with self.subTest(closed=closed, ownership=ownership, removal=removal), \
                     tempfile.TemporaryDirectory() as temp:
                    root = Path(temp) / "fixture"
                    evidence = Evidence()
                    wrapper = MagicMock()
                    wrapper.poll.return_value = 0
                    remove = Mock(return_value="removed" if removal else "retained")
                    output = StringIO()
                    end = time.monotonic() + 5

                    def prepare(owned):
                        owned.extend((("group", "fixture", BUILD), ("user", "fixture", BUILD)))
                        evidence.confirmed_ownership = ownership

                        def work(_):
                            raise Fault("witness", "timeout")

                        supervise(wrapper, work, lambda _: closed, evidence, end, end)

                    with redirect_stdout(output):
                        staged_scope(root, prepare, remove, evidence, end)
                    self.assertEqual(evidence.confirmed_close, closed)
                    self.assertTrue(evidence.failed)
                    self.assertEqual(output.getvalue().count("C14 first"), 1)
                    self.assertIn("checkpoint=witness category=timeout", output.getvalue())
                    self.assertEqual(root.exists(), not removal)
                    self.assertEqual([call.args[:3] for call in remove.call_args_list],
                                     [("user", "fixture", BUILD), ("group", "fixture", BUILD)]
                                     if closed and ownership else [])

    suite = unittest.defaultTestLoader.loadTestsFromTestCase(ProbeTest)
    if not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful():
        raise SystemExit(1)


def main():
    here = Path(__file__).resolve()
    if sys.argv[1:] == ["--self-test"]:
        return self_test(here.parents[3])
    if here.name == "cargo":
        return child(here.parent.parent)
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
    if sys.argv[1:2] == ["--staged"] and len(sys.argv) == 4:
        end = int(sys.argv[3]) / 1000
        expect(0 < end - time.monotonic() <= 95, "staged-deadline")
        return privileged(Path(sys.argv[2]), here, a10, c13, receipt, end)
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
        end = time.monotonic() + 95
        completed = subprocess.run(["/usr/bin/sudo", "-n", "/usr/bin/env",
                        *[f"{k}={v}" for k, v in zip(PROVENANCE, facts)],
                        "/usr/bin/python3", "-I", "-B", here, "--staged", stage,
                        str(int((end - 1) * 1000))],
                       env=ENV, stdin=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       timeout=left(end, 95))
        return completed.returncode


if __name__ == "__main__":
    try:
        sys.exit(main())
    except PROBE_ERRORS:
        stream = sys.stdout if sys.argv[1:2] == ["--staged"] else sys.stderr
        print("C14 first checkpoint=launcher category=invalid", file=stream, flush=True)
        sys.exit(1)
