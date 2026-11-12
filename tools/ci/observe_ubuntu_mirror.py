"""Read-only, fail-closed predicate observation; never a mitigation entry point.

CLI roots and intent are fixed. Private dependencies exist for owned fixtures only.
Refusal facts decode only against the pinned helper source.
"""
import errno
import hashlib
import importlib.util
import json
import os
import re
from pathlib import Path
import signal
import stat
import sys
import time
from urllib.parse import urlsplit

# -I excludes the script directory. Load only the fixed, reviewed sibling module.
_spec = importlib.util.spec_from_file_location(
    "_mirror_pure", Path(__file__).with_name("ubuntu_mirror.py"))
mirror = importlib.util.module_from_spec(_spec)
with open(_spec.origin, "rb") as _stream:
    _source = _stream.read(65537)
_HELPER_PINNED = hashlib.sha256(b"\n".join(_source.split(b"\r\n"))).hexdigest() == (
    "a945b3d812f1fc47fa43ac696e3ec52026b657867b3f6c0d4b274a5d409b80ff")
# Execute exactly the bytes whose identity was checked, not a second path read.
exec(compile(_source, _spec.origin, "exec"), mirror.__dict__)
del _source

FUNCTIONS = ("replacement", "trust", "rewrite_list", "rewrite_sources", "rewrite",
             "rewrite_mirror_list")

MAX_ENTRIES = 128
MAX_READS = 2_200_000
MAX_REPORT = 16_384
SECONDS = 10
ERRNOS = {getattr(errno, name): name for name in (
    "EACCES", "EPERM", "ENOENT", "ELOOP", "ENOTDIR", "EISDIR", "EIO",
    "EMFILE", "ENFILE", "EINVAL", "ESTALE", "EINTR")}
STAGES = {"context", "parents", "enumeration", "read", "parser", "mirror-parser",
          "plan", "recheck", "done", "arguments", "internal"}
PREDICATES = {"linux", "root", "actions", "runner", "ci", "repository",
              "directory", "owner", "private-write", "regular", "single-link",
              "size", "read-size", "entries", "sources", "total-read", "deadline",
              "identity-bytes", "nonempty", "parser-unknown", "os-error",
              "unknown", "passed", "publication-unresolved", "report-bound",
              "no-arguments", "deb822", "raise-function", "raise-line",
              "has-types", "has-suites", "has-components", "has-signed-by",
              "signed-by-pinned", "continuation", "duplicate", "host-allowed",
              "host-ubuntu", "host-kernel", "scheme-allowed", "path-allowed",
              "netloc-host", "query-present", "fragment-present"}


def refusal_facts(error, report):
    """Only the innermost pinned frame and explicitly named locals are inspected."""
    trace = error.__traceback__
    while trace is not None and trace.tb_next is not None:
        trace = trace.tb_next
    code = trace.tb_frame.f_code if trace is not None else None
    known = (_HELPER_PINNED and code is not None and code.co_filename == _spec.origin
             and code.co_name in FUNCTIONS and 1 <= trace.tb_lineno <= 251)
    report.emit("raise-function", FUNCTIONS.index(code.co_name) + 1 if known else 0)
    report.emit("raise-line", trace.tb_lineno if known else 0)
    if not known:
        return
    local = trace.tb_frame.f_locals
    site = (code.co_name, trace.tb_lineno)
    if site == ("rewrite_sources", 97):
        fields = local.get("fields")
        if type(fields) is dict:
            for key in ("types", "suites", "components"):
                report.emit("has-" + key, bool(fields.get(key)))
            report.emit("has-signed-by", "signed-by" in fields)
            report.emit("signed-by-pinned", fields.get("signed-by", mirror.KEYRING) == mirror.KEYRING)
    elif site == ("rewrite_sources", 93):
        line, fields = local.get("line"), local.get("fields")
        if type(line) is str and type(fields) is dict:
            field = re.fullmatch(r"([A-Za-z][A-Za-z0-9-]*):[ \t]*(.*)", line)
            report.emit("continuation", line.startswith((" ", "\t")))
            report.emit("duplicate", bool(field and field[1].lower() in fields))
    elif site in (("replacement", 35), ("replacement", 39),
                  ("rewrite_mirror_list", 166), ("rewrite_mirror_list", 169)):
        uri = local.get("uri") if code.co_name == "replacement" else None
        tokens = local.get("tokens")
        if uri is None and type(tokens) is list and tokens and type(tokens[0]) is str:
            uri = tokens[0]
        parts = None
        if type(uri) is str:
            try:
                parts = urlsplit(uri)
                host = parts.hostname
            except ValueError:
                parts = None
        values = (False,) * 8 if parts is None else (
            host in mirror.HOSTS, bool(host and host.endswith(".ubuntu.com")),
            host == "mirrors.edge.kernel.org", parts.scheme in ("http", "https"),
            parts.path in ("/ubuntu", "/ubuntu/"), parts.netloc == host,
            bool(parts.query), bool(parts.fragment))
        for key, value in zip(("host-allowed", "host-ubuntu", "host-kernel", "scheme-allowed",
                               "path-allowed", "netloc-host", "query-present", "fragment-present"), values):
            report.emit(key, value)


class Stop(Exception):
    pass


class Deadline(Stop):
    pass


class Report:
    def __init__(self, clock=time.monotonic):
        self.clock = clock
        self.started = clock()
        self.rows = []
        self.stage = "context"
        self.category = 0
        self.index = 0
        self.total = 0

    def emit(self, predicate, value):
        # Closed vocabulary and bounded scalar schema, including injected facts.
        if (predicate not in PREDICATES or self.stage not in STAGES
                or type(value) not in (bool, int) or not 0 <= value <= MAX_READS
                or (predicate == "raise-line" and value > 251)
                or (predicate == "raise-function" and value > len(FUNCTIONS))
                or not 0 <= self.category <= 3 or not 0 <= self.index <= 32):
            raise Stop()
        row = [self.stage, self.category, self.index, predicate, value]
        candidate = self.rows + [row]
        if len(self.serialize(candidate).encode()) > MAX_REPORT - 256:
            raise Stop()
        self.rows = candidate

    def require(self, predicate, value):
        self.emit(predicate, bool(value))
        if not value:
            raise Stop()

    def tick(self):
        if self.clock() - self.started >= SECONDS:
            raise Deadline()

    @staticmethod
    def serialize(rows):
        events = [[sorted(STAGES).index(stage), category, index,
                   sorted(PREDICATES).index(predicate), value]
                  for stage, category, index, predicate, value in rows]
        return json.dumps({"version": 2, "events": events}, separators=(",", ":"))

    def encoded(self):
        return self.serialize(self.rows)


def policy(info, directory, report):
    report.require("directory" if directory else "regular",
                   stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode))
    report.require("owner", info.st_uid == 0)
    report.require("private-write", not info.st_mode & 0o022)
    if not directory:
        report.require("single-link", info.st_nlink == 1)


class DescriptorTree:
    """No path resolution after opening each validated parent descriptor."""
    def __init__(self, ops=os):
        self.ops = ops
        self.fds = []

    def close(self):
        while self.fds:
            self.ops.close(self.fds.pop())

    def parents(self, report):
        flags = self.ops.O_RDONLY | self.ops.O_DIRECTORY | self.ops.O_NOFOLLOW | self.ops.O_CLOEXEC
        for index, name in enumerate(("/", "etc", "apt", "sources.list.d")):
            report.index = index
            report.tick()
            fd = self.ops.open(name, flags, **({"dir_fd": self.fds[-1]} if self.fds else {}))
            self.fds.append(fd)
            policy(self.ops.fstat(fd), True, report)

    def entries(self, category):
        # scandir(fd) is lazy: unrelated entries count toward the same cap.
        return self.ops.scandir(self.fds[2 if category == 1 else 3])

    def read(self, category, name, limit, report):
        fd = self.ops.open(name, self.ops.O_RDONLY | self.ops.O_NOFOLLOW | self.ops.O_CLOEXEC | self.ops.O_NONBLOCK,
                     dir_fd=self.fds[3 if category == 2 else 2])
        try:
            info = self.ops.fstat(fd)
            policy(info, False, report)
            report.require("size", 0 <= info.st_size <= limit)
            chunks = []
            length = 0
            while True:
                report.tick()
                remaining = MAX_READS - report.total
                if remaining <= 0:
                    report.require("total-read", False)
                data = self.ops.read(fd, min(8192, limit + 1 - length, remaining))
                report.total += len(data)
                if report.total > MAX_READS:
                    report.require("total-read", False)
                length += len(data)
                if length > limit:
                    report.require("read-size", False)
                if not data:
                    break
                chunks.append(data)
            return b"".join(chunks), info
        finally:
            self.ops.close(fd)


def context():
    return (sys.platform == "linux", getattr(os, "geteuid", lambda: -1)() == 0,
            os.environ.get("GITHUB_ACTIONS") == "true",
            os.environ.get("RUNNER_OS") == "Linux", os.environ.get("CI") == "true",
            os.environ.get("GITHUB_REPOSITORY") == "Dytschgo/arxcovo")


def observe(tree, facts, report):
    """Private injected seam; no writer dependency and no caller-selected CLI root."""
    try:
        for key, value in zip(("linux", "root", "actions", "runner", "ci", "repository"), facts):
            report.require(key, value)
        report.require("passed", len(facts) == 6)
        report.stage = "parents"
        tree.parents(report)
        report.stage = "enumeration"
        paths = []
        count = 0
        legacy = False
        for category in (1, 2):
            report.category = category
            with tree.entries(category) as entries:
                for entry in entries:
                    report.tick()
                    count += 1
                    if count > MAX_ENTRIES:
                        report.require("entries", False)
                    name = entry.name
                    if category == 1:
                        legacy = legacy or name == "sources.list"
                    elif Path(name).suffix in (".list", ".sources"):
                        paths.append((2, name))
                        report.require("sources", len(paths) <= mirror.MAX_FILES)
        paths.sort()
        if legacy:
            paths.append((1, "sources.list"))
        report.emit("entries", count)
        report.require("sources", 1 <= len(paths) <= mirror.MAX_FILES)
        records = []
        for index, (category, name) in enumerate(paths):
            report.tick()
            report.stage, report.category, report.index = "read", category, index
            data, info = tree.read(category, name, mirror.MAX_BYTES, report)
            report.emit("size", len(data))
            records.append((category, name, data, info, index))
        indirect = any(mirror.MIRROR_LIST.encode() in row[2] for row in records)
        plan = []
        for category, name, before, info, index in records:
            report.tick()
            report.stage, report.category, report.index = "parser", category, index
            report.emit("deb822", Path(name).suffix == ".sources")
            after = mirror.rewrite(before, Path(name).suffix, indirect)
            report.emit("passed", True)
            if after != before:
                plan.append((category, name, before, info, index))
        if indirect:
            report.stage, report.category, report.index = "read", 3, 0
            before, info = tree.read(3, "apt-mirrors.txt", mirror.MAX_MIRROR_LIST, report)
            report.emit("size", len(before))
            report.stage = "mirror-parser"
            mirror.rewrite_mirror_list(before)
            report.emit("passed", True)
            plan.append((3, "apt-mirrors.txt", before, info, 0))
        report.stage = "plan"
        report.require("nonempty", bool(plan))
        report.emit("sources", len(plan))
        for category, name, before, old, index in plan:
            report.tick()
            report.stage, report.category, report.index = "recheck", category, index
            limit = mirror.MAX_MIRROR_LIST if category == 3 else mirror.MAX_BYTES
            current, info = tree.read(category, name, limit, report)
            report.require("identity-bytes", current == before and
                           (info.st_dev, info.st_ino) == (old.st_dev, old.st_ino))
        report.tick()
        report.stage = "done"
        report.emit("publication-unresolved", True)
    except mirror.Refusal as error:
        refusal_facts(error, report)
        report.emit("parser-unknown", False)
    except OSError as error:
        # Encode errno as a small allowlist index. Never stringify exceptions.
        report.emit("os-error", list(ERRNOS).index(error.errno) + 1 if error.errno in ERRNOS else 0)
    except Deadline:
        report.emit("deadline", False)
    except Stop:
        # Fixed fallback also handles an exhausted report budget.
        if not report.rows or report.rows[-1][-1] is not False:
            report.rows = [["internal", 0, 0, "report-bound", False]]
    return report


def main():
    report = Report()
    if len(sys.argv) != 1:
        report.stage = "arguments"
        report.emit("no-arguments", False)
        print(report.encoded())
        return 2
    tree = DescriptorTree()
    try:
        # CLI only: a hard wall-clock stop also bounds a blocking filesystem call.
        # Unit tests replace the timer, context, tree and output dependencies.
        if sys.platform == "linux":
            def expired(_signum, _frame):
                raise Deadline()
            signal.signal(signal.SIGALRM, expired)
            signal.setitimer(signal.ITIMER_REAL, SECONDS)
        observe(tree, context(), report)
    except BaseException:
        report.rows = [["internal", 0, 0, "unknown", False]]
    finally:
        # Catch an alarm delivered during cancellation or descriptor cleanup too.
        try:
            try:
                if sys.platform == "linux":
                    signal.setitimer(signal.ITIMER_REAL, 0)
            finally:
                tree.close()
        except OSError:
            report.rows = [["internal", 0, 0, "os-error", 0]]
        except BaseException:
            report.rows = [["internal", 0, 0, "unknown", False]]
    print(report.encoded())
    return 1  # Observation never reports mitigation success.


if __name__ == "__main__":
    raise SystemExit(main())
