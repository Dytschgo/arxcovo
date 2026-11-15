"""D7h fixed entry; no comparator import or package read before admission."""

import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import types

_MODULE_SHA256 = "12a122db7d30be04dd7523db046d49f4083882c318b375ccf0986ec0eb338e75"
_MODULE_SIZE = 11180
_DIGESTS = {
    "sudo": "6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff",
    "sudoers": "151d681d51293a040dc479cb0175d1a6d461a40ff455661cf6ad485adf305d4c",
    "util": "aae167b9da2436a31eba12d9fb20b36120fd204b2c720d62881d09e89eecd26d",
}
_REVISION = "1.9.15p5-3ubuntu5.24.04.4"
_REFUSAL = '{"schema":1,"status":"refused"}'


def _need(condition):
    if not condition:
        raise ValueError()


def _gate():
    e = os.environ
    for key, value in {
        "CI": "true", "GITHUB_ACTIONS": "true",
        "RUNNER_ENVIRONMENT": "github-hosted", "RUNNER_OS": "Linux",
        "RUNNER_ARCH": "X64", "ImageOS": "ubuntu24",
        "GITHUB_EVENT_NAME": "workflow_dispatch",
        "GITHUB_REPOSITORY": "Dytschgo/arxcovo",
        "GITHUB_ACTOR": "Dytschgo", "GITHUB_TRIGGERING_ACTOR": "Dytschgo",
        "GITHUB_REF": "refs/heads/main", "GITHUB_RUN_ATTEMPT": "1",
        "GITHUB_WORKFLOW_REF": "Dytschgo/arxcovo/.github/workflows/sudo-package-readonly.yml@refs/heads/main",
    }.items():
        _need(e.get(key) == value)
    sha = e.get("GITHUB_SHA", "")
    _need(re.fullmatch(r"[0-9a-f]{40}", sha) is not None)
    _need(sha == e.get("SUDO_PACKAGE_REVIEWED_SHA") == e.get("GITHUB_WORKFLOW_SHA"))
    run = e.get("GITHUB_RUN_ID", "")
    _need(re.fullmatch(r"[1-9][0-9]{0,19}", run) is not None)
    _need(sys.platform == "linux" and len(sys.argv) == 1)
    _need(os.getuid() == 0 and os.geteuid() == 0)
    workspace = e.get("GITHUB_WORKSPACE", "")
    _need(workspace.startswith("/") and ".." not in workspace.split("/"))
    root = Path(workspace)
    _need(str(root) == workspace)
    _need(Path(__file__) == root / "tools/ci/sudo-package-readonly.py")
    return {"source_sha": sha, "workflow_sha": e["GITHUB_WORKFLOW_SHA"],
            "run": run, "attempt": "1"}


def _module_bytes():
    # Fixed __file__ sibling only, never argv/env/import-path supplied. Hold
    # nofollow directories from /; reject links, nonregular files and drift.
    path = Path(__file__).parent.parent / "unix-exec-validation/sudo_package_profile.py"
    held = []
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
    try:
        parent = os.open("/", flags | os.O_DIRECTORY)
        held.append(parent)
        for part in path.parts[1:-1]:
            parent = os.open(part, flags | os.O_DIRECTORY, dir_fd=parent)
            held.append(parent)
        fd = os.open(path.name, flags, dir_fd=parent)
        held.append(fd)
        before = os.fstat(fd)
        _need(stat.S_ISREG(before.st_mode) and before.st_nlink == 1
              and before.st_size == _MODULE_SIZE)
        data = bytearray()
        for _ in range((_MODULE_SIZE + 4095) // 4096 + 1):
            count = min(4096, _MODULE_SIZE + 1 - len(data))
            block = os.read(fd, count)
            _need(type(block) is bytes and len(block) <= count)
            if not block:
                break
            data.extend(block)
            _need(len(data) <= _MODULE_SIZE)
        _need(not block and len(data) == _MODULE_SIZE)
        after = os.fstat(fd)
        # Reading may update atime; identity, content and ownership may not drift.
        _need(all(getattr(after, key) == getattr(before, key) for key in (
            "st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink",
            "st_size", "st_mtime_ns", "st_ctime_ns")))
        _need(hashlib.sha256(data).hexdigest() == _MODULE_SHA256)
        return bytes(data)
    finally:
        close_failed = False
        for fd in reversed(held):
            try:
                os.close(fd)
            except Exception:
                close_failed = True
        _need(not close_failed)


def _load_comparator():
    # Compile the exact bounded, hashed bytes; no pycache or search-path import.
    source = _module_bytes()
    module = types.ModuleType("_d7h_fixed_comparator")
    exec(compile(source, "sudo_package_profile.py", "exec"), module.__dict__)
    return module


def _comparison(value):
    # Reconstruct a closed result; no strings from the private reader escape.
    _need(type(value) is dict and set(value) == {
        "schema", "status", "candidate_match", "qualified", "revision",
        "architecture", "digests"})
    _need(type(value["schema"]) is int and value["schema"] == 1)
    _need(type(value["qualified"]) is bool and value["qualified"] is False)
    _need(type(value["candidate_match"]) is bool)
    match = value["candidate_match"]
    _need(type(value["status"]) is str
          and value["status"] == ("candidate_match" if match else "refused"))
    _need(type(value["revision"]) is str and value["revision"] == _REVISION)
    _need(type(value["architecture"]) is str and value["architecture"] == "amd64")
    expected = _DIGESTS if match else dict.fromkeys(_DIGESTS)
    _need(type(value["digests"]) is dict and value["digests"] == expected)
    return {"schema": 1, "status": "candidate_match" if match else "refused",
            "candidate_match": match, "qualified": False,
            "revision": _REVISION, "architecture": "amd64", "digests": expected}


def _main():
    try:
        binding = _gate()
        comparator = _load_comparator()
        result = _comparison(comparator._read_linux())
        output = json.dumps({"schema": 1, "binding": binding,
                             "comparison": result}, sort_keys=True, separators=(",", ":"))
        code = 0 if result["candidate_match"] else 2
    except Exception:
        output, code = _REFUSAL, 2
    print(output)
    return code


if __name__ == "__main__":
    raise SystemExit(_main())
