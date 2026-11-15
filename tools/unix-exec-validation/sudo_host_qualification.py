"""SOURCE ONLY: injected reads; no host adapter, policy pins, or authority.

Ports model openat/fstat/read/close, never path-based convenience reads. A future
Linux adapter needs separate source review and a narrow root-read-only grant.
Only root-writable ancestry makes the held-descriptor snapshot meaningful;
concurrent privileged writers are outside this observation's trust boundary.
"""

import hashlib
import itertools
import json
import stat


_POLICY_CAP = 16 * 1024
_TOTAL_CAP = 128 * 1024
_ELF_CAP = 16 * 1024 * 1024
_POLICY_PATHS = (("etc", "sudo.conf"), ("etc", "nsswitch.conf"),
                 ("etc", "sudoers"))
_C1 = ("usr", "bin", "sudo")
_C2 = ("usr", "local", "bin", "sudo")
# Intentionally empty. Hashes alone cannot be added as policy proof. Any future
# candidate must be exact source literals with independent semantic review,
# including names/complete includes/plugins/NSS/defaults and both account cases.
_REVIEWED_POLICY = ()
_LABELS = ("sudo_conf", "nsswitch", "sudoers", "include_set", "c1", "c2")


class _Refused(Exception):
    pass


def _need(ok):
    if not ok:
        raise _Refused()


def _snapshot(info):
    return tuple(getattr(info, key) for key in (
        "st_dev", "st_ino", "st_mode", "st_uid", "st_nlink", "st_size",
        "st_mtime_ns", "st_ctime_ns"))


def _protected(info, directory=False):
    _need(info.st_uid == 0 and not info.st_mode & 0o022)
    _need(stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode))
    if not directory:
        _need(info.st_nlink == 1)


def _digest(data):
    return {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}


def _empty(status):
    return {"version": 1, "status": status, "qualified": False,
            "policy_reviewed": False, "floor_observed": False,
            "bin_alias_observed": False, "c2_state": "unknown",
            "include_count": 0,
            "records": {label: None for label in _LABELS}}


class _Reader:
    """Private syscall-shaped injected port; never defaults to an OS adapter.

    open_root/openat must implement read-only O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK;
    directories additionally O_DIRECTORY. openat takes one component only.
    statat/readlinkat never follow links. entries must bound getdents work as
    well as returned entries. facts() is a fixed non-executing floor port, NOT
    a caller-supplied host attestation. No process, network, write or proc port.
    """

    def __init__(self, ports):
        self.p = ports
        self.held = []
        self.dirs = {}

    def root(self):
        fd = self.p.open_root(readonly=True, nofollow=True, cloexec=True,
                              nonblock=True, directory=True)
        self.keep(fd, True)
        self.dirs[()] = fd

    def keep(self, fd, directory):
        # Keep even rejected descriptors so finally closes them.
        self.held.append((fd, None))
        info = self.p.fstat(fd)
        _protected(info, directory)
        self.held[-1] = (fd, _snapshot(info))
        return info

    def directory(self, parts):
        if parts in self.dirs:
            return self.dirs[parts]
        parent = self.directory(parts[:-1])
        fd = self.p.openat(parent, parts[-1], readonly=True, nofollow=True,
                           cloexec=True, nonblock=True, directory=True)
        self.keep(fd, True)
        self.dirs[parts] = fd
        return fd

    def file(self, parts, cap, optional=False, elf=False):
        parent = self.directory(parts[:-1])
        try:
            fd = self.p.openat(parent, parts[-1], readonly=True, nofollow=True,
                               cloexec=True, nonblock=True, directory=False)
        except FileNotFoundError:
            if optional:
                return None
            raise
        info = self.keep(fd, False)
        _need(0 <= info.st_size <= cap)
        data = bytearray()
        while len(data) <= cap:
            block = self.p.read(fd, min(4096, cap + 1 - len(data)))
            _need(type(block) is bytes and len(block) <= min(4096, cap + 1 - len(data)))
            if not block:
                break
            data.extend(block)
        _need(len(data) == info.st_size and len(data) <= cap)
        if elf:
            _need(data[:4] == b"\x7fELF" and info.st_mode & 0o111 != 0)
        return bytes(data)

    def entries(self):
        fd = self.directory(("etc", "sudoers.d"))
        names = tuple(itertools.islice(self.p.entries(fd, limit=33), 33))
        _need(len(names) <= 32 and len(set(names)) == len(names))
        for name in names:
            _need(type(name) is str and 0 < len(name) <= 255 and name not in (".", ".."))
            _need(all(c.isascii() and (c.isalnum() or c in "_.-") for c in name))
        return tuple(sorted(names))

    def alias(self):
        root = self.dirs[()]
        info = self.p.statat(root, "bin", nofollow=True)
        _need(stat.S_ISLNK(info.st_mode) and info.st_uid == 0)
        _need(self.p.readlinkat(root, "bin", limit=9) in ("usr/bin", "/usr/bin"))
        _need(_snapshot(self.p.statat(root, "bin", nofollow=True)) == _snapshot(info))

    def unchanged(self):
        for fd, before in self.held:
            _need(before == _snapshot(self.p.fstat(fd)))

    def close(self):
        failed = False
        for fd, _ in reversed(self.held):
            try:
                self.p.close(fd)
            except Exception:
                failed = True
        _need(not failed)


def _floor(facts):
    keys = {"systemd", "kernel", "cgroup2", "memory", "pids",
            "user_delegate", "hierarchy_protected"}
    _need(type(facts) is dict and set(facts) == keys)
    _need(type(facts["systemd"]) is int and 254 <= facts["systemd"] <= 65535)
    kernel = facts["kernel"]
    _need(type(kernel) is tuple and len(kernel) == 3)
    _need(all(type(n) is int and 0 <= n <= 65535 for n in kernel))
    _need(kernel >= (5, 14, 0))
    _need(all(facts[key] is True for key in keys - {"systemd", "kernel"}))


def _collect(ports):
    """Private SOURCEONLY entry: all I/O supplied explicitly by mock read ports.

    No policy semantic parser. Unknown bytes, includes, plugins and routes ALL
    remain unreviewed. No successful qualification result exists in this slice.
    Raw bytes/names/stat identities never leave this call, including on errors.
    """
    reader = _Reader(ports)
    result = _empty("refused")
    try:
        reader.root()
        policy = [reader.file(path, _POLICY_CAP) for path in _POLICY_PATHS]
        names = reader.entries()
        includes = []
        total = 0
        for name in names:
            data = reader.file(("etc", "sudoers.d", name), _POLICY_CAP)
            total += len(data)
            _need(total <= _TOTAL_CAP)
            includes.append((name, data))
        c1 = reader.file(_C1, _ELF_CAP, elf=True)
        c2 = reader.file(_C2, _ELF_CAP, optional=True, elf=True)
        reader.alias()
        _floor(ports.facts())
        _need(names == reader.entries())
        reader.unchanged()
        # Canonical length framing binds names AND all bytes, without publishing
        # a path listing or per-entry hash oracle. This digest is NOT proof.
        framed = bytearray()
        for name, data in includes:
            encoded = name.encode("ascii")
            framed.extend(len(encoded).to_bytes(2, "big") + encoded)
            framed.extend(len(data).to_bytes(4, "big") + data)
        result = _empty("policy_unreviewed")
        result.update(floor_observed=True, bin_alias_observed=True,
                      c2_state="absent" if c2 is None else "protected_elf",
                      include_count=len(names))
        for label, data in zip(_LABELS[:3], policy):
            result["records"][label] = _digest(data)
        result["records"]["include_set"] = _digest(bytes(framed))
        result["records"]["c1"] = _digest(c1)
        result["records"]["c2"] = None if c2 is None else _digest(c2)
        _need(not _REVIEWED_POLICY)  # no unreviewed future pin can enable this
    except Exception:
        # No exception messages, paths, raw policy, identities or traceback.
        result = _empty("refused")
    finally:
        try:
            reader.close()
        except Exception:
            result = _empty("refused")
    return result


def _cli():
    # Deliberately no arguments, adapter construction, privilege check or reads.
    print(json.dumps(_empty("root_reads_not_authorized"), sort_keys=True))
    return 2


if __name__ == "__main__":
    raise SystemExit(_cli())
