"""Fixed Noble package comparison. No host reads on import or from the CLI.

Private Linux entry requires a NEW reviewed read-only grant before invocation.
A1 trusts privileged writers/mounts and root-protected OS dependency resolution;
this observation is neither whole-host qualification nor a loader/OS audit.
"""

import hashlib
import json
import stat
import struct

_REVISION = "1.9.15p5-3ubuntu5.24.04.4"
_LABELS = ("sudo", "sudoers", "util")
_PATHS = (("usr", "bin", "sudo"),
          ("usr", "libexec", "sudo", "sudoers.so"),
          ("usr", "libexec", "sudo", "libsudo_util.so.0.0.0"))
_PINS = ((282032, "6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff"),
         (588080, "151d681d51293a040dc479cb0175d1a6d461a40ff455661cf6ad485adf305d4c"),
         (133944, "aae167b9da2436a31eba12d9fb20b36120fd204b2c720d62881d09e89eecd26d"))
_NEEDED = (("libaudit.so.1", "libselinux.so.1", "libapparmor.so.1",
            "libsudo_util.so.0", "libc.so.6"),
           ("libaudit.so.1", "libpam.so.0", "libssl.so.3", "libz.so.1",
            "libsudo_util.so.0", "libcrypto.so.3", "libc.so.6"),
           ("libcrypto.so.3", "libc.so.6"))
_SEARCH = ((None, "/usr/libexec/sudo"), (None, "/usr/libexec/sudo"), (None, None))
_LINKS = ("libsudo_util.so", "libsudo_util.so.0")
_TARGET = "libsudo_util.so.0.0.0"
_TOTAL = sum(size + 1 for size, _ in _PINS)


class _Refused(Exception):
    pass


def _need(ok):
    if not ok:
        raise _Refused()


def _elf(data):
    """Data-only ELF64 LE amd64 dynamic inspection, bounded fixed tables.

    At most 32 program headers, 128 dynamic entries, 8192 string bytes and
    16 dependency names. No sections, relocations, loading or execution.
    """
    _need(type(data) is bytes and 64 <= len(data) <= 588080)
    _need(data[:7] == b"\x7fELF\x02\x01\x01")
    typ, machine, version = struct.unpack_from("<HHI", data, 16)
    _need(typ == 3 and machine == 62 and version == 1)
    phoff = struct.unpack_from("<Q", data, 32)[0]
    ehsize, phsize, phnum = struct.unpack_from("<HHH", data, 52)
    _need(ehsize == 64 and phsize == 56 and 1 <= phnum <= 32)
    _need(64 <= phoff and phoff + phnum * 56 <= len(data))
    loads, dynamics = [], []
    for i in range(phnum):
        kind, _, off, va, _, size, mem, _ = struct.unpack_from(
            "<IIQQQQQQ", data, phoff + i * 56)
        _need(off <= len(data) and size <= len(data) - off and size <= mem)
        if kind == 1:
            loads.append((va, off, size))
        if kind == 2:
            dynamics.append((off, size))
    _need(len(dynamics) == 1)
    off, size = dynamics[0]
    _need(16 <= size <= 128 * 16 and size % 16 == 0)
    tags = []
    ended = False
    for pos in range(off, off + size, 16):
        tag, value = struct.unpack_from("<qQ", data, pos)
        if tag == 0:
            ended = True
        else:
            _need(not ended)
            tags.append((tag, value))
    _need(ended)
    def unique(tag):
        values = [v for t, v in tags if t == tag]
        _need(len(values) == 1)
        return values[0]
    address, length = unique(5), unique(10)
    _need(0 < length <= 8192)
    offsets = [o + address - a for a, o, n in loads
               if a <= address and address + length <= a + n]
    _need(len(offsets) == 1)
    strings = data[offsets[0]:offsets[0] + length]
    def string(index):
        _need(index < len(strings))
        end = strings.find(b"\0", index, min(len(strings), index + 256))
        _need(end >= index)
        return strings[index:end].decode("ascii")
    needed = tuple(string(v) for t, v in tags if t == 1)
    _need(1 <= len(needed) <= 16 and len(set(needed)) == len(needed))
    search = []
    for tag in (15, 29):
        values = [string(v) for t, v in tags if t == tag]
        _need(len(values) <= 1)
        search.append(values[0] if values else None)
    return needed, tuple(search)


def _result(status="refused"):
    return {"schema": 1, "status": status, "candidate_match": False,
            "qualified": False, "revision": _REVISION, "architecture": "amd64",
            "digests": dict.fromkeys(_LABELS)}


def compare(payloads):
    """Pure exact comparison. Callers cannot select paths, expected hashes or ELF facts."""
    result = _result()
    try:
        _need(type(payloads) is tuple and len(payloads) == 3)
        for i, data in enumerate(payloads):
            size, digest = _PINS[i]
            _need(type(data) is bytes and len(data) == size)
            _need(_elf(data) == (_NEEDED[i], _SEARCH[i]))
            _need(hashlib.sha256(data).hexdigest() == digest)
        result = _result("candidate_match")
        result["candidate_match"] = True
        result["digests"] = dict(zip(_LABELS, (pin[1] for pin in _PINS)))
    except Exception:
        pass  # Do not expose exception text or hashes of unrecognized bytes.
    return result


def _snapshot(info):
    return tuple(getattr(info, name) for name in (
        "st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink",
        "st_size", "st_mtime_ns", "st_ctime_ns"))


def _protected(info, directory):
    _need(info.st_uid == 0 and not info.st_mode & 0o022)
    _need(stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode))
    if not directory:
        _need(info.st_nlink == 1)


class _LinuxPorts:
    """Actual fixed-read adapter; OS object is mandatory, including in fake-FD tests.

    readlink_port must implement readlinkat with the supplied buffer bound;
    neither the returned bytes nor error messages may escape the comparison.
    No command, environment, policy, proc, account or directory-listing port.
    """

    def __init__(self, os_port, readlink_port):
        self.os = os_port
        self.readlink = readlink_port
        self.flags = (os_port.O_RDONLY | os_port.O_NOFOLLOW | os_port.O_CLOEXEC
                      | os_port.O_NONBLOCK)
        _need(all(getattr(os_port, n) != 0 for n in
                  ("O_NOFOLLOW", "O_CLOEXEC", "O_NONBLOCK", "O_DIRECTORY")))

    def open(self, parent, name, directory):
        flags = self.flags | (self.os.O_DIRECTORY if directory else 0)
        return self.os.open(name, flags, dir_fd=parent)

    def fstat(self, fd):
        return self.os.fstat(fd)

    def read(self, fd, count):
        return self.os.read(fd, count)

    def close(self, fd):
        self.os.close(fd)

    def link(self, parent, name):
        before = self.os.stat(name, dir_fd=parent, follow_symlinks=False)
        _need(stat.S_ISLNK(before.st_mode) and before.st_uid == 0
              and before.st_nlink == 1 and before.st_size == len(_TARGET))
        target = self.readlink(parent, name, len(_TARGET) + 1)
        _need(type(target) is str and len(target) <= 32 and target == _TARGET)
        after = self.os.stat(name, dir_fd=parent, follow_symlinks=False)
        _need(_snapshot(before) == _snapshot(after))
        return _snapshot(before)


def _collect(ports):
    """Fixed root-down held snapshots; path reopens compare, never adopt changes."""
    held, directories, links = [], {}, {}
    result = _result()
    budget = _TOTAL
    def opened(parent, name, directory, path):
        fd = ports.open(parent, name, directory)
        held.append((fd, path, None, directory))
        info = ports.fstat(fd)
        _protected(info, directory)
        held[-1] = (fd, path, _snapshot(info), directory)
        return fd, info
    try:
        root, _ = opened(None, "/", True, ())
        directories[()] = root
        for path in ((), ("usr",), ("usr", "bin"), ("usr", "libexec"),
                     ("usr", "libexec", "sudo")):
            if path:
                directories[path], _ = opened(directories[path[:-1]], path[-1], True, path)
        parent = directories[_PATHS[2][:-1]]
        for name in _LINKS:
            links[name] = ports.link(parent, name)
        payloads = []
        for path, (size, _) in zip(_PATHS, _PINS):
            fd, info = opened(directories[path[:-1]], path[-1], False, path)
            _need(info.st_size == size)
            data = bytearray()
            for _ in range((size + 4095) // 4096 + 1):
                count = min(4096, max(1, size - len(data)))
                _need(count > 0 and budget >= count)
                block = ports.read(fd, count)
                _need(type(block) is bytes and len(block) <= count)
                budget -= len(block)
                if not block:
                    break
                data.extend(block)
            _need(len(data) == size and not block)
            _need(_snapshot(ports.fstat(fd)) == _snapshot(info))
            payloads.append(bytes(data))
        for name in _LINKS:
            _need(ports.link(parent, name) == links[name])
        # Open the entire tree afresh from /, comparing each identity and all
        # metadata to the original. Do not follow or adopt replaced ancestors.
        fresh = {}
        for fd, path, before, directory in tuple(held):
            _need(_snapshot(ports.fstat(fd)) == before)
            newfd, info = opened(None if not path else fresh[path[:-1]],
                                 "/" if not path else path[-1], directory, path)
            _need(_snapshot(info) == before)
            if directory:
                fresh[path] = newfd
        for name in _LINKS:
            _need(ports.link(fresh[_PATHS[2][:-1]], name) == links[name])
        for fd, _, before, _ in held:
            _need(_snapshot(ports.fstat(fd)) == before)
        result = compare(tuple(payloads))
    except Exception:
        result = _result()
    finally:
        for fd, _, _, _ in reversed(held):
            try:
                ports.close(fd)
            except Exception:
                result = _result()
    return result



class _ReadlinkAt:
    """Fixed buffer syscall adapter; both ctypes and libc are injectable."""

    def __init__(self, ctypes_port, libc):
        self.c = ctypes_port
        self.call = libc.readlinkat
        self.call.argtypes = (self.c.c_int, self.c.c_char_p,
                              self.c.c_void_p, self.c.c_size_t)
        self.call.restype = self.c.c_ssize_t

    def __call__(self, parent, name, limit):
        _need(name in _LINKS and limit == len(_TARGET) + 1)
        buffer = self.c.create_string_buffer(limit)
        length = self.call(parent, name.encode("ascii"), buffer, limit)
        _need(0 <= length < limit)
        return buffer.raw[:length].decode("ascii")


def _read_linux():
    """UNAUTHORIZED HERE. Private real-read entry for a separately reviewed grant.

    No ordinary caller or CLI invokes this function. Explicit invocation is
    required; this function is not an authorization mechanism for future wiring.
    """
    import ctypes
    import os
    import sys
    if sys.platform != "linux":
        return _result()
    try:
        return _collect(_LinuxPorts(os, _ReadlinkAt(ctypes, ctypes.CDLL(None))))
    except Exception:
        return _result()


def _cli():
    # No argument/env selector, adapter construction, UID query or host reads.
    print(json.dumps(_result("root_reads_not_authorized"), sort_keys=True))
    return 2


if __name__ == "__main__":
    raise SystemExit(_cli())
