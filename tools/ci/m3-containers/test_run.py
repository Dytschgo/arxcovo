"""Pure controls for the R2 launcher and stage receipts."""

from contextlib import contextmanager
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import stat
import struct
import subprocess
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import fixture
import isolation
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
        identity = {"head": "a"*40, "run": "1", "attempt": "1"}
        with self.assertRaisesRegex(fixture.Refusal, "absolute-path"):
            fixture.select_daemon("relative/target", root, 123, root / "proof", identity, 0)
        with self.assertRaisesRegex(fixture.Refusal, "absolute-path"):
            fixture.select_daemon("/tmp/../target", root, 123, root / "proof", identity, 0)

    def test_artifact_selection_binds_elf_to_checked_build_receipt(self):
        workspace = Path("/work/arxcovo")
        proof_root = Path("/work/_temp/arx-bound-sqlite")
        identity = {"head": "a"*40, "run": "661", "attempt": "1"}
        good_elf = elf()
        good_sha = hashlib.sha256(good_elf).hexdigest()
        other_elf = good_elf + b"\x90" * 16
        self.assertNotEqual(hashlib.sha256(other_elf).hexdigest(), good_sha)

        def build_messages(target):
            return ("\n".join((
                json.dumps({
                    "reason": "build-script-executed",
                    "package_id": "path+file:///work/arxcovo/vendor/libsqlite3-sys-0.35.0#0.35.0",
                    "linked_libs": ["static=sqlite3"],
                    "cfgs": [], "env": [],
                    "out_dir": f"{target.as_posix()}/debug/build/libsqlite3-sys-1234/out",
                }),
                json.dumps({
                    "reason": "compiler-artifact",
                    "manifest_path": "/work/arxcovo/crates/arxd/Cargo.toml",
                    "target": {"name": "arxd", "kind": ["bin"]},
                }),
            )) + "\n").encode("ascii")

        def link_messages(target, *, executable=True, test_profile=False, finished=True):
            artifact = {
                "reason": "compiler-artifact",
                "manifest_path": "/work/arxcovo/crates/arxd/Cargo.toml",
                "target": {"name": "arxd", "kind": ["bin"]},
                "profile": {"test": test_profile},
            }
            if executable is True:
                artifact["executable"] = f"{target.as_posix()}/debug/arxd"
            elif executable is not None:
                artifact["executable"] = executable
            rows = [json.dumps(artifact)]
            if finished:
                rows.append(json.dumps({"reason": "build-finished", "success": True}))
            return ("\n".join(rows) + "\n").encode("ascii")

        def make_receipt(target, messages_bytes, link_bytes, *, sha=good_sha, size=len(good_elf), **overrides):
            body = {
                "schema": "arxcovo/r2-artifact/1",
                "head": identity["head"], "run": identity["run"], "attempt": identity["attempt"],
                "target_dir": target.as_posix(),
                "executable": (target / "debug/arxd").as_posix(),
                "sha256": sha, "bytes": size,
                "build_messages_sha256": hashlib.sha256(messages_bytes).hexdigest(),
                "link_messages_sha256": hashlib.sha256(link_bytes).hexdigest(),
                **overrides,
            }
            return json.dumps(body).encode("ascii")

        def run_select(target_raw, files, candidates=None):
            target = Path(target_raw) if target_raw else workspace / "target"
            profile = target / "debug"
            entries = [profile / "arxd", profile / "arxd.d"] if candidates is None else candidates

            @contextmanager
            def fake_open(path, owners, size, *, executable=False):
                data, mtime = files[path.as_posix()]
                yield io.BytesIO(data), SimpleNamespace(st_size=len(data), st_mtime_ns=mtime)

            with patch.object(fixture, "plain_path", side_effect=lambda p, *a, **kw: p), \
                 patch.object(fixture, "open_artifact", side_effect=fake_open), \
                 patch.object(Path, "iterdir", return_value=iter(entries)):
                return fixture.select_daemon(target_raw, workspace, 1001, proof_root, identity,
                                             fixture.time.monotonic() + 5)

        # Positive: default workspace/target and explicit validated CARGO_TARGET_DIR override.
        for target_raw, effective in (("", workspace / "target"), ("/mnt/custom-target", Path("/mnt/custom-target"))):
            with self.subTest(target=target_raw or "default"):
                msgs = build_messages(effective)
                links = link_messages(effective)
                rcpt = make_receipt(effective, msgs, links)
                files = {
                    (proof_root / "build-messages.jsonl").as_posix(): (msgs, 100),
                    (proof_root / "arxd-build-messages.jsonl").as_posix(): (links, 150),
                    (proof_root / "arxd-receipt.json").as_posix(): (rcpt, 200),
                    (effective / "debug/arxd").as_posix(): (good_elf, 200),
                }
                path, digest = run_select(target_raw, files)
                self.assertEqual(path, effective / "debug/arxd")
                self.assertEqual(digest, good_sha)

        # Negative: newer substituted unrelated ELF alongside a plausible receipt must be refused.
        default_target = workspace / "target"
        msgs = build_messages(default_target)
        links = link_messages(default_target)
        rcpt = make_receipt(default_target, msgs, links)
        base_files = {
            (proof_root / "build-messages.jsonl").as_posix(): (msgs, 100),
            (proof_root / "arxd-build-messages.jsonl").as_posix(): (links, 150),
            (proof_root / "arxd-receipt.json").as_posix(): (rcpt, 200),
            (default_target / "debug/arxd").as_posix(): (other_elf, 999),
        }
        with self.assertRaisesRegex(fixture.Refusal, "artifact-digest-mismatch"):
            run_select("", base_files)

        # Same-size substituted valid ELF with different bytes (e_entry) must also be refused.
        same_size_other = bytearray(good_elf)
        same_size_other[24] ^= 0x01
        with self.assertRaisesRegex(fixture.Refusal, "artifact-digest-mismatch"):
            run_select("", {**base_files, (default_target / "debug/arxd").as_posix(): (bytes(same_size_other), 999)})

        # Negative: cargo-check-style link messages without executable field must be refused.
        check_only_links = link_messages(default_target, executable=None)
        with self.assertRaisesRegex(fixture.Refusal, "artifact-link-messages"):
            run_select("", {
                **base_files,
                (proof_root / "arxd-build-messages.jsonl").as_posix(): (check_only_links, 150),
                (proof_root / "arxd-receipt.json").as_posix(): (make_receipt(default_target, msgs, check_only_links), 200),
                (default_target / "debug/arxd").as_posix(): (good_elf, 200),
            })

        # Negative: substituted ELF carrying its own digest in receipt without matching Cargo executable evidence is refused.
        other_sha = hashlib.sha256(other_elf).hexdigest()
        for bad_link_case, bad_links in (
            ("no-executable", check_only_links),
            ("wrong-executable", link_messages(default_target, executable="/tmp/other/arxd")),
            ("test-profile", link_messages(default_target, test_profile=True)),
            ("unfinished", link_messages(default_target, finished=False)),
        ):
            with self.subTest(substituted_with_self_receipt=bad_link_case):
                self_rcpt = make_receipt(default_target, msgs, bad_links, sha=other_sha, size=len(other_elf))
                with self.assertRaisesRegex(fixture.Refusal, "artifact-link-messages"):
                    run_select("", {
                        (proof_root / "build-messages.jsonl").as_posix(): (msgs, 100),
                        (proof_root / "arxd-build-messages.jsonl").as_posix(): (bad_links, 150),
                        (proof_root / "arxd-receipt.json").as_posix(): (self_rcpt, 200),
                        (default_target / "debug/arxd").as_posix(): (other_elf, 999),
                    })

        # Unrelated nonempty build-messages.jsonl is refused.
        unrelated = b'{"reason":"note","message":"unrelated"}\n'
        with self.assertRaisesRegex(fixture.Refusal, "artifact-build-messages"):
            run_select("", {
                (proof_root / "build-messages.jsonl").as_posix(): (unrelated, 100),
                (proof_root / "arxd-build-messages.jsonl").as_posix(): (links, 150),
                (proof_root / "arxd-receipt.json").as_posix(): (make_receipt(default_target, unrelated, links), 200),
                (default_target / "debug/arxd").as_posix(): (good_elf, 200),
            })

        # Mismatched digests, mismatched HEAD, stale binary, and multiple candidates are refused.
        for label, override_files, candidates, pattern in (
            ("binding-build", {(proof_root / "arxd-receipt.json").as_posix(): (make_receipt(default_target, b"wrong", links), 200),
                               (default_target / "debug/arxd").as_posix(): (good_elf, 200)}, None, "artifact-receipt-binding"),
            ("binding-link", {(proof_root / "arxd-receipt.json").as_posix(): (make_receipt(default_target, msgs, b"wrong"), 200),
                              (default_target / "debug/arxd").as_posix(): (good_elf, 200)}, None, "artifact-receipt-binding"),
            ("head", {(proof_root / "arxd-receipt.json").as_posix(): (make_receipt(default_target, msgs, links, head="b"*40), 200),
                      (default_target / "debug/arxd").as_posix(): (good_elf, 200)}, None, "artifact-receipt-identity"),
            ("stale", {(default_target / "debug/arxd").as_posix(): (good_elf, 50)}, None, "stale-artifact"),
            ("multiple", {(default_target / "debug/arxd").as_posix(): (good_elf, 200)},
             [default_target / "debug/arxd", default_target / "debug/arxd-alt"], "artifact-candidates"),
        ):
            with self.subTest(case=label):
                with self.assertRaisesRegex(fixture.Refusal, pattern):
                    run_select("", {**base_files, **override_files}, candidates=candidates)

    def test_gcc_multiarch_search_paths_canonicalize_for_manifest(self):
        ubuntu_lookups = {
            "/usr/libexec/gcc/x86_64-linux-gnu/13/cc1": "usr/libexec/gcc/x86_64-linux-gnu/13/cc1",
            "/usr/lib/gcc/x86_64-linux-gnu/13/libgcc.a": "usr/lib/gcc/x86_64-linux-gnu/13/libgcc.a",
            "/usr/lib/gcc/x86_64-linux-gnu/13/../../../x86_64-linux-gnu/crt1.o": "usr/lib/x86_64-linux-gnu/crt1.o",
            "/usr/lib/gcc/x86_64-linux-gnu/13/../../../x86_64-linux-gnu/Scrt1.o": "usr/lib/x86_64-linux-gnu/Scrt1.o",
            "/usr/lib/gcc/x86_64-linux-gnu/13/../../../x86_64-linux-gnu/crti.o": "usr/lib/x86_64-linux-gnu/crti.o",
            "/usr/lib/gcc/x86_64-linux-gnu/13/../../../x86_64-linux-gnu/crtn.o": "usr/lib/x86_64-linux-gnu/crtn.o",
            "/usr/lib/gcc/x86_64-linux-gnu/13/../../../x86_64-linux-gnu/libc.so": "usr/lib/x86_64-linux-gnu/libc.so",
            "/usr/lib/gcc/x86_64-linux-gnu/13/../../../../lib/x86_64-linux-gnu/libc_nonshared.a":
                "usr/lib/x86_64-linux-gnu/libc_nonshared.a",
        }
        canonical_map = {
            **{raw: "/" + key for raw, key in ubuntu_lookups.items()},
            **{"/" + key: "/" + key for key in ubuntu_lookups.values()},
        }
        for raw, expected in ubuntu_lookups.items():
            with self.subTest(raw=raw):
                relative, traversed = fixture.stage_compiler_path(raw, resolve=canonical_map.__getitem__)
                self.assertEqual(relative, expected)
                self.assertNotIn("..", PurePosixPath(relative).parts)
                if ".." in raw:
                    self.assertIn("usr/lib/gcc/x86_64-linux-gnu/13", traversed)

        # Escapes, bare unfound names, trailing slashes/dots, and symlink-before-parent divergence must fail.
        for bad in (
            "crt1.o",
            "/tmp/crt1.o",
            "/usr/lib/gcc/x86_64-linux-gnu/13/../../../../../etc/passwd",
            "/usr/../../etc/shadow",
            "/usr/lib/x86_64-linux-gnu/crt1.o/",
            "/usr/lib/x86_64-linux-gnu/crt1.o/.",
            "/usr/lib/./x86_64-linux-gnu/crt1.o",
        ):
            with self.subTest(bad=bad):
                with self.assertRaisesRegex(fixture.Refusal, "compiler-input-path"):
                    fixture.stage_compiler_path(bad)
        with self.assertRaisesRegex(fixture.Refusal, "compiler-input-path"):
            fixture.stage_compiler_path(
                "/usr/lib/symlinked/../x86_64-linux-gnu/crt1.o",
                resolve=lambda item: "/usr/other/crt1.o" if "symlinked" in item else "/usr/lib/x86_64-linux-gnu/crt1.o",
            )

    def test_tool_vetting_accepts_valid_aliases_and_rejects_broken_suffixes(self):
        dir_ok = SimpleNamespace(st_uid=0, st_mode=stat.S_IFDIR | 0o755)
        reg_ok = SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o755)
        suid_ok = SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o4755)
        lnk_ok = SimpleNamespace(st_uid=0, st_mode=stat.S_IFLNK | 0o777)

        def make_fs(extra_stats=None, extra_links=None):
            stats = {
                "/": dir_ok, "/usr": dir_ok, "/usr/bin": dir_ok, "/usr/sbin": dir_ok,
                "/bin": lnk_ok, "/sbin": lnk_ok,
                "/usr/bin/git": reg_ok,
                "/usr/bin/mount": suid_ok,
                "/usr/bin/gcc": lnk_ok,
                "/usr/bin/gcc-13": lnk_ok,
                "/usr/bin/x86_64-linux-gnu-gcc-13": reg_ok,
                "/usr/sbin/ip": lnk_ok,
                "/usr/bin/ip": reg_ok,
                **(extra_stats or {}),
            }
            links = {
                "/bin": "usr/bin", "/sbin": "usr/sbin",
                "/usr/bin/gcc": "gcc-13",
                "/usr/bin/gcc-13": "x86_64-linux-gnu-gcc-13",
                "/usr/sbin/ip": "/bin/ip",
                **(extra_links or {}),
            }
            return stats.__getitem__, links.__getitem__

        lstat_fn, readlink_fn = make_fs()
        self.assertEqual(fixture.vet_tool("/usr/bin/git", lstat=lstat_fn, readlink=readlink_fn), "/usr/bin/git")
        self.assertEqual(fixture.vet_tool("/usr/bin/mount", lstat=lstat_fn, readlink=readlink_fn), "/usr/bin/mount")
        self.assertEqual(fixture.vet_tool("/usr/bin/gcc", lstat=lstat_fn, readlink=readlink_fn),
                         "/usr/bin/x86_64-linux-gnu-gcc-13")
        self.assertEqual(fixture.vet_tool("/usr/sbin/ip", lstat=lstat_fn, readlink=readlink_fn), "/usr/bin/ip")

        # Sibling-relative ../bin/ip alias is also accepted cleanly.
        lstat_fn, readlink_fn = make_fs(extra_links={"/usr/sbin/ip": "../bin/ip"})
        self.assertEqual(fixture.vet_tool("/usr/sbin/ip", lstat=lstat_fn, readlink=readlink_fn), "/usr/bin/ip")

        # Broken trailing-slash, terminal-dot, out-of-policy, unsafe ownership/mode, bad /bin alias, and cycles fail.
        for label, extra_stats, extra_links in (
            ("trailing-slash-merged", None, {"/usr/sbin/ip": "/bin/ip/"}),
            ("trailing-slash-abs", None, {"/usr/sbin/ip": "/usr/bin/ip/"}),
            ("trailing-slash-rel", None, {"/usr/bin/gcc": "gcc-13/"}),
            ("terminal-dot-merged", None, {"/usr/sbin/ip": "/bin/ip/."}),
            ("terminal-dot-abs", None, {"/usr/sbin/ip": "/usr/bin/ip/."}),
            ("terminal-dot-rel", None, {"/usr/bin/gcc": "gcc-13/."}),
            ("bare-dot", None, {"/usr/bin/gcc": "."}),
            ("out-of-policy", None, {"/usr/sbin/ip": "/tmp/ip"}),
            ("bad-merged-bin", None, {"/bin": "/tmp/bin"}),
            ("non-root-link", {"/usr/sbin/ip": SimpleNamespace(st_uid=1000, st_mode=stat.S_IFLNK | 0o777)}, None),
            ("writable-dir", {"/usr/sbin": SimpleNamespace(st_uid=0, st_mode=stat.S_IFDIR | 0o775)}, None),
            ("writable-leaf", {"/usr/bin/ip": SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o775)}, None),
            ("cycle", None, {"/usr/bin/gcc": "gcc-13", "/usr/bin/gcc-13": "gcc"}),
        ):
            with self.subTest(case=label):
                ls, rl = make_fs(extra_stats, extra_links)
                target_path = "/usr/bin/gcc" if "rel" in label or label in ("bare-dot", "cycle") else "/usr/sbin/ip"
                with self.assertRaisesRegex(fixture.Refusal, "runner-tool"):
                    fixture.vet_tool(target_path, lstat=ls, readlink=rl)

        # Unvetted host_command refuses before spawning capture.
        with patch.object(fixture, "capture") as mocked_capture:
            with self.assertRaisesRegex(fixture.Refusal, "tool-unvetted"):
                fixture.host_command("git", 1, "--version")
            mocked_capture.assert_not_called()

    def test_capture_timeout_kills_reaps_and_raises_refusal(self):
        fake_stdout = SimpleNamespace(fileno=lambda: 3, close=lambda: None)
        killed = []
        waits = []

        class FakePopen:
            def __init__(self, *args, **kwargs):
                self.pid = 4242
                self.stdout = fake_stdout
                self.alive = True

            def poll(self):
                return None if self.alive else -9

            def wait(self, timeout=None):
                waits.append(timeout)
                if len(waits) == 1:
                    raise subprocess.TimeoutExpired(["fake"], timeout)
                self.alive = False
                return -9

            def kill(self):
                killed.append("kill")
                self.alive = False

        class FakeSelector:
            def __enter__(self):
                return self

            def __exit__(self, *args):
                return False

            def register(self, fileobj, events):
                pass

            def select(self, timeout):
                return [(None, None)]

        with patch.object(fixture.subprocess, "Popen", FakePopen), \
             patch.object(fixture.selectors, "DefaultSelector", FakeSelector), \
             patch.object(fixture.os, "read", return_value=b""), \
             patch.object(fixture.os, "killpg", side_effect=lambda pid, sig: killed.append((pid, sig)), create=True):
            with self.assertRaisesRegex(fixture.Refusal, "command-deadline"):
                fixture.capture(["/usr/bin/true"], fixture.time.monotonic() + 5)
        self.assertTrue(killed)
        self.assertEqual(len(waits), 2)

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
        vetted = {**fixture.TOOLS, "ip": "/usr/bin/ip"}
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
                    run.checked_runner_platform(env, host, head, end, vetted)
                package.assert_not_called()

                uname.return_value = SimpleNamespace(release=profile["kernel"], machine="x86_64")
                receipt = run.checked_runner_platform(env, host, head, end, vetted)
                package.assert_called_once_with("dpkg", end, "-W", "-f=${Version}", "systemd", tools=vetted)
                self.assertEqual(receipt["image_version"], profile["image_version"])
                self.assertEqual(receipt["kernel"], profile["kernel"])
                self.assertEqual(receipt["systemd"], profile["systemd_package"])

                package.return_value = "255.4-1ubuntu8.19"
                with self.assertRaisesRegex(fixture.Refusal, "runner-systemd"):
                    run.checked_runner_platform(env, host, head, end, vetted)

    def test_preflight_vets_tools_before_any_host_command(self):
        pins = fixture.load_pins()
        profile = pins["host"]["profiles"][0]
        workspace = run.ROOT
        scratch = workspace.parent.parent / "_temp"
        env = {
            **self.hosted_env(profile),
            "GITHUB_WORKSPACE": str(workspace),
            "RUNNER_TEMP": str(scratch),
            "CARGO_TARGET_DIR": str(workspace / "target"),
        }
        # Refusal during tool vetting must stop before git, dpkg-query or any other command runs.
        with patch.object(run.sys, "platform", "linux"), \
             patch.object(run, "plain_path", side_effect=lambda p, *a, **kw: p), \
             patch.object(run, "vet_tools", side_effect=fixture.Refusal("runner-tool")), \
             patch.object(run, "host_command") as mocked_cmd:
            with self.assertRaisesRegex(fixture.Refusal, "runner-tool"):
                run.checked_metadata(env, pins, 1001, 10)
            mocked_cmd.assert_not_called()

        # When vetting succeeds, vet_tools runs first and callers receive the vetted canonical paths.
        order = []
        vetted = {**fixture.TOOLS, "ip": "/usr/bin/ip", "cc": "/usr/bin/x86_64-linux-gnu-gcc-13"}

        def fake_vet(_tools):
            order.append("vet_tools")
            return vetted

        def fake_cmd(tool, _end, *args, tools=None, **kwargs):
            order.append((tool, tools[tool]))
            if tool == "git":
                return env["GITHUB_SHA"] + "\n"
            raise fixture.Refusal("stop-after-platform")

        with patch.object(run.sys, "platform", "linux"), \
             patch.object(run, "plain_path", side_effect=lambda p, *a, **kw: p), \
             patch.object(run, "vet_tools", side_effect=fake_vet), \
             patch.object(run, "host_command", side_effect=fake_cmd), \
             patch.object(run, "os_version", return_value="24.04"), \
             patch.object(run.os, "uname", return_value=SimpleNamespace(release=profile["kernel"], machine="x86_64"),
                          create=True):
            with self.assertRaisesRegex(fixture.Refusal, "stop-after-platform"):
                run.checked_metadata(env, pins, 1001, 10)
        self.assertEqual(order[0], "vet_tools")
        self.assertEqual(order[1], ("git", "/usr/bin/git"))
        self.assertEqual(order[2], ("dpkg", "/usr/bin/dpkg-query"))

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

    def test_probe_exit_diagnostic_is_included_in_the_encoded_failure_receipt(self):
        identity = {"repo": "org/repo", "head": "a"*40, "run": "1", "attempt": "1"}
        resources = isolation.Resources(identity, fixture.load_pins())
        unit = f"arx-r2-{resources.id}-b0.service"

        def contain(end):
            resources.event("b", "boot0", b"r2-init-v1\n", end, process=(unit, "a" * 32, 123))

        with patch.object(resources, "contain", side_effect=contain), patch.object(Path, "exists", return_value=False), \
             patch.object(isolation, "vet_tool", return_value="/usr/bin/journalctl"), \
             patch.object(isolation, "capture", return_value=b'{"MESSAGE":"R2 bootstrap await-broker errno=2"}\n'):
            receipt = run.Qualification().run(lambda _end: (identity, {}, resources), lambda *_: {})
        encoded = run.encode_receipt(receipt)
        self.assertTrue(encoded.isascii())
        self.assertEqual(json.loads(encoded)["diagnostic"], {
            "instance": "b0", "gate": "boot0", "bootstrap": [{"category": "await-broker", "errno": 2}],
        })
        self.assertEqual(receipt["first"], {"phase": "contain", "category": "probe-process-exited"})
        self.assertEqual(receipt["cleanup"], "confirmed")

    def test_success_requires_every_phase_and_complete_cleanup(self):
        steps = []
        resource = SimpleNamespace(facts={}, failure_diagnostic=None, contain=lambda _: steps.append("contain"),
                                   network=lambda _: steps.append("network"), cleanup=lambda _: [])
        result = run.Qualification().run(lambda _end: ({}, {}, resource), lambda *_: {})
        self.assertEqual(steps, ["contain", "network"])
        self.assertEqual(set(result["durations_seconds"]), set(fixture.TIME))
        self.assertEqual(sum(fixture.TIME.values()), 85)
        self.assertEqual(result["cleanup"], "confirmed")
        self.assertIsNone(result["first"])
        self.assertNotIn("diagnostic", result)
