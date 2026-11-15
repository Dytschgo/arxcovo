"""Pure injected filesystem tests: no driver import or OS-backed read ports."""

import contextlib
import copy
import io
import inspect
import json
import stat
import unittest
from types import SimpleNamespace
from types import ModuleType
from unittest.mock import patch

import sudo_host_qualification as q


class _FS:
    def __init__(self):
        self.nodes = {}
        self.handles = {}
        self.closed = []
        self.calls = []
        self.names = ["synthetic-rule"]
        self.floor = dict(systemd=254, kernel=(5, 14, 0), cgroup2=True,
                          memory=True, pids=True, user_delegate=True,
                          hierarchy_protected=True)
        for path in ((), ("etc",), ("etc", "sudoers.d"), ("usr",),
                     ("usr", "bin"), ("usr", "local"), ("usr", "local", "bin")):
            self.add(path, mode=stat.S_IFDIR | 0o755)
        for path in q._POLICY_PATHS:
            self.add(path, b"SYNTHETIC_PRIVATE_MARKER\n")
        self.add(("etc", "sudoers.d", self.names[0]), b"synthetic only\n")
        self.add(q._C1, b"\x7fELFsynthetic", stat.S_IFREG | 0o4755)
        self.add(("bin",), b"usr/bin", stat.S_IFLNK | 0o777)

    def add(self, path, data=b"", mode=stat.S_IFREG | 0o440):
        self.nodes[path] = [SimpleNamespace(
            st_mode=mode, st_uid=0, st_nlink=1, st_size=len(data),
            st_dev=1, st_ino=len(self.nodes) + 1, st_mtime_ns=1, st_ctime_ns=1), data]

    def _open(self, path, flags):
        self.calls.append(path)
        assert flags == dict(readonly=True, nofollow=True, cloexec=True,
                             nonblock=True, directory=stat.S_ISDIR(self.nodes[path][0].st_mode))
        fd = len(self.handles) + 10
        self.handles[fd] = [path, 0]
        return fd

    def open_root(self, **flags):
        return self._open((), flags)

    def openat(self, parent, name, **flags):
        assert parent not in self.closed and "/" not in name
        path = self.handles[parent][0] + (name,)
        if path not in self.nodes:
            raise FileNotFoundError("SYNTHETIC_PRIVATE_MARKER")
        # Model a malicious adapter returning links/special nodes too: collector
        # must independently reject their fstat, not trust open flags alone.
        adjusted = dict(flags, directory=stat.S_ISDIR(self.nodes[path][0].st_mode))
        assert adjusted == flags or not flags["directory"]
        return self._open(path, adjusted)

    def fstat(self, fd):
        return copy.copy(self.nodes[self.handles[fd][0]][0])

    def read(self, fd, count):
        assert 0 < count <= 4096
        path, offset = self.handles[fd]
        block = self.nodes[path][1][offset:offset + count]
        self.handles[fd][1] += len(block)
        return block

    def entries(self, fd, limit):
        assert self.handles[fd][0] == ("etc", "sudoers.d") and limit == 33
        return iter(self.names[:limit])

    def statat(self, fd, name, **flags):
        assert self.handles[fd][0] == () and name == "bin" and flags == {"nofollow": True}
        return copy.copy(self.nodes[("bin",)][0])

    def readlinkat(self, fd, name, limit):
        assert self.handles[fd][0] == () and name == "bin" and limit == 9
        return self.nodes[("bin",)][1].decode("ascii")[:limit]

    def facts(self):
        return self.floor

    def close(self, fd):
        assert fd not in self.closed
        self.closed.append(fd)


class Qualification(unittest.TestCase):
    def collect(self, fs):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            result = q._collect(fs)
        self.assertEqual((out.getvalue(), err.getvalue()), ("", ""))
        self.assertEqual(set(fs.closed), set(fs.handles))
        encoded = json.dumps(result)
        for private in ("SYNTHETIC_PRIVATE_MARKER", "synthetic-rule", "/etc", "Traceback", "st_uid"):
            self.assertNotIn(private, encoded)
        self.assertFalse(result["qualified"])
        self.assertFalse(result["policy_reviewed"])
        self.assertEqual(set(result), set(q._empty("refused")))
        self.assertEqual(set(result["records"]), set(q._LABELS))
        return result

    def refused(self, fs):
        self.assertEqual(self.collect(fs), q._empty("refused"))

    def test_snapshot_is_not_attestation(self):
        fs = _FS()
        result = self.collect(fs)
        self.assertEqual(result["status"], "policy_unreviewed")
        self.assertTrue(result["floor_observed"])
        self.assertEqual(result["c2_state"], "absent")
        self.assertEqual(result["records"]["sudo_conf"], q._digest(fs.nodes[q._POLICY_PATHS[0]][1]))
        self.assertEqual(len(result["records"]["include_set"]["sha256"]), 64)
        self.assertEqual(q._REVIEWED_POLICY, ())

    def test_owner_write_types_and_links_every_level(self):
        paths = [(), ("usr",), ("etc",), ("etc", "sudoers.d"), q._C1,
                 q._POLICY_PATHS[0], ("etc", "sudoers.d", "synthetic-rule")]
        for path in paths:
            for attr, value in (("st_uid", 1000), ("st_mode", stat.S_IFLNK | 0o777),
                                ("st_mode", stat.S_IFIFO | 0o440),
                                ("st_mode", (stat.S_IFDIR if path in paths[:4] else stat.S_IFREG) | 0o777)):
                with self.subTest(path=path, attr=attr, value=value):
                    fs = _FS()
                    setattr(fs.nodes[path][0], attr, value)
                    self.refused(fs)
        for path in (q._C1, q._POLICY_PATHS[0]):
            fs = _FS()
            fs.nodes[path][0].st_nlink = 2
            self.refused(fs)

    def test_file_and_complete_set_caps(self):
        for size in (q._POLICY_CAP + 1, -1):
            fs = _FS()
            fs.nodes[q._POLICY_PATHS[0]][0].st_size = size
            with patch.object(fs, "read", wraps=fs.read) as read:
                self.refused(fs)
                read.assert_not_called()
        for count, size in ((33, 0), (9, q._POLICY_CAP), (1, q._POLICY_CAP + 1)):
            fs = _FS()
            fs.names = [f"f{i}" for i in range(count)]
            for name in fs.names:
                fs.add(("etc", "sudoers.d", name), b"x" * size)
            self.refused(fs)
        fs = _FS()
        fs.names = [f"f{i}" for i in range(8)]
        for name in fs.names:
            fs.add(("etc", "sudoers.d", name), b"x" * q._POLICY_CAP)
        self.assertEqual(self.collect(fs)["status"], "policy_unreviewed")
        fs = _FS()
        fs.nodes[q._C1][0].st_size = q._ELF_CAP + 1
        self.refused(fs)
        fs = _FS()
        fs.names = []
        self.assertEqual(self.collect(fs)["include_count"], 0)

    def test_names_and_entry_drift(self):
        for names in (("..",), ("a/b",), ("x\n",), ("a", "a"), ("x" * 256,), ("é",)):
            fs = _FS()
            fs.names = names
            self.refused(fs)
        fs = _FS()
        with patch.object(fs, "entries", side_effect=[iter(fs.names), iter(())]):
            self.refused(fs)

    def test_unknown_includes_plugins_routes_never_qualify(self):
        for path, data in ((q._POLICY_PATHS[2], b"@include /private/unknown\n"),
                           (q._POLICY_PATHS[0], b"Plugin unknown /private/plugin.so\n"),
                           (q._POLICY_PATHS[1], b"sudoers: ldap sss\n"),
                           (q._POLICY_PATHS[2], b"syntactically valid is not proof\n")):
            fs = _FS()
            fs.add(path, data)
            self.assertEqual(self.collect(fs)["status"], "policy_unreviewed")
            self.assertTrue(all("private" not in "/".join(p) for p in fs.calls))

    def test_c2_elf_and_exact_alias(self):
        fs = _FS()
        fs.add(q._C2, b"\x7fELFsynthetic", stat.S_IFREG | 0o755)
        self.assertEqual(self.collect(fs)["c2_state"], "protected_elf")
        for path in (q._C1, q._C2):
            for data, mode in ((b"not ELF", stat.S_IFREG | 0o755),
                               (b"\x7fELF", stat.S_IFREG | 0o440)):
                fs = _FS()
                fs.add(path, data, mode)
                self.refused(fs)
        for target in (b"usr/bin", b"/usr/bin"):
            fs = _FS()
            fs.nodes[("bin",)][1] = target
            self.assertTrue(self.collect(fs)["bin_alias_observed"])
        for target in (b"/tmp/bin", b"usr/bin/", b"../usr/bin"):
            fs = _FS()
            fs.nodes[("bin",)][1] = target
            self.refused(fs)

    def test_floor_and_snapshot_drift(self):
        for key, value in (("systemd", 253), ("systemd", True), ("kernel", (5, 13, 99)),
                           ("kernel", (6,)), ("extra", True)) + tuple(
                               (k, False) for k in ("cgroup2", "memory", "pids", "user_delegate", "hierarchy_protected")):
            fs = _FS()
            fs.floor[key] = value
            self.refused(fs)
        fs = _FS()
        original = fs.facts
        def drift():
            fs.nodes[q._C1][0].st_ino += 1
            return original()
        with patch.object(fs, "facts", side_effect=drift):
            self.refused(fs)

    def test_read_failure_truncation_and_cleanup_failure_are_silent(self):
        for port in ("read", "facts", "entries", "close"):
            fs = _FS()
            original = fs.close
            def close_error(fd):
                original(fd)
                raise OSError("SYNTHETIC_PRIVATE_MARKER")
            with patch.object(fs, port, side_effect=close_error if port == "close" else OSError("SYNTHETIC_PRIVATE_MARKER")):
                self.refused(fs)
        fs = _FS()
        with patch.object(fs, "read", return_value=b""):
            self.refused(fs)
        fs = _FS()
        fs.nodes[q._POLICY_PATHS[0]][1] = b"short"
        self.refused(fs)

    def test_cli_never_constructs_reader(self):
        out = io.StringIO()
        with patch.object(q, "_Reader", side_effect=AssertionError("host access")), contextlib.redirect_stdout(out):
            self.assertEqual(q._cli(), 2)
        self.assertEqual(json.loads(out.getvalue()), q._empty("root_reads_not_authorized"))

    def test_security_mutants_are_killed_in_memory(self):
        global q
        # Read this repository's source, never policy/host files; no subprocess.
        source = inspect.getsource(q)
        mutants = (
            ("info.st_uid == 0 and not info.st_mode & 0o022", "True"),
            ("info.st_nlink == 1", "True"),
            ("stat.S_ISREG(info.st_mode)", "True"),
            ("0 <= info.st_size <= cap", "True"),
            ("len(data) == info.st_size and len(data) <= cap", "True"),
            ("len(names) <= 32", "True"),
            ("total <= _TOTAL_CAP", "True"),
            ("data[:4] == b\"\\x7fELF\" and info.st_mode & 0o111 != 0", "True"),
            ("in (\"usr/bin\", \"/usr/bin\")", "is not None"),
            ("before == _snapshot(self.p.fstat(fd))", "True"),
            ("names == reader.entries()", "True"),
            ("_floor(ports.facts())", "ports.facts()"),
            ("qualified\": False", "qualified\": True"),
            ("_need(not failed)", "pass"),
            ("nofollow=True,", "nofollow=False,"),
            ("cloexec=True,", "cloexec=False,"),
            ("nonblock=True,", "nonblock=False,"),
        )
        original = q
        names = [name for name in dir(Qualification)
                 if name.startswith("test_") and name != "test_security_mutants_are_killed_in_memory"]
        try:
            for before, after in mutants:
                self.assertIn(before, source)
                mutant = ModuleType("qualification_mutant")
                exec(compile(source.replace(before, after), "<in-memory-mutant>", "exec"), mutant.__dict__)
                q = mutant
                result = unittest.TestResult()
                unittest.TestSuite(Qualification(name) for name in names).run(result)
                self.assertTrue(result.failures, f"surviving mutant: {before}")
                self.assertFalse(result.errors, f"non-assertion mutant failure: {before}")
        finally:
            q = original


if __name__ == "__main__":
    unittest.main()
