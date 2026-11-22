"""Bounded R2 staging and host command primitives."""

import hashlib
import json
import os
from pathlib import Path
import re
import selectors
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
    "nodes": {node: [f"198.18.72.{i}", f"02:72:00:00:00:{i}"] for i, node in enumerate("abc", 11)},
    "prefix": 24, "ports": {"peer": 22171, "enrollment": 22172, "build": 22173, "forbidden": 22174},
    "local": ["198.18.72.14", "02:72:00:00:00:14"],
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
        "roles": ROLES, "network": NETWORK, "outer": OUTER, "inner": INNER, "seconds": TIME,
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
    with subprocess.Popen(argv, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, start_new_session=True) as child:
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
            require(child.wait(timeout=remaining(end)) in statuses, "command-status")
            return bytes(result)
        except BaseException:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGKILL)
            raise


def checked_output(result, block, limit):
    require(len(result) + len(block) <= limit, "command-output-bound")
    result.extend(block)


def host_command(tool, end, *args, **options):
    require(tool in TOOLS, "tool-identity")
    try:
        return capture([TOOLS[tool], *map(str, args)], end, **options).decode("ascii")
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


def select_daemon(target, workspace, owner, loader_receipt, end):
    require(target == workspace / "target" and loader_receipt.name == "build-messages.jsonl", "artifact-receipt")
    path = target / "debug/arxd"
    with open_artifact(loader_receipt, (0, owner), 64*MIB) as (_, receipt):
        with open_artifact(path, (0, owner), 256*MIB, executable=True) as (binary, value):
            require(value.st_mtime_ns >= receipt.st_mtime_ns, "stale-artifact")
            inspect_elf(binary, value.st_size)
            digest = stream_hash(binary, value.st_size, end)
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


def host_file(path, roots, owners=(0,)):
    canonical = path.resolve(strict=True)
    require(any(canonical.is_relative_to(root) for root in roots), "host-file-closure")
    with open_artifact(canonical, owners, 512*MIB):
        return canonical


def stage_rootfs(host, tree, stage, daemon, toolchain, host_uid, end):
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
    compiler = host_file(Path(TOOLS["cc"]), usr)
    binaries = {
        "usr/bin/sh": host_file(Path("/usr/bin/dash"), usr),
        "usr/bin/cc": compiler, "usr/bin/as": host_file(Path("/usr/bin/as"), usr),
        "usr/bin/ld": host_file(Path("/usr/bin/ld"), usr),
    }
    compiler_inputs = ("cc1", "collect2", "libgcc.a", "libgcc_s.so", "crtbegin.o", "crtbeginS.o",
                       "crtend.o", "crtendS.o", "crti.o", "crtn.o", "crt1.o", "Scrt1.o", "libc.so",
                       "libc_nonshared.a")
    compiler_exec = []
    for name in compiler_inputs:
        argument = "-print-prog-name" if name in ("cc1", "collect2") else "-print-file-name"
        value = capture([compiler, f"{argument}={name}"], end, limit=2048).decode("ascii").strip()
        source = host_file(Path(value), usr)
        relative = str(Path(value).relative_to("/"))
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
                 "-o", helper, stage / "bootstrap.c", limit=8192)
    binaries["fixture/probe"] = helper
    for executable in [helper, compiler, *compiler_exec, binaries["usr/bin/sh"], binaries["usr/bin/as"],
                       binaries["usr/bin/ld"], binaries["opt/rust/bin/cargo"], binaries["opt/rust/bin/rustc"]]:
        # ldd runs only on the fixed compiler/toolchain and our inert helper. The
        # selected daemon never enters this execution path.
        env = {**ENV, "LD_LIBRARY_PATH": str(rust_libs)} if executable.is_relative_to(toolchain) else ENV
        closure = host_command("ldd", end, executable, env=env, limit=8192)
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
                                       elf=True, mode=0o400, limit=256*MIB)
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
