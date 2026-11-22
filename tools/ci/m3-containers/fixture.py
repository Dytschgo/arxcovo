"""Bounded R2 staging and host command primitives."""

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import selectors
import secrets
import shutil
import signal
import stat
import struct
import subprocess
import time
from contextlib import contextmanager

MIB = 1024**2
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
TIME = {"preflight": 8, "stage": 24, "contain": 16, "network": 12, "cleanup": 25}
LIVE_TIME = {"setup": 60, "activation": 60, "build": 30, "recovery": 55, "evidence": 10, "cleanup": 25}
ANCHOR_TEST = ("cluster::consensus::bound_start::runtime::c16_probe::"
               "c16_distinct_reconnect_anchor_only")
CARGO_ARGS = ["build", "--offline", "--locked", "--target-dir", "out/target"]
ROLES = {"owner": 21070, "daemon": 21071, "build": 21072, "agent": 21073}
OUTER = {"cpu_percent": 100, "memory": 512*MIB, "swap": 0, "pids": 128, "nofile": 128, "core": 0}
INNER = {"caps": 193, "nnp": 1, "memory": 128*MIB, "swap": 0, "pids": 32}
TOOLS = {
    "cc": "/usr/bin/gcc", "nft": "/usr/sbin/nft", "ip": "/usr/sbin/ip",
    "mkfs": "/usr/sbin/mkfs.ext4", "loop": "/usr/sbin/losetup",
    "unit": "/usr/bin/systemd-run", "systemctl": "/usr/bin/systemctl",
    "mount": "/usr/bin/mount", "umount": "/usr/bin/umount",
    "findmnt": "/usr/bin/findmnt", "ldd": "/usr/bin/ldd",
    "dpkg": "/usr/bin/dpkg-query", "git": "/usr/bin/git",
}
HOST_PROFILES = [
    {"image_os": "ubuntu24", "image_version": "20261116.133.1",
     "kernel": "7.0.0-1014-aws", "systemd_package": "255.4-1ubuntu8.18"},
    {"image_os": "ubuntu24", "image_version": "20261116.376.1",
     "kernel": "7.0.0-1017-azure", "systemd_package": "255.4-1ubuntu8.18"},
]
NETWORK = {
    "nodes": {node: [f"192.168.72.{i}", f"02:72:00:00:00:{i}"] for i, node in enumerate("abc", 11)},
    "prefix": 24, "ports": {"peer": 22171, "enrollment": 22172, "build": 22173, "forbidden": 22174},
    "local": ["192.168.72.14", "02:72:00:00:00:14"],
    "outside": "192.0.2.72", "link_local": "169.254.254.72", "ipv6": "fd42:72::13",
}
ENV = {"PATH": "/usr/sbin:/usr/bin", "LANG": "C", "LC_ALL": "C", "TZ": "UTC"}


class Refusal(ValueError):
    """An enumerated checkpoint category suitable for the bounded receipt."""


def require(condition, category):
    if not condition:
        raise Refusal(category)


def remaining(end):
    duration = end - time.monotonic()
    require(duration > 0, "phase-deadline")
    return duration


def unique_pairs(pairs):
    result = dict(pairs)
    require(len(result) == len(pairs), "duplicate-json-key")
    return result


def bounded_read(path, limit):
    with path.open("rb") as source:
        data = source.read(limit + 1)
    require(len(data) <= limit, "read-bound")
    return data


def check_pins(pins):
    # Structural equality also rejects an unknown nested key. JSON canonical comparison
    # distinguishes bool, float and integer representations.
    expected = {
        "schema": "arxcovo/r2/1",
        "host": {
            "label": "ubuntu-24.04", "os": "24.04", "profiles": HOST_PROFILES,
            "rust": "1.95.0-x86_64-unknown-linux-gnu",
            "parent": "/var/lib", "tools": TOOLS,
        },
        "rootfs": {
            "userland": "24.04",
            "bytes": 1024*MIB, "data_bytes": 256*MIB, "run_bytes": 4*MIB,
            "tmp_bytes": 8*MIB, "dev_bytes": MIB, "block_size": 4096, "data_inodes": 16384,
        },
        "roles": ROLES, "network": NETWORK, "outer": OUTER, "inner": INNER, "seconds": TIME, "live_seconds": LIVE_TIME,
    }
    require(json.dumps(expected, sort_keys=True) == json.dumps(pins, sort_keys=True), "pins-schema")
    return pins


def load_pins():
    raw = bounded_read(HERE / "pins.json", 8192)
    pins = json.loads(raw, object_pairs_hook=unique_pairs)
    require(isinstance(pins, dict), "pins-schema")
    return check_pins(pins)


def capture(argv, end, *, env=ENV, limit=65536, statuses=(0,)):
    """Read bounded output without logging user-controlled environment or stdout."""
    child = subprocess.Popen(argv, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, start_new_session=True)
    try:
        result = bytearray()
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ)
            while selector.select(remaining(end)):
                block = os.read(child.stdout.fileno(), min(4096, limit + 1 - len(result)))
                if not block:
                    break
                checked_output(result, block, limit)
            else:
                raise Refusal("command-deadline")
        try:
            status = child.wait(timeout=remaining(end))
        except subprocess.TimeoutExpired:
            raise Refusal("command-deadline") from None
        require(status in statuses, "command-status")
        return bytes(result)
    except BaseException:
        if child.poll() is None:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except (AttributeError, ProcessLookupError, OSError):
                child.kill()
        try:
            child.wait(timeout=1)
        except subprocess.TimeoutExpired:
            pass
        raise
    finally:
        if child.stdout is not None:
            child.stdout.close()


def checked_output(result, block, limit):
    require(len(result) + len(block) <= limit, "command-output-bound")
    result.extend(block)


TOOL_LEAF = r"[A-Za-z0-9][A-Za-z0-9._+-]{0,127}"
TOOL_ABS = re.compile(r"/usr/s?bin/" + TOOL_LEAF)
TOOL_REL = re.compile(TOOL_LEAF)
TOOL_SIBLING = re.compile(r"\.\./s?bin/" + TOOL_LEAF)
TOOL_MERGED = re.compile(r"/s?bin/" + TOOL_LEAF)


def vet_tool(raw_path, *, lstat=os.lstat, readlink=os.readlink):
    """Vet a pinned host tool path and every symlink hop before execution."""
    raw = os.fspath(raw_path)
    require(isinstance(raw, str) and TOOL_ABS.fullmatch(raw) is not None, "runner-tool")
    seen = set()
    current = raw
    for _ in range(8):
        require(current not in seen, "runner-tool")
        seen.add(current)
        candidate = PurePosixPath(current)
        require(candidate.parent in (PurePosixPath("/usr/bin"), PurePosixPath("/usr/sbin")), "runner-tool")
        try:
            for ancestor in reversed(candidate.parents):
                info = lstat(ancestor.as_posix())
                require(stat.S_ISDIR(info.st_mode) and info.st_uid == 0 and info.st_mode & 0o022 == 0,
                        "runner-tool")
            info = lstat(current)
        except OSError:
            raise Refusal("runner-tool") from None
        require(info.st_uid == 0, "runner-tool")
        if stat.S_ISLNK(info.st_mode):
            try:
                target = readlink(current)
            except OSError:
                raise Refusal("runner-tool") from None
            require(isinstance(target, str) and target != "" and "\x00" not in target, "runner-tool")
            if TOOL_REL.fullmatch(target):
                current = f"{candidate.parent.as_posix()}/{target}"
            elif TOOL_ABS.fullmatch(target):
                current = target
            elif TOOL_SIBLING.fullmatch(target):
                current = f"/usr/{target[3:]}"
            elif TOOL_MERGED.fullmatch(target):
                alias = "/bin" if target.startswith("/bin/") else "/sbin"
                expected_dir = "usr/bin" if alias == "/bin" else "usr/sbin"
                try:
                    alias_info = lstat(alias)
                    require(alias_info.st_uid == 0 and stat.S_ISLNK(alias_info.st_mode), "runner-tool")
                    alias_target = readlink(alias)
                except OSError:
                    raise Refusal("runner-tool") from None
                require(alias_target in (expected_dir, "/" + expected_dir), "runner-tool")
                current = f"/{expected_dir}/{target.split('/', 2)[2]}"
            else:
                raise Refusal("runner-tool")
            continue
        require(stat.S_ISREG(info.st_mode) and info.st_mode & 0o100 and info.st_mode & 0o022 == 0,
                "runner-tool")
        return current
    raise Refusal("runner-tool")


def vet_tools(tools=TOOLS, *, lstat=os.lstat, readlink=os.readlink):
    require(isinstance(tools, dict) and set(tools) == set(TOOLS), "runner-tool")
    return {name: vet_tool(tools[name], lstat=lstat, readlink=readlink) for name in TOOLS}


def host_command(tool, end, *args, tools=None, **options):
    require(tool in TOOLS, "tool-identity")
    require(isinstance(tools, dict) and tool in tools, "tool-unvetted")
    executable = tools[tool]
    require(isinstance(executable, str) and TOOL_ABS.fullmatch(executable) is not None, "tool-unvetted")
    try:
        return capture([executable, *map(str, args)], end, **options).decode("ascii")
    except Refusal as error:
        raise Refusal(f"{tool}-{error}") from None


def plain_path(path, owners, *, directory):
    require(path.is_absolute() and ".." not in path.parts and path.anchor == "/", "absolute-path")
    chain = reversed(path.parents) if not directory else reversed((path, *path.parents))
    for parent in chain:
        metadata = parent.lstat()
        require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid in owners
                and metadata.st_mode & 0o022 == 0, "unsafe-ancestor")
    return path


def protected_mode(metadata, owners):
    return (stat.S_ISREG(metadata.st_mode) and metadata.st_uid in owners
            and metadata.st_mode & 0o7022 == 0 and metadata.st_nlink >= 1)


@contextmanager
def open_artifact(path, owners, size, *, executable=False):
    """Keep the descriptor tied to the no-follow identity used for selection."""
    plain_path(path, owners, directory=False)
    before = path.lstat()
    require(protected_mode(before, owners) and 0 < before.st_size <= size
            and (not executable or before.st_mode & 0o100), "artifact-type-size")
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC), "rb") as source:
        identity = lambda value: (value.st_dev, value.st_ino, value.st_size, value.st_mode, value.st_mtime_ns)
        require(identity(before) == identity(os.fstat(source.fileno())), "artifact-substituted")
        yield source, before
        require(identity(before) == identity(os.fstat(source.fileno()))
                == identity(path.lstat()), "artifact-changed")


def inspect_elf(stream, size):
    header = stream.read(64)
    require(len(header) == 64 and header[:7] == b"\x7fELF\x02\x01\x01", "elf-header")
    kind, arch, version = struct.unpack_from("<HHI", header, 16)
    offset = struct.unpack_from("<Q", header, 32)[0]
    ehsize, step, count = struct.unpack_from("<HHH", header, 52)
    require(arch == 62 and kind in (2, 3) and version == 1 and ehsize == 64
            and step == 56 and 1 <= count <= 64 and 64 <= offset <= size - step*count, "elf-architecture")
    stream.seek(offset)
    segments = [struct.unpack("<IIQQQQQQ", stream.read(56)) for _ in range(count)]
    require(any(item[0] == 1 and item[1] & 1 for item in segments), "elf-load")
    require(all(item[2] <= size and item[5] <= size - item[2] for item in segments), "elf-segments")
    loaders = [item for item in segments if item[0] == 3]
    require(len(loaders) == 1 and 1 < loaders[0][5] <= 128, "elf-interpreter")
    stream.seek(loaders[0][2])
    loader = stream.read(loaders[0][5])
    require(loader == b"/lib64/ld-linux-x86-64.so.2\0", "elf-interpreter")
    stream.seek(0)


def stream_hash(stream, size, end, output=None):
    digest = hashlib.sha256()
    while size:
        remaining(end)
        block = stream.read(min(MIB, size))
        require(block, "artifact-truncated")
        digest.update(block)
        if output is not None:
            output.write(block)
        size -= len(block)
    require(not stream.read(1), "artifact-surplus")
    return digest.hexdigest()


def file_digest(path, end, limit, owners=(0,)):
    with open_artifact(path, owners, limit) as (source, metadata):
        return stream_hash(source, metadata.st_size, end)


def stage_copy(path, destination, end, owners, *, elf=False, mode=0o644, limit=512*MIB):
    with open_artifact(path, owners, limit, executable=elf) as (source, metadata):
        if elf:
            inspect_elf(source, metadata.st_size)
        with destination.open("xb") as output:
            digest = stream_hash(source, metadata.st_size, end, output)
    destination.chmod(mode)
    return digest


def validate_build_messages(raw, target, workspace):
    sqlite_ok = False
    arxd_ok = False
    expected_manifest = (PurePosixPath(workspace.as_posix()) / "crates/arxd/Cargo.toml").as_posix()
    target_posix = PurePosixPath(target.as_posix())
    lines = raw.splitlines()
    require(1 <= len(lines) <= 50000, "artifact-build-messages")
    for line in lines:
        if not line.strip():
            continue
        try:
            entry = json.loads(line)
        except ValueError:
            raise Refusal("artifact-build-messages") from None
        if not isinstance(entry, dict):
            continue
        reason = entry.get("reason")
        if reason == "build-script-executed":
            pkg_id = entry.get("package_id", "")
            out_dir = entry.get("out_dir", "")
            if ("libsqlite3-sys" in str(pkg_id)
                    and entry.get("linked_libs") == ["static=sqlite3"]
                    and not entry.get("cfgs") and not entry.get("env")
                    and isinstance(out_dir, str) and out_dir.startswith("/")):
                out_path = PurePosixPath(out_dir)
                if ".." not in out_path.parts and out_path.is_relative_to(target_posix):
                    sqlite_ok = True
        elif reason == "compiler-artifact":
            tgt = entry.get("target")
            manifest = entry.get("manifest_path")
            if (isinstance(tgt, dict) and tgt.get("name") == "arxd"
                    and isinstance(tgt.get("kind"), list) and "bin" in tgt["kind"]
                    and manifest == expected_manifest):
                arxd_ok = True
    require(sqlite_ok and arxd_ok, "artifact-build-messages")


def validate_link_messages(raw, workspace, expected_executable):
    expected_manifest = (PurePosixPath(workspace.as_posix()) / "crates/arxd/Cargo.toml").as_posix()
    lines = raw.splitlines()
    require(1 <= len(lines) <= 50000, "artifact-link-messages")
    matched = []
    finished_ok = False
    for line in lines:
        if not line.strip():
            continue
        try:
            entry = json.loads(line)
        except ValueError:
            raise Refusal("artifact-link-messages") from None
        if not isinstance(entry, dict):
            continue
        reason = entry.get("reason")
        if reason == "compiler-artifact":
            tgt = entry.get("target")
            profile = entry.get("profile")
            if (isinstance(tgt, dict) and tgt.get("name") == "arxd"
                    and isinstance(tgt.get("kind"), list) and "bin" in tgt["kind"]):
                require(entry.get("manifest_path") == expected_manifest
                        and isinstance(profile, dict) and profile.get("test") is False
                        and isinstance(entry.get("executable"), str)
                        and entry["executable"] == expected_executable, "artifact-link-messages")
                matched.append(entry["executable"])
        elif reason == "build-finished":
            require(entry.get("success") is True, "artifact-link-messages")
            finished_ok = True
    require(len(matched) == 1 and finished_ok, "artifact-link-messages")


def select_daemon(target_raw, workspace, owner, proof_root, identity, end):
    if target_raw:
        target = Path(target_raw)
    else:
        target = workspace / "target"
    plain_path(target, (0, owner), directory=True)
    profile = target / "debug"
    plain_path(profile, (0, owner), directory=True)
    path = profile / "arxd"
    loader_receipt = proof_root / "build-messages.jsonl"
    link_receipt = proof_root / "arxd-build-messages.jsonl"
    artifact_receipt = proof_root / "arxd-receipt.json"
    with open_artifact(loader_receipt, (0, owner), 64*MIB) as (messages_stream, loader_stat):
        raw_messages = messages_stream.read(loader_stat.st_size + 1)
        require(len(raw_messages) == loader_stat.st_size, "artifact-truncated")
    messages_sha256 = hashlib.sha256(raw_messages).hexdigest()
    validate_build_messages(raw_messages, target, workspace)
    with open_artifact(link_receipt, (0, owner), 64*MIB) as (link_stream, link_stat):
        raw_link = link_stream.read(link_stat.st_size + 1)
        require(len(raw_link) == link_stat.st_size, "artifact-truncated")
    require(link_stat.st_mtime_ns >= loader_stat.st_mtime_ns, "stale-artifact")
    link_sha256 = hashlib.sha256(raw_link).hexdigest()
    validate_link_messages(raw_link, workspace, path.as_posix())
    with open_artifact(artifact_receipt, (0, owner), 65536) as (receipt_stream, receipt_stat):
        raw_receipt = receipt_stream.read(receipt_stat.st_size + 1)
        require(len(raw_receipt) == receipt_stat.st_size, "artifact-truncated")
    require(receipt_stat.st_mtime_ns >= link_stat.st_mtime_ns, "stale-artifact")
    try:
        receipt_data = json.loads(raw_receipt, object_pairs_hook=unique_pairs)
    except ValueError:
        raise Refusal("artifact-receipt") from None
    expected_keys = {
        "schema", "head", "run", "attempt", "target_dir",
        "executable", "sha256", "bytes", "build_messages_sha256", "link_messages_sha256",
    }
    require(isinstance(receipt_data, dict) and set(receipt_data) == expected_keys
            and receipt_data["schema"] == "arxcovo/r2-artifact/1", "artifact-receipt")
    require(receipt_data["head"] == identity["head"]
            and receipt_data["run"] == identity["run"]
            and receipt_data["attempt"] == identity["attempt"], "artifact-receipt-identity")
    require(receipt_data["build_messages_sha256"] == messages_sha256
            and receipt_data["link_messages_sha256"] == link_sha256, "artifact-receipt-binding")
    require(receipt_data["target_dir"] == target.as_posix()
            and receipt_data["executable"] == path.as_posix(), "artifact-receipt-path")
    require(type(receipt_data["bytes"]) is int and 0 < receipt_data["bytes"] <= 256*MIB
            and isinstance(receipt_data["sha256"], str)
            and re.fullmatch(r"[a-f0-9]{64}", receipt_data["sha256"]) is not None, "artifact-receipt")
    candidates = [item for item in profile.iterdir()
                  if item.name == "arxd" or (item.name.startswith("arxd-") and not item.name.endswith(".d"))]
    require(candidates == [path], "artifact-candidates")
    with open_artifact(path, (0, owner), 256*MIB, executable=True) as (binary, value):
        require(value.st_mtime_ns >= loader_stat.st_mtime_ns, "stale-artifact")
        require(value.st_size == receipt_data["bytes"], "artifact-digest-mismatch")
        inspect_elf(binary, value.st_size)
        digest = stream_hash(binary, value.st_size, end)
        require(digest == receipt_data["sha256"], "artifact-digest-mismatch")
    return path, digest


def select_anchor_probe(target_raw, workspace, owner, proof_root, identity, end):
    target = Path(target_raw) if target_raw else workspace / "target"
    owners = (0, owner)
    proof = {}
    timestamps = {}
    for name, limit in (("c16-anchor-build.jsonl", 64*MIB), ("c16-anchor-receipt.json", 8192)):
        with open_artifact(proof_root / name, owners, limit) as (stream, metadata):
            proof[name] = stream.read(metadata.st_size + 1)
            require(len(proof[name]) == metadata.st_size, "anchor-proof-size")
            timestamps[name] = metadata.st_mtime_ns
    raw = proof["c16-anchor-build.jsonl"]
    receipt = json.loads(proof["c16-anchor-receipt.json"], object_pairs_hook=unique_pairs)
    require(set(receipt) == {"schema", "head", "run", "attempt", "target_dir", "executable",
                             "sha256", "bytes", "messages_sha256", "selector"}
            and receipt["schema"] == "arxcovo/c16-anchor/1" and receipt["selector"] == ANCHOR_TEST
            and all(receipt[key] == identity[key] for key in ("head", "run", "attempt"))
            and receipt["target_dir"] == target.as_posix()
            and receipt["messages_sha256"] == hashlib.sha256(raw).hexdigest(), "anchor-receipt")
    path = Path(receipt["executable"])
    require(path.parent == target / "debug/deps" and re.fullmatch(r"arxd-[a-f0-9]{16}", path.name),
            "anchor-path")
    manifest = (workspace / "crates/arxd/Cargo.toml").as_posix()
    lines = raw.splitlines()
    require(1 <= len(lines) <= 50000, "anchor-build-lines")
    rows = [json.loads(line, object_pairs_hook=unique_pairs) for line in lines if line.strip()]
    linked = [row.get("executable") for row in rows if row.get("reason") == "compiler-artifact"
              and row.get("manifest_path") == manifest and row["target"]["name"] == "arxd"
              and row["target"]["kind"] == ["lib"] and row["profile"]["test"] is True]
    require(linked == [path.as_posix()]
            and [row.get("success") for row in rows if row.get("reason") == "build-finished"] == [True]
            and timestamps["c16-anchor-receipt.json"] >= timestamps["c16-anchor-build.jsonl"],
            "anchor-build-messages")
    require(type(receipt["bytes"]) is int and re.fullmatch(r"[a-f0-9]{64}", receipt["sha256"]),
            "anchor-digest")
    with open_artifact(path, owners, 256*MIB, executable=True) as (stream, metadata):
        require(metadata.st_size == receipt["bytes"], "anchor-size")
        inspect_elf(stream, metadata.st_size)
        digest = stream_hash(stream, metadata.st_size, end)
        require(digest == receipt["sha256"], "anchor-digest")
    return path, digest


def allocate(path, size):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        os.posix_fallocate(fd, 0, size)
        image_stat(os.fstat(fd), size)
    finally:
        os.close(fd)


def image_stat(metadata, size):
    require(stat.S_ISREG(metadata.st_mode) and metadata.st_uid == 0
            and metadata.st_nlink == 1 and stat.S_IMODE(metadata.st_mode) in (0o600, 0o400)
            and metadata.st_size == size and metadata.st_blocks*512 >= size, "image-allocation")


def role_files(root):
    users = [("root", 0)] + [(f"arx-{name}", uid) for name, uid in ROLES.items()]
    for name, mode, data in (
        ("passwd", 0o644, "".join(f"{name}:x:{uid}:{uid}::/data/home:/bin/sh\n" for name, uid in users)),
        ("group", 0o644, "".join(f"{name}:x:{uid}:\n" for name, uid in users)),
        ("shadow", 0o400, "".join(f"{name}:!:1:::::1:\n" for name, _ in users)),
        ("nsswitch.conf", 0o644, "passwd: files\ngroup: files\nshadow: files\nhosts: files\n"),
        ("hosts", 0o644, "127.0.0.1 localhost\n"),
    ):
        path = root / "etc" / name
        path.write_text(data, encoding="ascii")
        path.chmod(mode)


def private_directory(path, mode, uid=0, gid=0):
    path.mkdir(mode=mode)
    path.chmod(mode)
    os.chown(path, uid, gid)


def data_layout(root, node):
    daemon = ROLES["daemon"]
    build = ROLES["build"]
    private_directory(root / "private", 0o700, daemon, daemon)
    private_directory(root / "probe", 0o755)
    (root / "probe/persistent").write_bytes(b"r2-persistent-v1\n")
    if node != "b":
        return
    private_directory(root / "home", 0o700, build, build)
    private_directory(root / "workspace", 0o755)
    slot = root / "workspace/ordinary"
    private_directory(slot, 0o755)
    private_directory(slot / "src", 0o755)
    private_directory(slot / "out", 0o700, build, build)
    for relative, contents in {
        "Cargo.toml": '[package]\nname="c14-fixture"\nversion="0.0.0"\nedition="2024"\n'
                      '[profile.dev]\ndebug=0\nincremental=false\npanic="abort"\ncodegen-units=1\n',
        "Cargo.lock": 'version = 4\n\n[[package]]\nname = "c14-fixture"\nversion = "0.0.0"\n',
        "src/main.rs": "fn main() {}\n",
    }.items():
        path = slot / relative
        path.write_text(contents, encoding="ascii")
        path.chmod(0o644)



def record_json(path, value, uid, mode):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, mode)
    with os.fdopen(fd, "w", encoding="ascii") as output:
        os.fchown(output.fileno(), uid, uid)
        os.fchmod(output.fileno(), mode)
        json.dump(value, output, separators=(",", ":"), ensure_ascii=True)


def broker_settings():
    daemon, build = ROLES["daemon"], ROLES["build"]
    return {
        "daemon_uid": daemon, "daemon_gid": daemon,
        "daemon_roots": ["/data/" + name for name in
                         ("cluster-work", "state", "credentials", "wrap", "history", "private", "activity")],
        "service": {
            "schema": "arxd-service/1", "client": {"identity": str(daemon)},
            "build_account": {"name": "arx-build", "identity": str(build), "gid": build},
            "rights": "standard", "workspace": "/data/workspace",
            "environment": {"PATH": "/fixture/tools:/usr/bin", "HOME": "/data/home",
                            "CARGO_HOME": "/data/home"},
            "limits": {"max_concurrent_runs": 1, "memory_bytes": INNER["memory"], "pids": INNER["pids"]},
            "agents": [{"principal": "qualification", "token_sha256": "0" * 64}],
            "granted_batch_right": False,
        },
    }


def node_endpoint(node, service):
    return f'{NETWORK["nodes"][node][0]}:{NETWORK["ports"][service]}'


def build_descriptor(slot="ordinary"):
    return {"workspace": slot, "tool": "cargo", "args": CARGO_ARGS, "timeout_s": 12, "output_bytes": 1024}


def install_live(root, node, *, founder_key=None):
    owner, daemon, build, agent = (ROLES[role] for role in ("owner", "daemon", "build", "agent"))
    require(node in ("a", "b", "c") and (node == "b") == (founder_key is not None), "installation-node")
    private = ("cluster-work", "state", "credentials", "wrap", "history", "private/custody")
    for name in private:
        private_directory(root / name, 0o700, daemon, daemon)
    settings = dict(zip(("workspace", "state", "credentials", "wrap", "history", "custody"),
                        ("/data/" + name for name in private)))
    settings.update(owner_socket="/run/c16/owner/socket", owner_uid=owner, owner_gid=owner,
                    daemon_uid=daemon, daemon_gid=daemon, build_uid=build, agent_uid=agent,
                    peer=node_endpoint(node, "peer"), enrollment=node_endpoint(node, "enrollment"),
                    founder_cluster=[16] * 16 if node == "a" else None,
                    issuer_peer=None if node == "a" else node_endpoint("a", "peer"),
                    executor_socket="/fixture/control/executor.sock" if node == "b" else None)
    if node != "c":
        private_directory(root / "activity", 0o700, daemon, daemon)
        private_directory(root / "clients", 0o755)
        agents = []
        for name in ("fixture", "foreign"):
            token = secrets.token_hex(32)
            agents.append({"uid": agent, "principal": name, "agent": name,
                           "token_sha256": hashlib.sha256(token.encode("ascii")).hexdigest()})
            record_json(root / "clients" / f"{name}.json", {
                "address": "/run/c16/mcp/socket", "expected_server_identity": str(daemon),
                "token": token, "delegated": {"owner_uid": owner, "daemon_uid": daemon, "agent_group": agent},
            }, agent, 0o400)
        grants = []
        if node == "b":
            require(len(founder_key) == 32 and all(type(b) is int and 0 <= b <= 255 for b in founder_key)
                    and any(founder_key), "founder-public-key")
            grants.append({"source": 1, "key": founder_key, "agent": "fixture",
                           "commands": [build_descriptor(slot) for slot in ("ordinary", "link", "writable")]})
            (root / "workspace/link").symlink_to("ordinary")
            private_directory(root / "workspace/writable", 0o775)
        settings["delegated_build"] = {
            "socket": "/run/c16/mcp/socket", "activity": "/data/activity", "peer": node_endpoint(node, "build"),
            "agent_group": agent, "agents": agents, "grants": grants,
            "routes": [{"node": 2 if node == "a" else 1, "address": node_endpoint("b" if node == "a" else "a", "build")}],
        }
    record_json(root / "install.json", settings, daemon, 0o400)

def host_file(path, roots, owners=(0,)):
    canonical = path.resolve(strict=True)
    require(any(canonical.is_relative_to(root) for root in roots), "host-file-closure")
    with open_artifact(canonical, owners, 512*MIB):
        return canonical


def stage_compiler_path(raw_value, *, resolve=None, links=("bin", "lib", "lib64")):
    """Canonicalize a GCC search-path lookup into a clean in-image manifest key."""
    require(isinstance(raw_value, str) and raw_value.startswith("/") and "\x00" not in raw_value
            and not raw_value.endswith("/") and not raw_value.endswith("/."), "compiler-input-path")
    parts = raw_value.split("/")
    stack = []
    traversed_dirs = []
    for segment in parts[1:]:
        require(segment != "" and segment != ".", "compiler-input-path")
        if segment == "..":
            require(bool(stack), "compiler-input-path")
            if stack[0] == "usr" or stack[0] in links:
                prefix = stack if stack[0] == "usr" else ["usr", *stack]
                traversed_dirs.append("/".join(prefix))
            stack.pop()
        else:
            stack.append(segment)
    require(len(stack) >= 2, "compiler-input-path")
    if stack[0] in links:
        normalized = PurePosixPath("/usr", *stack)
    else:
        normalized = PurePosixPath("/", *stack)
    require(normalized.is_relative_to(PurePosixPath("/usr")) and ".." not in normalized.parts,
            "compiler-input-path")
    relative = normalized.relative_to("/").as_posix()
    if resolve is not None:
        raw_canonical = resolve(raw_value)
        norm_canonical = resolve(normalized.as_posix())
        require(raw_canonical == norm_canonical, "compiler-input-path")
    return relative, traversed_dirs


def stage_rootfs(host, tree, stage, daemon, toolchain, host_uid, end, vetted, *, anchor=None):
    """Populate a merged-/usr tree from named inputs and their loader closure."""
    rootfs = host["rootfs"]
    require(rootfs["userland"] == host["host"]["os"], "offline-userland-input")
    for name in ("usr/bin", "usr/lib", "usr/lib64", "etc", "sys/fs/cgroup",
                 "proc", "dev", "run", "tmp", "data", "put_old", "fixture", "opt/rust"):
        (tree / name).mkdir(mode=0o755, parents=True, exist_ok=True)
    links = {"bin": "usr/bin", "lib": "usr/lib", "lib64": "usr/lib64"}
    for name, destination in links.items():
        (tree / name).symlink_to(destination)
    role_files(tree)
    hashes = {}
    usr = (Path("/usr"),)
    compiler = host_file(Path(vetted["cc"]), usr)
    binaries = {
        "usr/bin/sh": host_file(Path("/usr/bin/dash"), usr),
        "usr/bin/cc": compiler, "usr/bin/as": host_file(Path("/usr/bin/as"), usr),
        "usr/bin/ld": host_file(Path("/usr/bin/ld"), usr),
    }
    compiler_inputs = ("cc1", "collect2", "libgcc.a", "libgcc_s.so", "crtbegin.o", "crtbeginS.o",
                       "crtend.o", "crtendS.o", "crti.o", "crtn.o", "crt1.o", "Scrt1.o", "libc.so",
                       "libc_nonshared.a")
    if anchor:
        # The compiler version probes never reach GNU's linker scripts. The
        # actual Rust stdlib link uses these libc development entry points.
        compiler_inputs += ("libm.so", "libmvec.so.1", "libpthread.a", "libdl.a", "librt.a",
                            "libutil.a", "libresolv.so")
        for name in ("as", "ld"):
            target = f"x86_64-linux-gnu-{name}"
            binaries[f"usr/bin/{target}"] = host_file(Path("/usr/bin") / target, usr)
    compiler_exec = []
    for name in compiler_inputs:
        argument = "-print-prog-name" if name in ("cc1", "collect2") else "-print-file-name"
        value = capture([compiler, f"{argument}={name}"], end, limit=2048).decode("ascii").strip()
        relative, traversed_dirs = stage_compiler_path(value, resolve=lambda item: host_file(Path(item), usr))
        source = host_file(Path("/") / relative, usr)
        for directory in traversed_dirs:
            (tree / directory).mkdir(mode=0o755, parents=True, exist_ok=True)
        binaries[relative] = source
        if name in ("cc1", "collect2"):
            compiler_exec.append(source)
    for name in ("cargo", "rustc"):
        binaries[f"opt/rust/bin/{name}"] = host_file(toolchain / "bin" / name, (toolchain,), (0, host_uid))
    compiler_files = set(binaries)
    rust_libs = toolchain / "lib"
    allowed_rust = [*sorted(rust_libs.glob("*.so*"))]
    name = "x86_64-unknown-linux-gnu"
    for group in ("lib", "bin"):
        arch = rust_libs / "rustlib" / name / group
        require(arch.is_dir(), "rust-target")
        allowed_rust.extend(item for item in arch.rglob("*") if not item.is_dir())
    require(5 <= len(allowed_rust) <= 128, "rust-closure-count")
    for path in allowed_rust:
        binaries[str(Path("opt/rust") / path.relative_to(toolchain))] = host_file(path, (toolchain,), (0, host_uid))
    helper = stage / "bootstrap"
    host_command("cc", end, "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                 "-o", helper, stage / "bootstrap.c", tools=vetted, limit=8192)
    binaries["fixture/probe"] = helper
    for executable in [helper, compiler, *compiler_exec, binaries["usr/bin/sh"], binaries["usr/bin/as"],
                       binaries["usr/bin/ld"], binaries["opt/rust/bin/cargo"], binaries["opt/rust/bin/rustc"]]:
        # ldd runs only on the fixed compiler/toolchain and our inert helper. The
        # selected daemon never enters this execution path.
        env = {**ENV, "LD_LIBRARY_PATH": str(rust_libs)} if executable.is_relative_to(toolchain) else ENV
        closure = host_command("ldd", end, executable, env=env, tools=vetted, limit=8192)
        for record in closure.splitlines():
            if re.fullmatch(r"\s*linux-vdso\.so\.1 \(0x[0-9a-f]+\)", record):
                continue
            match = re.fullmatch(r"\s*(?:[\w.+-]+ => )?(/[^ ]+) \(0x[0-9a-f]+\)", record)
            require(match is not None, "loader-closure")
            source = Path(match[1])
            if source.is_relative_to(toolchain):
                relative = Path("opt/rust") / source.relative_to(toolchain)
            else:
                require(source.is_relative_to("/lib") or source.is_relative_to("/lib64")
                        or source.is_relative_to("/usr/lib") or source.is_relative_to("/usr/lib64"),
                        "loader-path")
                relative = source.relative_to("/")
                if relative.parts[0] in links:
                    relative = Path("usr") / relative
            binaries[str(relative)] = host_file(source, (toolchain, *usr), (0, host_uid))
    require(len(binaries) <= 256, "image-inventory")
    for relative, source in sorted(binaries.items()):
        path = tree / relative
        path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
        mode = 0o755 if source.stat().st_mode & 0o100 else 0o644
        hashes[relative] = stage_copy(source, path, end, (0, host_uid), mode=mode)
    hashes["fixture/arxd"] = stage_copy(daemon, tree / "fixture/arxd", end, (0, host_uid),
                                       elf=True, mode=0o755 if anchor else 0o400, limit=256*MIB)
    if anchor:
        hashes["fixture/c16-anchor"] = stage_copy(anchor[0], tree / "fixture/c16-anchor", end, (0, host_uid),
                                                 elf=True, mode=0o755, limit=256*MIB)
        require(hashes["fixture/c16-anchor"] == anchor[1], "anchor-digest-drift")
        (tree / "fixture/tools").mkdir(mode=0o755)
        wrapper = tree / "fixture/tools/cargo"
        shutil.copyfile(tree / "fixture/probe", wrapper)
        wrapper.chmod(0o755)
        hashes["fixture/tools/cargo"] = file_digest(wrapper, end, MIB)
        private_directory(tree / "fixture/broker", 0o700)
        private_directory(tree / "fixture/control", 0o755)
        record_json(tree / "fixture/broker/settings.json", broker_settings(), 0, 0o400)
        hashes["fixture/broker/settings.json"] = file_digest(tree / "fixture/broker/settings.json", end, 8192)
    require(compiler_files <= hashes.keys() and "usr/lib64/ld-linux-x86-64.so.2" in hashes,
            "image-loader")
    for path in sorted((tree / "etc").iterdir()):
        hashes[str(path.relative_to(tree))] = file_digest(path, end, 8192)
    manifest = {"files": hashes, "links": links}
    manifest_bytes = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode("ascii")
    (tree / "fixture/manifest").write_bytes(manifest_bytes)
    return manifest, hashlib.sha256(manifest_bytes).hexdigest()


def verify_manifest(root, manifest, digest, end):
    raw = bounded_read(root / "fixture/manifest", 32768)
    require(hashlib.sha256(raw).hexdigest() == digest
            and json.loads(raw, object_pairs_hook=unique_pairs) == manifest, "manifest-digest")
    for name, destination in manifest["links"].items():
        path = root / name
        require(path.is_symlink() and os.readlink(path) == destination
                and path.resolve(strict=True) == root / destination, "merged-usr")
    for name, expected in manifest["files"].items():
        require(file_digest(root / name, end, 512*MIB) == expected, "loaded-image-digest")


def filesystem_facts(stats, budget, *, writable):
    require(stats.f_frsize == budget["block_size"] and 0 < stats.f_bavail <= stats.f_blocks
            and 0 < stats.f_favail <= stats.f_files, "image-capacity")
    if writable:
        require(stats.f_files == budget["data_inodes"]
                and stats.f_blocks * stats.f_frsize <= budget["data_bytes"]
                and stats.f_bavail * stats.f_frsize >= 200*MIB
                and stats.f_favail >= 15000, "data-filesystem-limits")
    else:
        require(stats.f_blocks * stats.f_frsize <= budget["bytes"], "rootfs-filesystem-limit")
    return {"blocks_available": stats.f_bavail, "inodes_available": stats.f_favail}
