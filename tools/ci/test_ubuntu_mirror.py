import contextlib
import io
import stat
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import ubuntu_mirror as mirror

MIRRORS = (b"http://azure.archive.ubuntu.com/ubuntu/\tpriority:1\n"
           b"https://archive.ubuntu.com/ubuntu/\tpriority:2\n"
           b"https://security.ubuntu.com/ubuntu/\tpriority:3\n")
INDIRECT_SOURCE = (b"Types: deb\nURIs: mirror+file:/etc/apt/apt-mirrors.txt\n"
                   b"Suites: resolute resolute-updates resolute-backports resolute-security\n"
                   b"Components: main restricted universe multiverse\n"
                   b"Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n")


def fixture_reader(path):
    info = path.stat()
    # Model root policy facts on owned temporary files; no root file is opened.
    facts = SimpleNamespace(st_mode=stat.S_IFREG | 0o644, st_uid=0, st_nlink=1,
                            st_dev=info.st_dev, st_ino=info.st_ino)
    return path.read_bytes(), facts


def fixture_directory(_):
    return SimpleNamespace(st_mode=stat.S_IFDIR | 0o755, st_uid=0)


def make_tree(directory):
    root = Path(directory) / "apt"
    sources = root / "sources.list.d"
    sources.mkdir(parents=True)
    source = sources / "ubuntu.sources"
    source.write_bytes(INDIRECT_SOURCE)
    (root / "apt-mirrors.txt").write_bytes(MIRRORS)
    return root, source


class UbuntuMirrorTests(unittest.TestCase):
    def setUp(self):
        # Windows lacks fchmod. Record the publication request on owned temp
        # files without claiming a real Unix permission change was exercised.
        chmod = patch.object(mirror.os, "fchmod", create=True)
        self.addCleanup(chmod.stop)
        chmod.start()

    def test_complete_indirect_runner_plan_rechecks_and_publishes(self):
        self.assertEqual(len(MIRRORS), 144)
        with tempfile.TemporaryDirectory() as directory:
            root, source = make_tree(directory)
            legacy = root / "sources.list"
            legacy.write_bytes(b"deb https://security.ubuntu.com/ubuntu resolute-security main\n")
            unrelated = root / "sources.list.d" / "vendor.list"
            unrelated.write_bytes(b"deb https://packages.microsoft.com/repos/code stable main\n")
            reads = []

            def read(path):
                reads.append(path.name)
                return fixture_reader(path)

            with patch.object(mirror.os, "fchmod", create=True) as mode:
                self.assertEqual(mirror._apply(root, read, fixture_directory), 2)
            self.assertEqual(source.read_bytes(), INDIRECT_SOURCE.replace(mirror.MIRROR_LIST.encode(),
                                                                          mirror.MIRROR.encode()))
            self.assertEqual((root / "apt-mirrors.txt").read_bytes(), MIRRORS)
            self.assertEqual(unrelated.read_bytes(), b"deb https://packages.microsoft.com/repos/code stable main\n")
            self.assertEqual(reads.count("ubuntu.sources"), 2)
            self.assertEqual(reads.count("sources.list"), 2)
            self.assertEqual(reads.count("apt-mirrors.txt"), 1)
            self.assertEqual([call.args[1] for call in mode.call_args_list], [0o644, 0o644])
            self.assertFalse(any(root.rglob(".arxcovo-mirror-*")))

    def test_entire_plan_validation_precedes_any_publication(self):
        with tempfile.TemporaryDirectory() as directory:
            root, source = make_tree(directory)
            bad = root / "sources.list.d" / "z.sources"
            bad.write_bytes(INDIRECT_SOURCE.replace(b"Signed-By:", b"Trusted: yes\nSigned-By:"))
            with patch.object(mirror.os, "replace") as publish:
                with self.assertRaises(mirror.Refusal):
                    mirror._apply(root, fixture_reader, fixture_directory)
            publish.assert_not_called()
            self.assertEqual(source.read_bytes(), INDIRECT_SOURCE)

    def test_invalid_indirect_mirror_list_never_changes_sources(self):
        for data in (b"", b"https://foreign.invalid/ubuntu priority:1\n",
                     b"http://archive.ubuntu.com/ubuntu priority:1 extra\n",
                     b"mirror+file:/private priority:1\n", b"https://[bad/\n",
                     b"http://archive.ubuntu.com/ubuntu priority:-1\n", b"x" * 4097):
            with self.subTest(data_size=len(data)), tempfile.TemporaryDirectory() as directory:
                root, source = make_tree(directory)
                (root / "apt-mirrors.txt").write_bytes(data)
                with self.assertRaises(mirror.Refusal):
                    mirror._apply(root, fixture_reader, fixture_directory)
                self.assertEqual(source.read_bytes(), INDIRECT_SOURCE)

    def test_fresh_bytes_and_identity_each_refuse_stale_publication(self):
        for change in ("bytes", "identity"):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                root, source = make_tree(directory)
                calls = 0

                def read(path):
                    nonlocal calls
                    data, facts = fixture_reader(path)
                    if path == source:
                        calls += 1
                        if calls == 2:
                            if change == "bytes":
                                data += b"# changed\n"
                            else:
                                facts.st_ino += 1
                    return data, facts

                with patch.object(mirror.os, "replace") as publish:
                    with self.assertRaises(mirror.Refusal):
                        mirror._apply(root, read, fixture_directory)
                publish.assert_not_called()
                self.assertEqual(source.read_bytes(), INDIRECT_SOURCE)

    def test_publication_error_cleans_only_its_stage_and_preserves_target(self):
        with tempfile.TemporaryDirectory() as directory:
            root, source = make_tree(directory)
            with patch.object(mirror.os, "replace", side_effect=OSError("injected")), \
                 patch.object(mirror.os, "fchmod", create=True):
                with self.assertRaises(OSError):
                    mirror._apply(root, fixture_reader, fixture_directory)
            self.assertEqual(source.read_bytes(), INDIRECT_SOURCE)
            self.assertFalse(any(root.rglob(".arxcovo-mirror-*")))

    def test_parent_guards_file_count_and_empty_plan_refuse_before_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            root, _ = make_tree(directory)
            for mode, uid in [(stat.S_IFDIR | 0o777, 0), (stat.S_IFDIR | 0o755, 1),
                              (stat.S_IFLNK | 0o755, 0)]:
                with self.subTest(mode=mode, uid=uid), patch.object(mirror.os, "replace") as publish:
                    with self.assertRaises(mirror.Refusal):
                        mirror._apply(root, fixture_reader,
                                      lambda _: SimpleNamespace(st_mode=mode, st_uid=uid))
                    publish.assert_not_called()
            for i in range(32):
                (root / "sources.list.d" / f"vendor-{i}.list").write_bytes(b"# empty\n")
            with self.assertRaises(mirror.Refusal):
                mirror._apply(root, fixture_reader, fixture_directory)
        with tempfile.TemporaryDirectory() as directory:
            root, source = make_tree(directory)
            source.write_bytes(b"# no active Ubuntu source\n")
            with self.assertRaises(mirror.Refusal):
                mirror._apply(root, fixture_reader, fixture_directory)

    def test_lowercase_deb822_uri_is_recognized(self):
        before = INDIRECT_SOURCE.replace(b"URIs:", b"uris:")
        self.assertEqual(mirror.rewrite(before, ".sources", indirect=True),
                         before.replace(mirror.MIRROR_LIST.encode(), mirror.MIRROR.encode()))

    def test_uri_expansion_cannot_exceed_output_bound(self):
        prefix = b"deb http://archive.ubuntu.com/ubuntu resolute main\n#"
        before = prefix + b"x" * (mirror.MAX_BYTES - len(prefix))
        self.assertEqual(len(before), mirror.MAX_BYTES)
        with self.assertRaises(mirror.Refusal):
            mirror.rewrite(before, ".list")

    def test_observed_runner_mirror_file_requires_validation(self):
        before = (b"Types: deb\nURIs: mirror+file:/etc/apt/apt-mirrors.txt\n"
                  b"Suites: resolute resolute-updates resolute-backports resolute-security\n"
                  b"Components: main restricted universe multiverse\n"
                  b"Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n")
        with self.assertRaises(mirror.Refusal):
            mirror.rewrite(before, ".sources")

    def test_source_reads_are_bounded_and_check_native_facts(self):
        facts = dict(st_mode=stat.S_IFREG | 0o644, st_uid=0, st_nlink=1, st_size=5)
        cases = [(None, b"fixed", True), (dict(st_uid=1), b"fixed", False),
                 (dict(st_nlink=2), b"fixed", False),
                 (dict(st_mode=stat.S_IFREG | 0o666), b"fixed", False),
                 (dict(st_mode=stat.S_IFDIR | 0o755), b"fixed", False),
                 (dict(st_size=65537), b"fixed", False), (None, b"x" * 65537, False)]
        for changes, data, accepted in cases:
            info = SimpleNamespace(**(facts | (changes or {})))
            with self.subTest(changes=changes, accepted=accepted), \
                 patch.object(mirror.os, "O_NOFOLLOW", 0x20000, create=True), \
                 patch.object(mirror.os, "O_CLOEXEC", 0x80000, create=True), \
                 patch.object(mirror.os, "O_NONBLOCK", 0x800, create=True), \
                 patch.object(mirror.os, "open", return_value=123) as opened, \
                 patch.object(mirror.os, "fstat", return_value=info), \
                 patch.object(mirror.os, "fdopen") as file:
                stream = file.return_value.__enter__.return_value
                stream.read.return_value = data
                if accepted:
                    self.assertEqual(mirror.read_source("fixed"), (data, info))
                    stream.read.assert_called_once_with(65537)
                else:
                    with self.assertRaises(mirror.Refusal):
                        mirror.read_source("fixed")
                self.assertEqual(opened.call_args.args[1],
                                 mirror.os.O_RDONLY | 0x20000 | 0x80000 | 0x800)

    def test_deb822_preserves_every_byte_except_uri_values(self):
        before = (b"# fixed comment\nTypes: deb deb-src\nURIs: http://azure.archive.ubuntu.com/ubuntu/\n"
                  b"Suites: resolute resolute-updates resolute-backports resolute-security\n"
                  b"Components: main restricted universe multiverse\nArchitectures: amd64\n"
                  b"Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n")
        after = mirror.rewrite(before, ".sources")
        self.assertEqual(after, before.replace(b"http://azure.archive.ubuntu.com/ubuntu/",
                                               mirror.MIRROR.encode()))
        self.assertEqual(mirror.rewrite(after, ".sources"), after)

    def test_legacy_options_suites_components_and_comments_are_preserved(self):
        before = (b"# archive.ubuntu.com comment\n"
                  b"deb [arch=amd64 signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] "
                  b"https://archive.ubuntu.com/ubuntu resolute main universe # fixed\n"
                  b"deb-src http://security.ubuntu.com/ubuntu/ resolute-security main\n")
        after = mirror.rewrite(before, ".list")
        self.assertEqual(after, before.replace(b"https://archive.ubuntu.com/ubuntu", mirror.MIRROR.encode())
                         .replace(b"http://security.ubuntu.com/ubuntu/", mirror.MIRROR.encode()))
        self.assertEqual(mirror.rewrite(after, ".list"), after)

    def test_third_party_repositories_are_unchanged(self):
        for data, suffix in [(b"deb https://packages.microsoft.com/repos/code stable main\n", ".list"),
                             (b"Types: deb\nURIs: https://packages.microsoft.com/repos/code\n"
                              b"Suites: stable\nComponents: main\n", ".sources")]:
            self.assertEqual(mirror.rewrite(data, suffix), data)

    def test_fixed_mirror_and_exact_original_uri_allowlist(self):
        self.assertEqual(mirror.MIRROR, "https://mirrors.edge.kernel.org/ubuntu")
        for host in ("azure.archive.ubuntu.com", "archive.ubuntu.com", "security.ubuntu.com"):
            for scheme in ("http", "https"):
                self.assertEqual(mirror.replacement(f"{scheme}://{host}/ubuntu"), mirror.MIRROR)
        for uri in ("https://user@archive.ubuntu.com/ubuntu", "http://archive.ubuntu.com:80/ubuntu",
                    "http://archive.ubuntu.com/other", "http://archive.ubuntu.com/ubuntu?x=1",
                    "http://archive.ubuntu.com/ubuntu#fragment", "ftp://archive.ubuntu.com/ubuntu",
                    "https://unrecognized.ubuntu.com/ubuntu"):
            with self.subTest(uri=uri), self.assertRaises(mirror.Refusal):
                mirror.replacement(uri)

    def test_insecure_or_foreign_key_settings_refuse(self):
        for options in ("trusted=yes", "ALLOW-INSECURE=YES", "allow-weak=yes",
                        "allow-downgrade-to-insecure=yes", "check-valid-until=no",
                        "signed-by=/private-keyring", "trusted=true", "trusted=1",
                        "allow-insecure=true", "check-valid-until=false"):
            with self.subTest(options=options), self.assertRaises(mirror.Refusal):
                mirror.rewrite(f"deb [{options}] http://archive.ubuntu.com/ubuntu resolute main\n".encode(), ".list")
        for field in ("Trusted: yes", "Allow-Insecure: yes", "Check-Valid-Until: no",
                      "Signed-By: /private-keyring", "Allow-Weak: yes",
                      "Allow-Downgrade-To-Insecure: yes"):
            with self.subTest(field=field), self.assertRaises(mirror.Refusal):
                mirror.rewrite(("Types: deb\nURIs: http://archive.ubuntu.com/ubuntu\n"
                                "Suites: resolute\nComponents: main\n" + field + "\n").encode(), ".sources")

    def test_mixed_or_duplicate_fields_refuse(self):
        for extra in ("URIs: http://security.ubuntu.com/ubuntu\n", "Suites: other\n",
                      " continuation\n"):
            with self.subTest(extra=extra), self.assertRaises(mirror.Refusal):
                mirror.rewrite(("Types: deb\nURIs: http://archive.ubuntu.com/ubuntu\n"
                                "Suites: resolute\nComponents: main\n" + extra).encode(), ".sources")
        with self.assertRaises(mirror.Refusal):
            mirror.rewrite(b"Types: deb\nURIs: http://archive.ubuntu.com/ubuntu https://foreign.invalid/repo\n"
                           b"Suites: resolute\nComponents: main\n", ".sources")

    def test_missing_required_fields_and_malformed_input_refuse(self):
        for data, suffix in [(b"x" * 65537, ".list"), (b"\xff", ".sources"),
                             (b"\x00", ".list"), (b"not a source", ".list"),
                             (b"deb http://archive.ubuntu.com/ubuntu resolute\n", ".list"),
                             (b"URIs: http://archive.ubuntu.com/ubuntu\n", ".sources"),
                             (b"", ".unknown")]:
            with self.subTest(suffix=suffix), self.assertRaises(mirror.Refusal):
                mirror.rewrite(data, suffix)

    def test_context_refusal_precedes_any_filesystem_access(self):
        with patch.object(mirror, "runner_context", return_value=False), \
             patch.object(mirror.Path, "lstat") as filesystem:
            with self.assertRaises(mirror.Refusal):
                mirror.apply()
        filesystem.assert_not_called()

    def test_invalid_arguments_never_apply_or_echo(self):
        with patch.object(mirror.sys, "argv", ["ubuntu_mirror.py", "private-input"]), \
             patch.object(mirror, "apply") as apply, \
             contextlib.redirect_stderr(io.StringIO()) as error:
            self.assertEqual(mirror.main(), 2)
        apply.assert_not_called()
        self.assertEqual(error.getvalue(), "ubuntu-mirror-arguments\n")

    def test_native_failure_reports_only_fixed_token(self):
        with patch.object(mirror.sys, "argv", ["ubuntu_mirror.py"]), \
             patch.object(mirror, "apply", side_effect=OSError("private-error")), \
             contextlib.redirect_stderr(io.StringIO()) as error:
            self.assertEqual(mirror.main(), 1)
        self.assertEqual(error.getvalue(), "ubuntu-mirror-refused\n")


if __name__ == "__main__":
    unittest.main()
