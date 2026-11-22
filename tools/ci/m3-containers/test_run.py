"""Pure controls for the R2 launcher and stage receipts."""

import hashlib
import io
import json
from pathlib import Path
import stat
import struct
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import fixture
import run


def elf():
    loader = b"/lib64/ld-linux-x86-64.so.2\0"
    header = bytearray(64)
    header[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HHI", header, 16, 3, 62, 1)
    struct.pack_into("<Q", header, 32, 64)
    struct.pack_into("<HHH", header, 52, 64, 56, 2)
    return (bytes(header)
            + struct.pack("<IIQQQQQQ", 1, 5, 0, 0, 0, 64, 64, 4096)
            + struct.pack("<IIQQQQQQ", 3, 4, 176, 0, 0, len(loader), len(loader), 1)
            + loader)


class FixtureTests(unittest.TestCase):
    def test_closed_pins(self):
        pins = fixture.load_pins()
        for key, value in (("data_bytes", 268435455), ("block_size", 512), ("dev_bytes", True),
                           ("extra", 0), ("userland", "latest")):
            with self.subTest(key=key):
                modified = {**pins, "rootfs": {**pins["rootfs"], key: value}}
                with self.assertRaisesRegex(fixture.Refusal, "pins-schema"):
                    fixture.check_pins(modified)
        profiles = pins["host"]["profiles"]
        for replacement in (profiles[:1], [profiles[0], profiles[0]],
                            [{**profiles[0], "kernel": profiles[1]["kernel"]}, profiles[1]]):
            modified = {**pins, "host": {**pins["host"], "profiles": replacement}}
            with self.assertRaisesRegex(fixture.Refusal, "pins-schema"):
                fixture.check_pins(modified)
        with self.assertRaisesRegex(fixture.Refusal, "duplicate-json-key"):
            json.loads('{"host": 1, "host": 2}', object_pairs_hook=fixture.unique_pairs)

    def test_elf_checks_machine_interpreter_bounds_and_executable_segment(self):
        data = elf()
        fixture.inspect_elf(io.BytesIO(data), len(data))
        for offset, byte in ((0, 0), (18, 183), (32, 255), (56, 65), (68, 4), (176, 0)):
            with self.subTest(offset=offset):
                changed = bytearray(data)
                changed[offset] = byte
                with self.assertRaisesRegex(fixture.Refusal, "elf-"):
                    fixture.inspect_elf(io.BytesIO(changed), len(changed))

    def test_stream_copy_and_short_or_surplus_input(self):
        end = fixture.time.monotonic() + 1
        result = io.BytesIO()
        data = elf()
        actual = fixture.stream_hash(io.BytesIO(data), len(data), end, result)
        self.assertEqual(actual, hashlib.sha256(data).hexdigest())
        self.assertEqual(result.getvalue(), data)
        for size, reason in ((len(data)+1, "truncated"), (len(data)-1, "surplus")):
            with self.subTest(size=size):
                with self.assertRaisesRegex(fixture.Refusal, reason):
                    fixture.stream_hash(io.BytesIO(data), size, end)

    def test_no_follow_ancestors_and_file_modes(self):
        writable = SimpleNamespace(st_uid=0, st_mode=stat.S_IFDIR | 0o777)
        link = SimpleNamespace(st_uid=0, st_mode=stat.S_IFLNK | 0o755)
        with self.assertRaisesRegex(fixture.Refusal, "absolute-path"):
            fixture.plain_path(Path("stage/../artifact"), (0,), directory=False)
        # Substitute each ancestor's observed kernel metadata on any unit-test host.
        for observed in (writable, link):
            parent = SimpleNamespace(lstat=lambda: observed)
            path = SimpleNamespace(is_absolute=lambda: True, parts=("/", "stage", "artifact"),
                                   anchor="/", parents=(parent,))
            with self.assertRaisesRegex(fixture.Refusal, "unsafe-ancestor"):
                fixture.plain_path(path, (0,), directory=False)
        for altered in (stat.S_IFLNK | 0o755, stat.S_IFREG | 0o775, stat.S_IFREG | 0o4755):
            self.assertFalse(fixture.protected_mode(SimpleNamespace(st_uid=0, st_mode=altered, st_nlink=1), (0,)))

    def test_artifact_selection_rejects_wrong_path_before_open(self):
        root = Path("checkout")
        for target, receipt in ((root / "debug", Path("build-messages.jsonl")),
                                (root / "target", Path("other.jsonl"))):
            with self.assertRaisesRegex(fixture.Refusal, "artifact-receipt"):
                fixture.select_daemon(target, root, 123, receipt, 0)

    def test_unallocated_short_or_aliased_images_fail(self):
        size = 256*fixture.MIB
        metadata = dict(st_mode=stat.S_IFREG | 0o600, st_uid=0, st_nlink=1, st_size=size, st_blocks=size//512)
        fixture.image_stat(SimpleNamespace(**metadata), size)
        for key, value in (("st_uid", 1), ("st_nlink", 2), ("st_size", size-1), ("st_blocks", 10)):
            with self.subTest(key=key):
                with self.assertRaisesRegex(fixture.Refusal, "image-allocation"):
                    fixture.image_stat(SimpleNamespace(**{**metadata, key: value}), size)

    def test_bounds_fail_before_expanding_output(self):
        output = bytearray(b"ab")
        fixture.checked_output(output, b"c", 3)
        with self.assertRaisesRegex(fixture.Refusal, "command-output-bound"):
            fixture.checked_output(output, b"d", 3)
        self.assertEqual(output, b"abc")
        with self.assertRaisesRegex(fixture.Refusal, "receipt-output-bound"):
            run.encode_receipt({"samples": ["x"*31]}, limit=32)

    def test_manifest_integrity_checked_before_parsing_paths(self):
        with patch.object(fixture, "bounded_read", return_value=b'{"files":{}}'):
            with self.assertRaisesRegex(fixture.Refusal, "manifest-digest"):
                fixture.verify_manifest(Path("root"), {"files": {}}, "0"*64, 0)


class RunnerTests(unittest.TestCase):
    @staticmethod
    def hosted_env(profile):
        head = "a"*40
        return {**run.FIXED, "GITHUB_RUN_ID": "661", "GITHUB_RUN_ATTEMPT": "1", "GITHUB_SHA": head,
                "GITHUB_WORKFLOW_REF": run.REPO + "/.github/workflows/ci.yml@refs/pull/7/merge",
                "GITHUB_WORKFLOW_SHA": head, "ImageOS": profile["image_os"],
                "ImageVersion": profile["image_version"]}

    def test_userland_mismatch_refuses_before_privileged_resources(self):
        pins = fixture.load_pins()
        mismatch = {**pins, "rootfs": {**pins["rootfs"], "userland": "26.04"}}
        with self.assertRaisesRegex(fixture.Refusal, "offline-userland-input"):
            run.require_userland(mismatch)
        run.require_userland(pins)

    def test_provenance_refuses_mismatched_source_run_attempt_os_and_job(self):
        host = fixture.load_pins()["host"]
        values = self.hosted_env(host["profiles"][0])
        head = values["GITHUB_SHA"]
        receipt, selected = run.provenance(values, host, head)
        self.assertEqual(receipt["head"], head)
        self.assertEqual(selected, host["profiles"][0])
        for key, invalid in (("GITHUB_SHA", "b"*40), ("GITHUB_WORKFLOW_SHA", "b"*40), ("GITHUB_RUN_ID", "../../"),
                             ("GITHUB_RUN_ATTEMPT", "01"), ("GITHUB_WORKFLOW_REF", "another-workflow"),
                             ("ImageOS", "ubuntu26"), ("ImageVersion", "new"), ("GITHUB_REPOSITORY", "outsider/fork"),
                             ("RUNNER_ENVIRONMENT", "self-hosted"), ("GITHUB_JOB", "verify")):
            with self.subTest(key=key):
                with self.assertRaises(fixture.Refusal):
                    run.provenance({**values, key: invalid}, host, head)

    def test_each_runner_profile_requires_its_own_kernel_and_package(self):
        host = fixture.load_pins()["host"]
        first, second = host["profiles"]
        for profile, other in ((first, second), (second, first)):
            env = self.hosted_env(profile)
            head = env["GITHUB_SHA"]
            end = 1
            with self.subTest(image=profile["image_version"]), patch.object(
                run, "os_version", return_value="24.04"
            ), patch.object(run.os, "uname", create=True) as uname, patch.object(
                run, "host_command", return_value=profile["systemd_package"]
            ) as package:
                uname.return_value = SimpleNamespace(release=other["kernel"], machine="x86_64")
                with self.assertRaisesRegex(fixture.Refusal, "runner-kernel-os"):
                    run.checked_runner_platform(env, host, head, end)
                package.assert_not_called()

                uname.return_value = SimpleNamespace(release=profile["kernel"], machine="x86_64")
                receipt = run.checked_runner_platform(env, host, head, end)
                package.assert_called_once_with("dpkg", end, "-W", "-f=${Version}", "systemd")
                self.assertEqual(receipt["image_version"], profile["image_version"])
                self.assertEqual(receipt["kernel"], profile["kernel"])
                self.assertEqual(receipt["systemd"], profile["systemd_package"])

                package.return_value = "255.4-1ubuntu8.19"
                with self.assertRaisesRegex(fixture.Refusal, "runner-systemd"):
                    run.checked_runner_platform(env, host, head, end)

    def test_only_dry_dispatches_and_uses_no_legacy_driver(self):
        expected = {"first": None, "cleanup": "confirmed"}
        with patch.object(run.sys, "platform", "linux"), patch.object(run.os, "geteuid", return_value=0, create=True):
            with patch.object(run, "dry_run", return_value=expected) as dry, patch("builtins.print"):
                with patch("sys.stderr", new_callable=io.StringIO):
                    for args in ([], ["--mode", "live"], ["--mode", "daemon"], ["--mode", "dry", "arxd"],
                                 ["--mod", "dry"], ["--mode", "dry", "--mode", "live"]):
                        with self.subTest(args=args):
                            with self.assertRaises(SystemExit):
                                run.entry(args, {"SUDO_UID": "1001"})
                dry.assert_not_called()
                self.assertEqual(run.entry(["--mode", "dry"], {"SUDO_UID": "1001"}), 0)
                dry.assert_called_once_with({"SUDO_UID": "1001"}, 1001)
        directory = fixture.HERE
        helper = (directory / "bootstrap.c").read_text(encoding="ascii")
        self.assertEqual(helper.count("execve("), 2)
        self.assertNotIn('"/fixture/arxd"', helper)
        self.assertNotIn('"build", "--offline"', helper)
        for path in ("run.py", "fixture.py", "isolation.py"):
            text = (directory / path).read_text(encoding="ascii")
            for command in ("executor-probe.py", "delegated-build-probe.py", "m3-c13", "cargo build",
                            "cluster join", "cluster mint"):
                self.assertNotIn(command, text)

    def test_first_failure_latched_and_cleanup_runs(self):
        steps = []
        def contain(_end):
            steps.append("contain")
            raise fixture.Refusal("mount-instance")

        resource = SimpleNamespace(contain=contain, cleanup=lambda _end: steps.append("cleanup") or ["unit"])
        receipt = run.Qualification().run(lambda _end: ({}, {}, resource), lambda *_: {})
        self.assertEqual(steps, ["contain", "cleanup"])
        self.assertEqual(receipt["first"], {"phase": "contain", "category": "mount-instance"})
        self.assertEqual(receipt["cleanup_errors"], ["unit"])
        self.assertEqual(receipt["cleanup"], "incomplete")

    def test_success_requires_every_phase_and_complete_cleanup(self):
        steps = []
        resource = SimpleNamespace(facts={}, contain=lambda _: steps.append("contain"),
                                   network=lambda _: steps.append("network"), cleanup=lambda _: [])
        result = run.Qualification().run(lambda _end: ({}, {}, resource), lambda *_: {})
        self.assertEqual(steps, ["contain", "network"])
        self.assertEqual(set(result["durations_seconds"]), set(fixture.TIME))
        self.assertEqual(sum(fixture.TIME.values()), 85)
        self.assertEqual(result["cleanup"], "confirmed")
        self.assertIsNone(result["first"])
