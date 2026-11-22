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
from unittest.mock import Mock, call, patch

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

    def test_live_client_credentials_have_agent_private_parent(self):
        for node, options in (("a", {}), ("b", {"founder_key": [1] * 32})):
            with self.subTest(node=node):
                root = Path("/data")
                with patch.object(fixture, "private_directory") as directory, patch.object(
                    fixture, "record_json"
                ) as record, patch.object(type(root), "symlink_to"):
                    fixture.install_live(root, node, **options)

                clients = root / "clients"
                self.assertEqual([entry for entry in directory.call_args_list if entry.args[0] == clients],
                                 [call(clients, 0o700, 21073, 21073)])
                self.assertEqual(
                    [(entry.args[0], entry.args[2], entry.args[3]) for entry in record.call_args_list
                     if entry.args[0].parent == clients],
                    [(clients / name, 21073, 0o400) for name in ("fixture.json", "foreign.json")],
                )

    def test_anchor_artifact_is_bound_to_the_exact_rust_library_test_and_receipt(self):
        target = PurePosixPath("/work/target")
        work = target.parent
        proofs = PurePosixPath("/proof")
        executable = target / "debug/deps/arxd-0123456789abcdef"
        raw_elf = elf()
        identity = {"head": "a"*40, "run": "1", "attempt": "2"}
        link = {
            "reason": "compiler-artifact", "manifest_path": str(work / "crates/arxd/Cargo.toml"),
            "target": {"name": "arxd", "kind": ["lib"]}, "profile": {"test": True},
            "executable": str(executable),
        }
        finish = {"reason": "build-finished", "success": True}

        def select(messages, **changes):
            raw = "\n".join(map(json.dumps, messages)).encode("ascii")
            receipt = {
                **identity, "schema": "arxcovo/c16-anchor/1", "target_dir": str(target),
                "executable": str(executable), "sha256": hashlib.sha256(raw_elf).hexdigest(),
                "bytes": len(raw_elf), "messages_sha256": hashlib.sha256(raw).hexdigest(),
                "selector": fixture.ANCHOR_TEST, **changes,
            }
            files = {
                "c16-anchor-build.jsonl": (raw, 100),
                "c16-anchor-receipt.json": (json.dumps(receipt).encode("ascii"), 200),
                executable.name: (raw_elf, 60),
            }

            @contextmanager
            def opened(path, *_args, **_kwargs):
                data, stamp = files[path.name]
                yield io.BytesIO(data), SimpleNamespace(st_size=len(data), st_mtime_ns=stamp)

            with patch.object(fixture, "Path", PurePosixPath), patch.object(fixture, "open_artifact", opened), \
                 patch.object(fixture, "remaining", return_value=10):
                return fixture.select_anchor_probe(str(target), work, 1001, proofs, identity, 100)

        self.assertEqual(select([link, finish]), (executable, hashlib.sha256(raw_elf).hexdigest()))
        for rows, changes, category in (
            ([link, link, finish], {}, "anchor-build-messages"),
            ([{**link, "target": {"name": "arxd", "kind": ["bin"]}}, finish], {}, "anchor-build-messages"),
            ([link, finish], {"attempt": "1"}, "anchor-receipt"),
            ([link, finish], {"selector": "another-item"}, "anchor-receipt"),
            ([link, finish], {"executable": "/wrong/arxd-0123456789abcdef"}, "anchor-path"),
            ([link, finish], {"sha256": "b"*64}, "anchor-digest"),
        ):
            with self.subTest(category=category), self.assertRaisesRegex(fixture.Refusal, category):
                select(rows, **changes)

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

    def test_closed_dispatch_keeps_dry_inert_and_live_separate(self):
        env = {"SUDO_UID": "1001"}
        with patch.object(run.sys, "platform", "linux"), patch.object(run.os, "geteuid", return_value=0, create=True):
            result = {"first": None, "cleanup": "confirmed"}
            with patch.object(run, "dry_run", return_value=result) as dry, \
                 patch.object(run, "live_run", return_value=result) as live, patch("builtins.print") as output:
                with patch("sys.stderr", new_callable=io.StringIO):
                    for args in ([], ["--mode", "daemon"], ["--mode", "dry", "arxd"],
                                 ["--mod", "dry"], ["--mode", "dry", "--mode", "live"]):
                        with self.subTest(args=args):
                            with self.assertRaises(SystemExit):
                                run.entry(args, env)
                live.assert_not_called()
                self.assertEqual(run.entry(["--mode", "dry"], env), 0)
                dry.assert_called_once_with(env, 1001)
                self.assertTrue(output.call_args.args[0].startswith("R2 receipt "))
                live.assert_not_called()
                self.assertEqual(run.entry(["--mode", "live"], env), 0)
                live.assert_called_once_with(env, 1001)
                dry.assert_called_once_with(env, 1001)
                self.assertTrue(output.call_args.args[0].startswith("C16 receipt "))
        helper = (fixture.HERE / "bootstrap.c").read_text(encoding="ascii")
        inert = helper.split('    dir("/run/r2", 0700);')[1].split('static bool run_id')[0]
        self.assertIn("launch_broker(false);", inert)
        self.assertNotIn("ARXD", inert)
        self.assertIn("live ? ARXD : PROBE", helper)
        self.assertIn('execve(live ? ARXD : PROBE, live ? live_args : dry_args', helper)
        for path in ("run.py", "fixture.py", "isolation.py"):
            text = (fixture.HERE / path).read_text(encoding="ascii")
            for command in ("executor-probe.py", "delegated-build-probe.py", "m3-c13",
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


class LiveCampaign(unittest.TestCase):
    @staticmethod
    def receipt(term, leader, index, tag):
        return struct.pack(">QQQ", term, leader, index) + bytes([tag])*32

    def test_consumed_reconnect_accepts_both_libtest_stdout_layouts(self):
        before = bytes((1, 1, 1, 1, 1)) + struct.pack(">QQ", 120, 121) + b"h"*32
        after = bytes((1, 2, 2, 1, 1)) + struct.pack(">QQ", 130, 131) + b"h"*32
        prefix = b"test " + fixture.ANCHOR_TEST.encode("ascii") + b" ... "
        summary = b"test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
        for stdout in (b"C16_ANCHOR anchor-refused\n" + prefix + b"ok\n" + summary,
                       prefix + b"C16_ANCHOR anchor-refused\nok\n" + summary):
            with self.subTest(serial=stdout.startswith(prefix)):
                resources = SimpleNamespace(root=Path("/fixture"))
                campaign = run.Campaign(resources)
                with patch.object(campaign, "owner", side_effect=[before, after]) as owner, patch.object(
                    campaign, "ready", return_value=b"\x04\x01\x01"
                ), patch.object(campaign, "evidence", return_value=b"\x04\x02\x01"), patch.object(
                    run, "log_audit", return_value=({1: 1, 3: 2, 4: 1, 6: 1}, [], [], (5, 1))
                ), patch.object(run, "child_reply", return_value=stdout) as child:
                    campaign.retry_consumed(bytearray(b"test-only"), 100, 1000)
                child.assert_called_once_with(resources, "b", "anchor", 1000, bytearray(b"test-only"), limit=1024)
                self.assertEqual(owner.call_args_list, [call("a", "owner-time", 16, 53, 1000)] * 2)

    def test_anchor_stdout_classifies_only_closed_tokens_and_one_summary(self):
        summary = b"test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
        for outcome in ("input-refused", "anchor-received", "anchor-refused", "anchor-unconfirmed", "deadline"):
            with self.subTest(outcome=outcome):
                self.assertEqual(run.anchor_client_status(b"C16_ANCHOR " + outcome.encode() + b"\n" + summary),
                                 (outcome, "1"))
        for line in (b"C16_ANCHOR injected\n", b"test wrong::name ... C16_ANCHOR anchor-refused\n",
                     b"C16_ANCHOR anchor-refused\nC16_ANCHOR deadline\n"):
            self.assertEqual(run.anchor_client_status(line + summary), ("missing", "1"))
        self.assertEqual(run.anchor_client_status(summary.replace(b"1 passed", b"0 passed")), ("missing", "0"))
        self.assertEqual(run.anchor_client_status(summary.replace(b"1 passed", b"42 passed")), ("missing", "other"))
        self.assertEqual(run.anchor_client_status(summary + summary), ("missing", "other"))
        self.assertEqual(run.anchor_client_status(summary.replace(b"ok.", b"FAILED.")), ("missing", "other"))

    def test_anchor_refusal_diagnostics_keep_only_tokens_and_owner_delta(self):
        before = bytes((1, 1, 1, 1, 1)) + struct.pack(">QQ", 120, 121) + b"h"*32
        after = bytes((1, 2, 2, 1, 1)) + struct.pack(">QQ", 130, 131) + b"h"*32
        stdout = (b"raw-output-sentinel\nC16_ANCHOR anchor-unconfirmed\n"
                  b"test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n")
        for reply, expected_after, delta in ((after, "refused", "1"),
                                             (fixture.Refusal("owner-reply"), "missing", "missing")):
            with self.subTest(delta=delta):
                campaign = run.Campaign(SimpleNamespace(root=Path("/fixture")))
                with patch.object(campaign, "owner", side_effect=[before, reply]) as owner, patch.object(
                    campaign, "ready", return_value=b"\x04\x01\x01"
                ), patch.object(campaign, "evidence") as evidence, patch.object(
                    run, "log_audit", return_value=({1: 1, 3: 2, 4: 1, 6: 1}, [], [], (5, 1))
                ), patch.object(run, "child_reply", return_value=stdout):
                    with self.assertRaisesRegex(fixture.Refusal, "^anchor-client-refusal$"):
                        campaign.retry_consumed(bytearray(b"paste-sentinel"), 100, 1000)
                self.assertEqual(owner.call_count, 2)
                evidence.assert_not_called()
                self.assertEqual(campaign.anchor_diagnostic, {
                    "token": "anchor-unconfirmed", "passed": "1",
                    "owner_time": {"ordinal_delta": delta, "before": "accepted", "after": expected_after},
                })
                encoded = run.encode_receipt(campaign.anchor_diagnostic)
                for secret in ("raw-output-sentinel", "paste-sentinel", "hhhh", "130"):
                    self.assertNotIn(secret, encoded)

    def test_reconnect_requires_same_query_valid_sample_epoch_and_counts(self):
        good = bytes((1, 1, 1, 1, 1)) + struct.pack(">QQ", 120, 121) + b"d"*32
        rejected = bytes((1, 2, 2, 1, 1)) + struct.pack(">QQ", 130, 131) + b"d"*32
        before, after = run.time_evidence(good), run.time_evidence(rejected)
        base = (before, after, 100, (1, 1), (1, 1), (2, 1))
        run.check_reconnect(*base)
        for offset, wrong in ((1, (2, 3, 130, 131, b"d"*32)),
                              (1, (2, 2, 130, 131, b"x"*32)),
                              (1, (2, 2, 130, 780_100, b"d"*32)),
                              (2, 125), (4, (2, 1)), (5, (2, 2))):
            args = list(base)
            args[offset] = wrong
            with self.subTest(offset=offset, wrong=wrong), self.assertRaisesRegex(fixture.Refusal, "consumed-anchor"):
                run.check_reconnect(*args)
        for invalid in (rejected[:3] + b"\0" + rejected[4:], rejected[:4] + b"\0" + rejected[5:], rejected[:-1]):
            with self.assertRaisesRegex(fixture.Refusal, "owner-"):
                run.time_evidence(invalid)

    def test_ledger_slots_and_distinct_observers_are_closed(self):
        receipts = [self.receipt(5, 1, 15, 13), self.receipt(5, 1, 16, 14)]
        encoded = run.ledger_slots(receipts)
        self.assertEqual(encoded[:2], b"\x01\x02")
        self.assertEqual(encoded[2:114], b"".join(receipts))
        self.assertEqual(encoded[114:], bytes(112))
        for invalid in ([receipts[0]], list(reversed(receipts)), [receipts[0]]*2,
                        [receipts[0], self.receipt(5, 1, 17, 13)]):
            with self.assertRaisesRegex(fixture.Refusal, "ledger-"):
                run.ledger_slots(invalid)

        def checkpoint(node):
            raw = bytes((1, 5 if node == 1 else 3, 1 if node == 1 else 2, int(node != 1)))
            raw += struct.pack(">QQQ", node, 5, 1) + bytes([node])*16 + bytes([16])*16 + b"k"*32
            raw += struct.pack(">QQQQQQQQQ", 5, 1, 10, 5, 1, 16, 1, 2, 3)
            return run.ledger_checkpoint(raw)

        nodes = {name: checkpoint(i) for i, name in enumerate("abc", 1)}
        identities = {name: value["identity"] for name, value in nodes.items()}
        self.assertEqual(run.check_checkpoint_set(nodes, "abc", receipts, identities), ((5, 1), (5, 1, 10)))
        for invalid in ({key: nodes[key] for key in "bc"}, {**nodes, "b": nodes["c"]},
                        {**nodes, "b": {**nodes["b"], "applied": (5, 1, 15)}},
                        {**nodes, "b": {**nodes["b"], "member_pos": (5, 1, 11)}},
                        {**nodes, "c": {**nodes["c"], "identity": b"	"*16 + nodes["c"]["identity"][16:]}}):
            with self.assertRaisesRegex(fixture.Refusal, "ledger-"):
                run.check_checkpoint_set(invalid, "abc", receipts, identities)

    def test_build_dispatches_once_only_after_the_three_denials(self):
        resources = Mock()
        resources.root = Path("/fixture")
        driver = run.Campaign(resources)
        foreign = Mock()
        foreign.call.return_value = {"outcome": "refused"}
        agent = Mock()
        agent.call.side_effect = [{"targets": [2]}, {"outcome": "refused"}, {"outcome": "refused"}]
        operation = "a" * 64
        agent.tool_result.return_value = {
            "outcome": "succeeded", "operation_id": operation, "target_activity": {
                "uid": 21072, "gid": 21072, "exit_code": 0, "cleanup_confirmed": True, "activity_id": 1,
            },
        }
        events = Mock()
        events.attach_mock(foreign.call, "foreign")
        events.attach_mock(agent.call, "probe")
        events.attach_mock(agent.send, "dispatch")
        events.attach_mock(agent.tool_result, "terminal")

        @contextmanager
        def connect(role, _end):
            yield {"foreign": foreign, "agent": agent}[role]

        @contextmanager
        def artifact(*_args, **_kwargs):
            yield None, SimpleNamespace(st_uid=21072, st_gid=21072)

        expected = (operation.upper(), 1, "succeeded")
        with patch.object(driver, "mcp", side_effect=connect), patch.object(
            run, "activity", side_effect=[[], [], [], [expected], [expected], []]
        ), patch.object(driver, "build_witness", return_value=(Path("/fixture/runs"), "arx-2-0")), \
             patch.object(run, "open_artifact", side_effect=artifact):
            driver.delegated_build(100)
        args = {"target": 2, **fixture.build_descriptor()}
        self.assertEqual(events.mock_calls, [
            call.foreign("run_command", args), call.probe("discover_build_targets", {}),
            call.probe("run_command", {**args, "workspace": "link"}),
            call.probe("run_command", {**args, "workspace": "writable"}),
            call.dispatch("run_command", args), call.terminal(),
        ])
        self.assertEqual(driver.status["builds"], 1)

    def test_fault_removes_founder_before_mints_and_recreates_endpoint_once(self):
        res = Mock()
        res.root = Path("/fixture")
        res.nodes = dict.fromkeys("abc")
        res.stop_live.side_effect = lambda node, *args, **kwargs: res.nodes.pop(node)
        driver = run.Campaign(res)
        driver.pre_epoch = (5, 1)
        driver.receipts = [self.receipt(5, 1, 15, 13), self.receipt(5, 1, 16, 14)]
        driver.identities = {"a": b"original"}
        member_pos = (5, 1, 13)
        driver.membership = [(member_pos, b"members")]
        events = Mock()
        events.attach_mock(res.stop_live, "stop")
        events.attach_mock(res.drop_founder_endpoint, "drop")
        events.attach_mock(res.restore_founder_endpoint, "restore")
        events.attach_mock(Mock(side_effect=lambda node, _: b"\x05" if node == "c" else b"\x03"), "evidence")
        events.attach_mock(Mock(), "mint")
        events.attach_mock(Mock(return_value=(6, 3)), "observe")
        events.attach_mock(Mock(), "start")
        with patch.object(driver, "owner"), patch.object(
            run, "ledger_checkpoint", return_value={"node": 1, "epoch": (5, 1), "member_pos": member_pos,
                                                      "identity": b"original"}
        ), patch.object(run, "image_id", return_value=(3, 7)), patch.multiple(
            driver, evidence=events.evidence, mint_ledger=events.mint, observe=events.observe, start=events.start
        ):
            driver.recover(100)
        self.assertEqual(events.mock_calls, [
            call.stop("a", 100, crash=True), call.drop(100), call.evidence("b", 100), call.evidence("c", 100),
            call.mint("c", 100), call.observe("during_loss", "bc", 100), call.restore(100),
            call.start("a3", (3, 4, 5), 100), call.observe("after_recovery", "abc", 100),
        ])
        self.assertEqual(driver.status["leader_kill"], 1)

    def test_live_setup_and_campaign_order_including_cleanup_failure(self):
        events = Mock()
        res = Mock()
        res.facts = {"probe": "qualified"}
        res.failure_diagnostic = None
        for name in ("contain", "network", "retire_probes", "cleanup", "stop_live"):
            events.attach_mock(getattr(res, name), name)
        res.cleanup.return_value = ["mount-b"]
        operations = Mock()
        operations.checkpoints = {"activation": {"consumed_refusals": 1}}
        operations.status = {**run.ACCOUNT, "normal_closes": 0}
        for name in ("activate", "build_and_mint", "recover", "seal"):
            events.attach_mock(getattr(operations, name), name)
        with patch.object(run, "checked_metadata", return_value=({"modules": {}}, None, None, None, None)), \
             patch.object(run, "Resources", return_value=res), patch.object(run, "stage", return_value={}), \
             patch.object(run, "Campaign", return_value=operations):
            result = run.live_run({}, 1001)
        self.assertEqual([entry[0] for entry in events.mock_calls],
                         ["contain", "network", "retire_probes", "activate", "build_and_mint", "recover",
                          "seal", "stop_live", "stop_live", "stop_live", "cleanup"])
        self.assertEqual(result["accounting"], run.ACCOUNT)
        self.assertEqual(result["first"], {"phase": "cleanup", "category": "partial-cleanup"})
        self.assertEqual(result["cleanup"], "incomplete")
        self.assertEqual(sum(fixture.LIVE_TIME.values()), 240)
        self.assertEqual(set(result["durations_seconds"]), set(fixture.LIVE_TIME))
        self.assertTrue(run.encode_receipt(result).isascii())

    def test_live_receipt_preserves_anchor_diagnostic_and_first_failure(self):
        resources = Mock()
        resources.facts = {}
        resources.cleanup.return_value = ["unit-a2"]
        resources.failure_diagnostic = None
        campaign = Mock()
        campaign.activate.side_effect = fixture.Refusal("anchor-client-refusal")
        campaign.anchor_diagnostic = {
            "token": "input-refused", "passed": "1",
            "owner_time": {"ordinal_delta": "0", "before": "accepted", "after": "accepted"},
        }
        with patch.object(run, "checked_metadata", return_value=({"modules": {}}, None, None, None, None)), \
             patch.object(run, "Resources", return_value=resources), patch.object(run, "stage", return_value={}), \
             patch.object(run, "Campaign", return_value=campaign):
            result = json.loads(run.encode_receipt(run.live_run({}, 1001)))
        self.assertEqual(result["anchor_diagnostic"], campaign.anchor_diagnostic)
        self.assertEqual(result["first"], {"phase": "activation", "category": "anchor-client-refusal"})
        self.assertEqual(result["cleanup"], "incomplete")
        campaign.build_and_mint.assert_not_called()

    def test_live_activation_refusal_prevents_build_loss_and_restart(self):
        res = Mock()
        res.facts = {}
        res.cleanup.return_value = ["unit-a2"]
        res.failure_diagnostic = None
        operations = Mock()
        operations.activate.side_effect = fixture.Refusal("consumed-anchor-evidence")
        with patch.object(run, "checked_metadata", return_value=({"modules": {}}, None, None, None, None)), \
             patch.object(run, "Resources", return_value=res), patch.object(run, "stage", return_value={}), \
             patch.object(run, "Campaign", return_value=operations):
            result = run.live_run({}, 1001)
        self.assertEqual(result["first"], {"phase": "activation", "category": "consumed-anchor-evidence"})
        self.assertEqual(result["cleanup_errors"], ["unit-a2"])
        self.assertNotIn("accounting", result)
        operations.build_and_mint.assert_not_called()
        operations.recover.assert_not_called()
        res.stop_live.assert_not_called()
        res.cleanup.assert_called_once()
