import contextlib
import io
import stat
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import ubuntu_mirror as mirror


class UbuntuMirrorTests(unittest.TestCase):
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
                                 mirror.os.O_RDONLY | 0x20000 | 0x80000)

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
                        "signed-by=/private-keyring"):
            with self.subTest(options=options), self.assertRaises(mirror.Refusal):
                mirror.rewrite(f"deb [{options}] http://archive.ubuntu.com/ubuntu resolute main\n".encode(), ".list")
        for field in ("Trusted: yes", "Allow-Insecure: yes", "Check-Valid-Until: no",
                      "Signed-By: /private-keyring"):
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
