"""Hosted Windows gate for the qualification authority fence.

Stable rustdoc passes a ``compile_fail`` doctest on any compile error and
does not enforce the error code in its tag. This gate compiles every fenced
block from the ``Baseline`` documentation against the built arxd library.

- A ``compile_fail,EXXXX`` block passes only if rustc exits nonzero and
  reports that code as a structured diagnostic. Its only other errors may
  be code-less private-type errors, which come from the private types of
  the struct's own fields.
- The ``no_run`` positive control passes only if rustc exits 0 with no
  error, so each refusal is the fence and not a missing path.
- With ``--error-format=json`` every nonempty stderr line is a JSON
  diagnostic. Any other nonempty line, or one that does not parse, fails
  the block, whatever its tag and exit status.

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
MANIFEST = ROOT / "crates" / "arxd" / "Cargo.toml"
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
    "compile_fail,E0599",
    "compile_fail,E0560",
    "no_run",
    "compile_fail,E0423",
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
    """The error diagnostics in rustc's JSON output, without the summary.
    Every nonempty line must be a JSON object; a plain-text line or one that
    does not parse raises ValueError."""
    found = []
    for line in stderr.splitlines():
        if not line.strip():
            continue
        if not line.startswith("{"):
            raise ValueError(f"non-JSON compiler output: {line[:120]!r}")
        diagnostic = json.loads(line)
        if not isinstance(diagnostic, dict):
            raise ValueError("diagnostic is not an object")
        if diagnostic.get("$message_type", "diagnostic") != "diagnostic":
            continue
        if diagnostic.get("level") != "error":
            continue
        if diagnostic.get("message", "").startswith("aborting due to"):
            continue
        found.append(((diagnostic.get("code") or {}).get("code"), diagnostic.get("message", "")))
    return found


def verdict(tag, status, found):
    """None when rustc's exit status and structured errors match the block's
    declaration, else the reason."""
    codes = expected_codes(tag)
    if not codes:
        if status != 0:
            return f"positive control: rustc exited {status}: {found}"
        return None if not found else f"positive control does not compile: {found}"
    if status == 0:
        return "compiled, but the fence must refuse it"
    if not found:
        return f"rustc exited {status} without a structured diagnostic"
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


def judge(tag, status, stderr):
    """The block's verdict from rustc's exit status and its complete stderr."""
    try:
        return verdict(tag, status, errors(stderr))
    except ValueError as error:
        return f"unparseable compiler output: {error}"


def doctest_library(output):
    """Find the workspace arxd rlib in the doctest build's Cargo messages."""
    rlibs = []
    build_finished = False
    for line in output.splitlines():
        if build_finished or not line.startswith("{"):
            print(line)
            continue
        message = json.loads(line)
        if message.get("reason") == "build-finished":
            build_finished = True
        target = message.get("target") or {}
        if (
            message.get("reason") == "compiler-artifact"
            and Path(message["manifest_path"]) == MANIFEST
            and target.get("name") == "arxd"
            and "lib" in target.get("kind", [])
            and message["profile"]["test"] is False
        ):
            for name in message.get("filenames", []):
                path = Path(name)
                if path.suffix == ".rlib" and path.parent.name == "deps":
                    rlibs.append(path)
    if len(rlibs) != 1:
        raise RuntimeError(f"expected one arxd doctest rlib in deps: {rlibs}")
    return rlibs[0]


def library():
    """Run doctests and reuse their library, which nextest normally builds."""
    tested = subprocess.run(
        ["cargo", "test", "-p", "arxd", "--doc", "--locked", "--message-format=json-render-diagnostics"],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        encoding="utf-8",
    )
    if tested.returncode:
        print(tested.stdout, end="")
        tested.check_returncode()
    return doctest_library(tested.stdout)


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
    return run.returncode, run.stderr


def main():
    found = blocks(SOURCE.read_text(encoding="utf-8"))
    tags = [tag for tag, _ in found]
    if tags != EXPECTED_TAGS:
        print(f"authority fence blocks changed: {tags}", file=sys.stderr)
        return 1
    rlib = library()
    failures = 0
    with tempfile.TemporaryDirectory(prefix="arx-fence-") as directory:
        for index, (tag, body) in enumerate(found):
            status, stderr = compile_block(body, rlib, rlib.parent, Path(directory), index)
            reason = judge(tag, status, stderr)
            outcome = "ok" if reason is None else f"FAILED: {reason}"
            print(f"block {index} ({tag}): {outcome}")
            failures += reason is not None
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
