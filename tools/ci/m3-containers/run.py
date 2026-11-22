#!/usr/bin/env python3
"""R2 hosted containment qualification with an inert, closed dispatcher."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import time

# Python isolated mode removes the script directory from sys.path.
sys.path.insert(0, str(Path(__file__).resolve().parent))

from fixture import (ENV, HERE, MIB, ROOT, TIME, TOOLS, Refusal, bounded_read, capture,
                     file_digest, host_command, load_pins, plain_path, require, select_daemon,
                     stage_copy, stage_rootfs, verify_manifest, vet_tools)
from isolation import Resources

REPO = "Dytschgo/arxcovo"
FIXED = {
    "CI": "true", "GITHUB_ACTIONS": "true", "RUNNER_OS": "Linux", "RUNNER_ARCH": "X64",
    "RUNNER_ENVIRONMENT": "github-hosted", "GITHUB_REPOSITORY": REPO, "GITHUB_JOB": "rust",
    "GITHUB_WORKFLOW": "CI",
}
FIELDS = (
    "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_SHA", "GITHUB_WORKFLOW_REF", "GITHUB_WORKFLOW_SHA",
    "GITHUB_WORKSPACE", "RUNNER_TEMP", "CARGO_TARGET_DIR", "ImageOS", "ImageVersion",
)
ERRORS = (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError)


def provenance(env, host, checkout):
    require(all(env.get(name) == value for name, value in FIXED.items()), "hosted-profile")
    run, attempt, head = (env.get(name, "") for name in FIELDS[:3])
    require(re.fullmatch(r"[1-9][0-9]{0,14}", run) and re.fullmatch(r"[1-9][0-9]{0,3}", attempt)
            and re.fullmatch(r"[a-f0-9]{40}", head), "run-identity")
    workflow = env.get("GITHUB_WORKFLOW_REF", "")
    require(re.fullmatch(re.escape(REPO) + r"/\.github/workflows/ci\.yml@refs/(heads/main|pull/[1-9][0-9]{0,14}/merge)",
                         workflow) and env.get("GITHUB_WORKFLOW_SHA") == head == checkout,
            "source-workflow-head")
    profiles = [profile for profile in host["profiles"]
                if (profile["image_os"], profile["image_version"])
                == (env.get("ImageOS"), env.get("ImageVersion"))]
    require(len(profiles) == 1, "runner-image")
    profile = profiles[0]
    return {"repo": REPO, "head": head, "run": run, "attempt": attempt, "job": "rust (ubuntu-latest)",
            "image_os": profile["image_os"], "image_version": profile["image_version"]}, profile


def require_userland(pins):
    require(pins["rootfs"]["userland"] == pins["host"]["os"], "offline-userland-input")


def os_version(file):
    lines = bounded_read(file, 8192).decode("ascii").splitlines()
    values = {}
    for line in lines:
        match = re.fullmatch(r'(ID|VERSION_ID)=(?:"([a-z0-9.]+)"|([a-z0-9.]+))', line)
        if match:
            require(match[1] not in values, "runner-os-duplicate")
            values[match[1]] = match[2] or match[3]
    require(values.get("ID") == "ubuntu", "runner-os")
    return values.get("VERSION_ID")


def checked_runner_platform(env, host, head, end, tools):
    receipt, profile = provenance(env, host, head)
    uname = os.uname()
    require(os_version(Path("/usr/lib/os-release")) == host["os"]
            and uname.release == profile["kernel"] and uname.machine == "x86_64", "runner-kernel-os")
    systemd = host_command("dpkg", end, "-W", "-f=${Version}", "systemd", tools=tools).strip()
    require(systemd == profile["systemd_package"], "runner-systemd")
    receipt.update(os=host["os"], kernel=uname.release, systemd=systemd)
    return receipt


def checked_metadata(env, pins, uid, end):
    require(sys.platform == "linux" and uid > 0, "hosted-entry")
    host = pins["host"]
    workspace = Path(env.get("GITHUB_WORKSPACE", ""))
    scratch = Path(env.get("RUNNER_TEMP", ""))
    require(workspace == ROOT and workspace.parent.parent / "_temp" == scratch, "checkout-location")
    plain_path(workspace, (0, uid), directory=True)
    plain_path(scratch, (0, uid), directory=True)
    require_userland(pins)
    vetted = vet_tools(host["tools"])
    head = host_command("git", end, "-c", f"safe.directory={workspace}", "-C", workspace,
                        "rev-parse", "HEAD", tools=vetted).strip()
    receipt = checked_runner_platform(env, host, head, end, vetted)
    tools = {}
    for name in ("cc", "nft", "ip", "mkfs", "loop", "unit", "mount", "umount"):
        argument = "-V" if name in ("ip", "mkfs") else "--version"
        output = host_command(name, end, argument, tools=vetted, limit=8192)
        first = output.splitlines()[0].strip()
        require(re.fullmatch(r"[\w .,+:/()'#~=-]{1,160}", first), "runner-tool-version")
        tools[name] = first
    receipt["tools"] = tools
    stats = os.statvfs("/var/lib")
    require(stats.f_bavail * stats.f_frsize >= 4*1024*MIB, "runner-disk")
    controllers = bounded_read(Path("/sys/fs/cgroup/cgroup.controllers"), 512).decode("ascii").split()
    require({"memory", "pids", "cpu"} <= set(controllers)
            and stat.S_ISCHR(Path("/dev/loop-control").stat().st_mode)
            and all((Path("/proc/self/ns") / item).is_symlink()
                    for item in ("cgroup", "mnt", "net", "pid", "ipc", "uts")), "kernel-prerequisites")
    source = select_daemon(env.get("CARGO_TARGET_DIR", ""), workspace, uid,
                           scratch / "arx-bound-sqlite", receipt, end)
    home = Path(__import__("pwd").getpwuid(uid).pw_dir)
    toolchain = home / ".rustup/toolchains" / host["rust"]
    plain_path(toolchain, (0, uid), directory=True)
    sources = {}
    for name in ("run.py", "isolation.py", "fixture.py", "bootstrap.c", "pins.json"):
        path = HERE / name
        digest = file_digest(path, end, MIB, (0, uid))
        revision = host_command("git", end, "-c", f"safe.directory={workspace}", "-C", workspace,
                                "show", f"{head}:tools/ci/m3-containers/{name}", tools=vetted, limit=MIB)
        require(hashlib.sha256(revision.encode("ascii")).hexdigest() == digest, "source-module-digest")
        sources[name] = digest
    receipt["modules"] = sources
    return receipt, source, toolchain, vetted


class Qualification:
    def __init__(self):
        self.resources = None
        self.phase_name = "preflight"
        self.receipt = {"mode": "dry", "durations_seconds": dict.fromkeys(TIME), "first": None, "cleanup": "pending"}

    def phase(self, name, callback):
        self.phase_name = name
        start = time.monotonic()
        end = start + TIME[name]
        try:
            result = callback(end)
            require(time.monotonic() < end, "phase-deadline")
            return result
        finally:
            self.receipt["durations_seconds"][name] = round(time.monotonic() - start, 3)

    def run(self, preflight, stage):
        try:
            identity, assets, resources = self.phase("preflight", preflight)
            self.resources = resources
            self.receipt["runner"] = identity
            self.receipt["images"] = self.phase("stage", lambda end: stage(resources, assets, end))
            self.phase("contain", resources.contain)
            self.phase("network", resources.network)
            self.receipt["containment"] = resources.facts
        except ERRORS as error:
            self.latch(error)
        try:
            errors = self.phase("cleanup", self.resources.cleanup if self.resources else lambda _end: [])
            self.receipt["cleanup"] = "confirmed" if not errors else "incomplete"
            if errors:
                self.receipt["cleanup_errors"] = errors
                self.latch(Refusal("partial-cleanup"))
        except ERRORS as error:
            self.receipt["cleanup"] = "incomplete"
            self.latch(error)
        diagnostic = getattr(self.resources, "failure_diagnostic", None)
        if diagnostic is not None:
            self.receipt["diagnostic"] = diagnostic
        return self.receipt

    def latch(self, error):
        if self.receipt["first"] is None:
            category = str(error) if isinstance(error, Refusal) else type(error).__name__
            require(re.fullmatch(r"[A-Za-z0-9-]{1,80}", category), "error-category")
            self.receipt["first"] = {"phase": self.phase_name, "category": category}


def stage(resources, assets, end):
    pins, artifact, toolchain, uid, sources, vetted = assets
    daemon, expected_digest = artifact
    resources.arena()
    require(stage_copy(HERE / "bootstrap.c", resources.root / "bootstrap.c", end, (0, uid), limit=MIB)
            == sources["bootstrap.c"], "helper-digest")
    tree = resources.root / "tree"
    manifest, manifest_digest = stage_rootfs(pins, tree, resources.root, daemon, toolchain, uid, end, vetted)
    require(manifest["files"]["fixture/arxd"] == expected_digest, "artifact-digest-drift")
    mounted = resources.volume("root", tree, end)
    image = resources.root / "root.ext4"
    digest = file_digest(image, end, pins["rootfs"]["bytes"])
    verify_manifest(mounted, manifest, manifest_digest, end)
    resources.stage_data(end)
    return {"rootfs_sha256": digest, "manifest_sha256": manifest_digest,
            "artifact_sha256": manifest["files"]["fixture/arxd"], "bootstrap_sha256": manifest["files"]["fixture/probe"]}


def dry_run(env, uid):
    def preflight(end):
        pins = load_pins()
        receipt, daemon, tools, vetted = checked_metadata(env, pins, uid, end)
        return receipt, (pins, daemon, tools, uid, receipt["modules"], vetted), Resources(receipt, pins, vetted)
    return Qualification().run(preflight, stage)


def encode_receipt(receipt, limit=12288):
    data = json.dumps(receipt, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    require(len(data) <= limit, "receipt-output-bound")
    return data


def entry(args, env):
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--mode", choices=("dry",), required=True)
    parser.parse_args(args)
    require(sys.platform == "linux", "linux-host")
    if os.geteuid() != 0:
        require(all(env.get(name) == value for name, value in FIXED.items()), "hosted-profile")
        allow = [f"{name}={env.get(name, '')}" for name in (*FIXED, *FIELDS)]
        require(all(0 < len(item) <= 512 and "\n" not in item and "\r" not in item for item in allow),
                "environment-bound")
        output = capture(["/usr/bin/sudo", "-n", "/usr/bin/env", *allow, "/usr/bin/python3",
                          "-I", "-B", HERE / "run.py", "--mode", "dry"],
                         time.monotonic() + sum(TIME.values()) + 3, statuses=(0, 1), limit=12544)
        require(output.startswith(b"R2 receipt ") and output.count(b"\n") == 1, "privileged-receipt")
        receipt = json.loads(output[len(b"R2 receipt "):])
    else:
        owner = env.get("SUDO_UID", "")
        require(re.fullmatch(r"[1-9][0-9]{0,8}", owner), "staging-identity")
        receipt = dry_run(env, int(owner))
    print("R2 receipt " + encode_receipt(receipt), flush=True)
    return int(receipt["first"] is not None or receipt["cleanup"] != "confirmed")


if __name__ == "__main__":
    try:
        sys.exit(entry(sys.argv[1:], os.environ))
    except ERRORS as error:
        failure = Qualification()
        failure.latch(error)
        print("R2 receipt " + encode_receipt(failure.receipt), flush=True)
        sys.exit(1)
