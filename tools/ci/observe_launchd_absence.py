"""Preparation-only, fixed launchd absence observation. Never a runtime parser."""
import json
import os
import plistlib
import re
import stat
import subprocess
import sys
import time
from dataclasses import dataclass, field

ARGV = ("/bin/launchctl", "print", "system/com.arxcovo.arxd")
ENV = {"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C"}
CAPS = (65536, 4096)
AGGREGATE = 69632
# Synthetic literal proposal, NOT a native fixture or production grammar.
# Fresh independent review is required before executing/publishing this source.
CANDIDATE = (113, b"", b'Bad request.\nCould not find service "com.arxcovo.arxd" in domain for system\n')


@dataclass(repr=False)
class Capture:
    status: str = "spawn-error"
    code: int | None = None
    stdout: bytes = field(default=b"", repr=False)
    stderr: bytes = field(default=b"", repr=False)
    eof: list = field(default_factory=lambda: [False, False])
    truncated: list = field(default_factory=lambda: [False, False])
    cleanup: str = "not-needed"


def capture():
    """Only the fixed argv; nonblocking fair dual-stream reads, no selectors."""
    result = Capture()
    buffers = [bytearray(), bytearray()]
    deadline = time.monotonic() + 2.0
    try:
        child = subprocess.Popen(list(ARGV), stdin=subprocess.DEVNULL,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 shell=False, cwd="/", bufsize=0, env=dict(ENV))
    except (OSError, ValueError):
        return result
    pipes = (child.stdout, child.stderr)
    try:
        result.status = "setup-error"
        for pipe in pipes:
            os.set_blocking(pipe.fileno(), False)
        result.status = "timeout"
        while time.monotonic() < deadline:
            # One <=1024 byte read per stream per pass prevents a flood starving
            # the other stream, process polling, or the absolute deadline.
            for index, pipe in enumerate(pipes):
                if result.eof[index]:
                    continue
                try:
                    chunk = os.read(pipe.fileno(), min(1024, CAPS[index] - len(buffers[index]) + 1))
                except BlockingIOError:
                    continue
                if not chunk:
                    result.eof[index] = True
                elif len(buffers[index]) + len(chunk) > CAPS[index] or sum(map(len, buffers)) + len(chunk) > AGGREGATE:
                    result.truncated[index] = True
                    result.status = "overflow"
                    break
                else:
                    buffers[index].extend(chunk)
            if result.status == "overflow":
                break
            result.status = "wait-error"
            code = child.poll()
            result.status = "timeout"
            if code is not None:
                result.code = code if code >= 0 else None
                if all(result.eof) and time.monotonic() < deadline:
                    result.status = "completed" if code >= 0 else "signalled"
                    break
            time.sleep(min(0.005, max(0, deadline - time.monotonic())))
    except (OSError, ValueError, AttributeError):
        if result.status not in ("setup-error", "wait-error"):
            result.status = "read-error"
    finally:
        # A single cleanup budget includes kill, wait and pipe closure. Blocking
        # OS spawn/kill/close latency is not preemptible by this Python deadline.
        cleanup_deadline = time.monotonic() + 1.0
        if result.status != "completed":
            result.cleanup = "reaped"
            try:
                child.kill()
            except ProcessLookupError:
                pass
            except OSError:
                result.cleanup = "kill-error"
            try:
                child.wait(timeout=max(0, cleanup_deadline - time.monotonic()))
            except subprocess.TimeoutExpired:
                result.cleanup = "reap-timeout"
            except OSError:
                result.cleanup = "wait-error"
        for pipe in pipes:
            try:
                pipe.close()
            except (OSError, AttributeError):
                result.status = "close-error"
        result.stdout, result.stderr = map(bytes, buffers)
    return result


def summarize(result):
    """Consume whole streams; serialize constants/facts only, never substrings."""
    complete = (result.status == "completed" and all(result.eof)
                and not any(result.truncated)
                and len(result.stdout) <= CAPS[0] and len(result.stderr) <= CAPS[1]
                and len(result.stdout) + len(result.stderr) <= AGGREGATE)
    candidate = complete and (result.code, result.stdout, result.stderr) == CANDIDATE
    return {"status": result.status, "exit_code": result.code,
            "stdout_eof": result.eof[0], "stderr_eof": result.eof[1],
            "stdout_truncated": result.truncated[0], "stderr_truncated": result.truncated[1],
            "cleanup": result.cleanup, "query": "Unknown", "fixture_credit": False,
            "tuple": "synthetic-candidate-1" if candidate else "unsupported",
            "diagnostics": "withheld"}


def provenance():
    """Bounded fixed public OS plist; no account/home/config or native command."""
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
    fd = os.open("/System/Library/CoreServices/SystemVersion.plist", flags)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_size > 16384:
            raise ValueError
        with os.fdopen(fd, "rb", closefd=False) as stream:
            raw = stream.read(16385)
        if len(raw) > 16384:
            raise ValueError
        data = plistlib.loads(raw)
        version, build = data.get("ProductVersion", ""), data.get("ProductBuildVersion", "")
    finally:
        os.close(fd)
    # Strict formats, no arbitrary strings or inherited environment dump.
    values = {"os_version": (version, r"[0-9]{1,2}(\.[0-9]{1,3}){1,2}"),
              "os_build": (build, r"[0-9]{2}[A-Z][0-9]{1,6}[a-z]?"),
              "source_pin": (os.environ.get("GITHUB_SHA", ""), r"[0-9a-f]{40}"),
              "runner_image": (os.environ.get("ImageOS", ""), r"macos[0-9]{2}(-arm64)?"),
              "runner_version": (os.environ.get("ImageVersion", ""), r"[0-9]{8}\.[0-9]{1,5}\.[0-9]{1,5}")}
    if any(not isinstance(v, str) or re.fullmatch(pattern, v) is None for v, pattern in values.values()):
        raise ValueError
    return {"host_os": "macOS", **{key: v for key, (v, _) in values.items()},
            "metadata_authority": "public-runner-claims", "locale": "C",
            "query_seconds": 2, "cleanup_seconds": 1,
            "stdout_cap": CAPS[0], "stderr_cap": CAPS[1], "aggregate_cap": AGGREGATE}


def main():
    if len(sys.argv) != 1:
        print("launchd-observation-arguments", file=sys.stderr)
        return 2
    if sys.platform != "darwin":
        print("launchd-observation-platform", file=sys.stderr)
        return 2
    try:
        metadata = provenance()
    except (OSError, ValueError, TypeError, plistlib.InvalidFileException):
        print("launchd-observation-provenance", file=sys.stderr)
        return 2
    report = {"schemaVersion": 1, "target": "system/com.arxcovo.arxd",
              "provenance": metadata, "observation": summarize(capture())}
    print(json.dumps(report, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
