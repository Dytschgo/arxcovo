"""Bounded read-only manager queries; output contains fixed labels and shapes."""
import json
import os
import re
import selectors
import stat
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

CAP = 16384
TIMEOUT = 6
SYSTEMD_ARGS = ("show", "--property=LoadState,ActiveState,FragmentPath,DropInPaths", "arxd.service")
MAC_TARGETS = (("arxcovo", "system/com.arxcovo.arxd"),
               ("apple_syslog_control", "system/com.apple.syslogd"))
R2_SYSTEMD_ARGS = ("/usr/bin/dpkg-query", "-W", "-f=${Version}", "systemd")
R2_TOOLS = (
    "/usr/bin/gcc", "/usr/sbin/nft", "/usr/sbin/ip", "/usr/sbin/mkfs.ext4",
    "/usr/sbin/losetup", "/usr/bin/systemd-run", "/usr/bin/systemctl",
    "/usr/bin/mount", "/usr/bin/umount", "/usr/bin/findmnt", "/usr/bin/ldd",
    "/usr/bin/dpkg-query", "/usr/bin/git",
)


@dataclass
class Capture:
    status: str
    code: int | None = None
    stdout: bytes = b""
    stderr: bytes = b""


def capture(argv, timeout=TIMEOUT):
    """Drain both pipes with one combined cap and a wall-clock deadline."""
    try:
        child = subprocess.Popen(
            argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, shell=False, cwd="/", bufsize=0,
            env={"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C",
                 "SYSTEMD_PAGER": "cat", "SYSTEMD_COLORS": "0"},
        )
    except OSError:
        return Capture("unavailable")
    out = [bytearray(), bytearray()]
    result = Capture("unavailable")
    try:
        with selectors.DefaultSelector() as selector:
            for index, pipe in enumerate((child.stdout, child.stderr)):
                os.set_blocking(pipe.fileno(), False)
                selector.register(pipe, selectors.EVENT_READ, index)
            deadline = time.monotonic() + timeout
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    result = Capture("timeout")
                    break
                if not selector.get_map() and child.poll() is not None:
                    code = child.returncode
                    result = Capture("completed", code if -128 <= code <= 255 else None,
                                     bytes(out[0]), bytes(out[1]))
                    break
                for key, _ in selector.select(min(remaining, 0.1)):
                    try:
                        chunk = os.read(key.fileobj.fileno(), 4096)
                    except BlockingIOError:
                        continue
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    if len(out[0]) + len(out[1]) + len(chunk) > CAP:
                        raise OverflowError
                    out[key.data].extend(chunk)
    except OverflowError:
        result = Capture("overflow")
    except OSError:
        result = Capture("unavailable")
    finally:
        if child.poll() is None:
            child.kill()
        try:
            child.wait(timeout=1)
        except subprocess.TimeoutExpired:
            result = Capture("reap_timeout")
        child.stdout.close()
        child.stderr.close()
    return result


def single_field(data, key, allowed, top_level=False):
    # launchctl print uses one tab for fields in the outer job object. Nested
    # service dictionaries can also contain state/path keys; keep them separate.
    indent = rb"\t" if top_level else rb"[ \t]*"
    values = re.findall(rb"(?m)^" + indent + key + rb"[ \t]*=[ \t]*([^\r\n]*)", data)
    if not values:
        return "absent"
    if len(values) != 1:
        return "duplicate"
    return allowed.get(values[0].strip(), "other")


def summarize(label, target, result, platform):
    row = {"target": label, "status": result.status, "exit_code": result.code}
    if result.status != "completed":
        return row
    if platform == "darwin":
        row.update(
            target_header=bool(re.search(rb"(?m)^" + re.escape(target.encode())
                                        + rb"\s*=\s*\{\s*$", result.stdout)),
            state=single_field(result.stdout, b"state", {
                b"running": "running", b"not running": "not_running",
                b"waiting": "waiting", b"exited": "exited"}, top_level=True),
            path=single_field(result.stdout, b"path", {
                b"/Library/LaunchDaemons/com.arxcovo.arxd.plist": "arxcovo",
                b"/System/Library/LaunchDaemons/com.apple.syslogd.plist": "apple_syslog"}, top_level=True),
            pid_field=bool(re.search(rb"(?m)^\tpid = [0-9]+[ \t]*$", result.stdout)),
            missing_service_phrase=b"Could not find service" in result.stderr,
            missing_target_named=target.split("/", 1)[1].encode() in result.stderr,
            system_domain_phrase=b"in domain for system" in result.stderr,
        )
    else:
        row.update(
            load=single_field(result.stdout, b"LoadState", {
                b"not-found": "not_found", b"loaded": "loaded"}),
            active=single_field(result.stdout, b"ActiveState", {
                b"inactive": "inactive", b"active": "active", b"failed": "failed",
                b"activating": "activating", b"deactivating": "deactivating"}),
            fragment=single_field(result.stdout, b"FragmentPath", {
                b"": "empty", b"/etc/systemd/system/arxd.service": "arxcovo"}),
            dropins=single_field(result.stdout, b"DropInPaths", {b"": "empty"}),
        )
    return row


def runner_tool_present(path):
    try:
        target = Path(path).resolve(strict=True)
        if target.parent not in (Path("/usr/bin"), Path("/usr/sbin")):
            return False
        info = target.stat()
    except (OSError, RuntimeError):
        return False
    return (info.st_uid == 0 and stat.S_ISREG(info.st_mode)
            and bool(info.st_mode & 0o111) and not (info.st_mode & 0o022))


def r2_runner_receipt(env, run):
    if (env.get("GITHUB_ACTIONS") != "true"
            or env.get("RUNNER_ENVIRONMENT") != "github-hosted"
            or env.get("RUNNER_OS") != "Linux"
            or env.get("ImageOS") != "ubuntu24"):
        return None
    image = env.get("ImageVersion", "")
    if not re.fullmatch(r"[0-9]{8}\.[0-9]{1,5}\.[0-9]{1,5}", image):
        return None
    kernel = os.uname().release
    package = run(list(R2_SYSTEMD_ARGS), timeout=1)
    return {
        "image_os": "ubuntu24",
        "image_version": image,
        "kernel": kernel if re.fullmatch(r"[0-9][0-9A-Za-z.+_-]{0,63}", kernel) else None,
        "systemd_package": (package.stdout.decode("ascii")
                            if package.status == "completed" and package.code == 0
                            and re.fullmatch(rb"[0-9][0-9A-Za-z.+:~_-]{0,63}", package.stdout)
                            else None),
        "tools": {path: runner_tool_present(path) for path in R2_TOOLS},
    }


def observe(platform, run=capture, runner_env=None):
    if platform == "darwin":
        rows = [summarize(label, target, run(["/bin/launchctl", "print", target]), platform)
                for label, target in MAC_TARGETS]
    elif platform == "linux":
        rows = [summarize("arxcovo", "arxd.service",
                          run(["/usr/bin/systemctl", *SYSTEMD_ARGS]), platform)]
    else:
        rows = [{"target": "platform", "status": "unsupported", "exit_code": None}]
    evidence = {"schemaVersion": 1, "observations": rows}
    if platform == "linux" and runner_env is not None:
        receipt = r2_runner_receipt(runner_env, run)
        if receipt is not None:
            evidence["r2_runner"] = receipt
    encoded = json.dumps(evidence, separators=(",", ":"))
    if len(encoded) > 2048:
        raise RuntimeError("manager-observation-bound")
    return encoded


def main():
    if len(sys.argv) != 1:
        print("manager-observation-arguments", file=sys.stderr)
        return 2
    print(observe(sys.platform, runner_env=os.environ))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
