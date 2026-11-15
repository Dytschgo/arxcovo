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



def image_layout(data):
    """Bounded static ELF facts for an already inventory-validated image."""
    h = struct.unpack_from("<HHIQQQIHHHHHH", data, 16)
    ph = [list(struct.unpack_from("<IIQQQQQQ", data, h[4] + i * h[8]))
          for i in range(h[9])]
    sections = [struct.unpack_from("<IIQQQQIIQQ", data, h[5] + i * h[10])
                for i in range(h[11])]
    names = sections[h[12]]
    names = data[names[4]:names[4] + names[5]]
    allowed = ("", ".text", ".rodata", ".eh_frame", ".comment", ".symtab",
               ".strtab", ".shstrtab", ".bss", ".data", ".eh_frame_hdr")
    result = []
    for section in sections:
        end = names.find(b"\0", section[0])
        name = names[section[0]:end].decode("ascii")
        result.append({"name": name if name in allowed else "other",
                       "type": section[1], "flags": section[2], "address": section[3],
                       "size": section[5], "alignment": section[8],
                       "sha256": None if section[1] == 8 else
                       sha(data[section[4]:section[4] + section[5]])})
    loads = []
    for segment in ph:
        if segment[0] == 1:
            start, size = segment[2], segment[5]
            mapped = bytearray(data[start:start + size])
            # Comparison evidence only: e_shoff locates the non-runtime section
            # table. Keep every other loaded byte, including all loader fields.
            if start <= 40 and start + size >= 48:
                mapped[40 - start:48 - start] = bytes(8)
            loads.append({"offset": start, "size": size, "sha256": sha(mapped)})
    return {"entry": h[3], "elf_header": list(h), "program_headers": ph,
            "loads_without_section_table_offset": loads, "sections": result}


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
            pin = json.loads(read(ROOT / "linux-image-pin.json"))
            keys = ("source_sha256", "flags", "image_sha256", "image_size", "symbols")
            matches = {key: observed[key] == pin[key] for key in keys}
            if not all(matches.values()):
                # Public artifact facts only, never paths, raw bytes or compiler output.
                print(json.dumps({"kind": "a4-compile-comparison", "matches": matches,
                                  "image_sha256": observed["image_sha256"],
                                  "image_size": observed["image_size"],
                                  "symbol_count": len(observed["symbols"]),
                                  "compiler_sha256": observed["compiler_sha256"],
                                  "linker_sha256": observed["linker_sha256"],
                                  "layout": image_layout(data),
                                  "symbol_names_match": [s["name"] for s in observed["symbols"]]
                                  == [s["name"] for s in pin["symbols"]]}, sort_keys=True))
                raise ValueError("Linux candidate differs from independently reviewed image")
        print("compile-only candidate written; reviewed pins unchanged")


def refusal_label(error):
    # Closed labels only: never print exception text, paths or compiler output.
    known = {
        "unprivileged build required": "privilege",
        "unreviewed source or flags": "source",
        "compiler identity mismatch": "compiler-identity",
        "Linux hosted compiler required": "compiler-host",
        "linked/reparse build path": "linked-path",
        "oversized input": "input-limit",
        "build pin mismatch": "build-pin",
        "Linux candidate differs from independently reviewed image": "image-pin",
    }
    if isinstance(error, ValueError):
        return known.get(str(error), "format")
    if isinstance(error, subprocess.SubprocessError):
        return "compiler-command"
    if isinstance(error, OSError):
        return "io"
    return "format"


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError, KeyError, StopIteration, struct.error) as error:
        sys.exit("fixed helper build/verification refused: " + refusal_label(error))
