"""Bounded source/artifact hashes only. Does not launch any executable or use network.

Commit argument is a coordinator pin, not independently verified HEAD evidence.
Runtime authorization is deliberately never emitted by this preparation utility.
"""
import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).absolute().parents[2]
SOURCES = (
    "crates/arxd/src/development/windows.rs", "Cargo.lock", "rust-toolchain.toml",
    "tools/windows-native-replay/child/Cargo.toml",
    "tools/windows-native-replay/child/Cargo.lock",
    "tools/windows-native-replay/child/src/main.rs",
    "tools/windows-native-replay/Invoke-BoundedReplay.ps1",
    "tools/windows-native-replay/Controller.cs",
    "tools/windows-native-replay/Prepare-Provenance.py",
    "tools/windows-native-replay/Check-Source.py",
    "tools/windows-native-replay/selectors.json",
    "tools/windows-native-replay/Test-PureController.ps1",
    "tools/windows-native-replay/PINNING.md",
)


def bounded_hash(relative, limit):
    # Fixed repository-relative paths only; never resolve a user supplied path.
    for ancestor in reversed((ROOT, *ROOT.parents)):
        if getattr(ancestor.lstat(), "st_file_attributes", 0) & 0x400:
            raise ValueError("unsafe repository ancestor")
    path = ROOT
    for component in Path(relative).parts:
        path = path / component
        if path.is_symlink() or path.is_junction() or getattr(path.lstat(), "st_file_attributes", 0) & 0x400:
            raise ValueError("unsafe provenance path")
    if path.stat().st_nlink != 1:
        raise ValueError("hardlinked provenance input")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError("oversized provenance input")
    return hashlib.sha256(data).hexdigest()


def main():
    if len(sys.argv) != 2 or re.fullmatch(r"[0-9a-f]{40}", sys.argv[1]) is None:
        raise ValueError("expected full source commit pin")
    report = {"schema": 1, "source_commit_claim": sys.argv[1],
              "runtime_authority": False, "native_executed": False,
              "source_sha256": {s: bounded_hash(s, 2 * 1024 * 1024) for s in SOURCES}}
    # No wildcard test-executable discovery. Its exact path needs the later
    # reviewed disposition; do not guess a hashed Cargo artifact name here.
    helper = "target/windows-native-replay-child/x86_64-pc-windows-msvc/debug/windows-native-replay-child.exe"
    report["helper_sha256"] = bounded_hash(helper, 16 * 1024 * 1024)
    print(json.dumps(report, sort_keys=True, separators=(",", ":")))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError):
        print("provenance unavailable", file=sys.stderr)
        sys.exit(1)
