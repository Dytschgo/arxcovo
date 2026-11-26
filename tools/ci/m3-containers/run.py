#!/usr/bin/env python3
"""Closed hosted entry points for inert R2 qualification and live C16 acceptance."""

import argparse
from collections import Counter
from contextlib import closing, contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import select
import sqlite3
import struct
import stat
import subprocess
import sys
import time

# Python isolated mode removes the script directory from sys.path.
sys.path.insert(0, str(Path(__file__).resolve().parent))

from fixture import (ANCHOR_TEST, ENV, HERE, LIVE_TIME, MIB, ROOT, TIME, TOOLS, Refusal, bounded_read, capture,
                     check_host_profiles, file_digest, host_command, load_pins, plain_path, require, select_daemon,
                     stage_copy, stage_rootfs, verify_manifest, vet_tools, build_descriptor, install_live,
                     node_endpoint, open_artifact, remaining, select_anchor_probe)
from isolation import Resources, check_host_routes, json_rows, stable_rules

REPO = "Dytschgo/arxcovo"
FIXED = {
    "CI": "true", "GITHUB_ACTIONS": "true", "RUNNER_OS": "Linux", "RUNNER_ARCH": "X64",
    "RUNNER_ENVIRONMENT": "github-hosted", "GITHUB_REPOSITORY": REPO, "GITHUB_JOB": "rust",
    "GITHUB_WORKFLOW": "CI",
}
FIELDS = (
    "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_SHA", "GITHUB_WORKFLOW_REF", "GITHUB_WORKFLOW_SHA",
    "GITHUB_WORKSPACE", "RUNNER_TEMP", "CARGO_TARGET_DIR", "ImageOS", "ImageVersion",
)
ERRORS = (OSError, ValueError, KeyError, TypeError, IndexError, sqlite3.Error, subprocess.SubprocessError)


def provenance(env, host, checkout):
    require(all(env.get(name) == value for name, value in FIXED.items()), "hosted-profile")
    run, attempt, head = (env.get(name, "") for name in FIELDS[:3])
    require(re.fullmatch(r"[1-9][0-9]{0,14}", run) and re.fullmatch(r"[1-9][0-9]{0,3}", attempt)
            and re.fullmatch(r"[a-f0-9]{40}", head), "run-identity")
    workflow = env.get("GITHUB_WORKFLOW_REF", "")
    require(re.fullmatch(re.escape(REPO) + r"/\.github/workflows/ci\.yml@refs/(heads/main|pull/[1-9][0-9]{0,14}/merge)",
                         workflow) and env.get("GITHUB_WORKFLOW_SHA") == head == checkout,
            "source-workflow-head")
    all_profiles = check_host_profiles(host["profiles"], image_category="runner-image",
                                       variant_category="runner-kernel-os")
    profiles = [profile for profile in all_profiles
                if (profile["image_os"], profile["image_version"])
                == (env.get("ImageOS"), env.get("ImageVersion"))]
    require(len(profiles) == 1, "runner-image")
    profile = profiles[0]
    return {"repo": REPO, "head": head, "run": run, "attempt": attempt, "job": "rust (ubuntu-latest)",
            "image_os": profile["image_os"], "image_version": profile["image_version"]}, profile


def require_userland(pins):
    require(pins["rootfs"]["userland"] == pins["host"]["os"], "offline-userland-input")


def os_version(file):
    lines = bounded_read(file, 8192).decode("ascii").splitlines()
    values = {}
    for line in lines:
        match = re.fullmatch(r'(ID|VERSION_ID)=(?:"([a-z0-9.]+)"|([a-z0-9.]+))', line)
        if match:
            require(match[1] not in values, "runner-os-duplicate")
            values[match[1]] = match[2] or match[3]
    require(values.get("ID") == "ubuntu", "runner-os")
    return values.get("VERSION_ID")


def checked_runner_platform(env, host, head, end, tools):
    receipt, profile = provenance(env, host, head)
    uname = os.uname()
    variants = [variant for variant in profile["variants"] if variant["kernel"] == uname.release]
    require(os_version(Path("/usr/lib/os-release")) == host["os"]
            and uname.machine == "x86_64" and len(variants) == 1, "runner-kernel-os")
    variant = variants[0]
    systemd = host_command("dpkg", end, "-W", "-f=${Version}", "systemd", tools=tools).strip()
    require(systemd == variant["systemd_package"], "runner-systemd")
    receipt.update(os=host["os"], kernel=uname.release, systemd=systemd)
    return receipt


def checked_metadata(env, pins, uid, end, *, live=False):
    require(sys.platform == "linux" and uid > 0, "hosted-entry")
    host = pins["host"]
    workspace = Path(env.get("GITHUB_WORKSPACE", ""))
    scratch = Path(env.get("RUNNER_TEMP", ""))
    require(workspace == ROOT and workspace.parent.parent / "_temp" == scratch, "checkout-location")
    plain_path(workspace, (0, uid), directory=True)
    plain_path(scratch, (0, uid), directory=True)
    require_userland(pins)
    vetted = vet_tools(host["tools"])
    head = host_command("git", end, "-c", f"safe.directory={workspace}", "-C", workspace,
                        "rev-parse", "HEAD", tools=vetted).strip()
    receipt = checked_runner_platform(env, host, head, end, vetted)
    tools = {}
    for name in ("cc", "nft", "ip", "mkfs", "loop", "unit", "mount", "umount"):
        argument = "-V" if name in ("ip", "mkfs") else "--version"
        output = host_command(name, end, argument, tools=vetted, limit=8192)
        first = output.splitlines()[0].strip()
        require(re.fullmatch(r"[\w .,+:/()'#~=-]{1,160}", first), "runner-tool-version")
        tools[name] = first
    receipt["tools"] = tools
    stats = os.statvfs("/var/lib")
    require(stats.f_bavail * stats.f_frsize >= 4*1024*MIB, "runner-disk")
    controllers = bounded_read(Path("/sys/fs/cgroup/cgroup.controllers"), 512).decode("ascii").split()
    require({"memory", "pids", "cpu"} <= set(controllers)
            and stat.S_ISCHR(Path("/dev/loop-control").stat().st_mode)
            and all((Path("/proc/self/ns") / item).is_symlink()
                    for item in ("cgroup", "mnt", "net", "pid", "ipc", "uts")), "kernel-prerequisites")
    source = select_daemon(env.get("CARGO_TARGET_DIR", ""), workspace, uid,
                           scratch / "arx-bound-sqlite", receipt, end)
    routes = host_command("ip", end, "-j", "-4", "route", "show", "table", "all", tools=vetted)
    check_host_routes(json_rows(routes))
    anchor = select_anchor_probe(env.get("CARGO_TARGET_DIR", ""), workspace, uid,
                                 scratch / "arx-bound-sqlite", receipt, end) if live else None
    home = Path(__import__("pwd").getpwuid(uid).pw_dir)
    toolchain = home / ".rustup/toolchains" / host["rust"]
    plain_path(toolchain, (0, uid), directory=True)
    sources = {}
    for name in ("run.py", "isolation.py", "fixture.py", "bootstrap.c", "pins.json"):
        path = HERE / name
        digest = file_digest(path, end, MIB, (0, uid))
        revision = host_command("git", end, "-c", f"safe.directory={workspace}", "-C", workspace,
                                "show", f"{head}:tools/ci/m3-containers/{name}", tools=vetted, limit=MIB)
        require(hashlib.sha256(revision.encode("ascii")).hexdigest() == digest, "source-module-digest")
        sources[name] = digest
    receipt["modules"] = sources
    return (receipt, source, toolchain, vetted, anchor) if live else (receipt, source, toolchain, vetted)


class Qualification:
    def __init__(self, mode="dry"):
        self.resources = None
        self.phase_name = "setup" if mode == "live" else "preflight"
        self.budgets = LIVE_TIME if mode == "live" else TIME
        self.end = time.monotonic() + sum(self.budgets.values())
        self.receipt = {"mode": mode, "durations_seconds": dict.fromkeys(self.budgets),
                        "first": None, "cleanup": "pending"}

    def phase(self, name, callback):
        self.phase_name = name
        start = time.monotonic()
        end = min(self.end, start + self.budgets[name])
        try:
            result = callback(end)
            require(time.monotonic() < end, "phase-deadline")
            return result
        finally:
            self.receipt["durations_seconds"][name] = round(time.monotonic() - start, 3)

    def run(self, preflight, stage):
        try:
            identity, assets, resources = self.phase("preflight", preflight)
            self.resources = resources
            self.receipt["runner"] = identity
            self.receipt["images"] = self.phase("stage", lambda end: stage(resources, assets, end))
            self.phase("contain", resources.contain)
            self.phase("network", resources.network)
            self.receipt["containment"] = resources.facts
        except ERRORS as error:
            self.latch(error)
        try:
            errors = self.phase("cleanup", self.resources.cleanup if self.resources else lambda _end: [])
            self.receipt["cleanup"] = "confirmed" if not errors else "incomplete"
            if errors:
                self.receipt["cleanup_errors"] = errors
                self.latch(Refusal("partial-cleanup"))
        except ERRORS as error:
            self.receipt["cleanup"] = "incomplete"
            self.latch(error)
        diagnostic = getattr(self.resources, "failure_diagnostic", None)
        if diagnostic is not None:
            self.receipt["diagnostic"] = diagnostic
        return self.receipt

    def latch(self, error):
        if self.receipt["first"] is None:
            category = str(error) if isinstance(error, Refusal) else type(error).__name__
            require(re.fullmatch(r"[A-Za-z0-9-]{1,80}", category), "error-category")
            self.receipt["first"] = {"phase": self.phase_name, "category": category}


def stage(resources, assets, end, *, anchor=None):
    pins, artifact, toolchain, uid, sources, vetted = assets
    daemon, expected_digest = artifact
    resources.arena()
    require(stage_copy(HERE / "bootstrap.c", resources.root / "bootstrap.c", end, (0, uid), limit=MIB)
            == sources["bootstrap.c"], "helper-digest")
    tree = resources.root / "tree"
    manifest, manifest_digest = stage_rootfs(pins, tree, resources.root, daemon, toolchain, uid, end, vetted, anchor=anchor)
    require(manifest["files"]["fixture/arxd"] == expected_digest, "artifact-digest-drift")
    mounted = resources.volume("root", tree, end)
    image = resources.root / "root.ext4"
    digest = file_digest(image, end, pins["rootfs"]["bytes"])
    verify_manifest(mounted, manifest, manifest_digest, end)
    resources.stage_data(end)
    images = {"rootfs_sha256": digest, "manifest_sha256": manifest_digest,
              "artifact_sha256": manifest["files"]["fixture/arxd"], "bootstrap_sha256": manifest["files"]["fixture/probe"]}
    if anchor:
        images["anchor_sha256"] = manifest["files"]["fixture/c16-anchor"]
        images["data"] = {node: [resources.budget["data_bytes"], *image_id(resources.root / f"{node}.ext4")]
                          for node in "abc"}
    return images


def dry_run(env, uid):
    def preflight(end):
        pins = load_pins()
        receipt, daemon, tools, vetted = checked_metadata(env, pins, uid, end)
        return receipt, (pins, daemon, tools, uid, receipt["modules"], vetted), Resources(receipt, pins, vetted)
    return Qualification().run(preflight, stage)



def image_id(path):
    metadata = path.stat(follow_symlinks=False)
    require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1 and metadata.st_uid == 0,
            "persistent-image")
    return metadata.st_dev, metadata.st_ino


def read_checked(path, owners, limit):
    with open_artifact(path, (0, *owners), limit) as (stream, info):
        data = stream.read(info.st_size + 1)
        require(len(data) == info.st_size and info.st_uid in owners and info.st_nlink == 1, "checked-read-size")
        return data


def sql_rows(path, query):
    plain_path(path, (0, 21071), directory=False)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and before.st_uid == 21071
                and 0 < before.st_size <= 16*MIB and os.path.samestat(path.lstat(), before), "sqlite-audit-file")
        with closing(sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=0.15)) as db:
            db.setlimit(sqlite3.SQLITE_LIMIT_LENGTH, 17000)
            rows = db.execute(query).fetchmany(65)
        require(os.path.samestat(path.lstat(), before) and len(rows) < 65, "sqlite-audit-bound")
        return rows
    finally:
        os.close(fd)


def quiescent_data(data):
    for stem in ("cluster-state", "cluster-log"):
        for suffix in ("-journal", "-wal", "-shm"):
            path = data / "state" / (stem + ".sqlite3" + suffix)
            try:
                facts = path.lstat()
            except FileNotFoundError:
                continue
            require(stat.S_ISREG(facts.st_mode) and facts.st_uid == 21071 and facts.st_nlink == 1
                    and facts.st_size == 0, "recovery-journal")


def checked_blob(raw, magic, limit):
    require(isinstance(raw, bytes) and len(magic) + 32 <= len(raw) <= limit
            and raw.startswith(magic) and hashlib.sha256(raw[:-32]).digest() == raw[-32:],
            "campaign-blob")
    return raw[:-32]


def position(raw):
    require(len(raw) == 24, "log-position")
    return struct.unpack(">QQQ", raw)


def covers(applied, target):
    return applied == target or (applied[2] > target[2] and
                                 (applied[0] > target[0] or applied[:2] == target[:2]))


def log_audit(data):
    path = data / "state/cluster-log.sqlite3"
    rows = sql_rows(path, "SELECT idx,body FROM log ORDER BY idx LIMIT 65")
    counts = Counter()
    membership = []
    mints = []
    for index, raw in rows:
        body = checked_blob(raw, b"ARXLE2\0\0", 4163)
        require(len(body) >= 33, "log-body")
        pos = position(body[8:32])
        kind = body[32]
        require(pos[2] == index and kind in (0, 1, 3, 4, 6), "log-kind")
        counts[kind] += 1
        if kind == 1:
            mints.append(pos)
        if kind == 3:
            require(len(body) >= 35 and int.from_bytes(body[33:35], "big") == len(body) - 35,
                    "membership-body")
            membership.append((pos, body[35:]))
    [(raw,)] = sql_rows(path, "SELECT blob FROM meta WHERE singleton=1 LIMIT 2")
    meta = checked_blob(raw, b"ARXLM1\0\0\0\x02", 256)
    require(len(meta) >= 45 and meta[26] == meta[43] == 1 and meta[44] == 0, "committed-vote")
    vote = struct.unpack(">QQ", meta[27:43])
    counts.pop(0, None)
    return counts, membership, mints, vote


def expected_membership():
    shapes = (([1],), ([1],), ([1],), ([1], [1, 2, 3]), ([1, 2, 3],))
    expected = []
    for i, sets in enumerate(shapes):
        raw = bytearray([1, len(sets)])
        for voters in sets:
            raw.append(len(voters))
            for voter in voters:
                raw.extend(voter.to_bytes(8, "big"))
        raw.append(min(i + 1, 3))
        for voter, node in enumerate("abc"[:i+1], 1):
            address = node_endpoint(node, "peer").encode("ascii")
            raw.extend(voter.to_bytes(8, "big") + bytes([len(address)]) + address)
        expected.append(bytes(raw))
    return expected


def time_evidence(raw):
    require(len(raw) == 53 and raw[0] == 1 and raw[4] == 1, "owner-clock-evidence")
    ordinal, outcome, sample_kind = raw[1:4]
    sample, checkpoint = struct.unpack(">QQ", raw[5:21])
    handle = raw[21:]
    require(checkpoint > 0, "owner-clock-zero")
    if ordinal:
        require(sample_kind == 1 and 0 < sample <= checkpoint and any(handle), "owner-query-clock")
    else:
        require(outcome == sample == sample_kind == 0 and not any(handle), "owner-query-absent")
    return ordinal, outcome, sample, checkpoint, handle


def anchor_client_status(stdout):
    # Single-threaded libtest writes the name prefix before running the test.
    # Parallel libtest prints it on a separate line after the test's output.
    prefix = b"test " + ANCHOR_TEST.encode("ascii") + b" ... "
    outcomes = {"input-refused", "anchor-received", "anchor-refused", "anchor-unconfirmed", "deadline"}
    allowed = {word.encode("ascii"): word for word in outcomes}
    markers = []
    summaries = []
    for line in stdout.splitlines():
        message = line.removeprefix(prefix)
        if message.startswith(b"C16_ANCHOR "):
            markers.append(message.removeprefix(b"C16_ANCHOR "))
        if line.startswith(b"test result:"):
            summaries.append(line)
    outcome = allowed.get(markers[0], "missing") if len(markers) == 1 else "missing"
    summary = None
    if len(summaries) == 1:
        summary = re.fullmatch(rb"test result: ok\. ([0-9]+) passed; 0 failed; 0 ignored; .+", summaries[0])
    count = {b"0": "0", b"1": "1"}.get(summary[1], "other") if summary else "other"
    return outcome, count


def anchor_owner_delta(before, after):
    categories = {0: "absent", 1: "accepted", 2: "refused", 3: "busy", 4: "unavailable"}
    delta, category = "missing", "missing"
    if after is not None and len(after) == 53 and after[0] == 1:
        ordinal, outcome = after[1:3]
        delta = {0: "0", 1: "1"}.get(ordinal - before[0], "other")
        category = categories.get(outcome, "other")
    return {"ordinal_delta": delta, "before": categories.get(before[1], "other"), "after": category}


def check_reconnect(before, after, mint_clock, start_epoch, end_epoch, counts):
    require(before[:2] == (1, 1) and after[:2] == (2, 2)
            and before[4] == after[4] and mint_clock <= before[2] <= after[2]
            and after[3] - mint_clock < 780_000 and start_epoch == end_epoch
            and start_epoch[1] == 1 and counts == (2, 1), "consumed-anchor-evidence")


def ledger_slots(receipts):
    require(len(receipts) in (2, 4) and all(len(slot) == 56 for slot in receipts), "ledger-slot-width")
    indexes = [position(slot[:24])[2] for slot in receipts]
    require(indexes == sorted(set(indexes)) and all(0 < i < 2**63 for i in indexes)
            and len({slot[24:] for slot in receipts}) == len(receipts), "ledger-slot-identity")
    return bytes((1, len(receipts))) + b"".join(receipts).ljust(224, b"\0")


LEDGER_RETRIES = 3
LEDGER_BACKOFF = 0.04
LEDGER_PEER = 2


class LedgerRefusal(Refusal):
    def __init__(self, category, predicate):
        super().__init__(category)
        self.predicate = predicate


def ledger_require(condition, predicate, category="ledger-checkpoint"):
    if not condition:
        raise LedgerRefusal(category, predicate)


def decode_ledger_checkpoint(raw, evidence):
    evidence["width"] = min(len(raw), 165)
    ledger_require(len(raw) == 164, "width")
    member, epoch, leader = struct.unpack(">QQQ", raw[4:28])
    voters = struct.unpack(">QQQ", raw[140:])
    membership, applied = position(raw[92:116]), position(raw[116:140])
    evidence.update(status=raw[1], role=raw[2], member=member, epoch=(epoch, leader), voters=voters,
                    membership=membership, applied=applied)
    ledger_require(raw[0] == 1, "version")
    ledger_require(raw[1] in (LEDGER_PEER, 3, 4, 5), "status")
    ledger_require(raw[2] in (1, 2), "role")
    ledger_require(raw[3] in (0, 1), "custody-code")
    ledger_require(voters == (1, 2, 3), "voters")
    ledger_require(member in voters, "voter-member")
    ledger_require(leader in voters, "voter-leader")
    ledger_require((raw[2] == 1) == (leader == member), "leader-role")
    ledger_require(covers(applied, membership), "applied-membership")
    ledger_require(any(raw[28:44]), "installation-id")
    ledger_require(any(raw[60:92]), "installation-key-digest")
    return {"node": member, "epoch": (epoch, leader), "member_pos": membership, "applied": applied,
            "identity": raw[28:92], "custody": bool(raw[3]), "status": raw[1]}


def check_checkpoint_set(observations, nodes, receipts, identities=None):
    ledger_require(set(observations) == set(nodes), "node-inventory", "ledger-node-inventory")
    records = list(observations.values())
    epoch, membership = records[0]["epoch"], records[0]["member_pos"]
    ledger_require(all(r["epoch"] == epoch for r in records), "shared-epoch", "ledger-consistency")
    ledger_require(all(r["member_pos"] == membership for r in records), "shared-membership", "ledger-consistency")
    ledger_require(all(r["identity"][16:32] == bytes([16]) * 16 for r in records), "cluster-id", "ledger-consistency")
    ledger_require(len({r["identity"][:16] for r in records}) == len(records), "unique-installation", "ledger-consistency")
    for node, record in observations.items():
        ledger_require(record["node"] == "abc".index(node) + 1, "expected-member", "ledger-node-proof")
        ledger_require(record["custody"] == (node != "a"), "expected-custody", "ledger-node-proof")
        ledger_require(all(covers(record["applied"], position(slot[:24])) for slot in receipts),
                       "applied-mints", "ledger-node-proof")
        if identities is not None:
            ledger_require(record["identity"] == identities[node], "expected-installation", "ledger-installation-drift")
    return epoch, membership


def build_output_excerpt(output):
    if not isinstance(output, dict):
        return ""
    lines = []
    for name in ("stderr", "stdout"):
        stream = output.get(name)
        if not isinstance(stream, dict):
            continue
        for section in ("head", "tail"):
            value = stream.get(section)
            if isinstance(value, str):
                lines.extend(value[:1024].splitlines())
    failures = [line for line in lines if any(token in line for token in (
        "fatal error", "error while loading shared libraries", "cannot find",
        "No such file or directory", "Too many open files", "signal: 9",
    ))]
    excerpt = " ".join(failures or lines)
    excerpt = re.sub(r"(?i)[0-9a-f]{32,}", "redacted", excerpt)
    return re.sub(r"[^A-Za-z0-9 _.:/()=-]+", " ", excerpt).strip()[:256]


def build_result_diagnostic(result, target):
    outcome = result.get("outcome")
    allowed = ("started", "succeeded", "failed", "rejected", "refused", "interrupted", "unknown")
    fields = {"activity_id", "cleanup_confirmed", "exit_code", "gid", "outcome", "uid"}
    diagnostic = {"outcome": outcome if outcome in allowed else "unknown",
                  "cleanup_confirmed": None, "keys": sorted(target.keys() & fields),
                  "output": build_output_excerpt(result.get("output"))}
    for name, minimum, maximum in (("exit_code", -(2**31), 2**31 - 1), ("activity_id", 0, 2**63 - 1),
                                    ("uid", 0, 2**32 - 1), ("gid", 0, 2**32 - 1)):
        value = target.get(name)
        diagnostic[name] = value if type(value) is int and minimum <= value <= maximum else None
    cleanup = target.get("cleanup_confirmed")
    if type(cleanup) is bool:
        diagnostic["cleanup_confirmed"] = cleanup
    return diagnostic


def activity(path, forwarded):
    query = ("SELECT hex(d.operation),a.id,a.outcome FROM delegated_build d "
             "JOIN activity a ON a.id=d.activity_id "
             f"WHERE d.forwarded={int(forwarded)} ORDER BY a.id LIMIT 3")
    return sql_rows(path / "activity/activity.sqlite3", query)


@contextmanager
def child(resources, node, action, end, **streams):
    process = resources.spawn_live(node, action, end, **streams)
    try:
        yield process
    finally:
        if process.stdin is not None:
            process.stdin.close()
        try:
            process.wait(timeout=max(0.05, min(1, end - time.monotonic())))
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=1)
        finally:
            if process.stdout is not None:
                process.stdout.close()


def write_fd(fd, payload, end):
    with memoryview(payload) as view:
        total = 0
        while total < len(view):
            require(bool(select.select([], [fd], [], remaining(end))[1]), "write-deadline")
            try:
                count = os.write(fd, view[total:])
            except BlockingIOError:
                continue
            require(count > 0, "write-short")
            total += count


def child_reply(resources, node, action, end, data=b"", limit=256):
    with child(resources, node, action, end, stdin=subprocess.PIPE, stdout=subprocess.PIPE) as proc:
        deadline = min(end, time.monotonic() + 16)
        os.set_blocking(proc.stdin.fileno(), False)
        write_fd(proc.stdin.fileno(), data, deadline)
        proc.stdin.close()
        result = bytearray()
        while True:
            require(bool(select.select([proc.stdout], [], [], remaining(deadline))[0]), "live-read-deadline")
            block = os.read(proc.stdout.fileno(), limit + 1 - len(result))
            if not block:
                break
            result.extend(block)
            require(len(result) <= limit, "live-command-output")
        require(proc.wait(timeout=remaining(deadline)) == 0, "live-command-exit")
        return bytes(result)


@contextmanager
def private_terminal(resources, node):
    import fcntl
    import termios

    root = resources.nodes[node][2] / "root"
    master = os.open(root / "dev/pts/ptmx", os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        number = struct.unpack("I", fcntl.ioctl(master, 0x80045430, bytes(4)))[0]  # TIOCGPTN
        require(number < 8, "pty-private-number")
        fcntl.ioctl(master, 0x40045431, struct.pack("i", 0))  # TIOCSPTLCK
        slave = os.open(root / "dev/pts" / str(number), os.O_RDWR | os.O_NOCTTY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            os.fchown(slave, 21070, 21070)
            mode = termios.tcgetattr(slave)
            mode[1] &= ~termios.OPOST
            mode[3] &= ~(termios.ECHO | termios.ECHONL)
            termios.tcsetattr(slave, termios.TCSANOW, mode)
            yield master, slave
        finally:
            os.close(slave)
    finally:
        os.close(master)


def wipe(data):
    data[:] = bytes(len(data))


def mint_paste(resources, node, end):
    output = bytearray(8802)
    complete = False
    try:
        with private_terminal(resources, node) as (master, slave):
            with child(resources, node, "mint", end, stdin=slave) as proc:
                size = 0
                while True:
                    require(proc.poll() in (0, None)
                            and bool(select.select([master], [], [], remaining(end))[0]), "mint-terminal-deadline")
                    with memoryview(output) as view:
                        size += os.readv(master, [view[size:]])
                    require(0 < size <= 8801, "mint-paste-width")
                    if output[size-1] == 10:
                        break
                require(size >= 8 and proc.wait(timeout=remaining(end)) == 0 and output.startswith(b"arxje1-")
                        and not select.select([master], [], [], 0)[0], "mint-terminal")
                require(all(48 <= output[i] <= 57 or 97 <= output[i] <= 102 for i in range(7, size-1)),
                        "mint-paste-alphabet")
                del output[size-1:]
        complete = True
        return output
    finally:
        if not complete:
            wipe(output)


def join_paste(resources, node, paste, end):
    import termios

    with private_terminal(resources, node) as (master, slave):
        with child(resources, node, "join", end, stdin=slave) as proc:
            while True:
                remaining(end)
                mode = termios.tcgetattr(slave)
                require(proc.poll() is None and not mode[3] & (termios.ECHO | termios.ECHONL),
                        "join-tty-echo")
                if not mode[3] & termios.ICANON:
                    break
                time.sleep(0.01)
            write_fd(master, paste, end)
            write_fd(master, b"\n", end)
            require(proc.wait(timeout=remaining(end)) == 0 and not select.select([master], [], [], 0)[0],
                    "join-terminal")


class Mcp:
    def __init__(self, process, end):
        self.process = process
        self.end = end
        self.pending = bytearray()
        self.id = 0

    def write(self, message):
        self.process.stdin.write(json.dumps(message).encode("ascii") + b"\n")
        self.process.stdin.flush()

    def request(self, method, params):
        self.id += 1
        self.write({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params})

    def response(self):
        while b"\n" not in self.pending:
            require(bool(select.select([self.process.stdout], [], [], remaining(self.end))[0]), "mcp-deadline")
            block = os.read(self.process.stdout.fileno(), 4096)
            require(block and len(self.pending) + len(block) <= 65536, "mcp-output-bound")
            self.pending.extend(block)
        line, _, self.pending = self.pending.partition(b"\n")
        result = json.loads(line)
        require(result["jsonrpc"] == "2.0" and result["id"] == self.id and "error" not in result,
                "mcp-response")
        return result["result"]

    def initialize(self):
        self.request("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                    "clientInfo": {"name": "c16", "version": "1"}})
        require(isinstance(self.response(), dict), "mcp-initialize")
        self.write({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, name, arguments):
        self.request("tools/call", {"name": name, "arguments": arguments})

    def tool_result(self):
        [item] = self.response()["content"]
        return json.loads(item["text"])

    def call(self, name, arguments):
        self.send(name, arguments)
        return self.tool_result()


ACCOUNT = {"found": 1, "mints": 6, "anchor_queries": 3, "redeems": 2, "enrollment_proposals": 9,
           "add_nodes": 2, "joint": 1, "flatten": 1, "ca_handoffs": 2, "builds": 1,
           "engine_starts": 4, "leader_kill": 1, "normal_closes": 3}



class Campaign:
    def __init__(self, resources):
        self.resources = resources
        self.receipts = []
        self.status = {"anchor_queries": 0, "redeems": 0, "ca_handoffs": 0, "builds": 0,
                       "engine_starts": 0, "leader_kill": 0, "normal_closes": 0}
        self.checkpoints = {}
        self.anchor_diagnostic = None
        self.build_diagnostic = None
        self.ledger_diagnostic = {}
        self.identities = None

    def owner(self, node, action, tag, width, end, data=b""):
        _, pid = self.resources.daemon_identity(node, end)
        raw = child_reply(self.resources, node, action, end, data)
        require(len(raw) == width + 11 and raw[:4] == pid.to_bytes(4, "big")
                and raw[4:9] == b"ARXO" + bytes([tag]) and raw[9:11] == width.to_bytes(2, "big"),
                "owner-reply")
        return raw[11:]

    def evidence(self, node, end):
        raw = self.owner(node, "owner-status", 15, 29, end)
        require(raw[0] < 6, "daemon-refused")
        return raw

    def ready(self, node, states, end):
        while True:
            evidence = self.evidence(node, end)
            if evidence[0] in states:
                return evidence
            time.sleep(min(0.04, remaining(end)))

    def data(self, node):
        return self.resources.root / node

    def start(self, instance, stages, end):
        self.resources.start_live(instance, end)
        self.ready(instance[0], stages, end)
        self.status["engine_starts"] += 1

    def activate(self, end):
        install_live(self.data("a"), "a")
        install_live(self.data("c"), "c")
        self.start("a2", (4,), end)
        key = json.loads(read_checked(self.data("a") / "credentials/identity.json", (21071,), 8192))["key"]
        install_live(self.data("b"), "b", founder_key=key)
        join_positions = []
        for node in "bc":
            self.resources.start_live(node + "2", end)
            self.ready(node, (1,), end)
            require(not (self.data(node) / "credentials/ca.der").exists(), "join-before-ca")
            clock = time_evidence(self.owner("a", "owner-time", 16, 53, end))[3]
            paste = mint_paste(self.resources, "a", end)
            try:
                join_paste(self.resources, node, paste, end)
                admitted = self.ready(node, (3,), end)
                require(admitted[3:5] == b"\x01\x01", "join-admission")
                join_positions.append(position(admitted[5:]))
                self.status["engine_starts"] += 1
                if node == "b":
                    self.retry_consumed(paste, clock, end)
            finally:
                wipe(paste)
        founder = self.ready("a", (5,), end)
        require(founder[1:3] == b"\x03\x02", "activation-counts")
        self.status.update(anchor_queries=3, redeems=2)
        audit = log_audit(self.data("a"))
        require(audit[0] == {1: 2, 3: 5, 4: 1, 6: 2}
                and [raw for _, raw in audit[1]] == expected_membership()
                and [pos for pos, _ in audit[1][1:3]] == join_positions, "activation-log")
        self.membership = audit[1]
        root_hash = hashlib.sha256(read_checked(self.data("a") / "credentials/ca.der", (21071,), 4096)).digest()
        for node in "bc":
            require(hashlib.sha256(read_checked(self.data(node) / "credentials/ca.der", (21071,), 4096)).digest()
                    == root_hash, "ca-root-continuity")
            for name in ("history/strict", "history/voter", "private/voter-custody/ca-key"):
                facts = (self.data(node) / name).lstat()
                require(stat.S_ISREG(facts.st_mode) and facts.st_uid == 21071
                        and facts.st_mode & 0o077 == 0, "ca-handoff-file")
        self.status["ca_handoffs"] = 2
        self.checkpoints["activation"] = {"membership": self.membership[-1][0], "consumed_refusals": 1}

    def retry_consumed(self, paste, mint_clock, end):
        require(self.ready("a", (4,), end)[1:3] == b"\x01\x01", "first-redeem")
        before = time_evidence(self.owner("a", "owner-time", 16, 53, end))
        first = log_audit(self.data("a"))
        require(first[0] == {1: 1, 3: 2, 4: 1, 6: 1}, "first-binding")
        result = child_reply(self.resources, "b", "anchor", end, paste, limit=1024)
        outcome, passed = anchor_client_status(result)
        if (outcome, passed) != ("anchor-refused", "1"):
            try:
                raw_time = self.owner("a", "owner-time", 16, 53, min(end, time.monotonic() + 2))
            except ERRORS:
                raw_time = None
            self.anchor_diagnostic = {"token": outcome, "passed": passed,
                                      "owner_time": anchor_owner_delta(before, raw_time)}
            raise Refusal("anchor-client-refusal")
        after = time_evidence(self.owner("a", "owner-time", 16, 53, end))
        evidence = self.evidence("a", end)
        latest = log_audit(self.data("a"))
        require(evidence[0] == 4 and first[:3] == latest[:3], "reconnect-no-binding")
        check_reconnect(before, after, mint_clock, first[3], latest[3], tuple(evidence[1:3]))

    @contextmanager
    def mcp(self, action, end):
        with child(self.resources, "a", action, end, stdin=subprocess.PIPE, stdout=subprocess.PIPE) as proc:
            client = Mcp(proc, end)
            client.initialize()
            yield client

    def delegated_build(self, end):
        args = {"target": 2, **build_descriptor()}
        with self.mcp("foreign", end) as client:
            require(client.call("run_command", args) == {"outcome": "refused"}, "ungranted-client")
        with self.mcp("agent", end) as client:
            require(client.call("discover_build_targets", {}) == {"targets": [2]}, "delegated-target")
            for slot in ("link", "writable"):
                require(client.call("run_command", {**args, "workspace": slot}) == {"outcome": "refused"},
                        "workspace-denial")
            require(not activity(self.data("b"), False) and not activity(self.data("a"), True)
                    and not activity(self.data("a"), False), "premature-build")
            client.send("run_command", args)
            runs, leaf = self.build_witness(client, end)
            result = client.tool_result()
        result = result if isinstance(result, dict) else {}
        target = result.get("target_activity")
        target = target if isinstance(target, dict) else {}
        operation = result.get("operation_id")
        if not (result.get("outcome") == "succeeded" and isinstance(operation, str)
                and re.fullmatch(r"[0-9a-f]{64}", operation)
                and all(type(target.get(name)) is int and target[name] == 21072 for name in ("uid", "gid"))
                and target.get("cleanup_confirmed") is True
                and type(target.get("exit_code")) is int and target["exit_code"] == 0
                and type(target.get("activity_id")) is int and target["activity_id"] > 0):
            self.build_diagnostic = build_result_diagnostic(result, target)
            raise Refusal("target-build-result")
        artifact = self.data("b") / "workspace/ordinary/out/target/debug/c14-fixture"
        with open_artifact(artifact, (0, 21072), 16*MIB, executable=True) as (_, info):
            require(info.st_uid == info.st_gid == 21072 and not (runs / leaf).exists(), "build-artifact")
        b = activity(self.data("b"), False)
        a = activity(self.data("a"), True)
        require(b == [(operation.upper(), target["activity_id"], "succeeded")] and len(a) == 1
                and a[0][0] == operation.upper() and a[0][2] == "succeeded"
                and not activity(self.data("a"), False), "build-activity")
        self.status["builds"] = 1
        self.checkpoints["build"] = {"target": 2, "uid": 21072, "gid": 21072, "activity": target["activity_id"]}

    def build_witness(self, client, end):
        _, broker, runs = self.resources.broker("b", end)
        witness = self.data("b") / "workspace/ordinary/out/observed"
        while True:
            require(not select.select([client.process.stdout], [], [], 0)[0], "early-build-response")
            if witness.exists() and witness.lstat().st_size > 0:
                observed = read_checked(witness, (21072,), 64)
                break
            time.sleep(min(0.02, remaining(end)))
        match = re.fullmatch(rb"([1-9][0-9]{0,9}) (arx-" + str(broker).encode() + rb"-[0-9]{1,10})\n", observed)
        require(match is not None, "build-witness")
        leaf = match[2].decode("ascii")
        group = runs / leaf
        raw = bounded_read(group / "cgroup.procs", 64)
        host_pids = raw.decode("ascii").split()
        require(len(host_pids) == 2 and len(set(host_pids)) == 2
                and all(re.fullmatch(r"[1-9][0-9]{0,9}", pid) for pid in host_pids), "build-processes")
        pids = {pid: bounded_read(Path("/proc") / pid / "status", 8192).decode("ascii") for pid in host_pids}
        matched = [pid for pid, value in pids.items()
                   if re.search(r"^NSpid:\s+" + pid + r"\s+" + match[1].decode() + "$", value, re.M)]
        require(len(matched) == 1, "build-namespace-pid")
        pid = matched[0]
        parent = next(item for item in host_pids if item != pid)
        status = pids[pid]
        require(re.search(r"^PPid:\s+" + parent + "$", status, re.M)
                and re.search(r"^Uid:\s+21072\s+21072\s+21072\s+21072$", status, re.M)
                and re.search(r"^Gid:\s+21072\s+21072\s+21072\s+21072$", status, re.M)
                and re.search(r"^Groups:[ \t]*$", status, re.M)
                and re.search(r"^NoNewPrivs:\s+1$", status, re.M), "build-process-identity")
        sealed = self.resources.root / "root/fixture"
        require(os.path.samestat((Path("/proc") / parent / "exe").stat(), (sealed / "arxd").stat())
                and os.path.samestat((Path("/proc") / pid / "exe").stat(), (sealed / "tools/cargo").stat()),
                "build-process-images")
        for name, value in (("memory.max", b"134217728"), ("pids.max", b"32"), ("memory.swap.max", b"0")):
            require(bounded_read(group / name, 32).strip() == value, "build-leaf-limit")
        ack = self.data("b") / "probe/build-ack"
        with ack.open("xb") as out:
            os.fchmod(out.fileno(), 0o644)
            out.write(match[1] + b"\n")
        return runs, leaf

    def mint_ledger(self, node, end):
        for _ in range(2):
            raw = self.owner(node, "owner-mint-receipt", 17, 57, end)
            require(raw[0] == 1 and position(raw[1:25])[1] == "abc".index(node) + 1
                    and any(raw[25:]), "mint-receipt")
            self.receipts.append(raw[1:])
        ledger_slots(self.receipts)

    def read_ledger_set(self, label, nodes, end, validate, prepare=None):
        payload = ledger_slots(self.receipts)
        observations = {}
        evidence_by_node = {}

        def reject(node, predicate):
            evidence = {**evidence_by_node[node], "predicate": predicate}
            diagnostic = self.ledger_diagnostic.setdefault(label, {}).setdefault(
                node, {"retries": 0, "first": evidence})
            if evidence != diagnostic["first"]:
                diagnostic["last"] = evidence
            else:
                diagnostic.pop("last", None)

        def sample(node, sampled_nodes, original=None):
            remaining(end)
            if original is not None:
                self.ledger_diagnostic[label][node]["retries"] += 1
            evidence = {}
            evidence_by_node[node] = evidence
            try:
                raw = self.owner(node, "owner-ledger", 18, 164, end, payload)
                record = decode_ledger_checkpoint(raw, evidence)
                validate({**observations, node: record}, sampled_nodes)
                if original is not None:
                    ledger_require(record["epoch"] == original["epoch"], "retry-epoch", "ledger-consistency")
                    ledger_require(record["identity"] == original["identity"], "retry-installation",
                                   "ledger-installation-drift")
            except LedgerRefusal as error:
                reject(node, error.predicate)
                raise
            observations[node] = record

        for index, node in enumerate(nodes):
            if prepare is not None:
                prepare(node)
            sample(node, nodes[:index + 1])

        # Every original reply has now passed the full checkpoint except for status.
        original = observations.copy()
        peers = [node for node in nodes if original[node]["status"] == LEDGER_PEER]
        for node in peers:
            reject(node, "peer-status")
        for _ in range(LEDGER_RETRIES):
            if not peers or time.monotonic() + LEDGER_BACKOFF >= end:
                break
            time.sleep(LEDGER_BACKOFF)
            if time.monotonic() >= end:
                break
            for node in peers:
                sample(node, nodes, original[node])
                if observations[node]["status"] == LEDGER_PEER:
                    reject(node, "peer-status")
            peers = [node for node in peers if observations[node]["status"] == LEDGER_PEER]
        ledger_require(not peers, "peer-status")
        remaining(end)
        return observations

    def observe(self, label, nodes, end, *, after_term=None, expected_epoch=None):
        goal = position(self.receipts[-1][:24])

        def prepare(node):
            while True:
                remaining(end)
                try:
                    [(prefix,)] = sql_rows(self.data(node) / "state/cluster-state.sqlite3",
                                          "SELECT substr(snapshot,1,35) FROM sm WHERE singleton=1 LIMIT 2")
                except sqlite3.OperationalError as error:
                    if error.sqlite_errorcode != sqlite3.SQLITE_BUSY:
                        raise
                else:
                    require(len(prefix) == 35 and prefix[:11] == b"ARXSM1\0\0\0\x03\x01", "applied-prefix")
                    if covers(position(prefix[11:35]), goal):
                        return
                time.sleep(0.04)

        def validate(observations, sampled_nodes):
            epoch, member_pos = check_checkpoint_set(observations, sampled_nodes, self.receipts, self.identities)
            ledger_require(member_pos == self.membership[-1][0], "expected-membership", "ledger-membership-position")
            if expected_epoch is not None:
                ledger_require(epoch == expected_epoch, "recovery-epoch", "recovery-epoch")
            elif after_term is not None:
                ledger_require(epoch[0] > after_term, "survivor-term", "survivor-epoch")
                ledger_require(epoch[1] == goal[1], "survivor-leader", "survivor-epoch")
            else:
                ledger_require(epoch[1] == 1, "founder-leader", "founder-leader-precondition")

        observations = self.read_ledger_set(label, nodes, end, validate, prepare)
        epoch, member_pos = observations[nodes[0]]["epoch"], observations[nodes[0]]["member_pos"]
        if self.identities is None:
            self.identities = {node: row["identity"] for node, row in observations.items()}
        self.checkpoints[label] = {"epoch": epoch, "membership": member_pos,
                                   "applied": {node: value["applied"] for node, value in observations.items()},
                                   "mints": [position(slot[:24]) for slot in self.receipts]}
        if label != "before_loss":
            self.checkpoints[label]["same_installations"] = True
        return epoch

    def build_and_mint(self, end):
        self.delegated_build(end)
        self.ready("a", (5,), end)
        self.mint_ledger("a", end)
        self.pre_epoch = self.observe("before_loss", "abc", end)

    def recover(self, end):
        def validate_held(observations, nodes):
            held = observations["a"]
            ledger_require(held["node"] == 1, "expected-member", "kill-epoch-drift")
            ledger_require(held["epoch"] == self.pre_epoch, "kill-epoch", "kill-epoch-drift")
            ledger_require(held["identity"] == self.identities["a"], "expected-installation", "kill-epoch-drift")
            ledger_require(held["member_pos"] == self.membership[-1][0], "expected-membership", "kill-epoch-drift")
            check_checkpoint_set(observations, nodes, self.receipts, self.identities)

        self.read_ledger_set("pre_kill", "a", end, validate_held)
        images = [self.resources.root / name for name in ("root.ext4", "a.ext4")]
        original = [image_id(path) for path in images]
        quiescent_data(self.data("a"))
        self.resources.stop_live("a", end, crash=True)
        self.resources.drop_founder_endpoint(end)
        self.status["leader_kill"] = 1
        while True:
            candidates = [node for node in "bc" if self.evidence(node, end)[0] == 5]
            require(len(candidates) <= 1 and "a" not in self.resources.nodes, "survivor-election")
            if candidates:
                break
            time.sleep(min(0.04, remaining(end)))
        leader = candidates[0]
        self.mint_ledger(leader, end)
        after = self.observe("during_loss", "bc", end, after_term=self.pre_epoch[0])
        require([image_id(path) for path in images] == original, "restart-image-drift")
        quiescent_data(self.data("a"))
        self.resources.restore_founder_endpoint(end)
        self.start("a3", (3, 4, 5), end)
        self.observe("after_recovery", "abc", end, expected_epoch=after)
        require([image_id(path) for path in images] == original, "restart-image-drift")
        self.checkpoints["restart"] = {"same_data_image": True, "fresh_volatile_state": True}
        self.survivor = leader

    def seal(self, end):
        counts, membership, mints, _ = log_audit(self.data(self.survivor))
        require(membership == self.membership and counts == {4: 1, 1: 6, 6: 2, 3: 5}
                and mints[2:] == [position(slot[:24]) for slot in self.receipts], "campaign-log")
        stable_rules(self.resources.rule_receipt, self.resources.counters(end))
        self.status.update(found=counts[4], mints=counts[1], enrollment_proposals=counts[4]+counts[1]+counts[6],
                           add_nodes=2, joint=1, flatten=1)
        require(self.status == {**ACCOUNT, "normal_closes": 0}, "operation-accounting")



def live_run(env, uid):
    driver = Qualification("live")
    campaign = None
    sealed = False
    try:
        def setup(end):
            started = time.monotonic()
            pins = load_pins()
            receipt, daemon, tools, vetted, anchor = checked_metadata(env, pins, uid, min(end, started + 8), live=True)
            resources = driver.resources = Resources(receipt, pins, vetted, live=True)
            driver.receipt["runner"] = receipt
            driver.receipt["images"] = stage(resources, (pins, daemon, tools, uid, receipt["modules"], vetted),
                                             min(end, started + 32), anchor=anchor)
            resources.contain(min(end, started + 48))
            resources.network(end)
            resources.retire_probes(end)
            driver.receipt["containment"] = resources.facts
            return Campaign(resources)

        campaign = driver.phase("setup", setup)
        for name, operation in (("activation", campaign.activate), ("build", campaign.build_and_mint),
                                ("recovery", campaign.recover), ("evidence", campaign.seal)):
            driver.phase(name, operation)
        driver.receipt["checkpoints"] = campaign.checkpoints
        sealed = True
    except ERRORS as error:
        driver.latch(error)
        if campaign is not None and driver.receipt["first"]["category"] == "anchor-client-refusal":
            driver.receipt["anchor_diagnostic"] = campaign.anchor_diagnostic
        if campaign is not None and driver.receipt["first"]["category"] == "target-build-result":
            driver.receipt["diagnostic"] = campaign.build_diagnostic

    def cleanup(end):
        if sealed:
            for node in "abc":
                try:
                    driver.resources.stop_live(node, min(end - 7, time.monotonic() + 5))
                    campaign.status["normal_closes"] += 1
                except ERRORS as error:
                    driver.latch(error)
            if campaign.status == ACCOUNT:
                driver.receipt["accounting"] = campaign.status
            else:
                driver.latch(Refusal("operation-accounting"))
        return driver.resources.cleanup(end) if driver.resources else []

    try:
        errors = driver.phase("cleanup", cleanup)
        driver.receipt["cleanup"] = "incomplete" if errors else "confirmed"
        if errors:
            driver.receipt["cleanup_errors"] = errors
            driver.latch(Refusal("partial-cleanup"))
    except ERRORS as error:
        driver.receipt["cleanup"] = "incomplete"
        driver.latch(error)
    if driver.resources and driver.resources.failure_diagnostic is not None:
        driver.receipt.setdefault("diagnostic", driver.resources.failure_diagnostic)
    if campaign is not None and campaign.ledger_diagnostic:
        driver.receipt["ledger_diagnostic"] = campaign.ledger_diagnostic
    return driver.receipt


def encode_receipt(receipt, limit=12288):
    data = json.dumps(receipt, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    require(len(data) <= limit, "receipt-output-bound")
    return data


def entry(args, env):
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--mode", choices=("dry", "live"), required=True)
    mode = parser.parse_args(args).mode
    if args != ["--mode", mode]:
        parser.error("expected exactly one --mode")
    prefix = "C16 receipt " if mode == "live" else "R2 receipt "
    try:
        require(sys.platform == "linux", "linux-host")
        if os.geteuid() != 0:
            require(all(env.get(name) == value for name, value in FIXED.items()), "hosted-profile")
            allow = [f"{name}={env.get(name, '')}" for name in (*FIXED, *FIELDS)]
            require(all(0 < len(item) <= 512 and "\n" not in item and "\r" not in item for item in allow),
                    "environment-bound")
            budgets = LIVE_TIME if mode == "live" else TIME
            output = capture(["/usr/bin/sudo", "-n", "/usr/bin/env", *allow, "/usr/bin/python3",
                              "-I", "-B", HERE / "run.py", "--mode", mode],
                             time.monotonic() + sum(budgets.values()) + 3, statuses=(0, 1), limit=12544)
            require(output.startswith(prefix.encode("ascii")) and output.count(b"\n") == 1, "privileged-receipt")
            receipt = json.loads(output[len(prefix):])
            require(receipt["mode"] == mode, "privileged-receipt-mode")
        else:
            owner = env.get("SUDO_UID", "")
            require(re.fullmatch(r"[1-9][0-9]{0,8}", owner), "staging-identity")
            receipt = (live_run if mode == "live" else dry_run)(env, int(owner))
        encoded = encode_receipt(receipt)
    except ERRORS as error:
        failure = Qualification(mode)
        failure.latch(error)
        receipt = failure.receipt
        encoded = encode_receipt(receipt)
    print(prefix + encoded, flush=True)
    return int(receipt["first"] is not None or receipt["cleanup"] != "confirmed")


if __name__ == "__main__":
    sys.exit(entry(sys.argv[1:], os.environ))
