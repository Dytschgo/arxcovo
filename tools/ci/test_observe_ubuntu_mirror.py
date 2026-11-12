"""Owned fixtures and injected main only; no production context, /etc or sudo."""
import ast
import contextlib
import errno
import json
import os
from pathlib import Path
import stat
import tempfile
from types import SimpleNamespace as NS
import unittest
from unittest.mock import Mock, patch

import observe_ubuntu_mirror as diag

SOURCE = b"deb https://archive.ubuntu.com/ubuntu resolute main\n"
INDIRECT = b"deb mirror+file:/etc/apt/apt-mirrors.txt resolute main\n"
MIRRORS = b"https://archive.ubuntu.com/ubuntu priority:1\n"


def facts(directory=False, **changes):
    values = dict(st_mode=(stat.S_IFDIR | 0o755) if directory else (stat.S_IFREG | 0o644),
                  st_uid=0, st_nlink=1, st_size=0, st_dev=1, st_ino=2)
    values.update(changes)
    return NS(**values)


class FixtureTree:
    def __init__(self, root):
        self.root = root
        self.parent = facts(True)
        self.file = facts()
        self.calls = []
        self.override = None

    def parents(self, report):
        diag.policy(self.parent, True, report)

    @contextlib.contextmanager
    def entries(self, category):
        root = self.root if category == 1 else self.root / "sources.list.d"
        yield (NS(name=p.name) for p in root.iterdir())

    def read(self, category, name, limit, report):
        self.calls.append((category, name))
        if self.override:
            result = self.override(category, name, limit, report)
            if result is not None:
                return result
        root = self.root / "sources.list.d" if category == 2 else self.root
        diag.policy(self.file, False, report)
        data = (root / name).read_bytes()
        report.require("size", len(data) <= limit)
        report.total += len(data)
        report.require("total-read", report.total <= diag.MAX_READS)
        return data, self.file


class ObservationTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        (self.root / "sources.list.d").mkdir()
        self.source = self.root / "sources.list.d" / "ubuntu.list"
        self.source.write_bytes(SOURCE)
        self.tree = FixtureTree(self.root)

    def run_observation(self, facts_=None, report=None):
        return diag.observe(self.tree, (True,) * 6 if facts_ is None else facts_,
                            report or diag.Report())

    def assert_tag(self, report, tag, value=False):
        self.assertTrue(any(row[3:] == [tag, value] for row in report.rows), report.rows)

    def test_success_is_publication_unresolved_and_tree_unchanged_no_writers(self):
        def snapshot():
            return sorted((str(p.relative_to(self.root)), p.read_bytes() if p.is_file() else None,
                           p.stat().st_mtime_ns, p.stat().st_mode, p.stat().st_nlink)
                          for p in self.root.rglob("*"))
        before = snapshot()
        # Module-local pure-helper entry spies; no process-wide OS/socket patch.
        with patch.object(diag.mirror, "apply") as apply, patch.object(diag.mirror, "_apply") as write:
            report = self.run_observation()
        apply.assert_not_called()
        write.assert_not_called()
        self.assertEqual(before, snapshot())
        self.assertEqual(len(self.tree.calls), 2)
        self.assert_tag(report, "publication-unresolved", True)

    def test_each_context_comparison_stops_before_reads(self):
        for index, key in enumerate(("linux", "root", "actions", "runner", "ci", "repository")):
            with self.subTest(key=key):
                values = [True] * 6
                values[index] = False
                self.assert_tag(self.run_observation(values), key)
                self.assertEqual(self.tree.calls, [])

    def test_parent_and_file_policies(self):
        for directory, changes, tag in (
            (True, dict(st_mode=stat.S_IFLNK | 0o755), "directory"),
            (True, dict(st_uid=42), "owner"),
            (True, dict(st_mode=stat.S_IFDIR | 0o777), "private-write"),
            (False, dict(st_mode=stat.S_IFIFO | 0o644), "regular"),
            (False, dict(st_uid=42), "owner"),
            (False, dict(st_mode=stat.S_IFREG | 0o666), "private-write"),
            (False, dict(st_nlink=2), "single-link")):
            with self.subTest(directory=directory, tag=tag):
                self.tree.parent, self.tree.file = facts(True), facts()
                setattr(self.tree, "parent" if directory else "file", facts(directory, **changes))
                self.assert_tag(self.run_observation(), tag)

    def test_empty_excessive_sources_and_directory_entry_bound(self):
        self.source.unlink()
        self.assert_tag(self.run_observation(), "sources")
        for index in range(33):
            (self.source.parent / f"{index}.list").write_bytes(SOURCE)
        self.assert_tag(self.run_observation(), "sources")
        for p in self.source.parent.iterdir():
            p.unlink()
        for index in range(128):
            (self.source.parent / f"{index}.ignored").touch()
        self.assert_tag(self.run_observation(), "entries")  # parent directory entry also counts

    def test_parser_refusal_inventory_and_redaction(self):
        cases = [b"\xff", b"\0private", b"private-owner-syntax", SOURCE.replace(b"https:", b"ftp:"),
                 SOURCE.replace(b"archive.ubuntu.com", b"secret.ubuntu.com"),
                 SOURCE.replace(b"/ubuntu", b"/ubuntu?private"),
                 SOURCE.replace(b"deb ", b"deb [trusted=yes] "),
                 SOURCE.replace(b"deb ", b"deb [signed-by=/private-owner] "),
                 SOURCE.replace(b" resolute main", b""),
                 b"Types: deb\nURIs: https://archive.ubuntu.com/ubuntu https://private.invalid\nSuites: x\nComponents: main\n",
                 b"Types: deb\nURIs: https://archive.ubuntu.com/ubuntu\n Suites: x\n",
                 b"Types: deb\nURIs: https://archive.ubuntu.com/ubuntu\nSuites: x\nSuites: y\nComponents: main\n"]
        sites = ((5, 120), (5, 110), (3, 63), (1, 39), (1, 35), (1, 39),
                 (2, 47), (2, 52), (3, 68), (4, 86), (4, 93), (4, 93))
        for data, (function, line) in zip(cases, sites):
            with self.subTest(data=data):
                self.source.unlink(missing_ok=True)
                self.source = self.source.with_suffix(".sources" if data.startswith(b"Types:") else ".list")
                self.source.write_bytes(data)
                report = self.run_observation()
                self.assert_tag(report, "parser-unknown")
                self.assert_tag(report, "raise-function", function)
                self.assert_tag(report, "raise-line", line)
                self.assertNotIn("private-owner", report.encoded())
                self.assertNotIn("ubuntu.com", report.encoded())
                self.assertNotIn(self.source.name, report.encoded())

    def test_mirror_refusals_and_bounds(self):
        self.source.write_bytes(INDIRECT)
        for data in (b"", b"\xff", b"\0", b"https://private.invalid/ubuntu\n",
                     MIRRORS * 17, b"x" * 4097,
                     MIRRORS.replace(b"priority:1", b"priority:1234567"),
                     MIRRORS.replace(b"priority:1", b"priority:1 extra")):
            with self.subTest(length=len(data)):
                (self.root / "apt-mirrors.txt").write_bytes(data)
                report = self.run_observation()
                self.assertFalse(any(row[3] == "publication-unresolved" for row in report.rows))
                self.assertNotIn("private.invalid", report.encoded())
        (self.root / "apt-mirrors.txt").write_bytes(MIRRORS)
        self.assert_tag(self.run_observation(), "publication-unresolved", True)

    def test_source_size_nonempty_plan_and_stale_bytes_identity(self):
        self.source.write_bytes(b"x" * 65537)
        self.assert_tag(self.run_observation(), "size")
        self.source.write_bytes(b"deb https://foreign.invalid/pkg stable main\n")
        self.assert_tag(self.run_observation(), "nonempty")
        self.source.write_bytes(SOURCE)
        for stale in ("bytes", "identity"):
            self.tree.calls.clear()
            def read(category, name, limit, report):
                if len(self.tree.calls) == 2:
                    return (b"changed" if stale == "bytes" else SOURCE,
                            facts(st_ino=3) if stale == "identity" else facts())
            self.tree.override = read
            self.assert_tag(self.run_observation(), "identity-bytes")

    def test_errno_allowlist_and_unknown_never_exception_text(self):
        for number in [*diag.ERRNOS, 9999]:
            def read(*_):
                raise OSError(number, "private-owner-value")
            self.tree.override = read
            result = self.run_observation()
            self.assertEqual(result.rows[-1][3], "os-error")
            self.assertNotIn("private-owner-value", result.encoded())
            self.assertEqual(result.rows[-1][4], list(diag.ERRNOS).index(number) + 1 if number in diag.ERRNOS else 0)

    def test_time_total_read_and_report_bounds(self):
        clock = Mock(side_effect=[0, 11])
        self.assert_tag(self.run_observation(report=diag.Report(clock)), "deadline")
        report = diag.Report()
        report.total = diag.MAX_READS
        self.assert_tag(self.run_observation(report=report), "total-read")
        report = diag.Report()
        with self.assertRaises(diag.Stop):
            for _ in range(2000):
                report.emit("passed", True)
        self.assertLessEqual(len(report.encoded().encode()), diag.MAX_REPORT)
        self.assertLessEqual(len(self.run_observation(report=report).encoded().encode()), diag.MAX_REPORT)

    def test_closed_report_schema(self):
        for key, value in (("private", True), ("passed", "secret"), ("size", -1), ("size", 2**40),
                           ("raise-line", 252), ("raise-function", 7)):
            with self.assertRaises(diag.Stop):
                diag.Report().emit(key, value)

    def test_maximum_source_count_and_size_boundary_are_not_silently_skipped(self):
        for index in range(31):
            (self.source.parent / f"{index}.list").write_bytes(SOURCE)
        result = self.run_observation()
        self.assert_tag(result, "publication-unresolved", True)
        self.assertEqual(len(self.tree.calls), 64)
        self.assertLessEqual(len(result.encoded()), diag.MAX_REPORT)
        for path in self.source.parent.iterdir():
            if path != self.source:
                path.unlink()
        self.source.write_bytes(SOURCE + b"#" + b"x" * (65520 - len(SOURCE) - 1))
        self.assert_tag(self.run_observation(), "publication-unresolved", True)
        # A maximum-size input whose replacement grows beyond 64 KiB stays refused.
        self.source.write_bytes(SOURCE + b"#" + b"x" * (65536 - len(SOURCE) - 1))
        self.assert_tag(self.run_observation(), "parser-unknown")

    def test_missing_mirror_is_reported_without_fallback_read(self):
        self.source.write_bytes(INDIRECT)
        result = self.run_observation()
        self.assertEqual(result.rows[-1][3], "os-error")
        self.assertEqual(self.tree.calls[-1], (3, "apt-mirrors.txt"))
        self.assertEqual(len(self.tree.calls), 2)

    def test_errno_at_parents_enumeration_and_recheck_is_stage_specific(self):
        for site in ("parents", "entries", "read"):
            with self.subTest(site=site):
                with patch.object(self.tree, site, side_effect=OSError(errno.EACCES, "sentinel-secret")):
                    report = self.run_observation()
                self.assertEqual(report.rows[-1][0], {"parents": "parents", "entries": "enumeration", "read": "read"}[site])
                self.assertNotIn("sentinel-secret", report.encoded())
        self.tree.calls.clear()
        def stale_error(*_):
            if len(self.tree.calls) == 2:
                raise OSError(errno.ESTALE, "sentinel-secret")
        self.tree.override = stale_error
        self.assertEqual(self.run_observation().rows[-1][0], "recheck")

    def test_source_fences_are_supplementary_not_semantic_proof(self):
        source = Path(diag.__file__).read_text()
        tree = ast.parse(source)
        forbidden = {"apply", "_apply", "write", "write_bytes", "write_text", "mkstemp", "chmod",
                     "fchmod", "fsync", "replace", "unlink", "remove", "mkdir", "system", "Popen",
                     "run", "connect", "urlopen", "request", "socket"}
        for node in ast.walk(tree):
            if isinstance(node, ast.Call):
                name = node.func.attr if isinstance(node.func, ast.Attribute) else getattr(node.func, "id", "")
                self.assertNotIn(name, forbidden)
        workflow = (Path(__file__).parents[2] / ".github/workflows/ci.yml").read_text()
        self.assertIn("if: failure() && runner.os == 'Linux' && steps.ubuntu_mirror.outcome == 'failure'", workflow)
        self.assertNotIn("continue-on-error", workflow)
        observation = workflow.index("      - name: Observe Ubuntu mirror refusal")
        tests = workflow.index("      - name: Test read-only Ubuntu mirror observer")
        package = workflow.index("      - run: npx playwright install")
        self.assertLess(observation, tests)
        self.assertLess(tests, package)
        condition = "if: failure() && runner.os == 'Linux' && steps.ubuntu_mirror.outcome == 'failure'"
        self.assertEqual(workflow[observation:package].count(condition), 2)
        self.assertIn("run: python3 -B -m unittest discover -s tools/ci -p 'test_observe_ubuntu_mirror.py'",
                      workflow[tests:package])
        self.assertNotIn("sudo", workflow[tests:package])
        self.assertNotIn("||", workflow[tests:package])


class ClassificationTests(unittest.TestCase):
    URI_KEYS = ("host-allowed", "host-ubuntu", "host-kernel", "scheme-allowed",
                "path-allowed", "netloc-host", "query-present", "fragment-present")

    def test_version_two_numeric_vocabulary_is_pinned(self):
        self.assertEqual(sorted(diag.PREDICATES), (
            "actions ci continuation deadline deb822 directory duplicate entries fragment-present "
            "has-components has-signed-by has-suites has-types host-allowed host-kernel host-ubuntu "
            "identity-bytes linux netloc-host no-arguments nonempty os-error owner parser-unknown "
            "passed path-allowed private-write publication-unresolved query-present raise-function "
            "raise-line read-size regular report-bound repository root runner scheme-allowed "
            "signed-by-pinned single-link size sources total-read unknown").split())
        self.assertEqual(sorted(diag.STAGES), ("arguments context done enumeration internal mirror-parser "
                                              "parents parser plan read recheck").split())
        self.assertEqual(json.loads(diag.Report().encoded())["version"], 2)

    def classify(self, call, function, line, extra=None):
        report = diag.Report()
        report.stage = "parser"
        try:
            call()
        except diag.mirror.Refusal as error:
            diag.refusal_facts(error, report)
        else:
            self.fail("probe must remain refused")
        expected = {"raise-function": diag.FUNCTIONS.index(function) + 1, "raise-line": line}
        expected.update(extra or {})
        self.assertEqual({row[3]: row[4] for row in report.rows}, expected)
        encoded = report.encoded()
        for secret in ("ubuntu.com", "kernel.org", "private.invalid", "/ubuntu", "keyring",
                       "private-owner", "/etc", "Signed-By", "Traceback", "fixture.list"):
            self.assertNotIn(secret, encoded)
        self.assertEqual(json.loads(encoded)["version"], 2)

    def test_uri_compound_sites_exact_booleans(self):
        for uri, line, values in (
            ("https://ports.ubuntu.com/ubuntu", 35, (0, 1, 0, 1, 1, 1, 0, 0)),
            ("https://eu-west-1.ec2.archive.ubuntu.com/ubuntu", 35, (0, 1, 0, 1, 1, 1, 0, 0)),
            ("ftp://archive.ubuntu.com/ubuntu", 39, (1, 1, 0, 0, 1, 1, 0, 0)),
            ("https://archive.ubuntu.com/private-owner", 39, (1, 1, 0, 1, 0, 1, 0, 0)),
            ("https://archive.ubuntu.com:443/ubuntu", 39, (1, 1, 0, 1, 1, 0, 0, 0)),
            ("https://archive.ubuntu.com/ubuntu?q", 39, (1, 1, 0, 1, 1, 1, 1, 0)),
            ("https://archive.ubuntu.com/ubuntu#f", 39, (1, 1, 0, 1, 1, 1, 0, 1)),
            ("https://archive.ubuntu.com/ubuntu?q#f", 39, (1, 1, 0, 1, 1, 1, 1, 1))):
            with self.subTest(line=line, values=values):
                self.classify(lambda: diag.mirror.rewrite(("deb " + uri + " x main\n").encode(), ".list"),
                              "replacement", line, dict(zip(self.URI_KEYS, map(bool, values))))
        for data, line, values in (
            (b"https://private.invalid/ubuntu\n", 169, (0, 0, 0, 1, 1, 1, 0, 0)),
            (b"https://mirrors.edge.kernel.org/ubuntu priority:1\n", 169, (0, 0, 1, 1, 1, 1, 0, 0)),
            (b"https://archive.ubuntu.com/ubuntu priority:1234567\n", 166, (1, 1, 0, 1, 1, 1, 0, 0)),
            (b"https://archive.ubuntu.com/ubuntu priority:1 extra\n", 166, (1, 1, 0, 1, 1, 1, 0, 0))):
            self.classify(lambda: diag.mirror.rewrite_mirror_list(data), "rewrite_mirror_list", line,
                          dict(zip(self.URI_KEYS, map(bool, values))))

    def test_deb822_compound_sites(self):
        base = "Types: deb\nURIs: https://archive.ubuntu.com/ubuntu\nSuites: x\nComponents: main\n"
        keys = ("has-types", "has-suites", "has-components", "has-signed-by", "signed-by-pinned")
        for text, values in (
            (base.replace("Types: deb\n", ""), (0, 1, 1, 0, 1)),
            (base.replace("Suites: x\n", ""), (1, 0, 1, 0, 1)),
            (base.replace("Components: main\n", ""), (1, 1, 0, 0, 1)),
            (base + "Signed-By: /private-owner/keyring\n", (1, 1, 1, 1, 0)),
            (base.replace("Components: main\n", "") + "Signed-By: " + diag.mirror.KEYRING + "\n",
             (1, 1, 0, 1, 1))):
            self.classify(lambda: diag.mirror.rewrite(text.encode(), ".sources"), "rewrite_sources", 97,
                          dict(zip(keys, map(bool, values))))
        for text, continuation, duplicate in ((base + " Signed-By: private-owner\n", True, False),
                                               (base + "Suites: y\n", False, True),
                                               (base + "private-owner\n", False, False)):
            self.classify(lambda: diag.mirror.rewrite(text.encode(), ".sources"), "rewrite_sources", 93,
                          {"continuation": continuation, "duplicate": duplicate})

    def test_simple_sites_and_prior_probe_inventory(self):
        cases = ((b"\xff", ".list", "rewrite", 120),
                 (b"\0private-owner", ".list", "rewrite", 110),
                 (b"private-owner-syntax", ".list", "rewrite_list", 63),
                 (SOURCE.replace(b"deb ", b"deb [trusted=yes] "), ".list", "trust", 47),
                 (SOURCE.replace(b"deb ", b"deb [signed-by=/private-owner] "), ".list", "trust", 52),
                 (SOURCE.replace(b" resolute main", b""), ".list", "rewrite_list", 68),
                 (b"Types: deb\nURIs: https://archive.ubuntu.com/ubuntu https://private.invalid\nSuites: x\nComponents: main\n",
                  ".sources", "rewrite_sources", 86),
                 (SOURCE + b"#" * (65536 - len(SOURCE)), ".list", "rewrite", 123))
        for data, suffix, function, line in cases:
            self.classify(lambda: diag.mirror.rewrite(data, suffix), function, line)
        for data, line in ((b"", 177), (b"\xff", 156), (b"\0", 152),
                           (MIRRORS * 17, 177), (b"x" * 4097, 152)):
            self.classify(lambda: diag.mirror.rewrite_mirror_list(data), "rewrite_mirror_list", line)

    def test_unknown_origin_function_line_or_identity_is_zero_only(self):
        self.assertTrue(diag._HELPER_PINNED)
        self.assertEqual(diag.FUNCTIONS, ("replacement", "trust", "rewrite_list", "rewrite_sources",
                                          "rewrite", "rewrite_mirror_list"))
        for origin, name, line, pinned in (("private-owner", "replacement", 35, True),
                                           (diag._spec.origin, "unknown", 35, True),
                                           (diag._spec.origin, "replacement", 252, True),
                                           (diag._spec.origin, "replacement", 35, False)):
            trace = NS(tb_next=None, tb_lineno=line,
                       tb_frame=NS(f_code=NS(co_filename=origin, co_name=name), f_locals={}))
            report = diag.Report()
            with patch.object(diag, "_HELPER_PINNED", pinned):
                diag.refusal_facts(NS(__traceback__=trace), report)
            self.assertEqual([r[3:] for r in report.rows], [["raise-function", 0], ["raise-line", 0]])


class MainTests(unittest.TestCase):
    def invoke(self, tree, observer=None, argv=None, linux=False, timer=None):
        output, errors = Mock(), Mock()
        fake_signal = NS(SIGALRM=14, ITIMER_REAL=0, signal=Mock(), setitimer=timer or Mock())
        with patch.object(diag, "sys", NS(argv=["observer"] if argv is None else argv,
                                          platform="linux" if linux else "test")), \
             patch.object(diag, "signal", fake_signal), \
             patch.object(diag, "context", return_value=(True,) * 6) as context, \
             patch.object(diag, "DescriptorTree", return_value=tree) as constructor, \
             patch.object(diag, "observe", observer or Mock()), \
             patch.object(diag, "print", output, create=True), \
             contextlib.redirect_stderr(errors):
            status = diag.main()
        output.assert_called_once()
        errors.write.assert_not_called()
        return status, json.loads(output.call_args.args[0]), context, constructor, fake_signal

    def test_complete_plan_still_returns_one_and_closes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "sources.list.d").mkdir()
            (root / "sources.list.d" / "fixture.list").write_bytes(SOURCE)
            tree = FixtureTree(root)
            tree.close = Mock()
            status, payload, _, _, _ = self.invoke(tree, diag.observe)
            self.assertEqual(status, 1)
            last = payload["events"][-1]
            self.assertEqual(last[0], sorted(diag.STAGES).index("done"))
            self.assertEqual(last[3:], [sorted(diag.PREDICATES).index("publication-unresolved"), True])
            tree.close.assert_called_once()

    def test_refusal_internal_and_close_errors_stay_red(self):
        for observer, close, predicate in ((Mock(), None, None),
                                           (Mock(side_effect=RuntimeError("private-owner")), None, "unknown"),
                                           (Mock(), OSError("private-owner"), "os-error"),
                                           (Mock(), diag.Deadline(), "unknown")):
            tree = NS(close=Mock(side_effect=close))
            status, payload, _, _, _ = self.invoke(tree, observer)
            self.assertEqual(status, 1)
            tree.close.assert_called_once()
            if predicate:
                self.assertEqual(payload["events"], [[4, 0, 0, sorted(diag.PREDICATES).index(predicate),
                                                     0 if predicate == "os-error" else False]])
            self.assertNotIn("private-owner", json.dumps(payload))

    def test_arguments_return_two_without_context_or_tree(self):
        tree = NS(close=Mock())
        status, payload, context, constructor, _ = self.invoke(tree, argv=["observer", "private-owner"])
        self.assertEqual(status, 2)
        self.assertEqual(payload["events"], [[0, 0, 0, sorted(diag.PREDICATES).index("no-arguments"), False]])
        context.assert_not_called()
        constructor.assert_not_called()
        tree.close.assert_not_called()

    def test_injected_alarm_cancel_precedes_close_and_cleanup_alarm_is_caught(self):
        order = []
        for cancel_alarm in (False, True):
            order.clear()
            def timer(_kind, seconds):
                order.append(seconds)
                if seconds == 0 and cancel_alarm:
                    raise diag.Deadline()
            tree = NS(close=Mock(side_effect=lambda: order.append("close")))
            status, payload, _, _, signal = self.invoke(tree, linux=True, timer=Mock(side_effect=timer))
            self.assertEqual(status, 1)
            self.assertEqual(order, [10, 0, "close"])
            handler = signal.signal.call_args.args[1]
            with self.assertRaises(diag.Deadline):
                handler(None, None)
            if cancel_alarm:
                self.assertEqual(payload["events"], [[4, 0, 0, sorted(diag.PREDICATES).index("unknown"), False]])


class DescriptorTests(unittest.TestCase):
    """Inject a local OS facade; never patch the process-wide os module."""
    def make_ops(self, info=None, data=b"ok"):
        return NS(O_RDONLY=0, O_DIRECTORY=65536, O_NOFOLLOW=131072, O_CLOEXEC=524288, O_NONBLOCK=2048,
                  open=Mock(return_value=7), close=Mock(), fstat=Mock(return_value=info or facts()),
                  read=Mock(side_effect=[data, b""]), scandir=Mock())

    def test_descriptor_relative_read_and_no_follow_all_parents(self):
        ops = self.make_ops()
        tree = diag.DescriptorTree(ops)
        tree.fds = [1, 2, 3, 4]
        data, _ = tree.read(2, "fixture.list", 65536, diag.Report())
        self.assertEqual(data, b"ok")
        args, kw = ops.open.call_args
        self.assertEqual(kw, {"dir_fd": 4})
        self.assertTrue(args[1] & ops.O_NOFOLLOW)
        self.assertFalse(args[1] & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC))
        ops.close.assert_called_once_with(7)
        ops.fstat.return_value = facts(True)
        tree.fds = []
        tree.parents(diag.Report())
        self.assertEqual([call.args[0] for call in ops.open.call_args_list[1:]],
                         ["/", "etc", "apt", "sources.list.d"])

    def test_entries_selects_correct_directory(self):
        ops = self.make_ops()
        tree = diag.DescriptorTree(ops)
        tree.fds = [10, 11, 12, 13]
        for category, fd in ((1, 12), (2, 13)):
            self.assertIs(tree.entries(category), ops.scandir.return_value)
            ops.scandir.assert_called_with(fd)

    def test_file_policy_size_actual_read_and_cleanup(self):
        for info, data, tag in ((facts(st_mode=stat.S_IFLNK | 0o644), b"", "regular"),
                                (facts(st_size=65537), b"", "size"),
                                (facts(), b"x" * 65537, "read-size")):
            ops = self.make_ops(info, data)
            tree = diag.DescriptorTree(ops)
            tree.fds = [1, 2, 3, 4]
            report = diag.Report()
            with self.assertRaises(diag.Stop):
                tree.read(2, "fixture.list", 65536, report)
            self.assertEqual(report.rows[-1][3], tag)
            ops.close.assert_called_once()

    def test_unsafe_parent_and_symlink_errno_do_not_reopen_or_fallback(self):
        ops = self.make_ops(facts(True, st_mode=stat.S_IFDIR | 0o777))
        tree = diag.DescriptorTree(ops)
        with self.assertRaises(diag.Stop):
            tree.parents(diag.Report())
        self.assertEqual(ops.open.call_count, 1)
        ops.read.assert_not_called()
        tree.close()
        ops.close.assert_called_once_with(7)
        ops = self.make_ops(facts(True))
        ops.open.side_effect = [7, OSError(errno.ELOOP, "sentinel-secret")]
        tree = diag.DescriptorTree(ops)
        with self.assertRaises(OSError):
            tree.parents(diag.Report())
        self.assertEqual(ops.open.call_count, 2)
        ops.read.assert_not_called()
        tree.close()

    def test_total_reads_are_capped_before_next_syscall(self):
        ops = self.make_ops(data=b"ok")
        tree = diag.DescriptorTree(ops)
        tree.fds = [1, 2, 3, 4]
        report = diag.Report()
        report.total = diag.MAX_READS
        with self.assertRaises(diag.Stop):
            tree.read(2, "fixture.list", 65536, report)
        ops.read.assert_not_called()
        self.assertEqual(report.rows[-1][3], "total-read")
        ops.close.assert_called_once()

    @unittest.skipUnless(os.name == "posix", "Owned Unix symlink/dir_fd coverage pending hosted Linux")
    def test_owned_symlink_and_unsafe_parent_stop_without_child_read(self):
        if os.geteuid() == 0:
            self.skipTest("ordinary non-root runner required")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "target").write_bytes(b"fixture")
            (root / "link").symlink_to(root / "target")
            parent = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
            try:
                tree = diag.DescriptorTree()
                tree.fds = [parent] * 4
                with self.assertRaises(OSError) as error:
                    tree.read(2, "link", 65536, diag.Report())
                self.assertEqual(error.exception.errno, errno.ELOOP)
            finally:
                os.close(parent)

    @unittest.skipUnless(os.name == "posix", "native Unix owned descriptor coverage")
    def test_native_owned_entries_regular_fifo_hardlink_directory(self):
        if os.geteuid() == 0:
            self.skipTest("ordinary non-root runner required")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "sources.list.d").mkdir()
            (root / "regular").write_bytes(b"fixture")
            os.mkfifo(root / "fifo")
            os.link(root / "regular", root / "hardlink")
            (root / "child").mkdir()
            apt = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
            sources = os.open(root / "sources.list.d", os.O_RDONLY | os.O_DIRECTORY)
            tree = diag.DescriptorTree()
            tree.fds = [apt, apt, apt, sources]
            try:
                with tree.entries(1) as entries:
                    self.assertEqual({e.name for e in entries},
                                     {"sources.list.d", "regular", "hardlink", "fifo", "child"})
                with tree.entries(2) as entries:
                    self.assertEqual(list(entries), [])
                report = diag.Report()
                with self.assertRaises(diag.Stop):
                    tree.read(1, "regular", 65536, report)
                self.assertEqual(report.rows[-1][3:], ["owner", False])
                # Native owned fixtures cannot satisfy root policy. Skip only that
                # comparison through a local wrapper; retain native file/link facts.
                original = diag.policy
                def owned_policy(info, directory, report):
                    self.assertNotEqual(info.st_uid, 0)
                    original(NS(st_mode=info.st_mode, st_uid=0, st_nlink=info.st_nlink), directory, report)
                with patch.object(diag, "policy", owned_policy):
                    for name, predicate in (("hardlink", "single-link"), ("fifo", "regular"), ("child", "regular")):
                        report = diag.Report()
                        started = diag.time.monotonic()
                        with self.assertRaises(diag.Stop):
                            tree.read(1, name, 65536, report)
                        self.assertLess(diag.time.monotonic() - started, 1)
                        self.assertEqual(report.rows[-1][3:], [predicate, False])
                    (root / "hardlink").unlink()
                    data, _ = tree.read(1, "regular", 65536, diag.Report())
                    self.assertEqual(data, b"fixture")
            finally:
                os.close(sources)
                os.close(apt)


if __name__ == "__main__":
    unittest.main()
