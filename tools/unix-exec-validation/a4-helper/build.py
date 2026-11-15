"""Fixed compile/verify recipe. Never executes the waiting image."""

import hashlib
import json
import os
from pathlib import Path
import stat
import struct
import subprocess
import sys

ROOT = Path(__file__).absolute().parent
LIMIT = 1024 * 1024
FLAGS = [
    "--edition=2024", "--target=x86_64-unknown-linux-gnu", "-D", "warnings",
    "-C", "panic=abort", "-C", "opt-level=z", "-C", "lto=fat",
    "-C", "codegen-units=1", "-C", "debuginfo=0", "-C", "overflow-checks=yes",
    "-C", "relocation-model=static", "-C", "linker-flavor=ld.lld",
    "-C", "link-arg=--build-id=none", "-C", "link-arg=-static",
]


def regular(path):
    """Reject links/reparse points for the fixed local inputs/output directory."""
    for part in [path, *path.parents]:
        if part.exists() or part.is_symlink():
            info = part.lstat()
            if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
                raise ValueError("linked/reparse build path")


def read(path):
    regular(path)
    with path.open("rb") as stream:
        data = stream.read(LIMIT + 1)
    if len(data) > LIMIT:
        raise ValueError("oversized input")
    return data


def sha(data):
    return hashlib.sha256(data).hexdigest()


def tool_sha(path):
    # Compiler/linker are large trusted installed tools; stream without logging paths.
    regular(path)
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def command(args):
    return subprocess.run(args, cwd=ROOT, check=True, capture_output=True,
                          timeout=60).stdout.decode("utf-8").replace("\r\n", "\n").strip()


def inventory(data):
    """Only the fixed little-endian ELF64 executable; bounded slices, no loader."""
    def region(offset, size):
        if offset < 0 or size < 0 or offset + size > len(data):
            raise ValueError("ELF range")
        return data[offset:offset + size]

    def unpack(fmt, offset):
        return struct.unpack(fmt, region(offset, struct.calcsize(fmt)))

    if region(0, 7) != b"\x7fELF\x02\x01\x01":
        raise ValueError("ELF identity")
    header = unpack("<HHIQQQIHHHHHH", 16)
    kind, machine, _, entry, phoff, shoff, _, ehsize, phsize, phnum, shsize, shnum, names = header
    if (kind, machine, ehsize, phsize, shsize) != (2, 62, 64, 56, 64):
        raise ValueError("ELF layout")
    if not 0 < phnum <= 16 or not 0 < shnum <= 64 or names >= shnum:
        raise ValueError("ELF table bounds")
    for i in range(phnum):
        p = unpack("<IIQQQQQQ", phoff + i * phsize)
        if p[0] in (2, 3, 7) or (p[1] & 3) == 3:
            raise ValueError("dynamic/interpreter/TLS or writable-executable segment")
    sections = [unpack("<IIQQQQIIQQ", shoff + i * shsize) for i in range(shnum)]
    strings = region(sections[names][4], sections[names][5])

    def name(table, offset):
        end = table.find(b"\0", offset)
        if offset >= len(table) or end < 0:
            raise ValueError("ELF name")
        return table[offset:end].decode("ascii")

    symbols = []
    for s in sections:
        label = name(strings, s[0])
        if s[1] in (4, 6, 9, 11, 14, 15, 16) or s[2] & 0x400:
            raise ValueError("relocation/dynamic/init/TLS section")
        if s[1] == 2:
            if s[9] != 24 or s[5] % 24 or s[6] >= shnum:
                raise ValueError("ELF symbols")
            table = sections[s[6]]
            table = region(table[4], table[5])
            for off in range(s[4] + 24, s[4] + s[5], 24):
                n, info, _, index, value, size = unpack("<IBBHQQ", off)
                text = name(table, n)
                if index == 0:
                    raise ValueError("undefined symbol")
                if any(word in text.lower() for word in ("fork", "exec", "thread", "atfork", "malloc", "tls")):
                    raise ValueError("forbidden symbol")
                symbols.append({"name": text, "type": info, "value": value, "size": size})
    if not any(s["name"] == "_start" and s["value"] == entry for s in symbols):
        raise ValueError("missing entry")
    return sorted(symbols, key=lambda s: (s["name"], s["value"]))


def main():
    if sys.argv[1:] not in (["build"], ["verify"], ["linux-candidate"]):
        raise ValueError("use build, verify or linux-candidate")
    # Neither the original build nor candidate compilation may run as root.
    if os.name != "nt" and (os.getuid() == 0 or os.geteuid() == 0):
        raise ValueError("unprivileged build required")
    candidate_mode = sys.argv[1] == "linux-candidate"
    recipe = json.loads(read(ROOT / "recipe.json"))
    if recipe["flags"] != FLAGS or sha(read(ROOT / "helper.rs")) != recipe["source_sha256"]:
        raise ValueError("unreviewed source or flags")
    identity = command(["rustc", "+1.95.0", "--version", "--verbose"])
    expected = recipe["compiler_identity"]
    if candidate_mode:
        if sys.platform != "linux":
            raise ValueError("Linux hosted compiler required")
        expected = expected.replace("host: x86_64-pc-windows-msvc",
                                    "host: x86_64-unknown-linux-gnu")
    if identity != expected:
        raise ValueError("compiler identity mismatch")
    sysroot = Path(command(["rustc", "+1.95.0", "--print", "sysroot"]))
    host = next(line[6:] for line in identity.splitlines() if line.startswith("host: "))
    suffix = ".exe" if os.name == "nt" else ""
    linker = sysroot / "lib" / "rustlib" / host / "bin" / ("rust-lld" + suffix)
    compiler = sysroot / "bin" / ("rustc" + suffix)
    out = ROOT / "out"
    regular(out)
    out.mkdir(exist_ok=True)
    image = out / "a4-helper"
    regular(image)
    if sys.argv[1] in ("build", "linux-candidate"):
        command(["rustc", "+1.95.0", *FLAGS, "-C", "linker=" + str(linker),
                 "helper.rs", "-o", "out/a4-helper"])
    data = read(image)
    observed = {"source_sha256": recipe["source_sha256"], "compiler_identity": identity,
                "compiler_sha256": tool_sha(compiler), "linker_sha256": tool_sha(linker),
                "flags": FLAGS, "image_sha256": sha(data), "image_size": len(data),
                "symbols": inventory(data)}
    if sys.argv[1] == "verify":
        if observed != json.loads(read(ROOT / "image-pin.json")):
            raise ValueError("build pin mismatch")
        print("fixed source/compiler/flags/image/size/symbol pin verified; image not run")
    else:
        candidate = out / "candidate-pin.json"
        regular(candidate)
        candidate.write_text(json.dumps(observed, indent=2) + "\n", encoding="utf-8")
        if candidate_mode:
            pin = json.loads(read(ROOT / "image-pin.json"))
            if any(observed[key] != pin[key] for key in
                   ("source_sha256", "flags", "image_sha256", "image_size", "symbols")):
                raise ValueError("Linux candidate differs from independently reviewed image")
        print("compile-only candidate written; reviewed image-pin.json unchanged")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError, KeyError, StopIteration, struct.error):
        sys.exit("fixed helper build/verification refused")
