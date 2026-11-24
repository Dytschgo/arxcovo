"""Hosted Windows gate for the qualification authority fence.

Stable rustdoc passes a ``compile_fail`` doctest on any compile error and
does not enforce the error code in its tag. This gate compiles every fenced
block from the ``Baseline`` documentation against the built arxd library.

- A ``compile_fail,EXXXX`` block must report that code. Its only other
  errors may be code-less private-type errors, which come from the private
  types of the struct's own fields.
- The ``no_run`` positive control must type-check with no error, so each
  refusal is the fence and not a missing path.

Everything is read-only. Scratch output goes to a temporary directory.
"""
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
SOURCE = ROOT / "crates" / "arxd" / "src" / "vm" / "windows" / "qualification.rs"
EDITION = "2024"
OPEN = re.compile(r"\s*/// ```(\S+)")
CLOSE = re.compile(r"\s*/// ```")
PRIVATE_TYPE = re.compile(r"type `[A-Za-z0-9_:]+` is private")
# The exact set of fenced blocks the gate owns, in source order.
EXPECTED_TAGS = [
    "compile_fail,E0432",
    "compile_fail,E0451",
    "compile_fail,E0277",
    "compile_fail,E0277",
    "compile_fail,E0599",
    "no_run",
]


def blocks(text):
    """Every tagged fenced block in ``///`` comments, as (tag, body)."""
    found = []
    lines = text.splitlines()
    index = 0
    while index < len(lines):
        opened = OPEN.fullmatch(lines[index])
        index += 1
        if not opened:
            continue
        body = []
        while index < len(lines) and not CLOSE.fullmatch(lines[index]):
            body.append(re.sub(r"^\s*///(?: |$)", "", lines[index]))
            index += 1
        if index == len(lines):
            raise ValueError("unterminated doc block")
        index += 1
        found.append((opened.group(1), "\n".join(body)))
    return found


def expected_codes(tag):
    """The error codes a block must report; empty for the positive control."""
    if tag == "no_run":
        return []
    matched = re.fullmatch(r"compile_fail,(E\d{4})", tag)
    if not matched:
        raise ValueError(f"unsupported doc block tag {tag!r}")
    return [matched.group(1)]


def program(body):
    """Wrap a block as rustdoc does when it has no ``main``."""
    if re.search(r"\bfn main\b", body):
        return body + "\n"
    return "fn main() {\n" + body + "\n}\n"


def errors(stderr):
    """The error diagnostics in rustc's JSON output, without the summary."""
    found = []
    for line in stderr.splitlines():
        if not line.startswith("{"):
            continue
        diagnostic = json.loads(line)
        if diagnostic.get("$message_type", "diagnostic") != "diagnostic":
            continue
        if diagnostic.get("level") != "error":
            continue
        if diagnostic.get("message", "").startswith("aborting due to"):
            continue
        found.append(((diagnostic.get("code") or {}).get("code"), diagnostic.get("message", "")))
    return found


def verdict(tag, found):
    """None when the block failed exactly as declared, else the reason."""
    codes = expected_codes(tag)
    if not codes:
        return None if not found else f"positive control does not compile: {found}"
    if not found:
        return "compiled, but the fence must refuse it"
    reported = {code for code, _ in found}
    missing = [code for code in codes if code not in reported]
    stray = [
        (code, message)
        for code, message in found
        if code not in codes and not (code is None and PRIVATE_TYPE.fullmatch(message))
    ]
    if missing:
        return f"declared code {missing} not reported; got {found}"
    if stray:
        return f"failed for another reason: {stray}"
    return None


def library():
    """The arxd library rlib and its dependency directory, from Cargo."""
    built = subprocess.run(
        ["cargo", "build", "--locked", "-p", "arxd", "--lib", "--message-format=json"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    rlibs = []
    deps = set()
    for line in built.stdout.splitlines():
        if not line.startswith("{"):
            continue
        message = json.loads(line)
        target = message.get("target") or {}
        if (
            message.get("reason") == "compiler-artifact"
            and target.get("name") == "arxd"
            and "lib" in target.get("kind", [])
        ):
            for name in message.get("filenames", []):
                path = Path(name)
                if path.suffix == ".rlib":
                    rlibs.append(path)
                # The uplifted rlib sits beside, not inside, its dependencies.
                if path.parent.name == "deps":
                    deps.add(path.parent)
    if len(rlibs) != 1 or len(deps) != 1:
        raise RuntimeError(f"expected one arxd rlib and deps directory: {rlibs} {deps}")
    return rlibs[0], deps.pop()


def compile_block(body, rlib, deps, scratch, index):
    source = scratch / f"block{index}.rs"
    source.write_text(program(body), encoding="utf-8")
    run = subprocess.run(
        [
            "rustc",
            "--edition",
            EDITION,
            "--crate-type",
            "bin",
            "--crate-name",
            f"fence{index}",
            "--emit=metadata",
            "--error-format=json",
            "-A",
            "warnings",
            "-o",
            str(scratch / f"block{index}.rmeta"),
            "--extern",
            f"arxd={rlib}",
            "-L",
            f"dependency={deps}",
            str(source),
        ],
        cwd=scratch,
        capture_output=True,
        text=True,
    )
    return errors(run.stderr)


def main():
    found = blocks(SOURCE.read_text(encoding="utf-8"))
    tags = [tag for tag, _ in found]
    if tags != EXPECTED_TAGS:
        print(f"authority fence blocks changed: {tags}", file=sys.stderr)
        return 1
    rlib, deps = library()
    failures = 0
    with tempfile.TemporaryDirectory(prefix="arx-fence-") as directory:
        for index, (tag, body) in enumerate(found):
            reason = verdict(tag, compile_block(body, rlib, deps, Path(directory), index))
            status = "ok" if reason is None else f"FAILED: {reason}"
            print(f"block {index} ({tag}): {status}")
            failures += reason is not None
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
