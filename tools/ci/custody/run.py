"""Fixed, hosted-only custody qualification. No caller commands or paths accepted."""
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from contextvars import ContextVar
from contextlib import ExitStack

PROBE = 'cluster::pki::custody::hosted_probe::hosted_custody_probe'
BASE = 'docker.io/library/ubuntu@sha256:9a04f13b3f80ef056361e30868593b1cfd2279fe19e4081cb93a90bf56819d6a'
ADOPTED_PINS = '28f882cb018eb956c60c190f8718fb982923cd096adb608e15e194c51fa2e167'
# SHA256 of docs/briefs/m3-custody-hosted-pins.md after amendment A4 (2026-11-23).
# Recorded evidence uses this digest. ADR 0037 must quote it and ADOPTED_PINS.
PINS = '32503a622d15ba8f786878ce4dfaeb4fd674db3a0b528b6ce2b450ce9c067c55'
MIB = 1024 * 1024
MODES = {'create-founder', 'create-learner', 'deny-build', 'same-uid-control', 'release', 'restart-founder', 'restart-learner'}
LIMIT = 1024 * 1024


DIAGNOSTIC = ContextVar('custody_diagnostic', default=None)


class Refusal(Exception):
    def __init__(self, code, subject=None, predicate=None, **facts):
        super().__init__(code)
        sink = DIAGNOSTIC.get()
        if sink is not None and code != 'EvidenceOverflow':
            sink('refusal', code=code, subject=subject or sys._getframe(1).f_code.co_name,
                 predicate=predicate or code, **facts)


def require(condition, code, subject='launcher', predicate='required_condition', **facts):
    if not condition:
        raise Refusal(code, subject, predicate, **facts)


def safe(path, parent=None, directory=True, *, input_file=False, subject='filesystem'):
    try:
        return checked_path(path, parent, directory, input_file=input_file, subject=subject)
    except (OSError, ValueError, RuntimeError) as error:
        raise Refusal('UnsafePath', subject, 'filesystem_error', error_type=type(error).__name__) from None


def checked_path(path, parent=None, directory=True, *, input_file=False, subject='filesystem'):
    path = Path(path)
    require(path.is_absolute() and len(str(path)) <= 4096, 'UnsafePath', subject, 'absolute_bounded_path')
    for depth, p in enumerate([path, *path.parents]):
        require(not p.is_symlink(), 'UnsafePath', subject, 'no_symlink_component', ancestor_depth=depth)
    require(path.resolve() == path, 'UnsafePath', subject, 'canonical_spelling')
    if parent:
        require(path.is_relative_to(parent) and path != parent, 'UnsafePath', subject, 'strict_descendant')
    m = path.lstat()
    facts = dict(device=m.st_dev, inode=m.st_ino, uid=m.st_uid, gid=m.st_gid,
                 mode=stat.S_IMODE(m.st_mode), nlink=m.st_nlink, file_type=stat.S_IFMT(m.st_mode))
    require(stat.S_ISDIR(m.st_mode) if directory else stat.S_ISREG(m.st_mode), 'UnsafePath', subject, 'directory_type' if directory else 'regular_type', **facts)
    if not directory:
        require(m.st_nlink >= 1 if input_file else m.st_nlink == 1, 'UnsafePath', subject, 'input_link_policy' if input_file else 'single_link', **facts)
    return m


TOOLS = ('docker', 'sudo', 'mkfs.ext4', 'losetup', 'mount', 'umount',
         'findmnt', 'readelf', 'git', 'mkdir', 'chown', 'python3')
SUDO_TARGET = '/usr/lib/cargo/bin/sudo'
MULTICALL_TARGETS = {'mkdir': '/usr/lib/cargo/bin/coreutils/mkdir',
                     'chown': '/usr/lib/cargo/bin/coreutils/chown'}
HARDLINKS = '/proc/sys/fs/protected_hardlinks'
SEARCH = '/usr/bin:/usr/sbin:/bin:/sbin'


def protected_hardlinks(record, fs=os):
    """Fixed bounded no-follow read in the trusted hosted procfs namespace."""
    fd = None
    value = None
    predicate = 'protected_hardlinks_one'
    try:
        fd = fs.open(HARDLINKS, fs.O_RDONLY | fs.O_NOFOLLOW | fs.O_CLOEXEC)
        m = fs.fstat(fd)
        require(stat.S_ISREG(m.st_mode) and m.st_uid == 0 and not m.st_mode & 0o022,
                'ToolOwnership', 'protected_hardlinks', 'stat.S_ISREG(m.st_mode) and m.st_uid == 0 and (not m.st_mode & 18)')
        data = fs.read(fd, 16)
        value = 1 if data in (b'1', b'1\n') else 0 if data in (b'0', b'0\n') else None
        require(value == 1 and identity(fs.fstat(fd)) == identity(m), 'ToolOwnership', 'protected_hardlinks', 'value == 1 and identity(fs.fstat(fd)) == identity(m)')
    except (OSError, ValueError, RuntimeError, Refusal):
        predicate = 'protected_hardlinks_refused'
    finally:
        if fd is not None:
            try:
                fs.close(fd)
            except OSError:
                predicate = 'protected_hardlinks_refused'
        record('hardlink-policy', predicate=predicate, value=value)
    require(predicate == 'protected_hardlinks_one', 'ToolOwnership', 'protected_hardlinks', "predicate == 'protected_hardlinks_one'")


def identity(m):
    return (m.st_dev, m.st_ino, m.st_mode, m.st_uid, m.st_gid, m.st_nlink)


class TrustedTool:
    """Retained CLOEXEC objects; the filesystem adapter is injectable in pure tests."""
    def __init__(self, role, record, entry, fs=os, hardlinks=False):
        self.role, self.record, self.entry, self.fs = role, record, entry, fs
        self.objects = []
        self.path = None
        self.current = None
        self.component = '<unapproved-path>'
        self.reported = False
        self.hardlinks = hardlinks
        self.multicall = False

    def facts(self, m):
        return dict(uid=None if m is None else m.st_uid,
                    gid=None if m is None else m.st_gid,
                    mode=None if m is None else format(stat.S_IMODE(m.st_mode), '04o'),
                    device=None if m is None else m.st_dev,
                    inode=None if m is None else m.st_ino,
                    nlink=None if m is None else m.st_nlink,
                    file_type=None if m is None else stat.S_IFMT(m.st_mode))

    def check(self, condition, predicate):
        if not condition:
            self.reported = True
            self.record('tool-refusal', tool=self.role if self.role in TOOLS else '<unknown-role>',
                        resolved_path=self.path or '<unapproved-path>',
                        component=self.component, predicate=predicate, **self.facts(self.current))
            raise Refusal('ToolOwnership')

    def opened(self, name, parent=None):
        self.current = None
        flags = self.fs.O_PATH | self.fs.O_NOFOLLOW | self.fs.O_CLOEXEC
        fd = self.fs.open(name, flags, dir_fd=parent)
        # Retain immediately, even if fstat fails, so all descriptors are closed.
        self.objects.append((fd, None))
        self.current = self.fs.fstat(fd)
        self.objects[-1] = (fd, identity(self.current))
        return fd

    def walk(self):
        try:
            self.check(self.role in TOOLS, 'closed_role')
            self.check(isinstance(self.entry, str) and self.entry.startswith('/') and
                       len(self.entry) <= 4096 and '\x00' not in self.entry, 'absolute_bounded_entry')
            root = self.opened('/')
            stack = [('', root)]
            pending = self.entry.split('/')[1:]
            hops = steps = 0
            while True:
                self.check(steps <= 256, 'bounded_components')
                m = self.current
                component = '/' + '/'.join(name for name, _ in stack[1:])
                self.component = component if component in (
                    '/', '/usr', '/usr/bin', '/usr/sbin', '/bin', '/sbin', '/etc',
                    '/etc/alternatives', '/usr/lib', '/usr/lib/cargo', '/usr/lib/cargo/bin'
                ) else '<unapproved-path>'
                self.check(m.st_uid == 0, 'owner_uid_root')
                # Symlink 0777 is normal; containing directories govern replacement.
                if not stat.S_ISLNK(m.st_mode):
                    self.check(not m.st_mode & 0o022, 'not_group_or_other_writable')
                if not pending:
                    resolved = '/' + '/'.join(name for name, _ in stack[1:])
                    self.multicall = resolved == MULTICALL_TARGETS.get(self.role)
                    self.check(stat.S_ISREG(m.st_mode) and
                               (m.st_nlink >= 1 if self.multicall else m.st_nlink == 1),
                               'regular_link_policy')
                    if self.multicall:
                        self.check(not m.st_mode & (stat.S_ISUID | stat.S_ISGID), 'multicall_no_setid')
                        self.check(self.hardlinks, 'protected_hardlinks_one')
                    self.check(bool(m.st_mode & stat.S_IXUSR), 'owner_executable')
                    self.check(self.role != 'sudo' or bool(m.st_mode & stat.S_ISUID), 'sudo_setuid')
                    allowed = resolved.startswith(('/usr/bin/', '/usr/sbin/')) or (
                        self.role == 'sudo' and resolved == SUDO_TARGET) or self.multicall
                    self.check(allowed, 'canonical_system_directory')
                    self.path = resolved
                    self.record('tool-validated', tool=self.role, entry=self.entry if self.entry in
                                tuple(d + '/' + self.role for d in SEARCH.split(':')) else '<interpreter-entry>',
                                resolved_path=self.path, predicate='validated',
                                policy='exact_multicall' if self.multicall else 'single_link',
                                components=len(self.objects), **self.facts(m))
                    return self
                self.check(stat.S_ISDIR(m.st_mode), 'directory_component')
                self.record('tool-directory', tool=self.role, component=self.component,
                            predicate='root_nonwritable_directory', **self.facts(m))
                part = pending.pop(0)
                steps += 1
                if part in ('', '.'):
                    continue
                if part == '..':
                    if len(stack) > 1:
                        stack.pop()
                    self.current = self.fs.fstat(stack[-1][1])
                    continue
                parent = stack[-1][1]
                fd = self.opened(part, parent)
                m = self.current
                if stat.S_ISLNK(m.st_mode):
                    self.check(m.st_uid == 0, 'owner_uid_root')
                    hops += 1
                    self.check(hops <= 40, 'bounded_symlinks')
                    target = self.fs.readlink(part, dir_fd=parent)
                    self.check(identity(self.fs.stat(part, dir_fd=parent, follow_symlinks=False)) ==
                               identity(m), 'stable_symlink')
                    self.check(len(target) <= 4096 and len(target) > 0, 'bounded_link')
                    if target.startswith('/'):
                        stack = [('', root)]
                    pending = target.split('/') + pending
                    self.current = self.fs.fstat(stack[-1][1])
                else:
                    stack.append((part, fd))
        except (OSError, ValueError, RuntimeError):
            self.current = None
            self.check(False, 'filesystem_error')
        except Refusal:
            raise
        finally:
            if self.path is None:
                self.close()

    def recheck(self):
        try:
            if self.multicall:
                protected_hardlinks(self.record, self.fs)
            for fd, before in self.objects:
                self.current = self.fs.fstat(fd)
                self.check(identity(self.current) == before, 'stable_open_object')
            other = TrustedTool(self.role, lambda *a, **k: None, self.entry, self.fs, self.hardlinks)
            try:
                other.walk()
                self.check(other.path == self.path and
                           [m for _, m in other.objects] == [m for _, m in self.objects],
                           'stable_route')
            except Refusal:
                if not self.reported:
                    self.current = other.current
                    self.check(False, 'stable_route')
                raise
            finally:
                other.close()
            # Pathname exec is not atomic FD execution. Retained checked objects and
            # root-owned nonwritable ancestry fence unprivileged replacement. Rechecks
            # detect drift, not a hostile root/mount administrator (excluded by ADR0037).
            # This harness must not perform concurrent system package updates.
            self.record('tool-rechecked', tool=self.role, resolved_path=self.path,
                        predicate='stable_route_and_objects')
            return self.path
        except (OSError, ValueError, RuntimeError):
            self.current = None
            self.check(False, 'filesystem_error')

    def close(self):
        objects, self.objects = self.objects, []
        failed = False
        for fd, _ in reversed(objects):
            try:
                self.fs.close(fd)
            except OSError:
                failed = True
        if failed and not self.reported:
            self.current = None
            self.check(False, 'filesystem_error')


def trusted_tool(tool, record, fs=os, entry=None, hardlinks=False):
    obj = TrustedTool(tool, record, entry, fs, hardlinks)
    try:
        obj.check(tool in TOOLS, 'closed_role')
        if entry is None:
            obj.entry = shutil.which(tool, path=SEARCH)
        return obj.walk()
    except (OSError, ValueError, RuntimeError):
        obj.check(False, 'filesystem_error')


def digest(path):
    safe(path, directory=False)
    with open(path, 'rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def scratch_usage(root, mounted, statvfs=None):
    """Bounded metadata-only accounting; never traverse the private ext4 tree.

    Backing-file blocks already include fixture allocation: ext4 used bytes are
    reported separately, never added again. Volume usage is not task attribution.
    """
    statvfs = statvfs or os.statvfs
    totals = dict(allocated_bytes=0, logical_file_bytes=0, context_bytes=0, files=0)
    device = safe(root, subject='scratch-root').st_dev

    def account(path, directory, context=False):
        m = safe(path, directory=directory, subject='scratch-object')
        require(m.st_dev == device and isinstance(m.st_blocks, int) and m.st_blocks >= 0,
                'ScratchMeasurement', 'scratch-object', 'attributed_blocks')
        totals['allocated_bytes'] += m.st_blocks * 512
        if not directory:
            totals['logical_file_bytes'] += m.st_size
            totals['context_bytes'] += m.st_size if context else 0
            totals['files'] += 1
        return identity(m)

    def entries(path, allowed):
        with os.scandir(path) as scan:
            for index, entry in enumerate(scan):
                require(index < len(allowed) and entry.name in allowed,
                        'ScratchMeasurement', 'scratch-tree', 'closed_bounded_entries')
                yield path / entry.name

    def usage(path):
        before = safe(path, subject='scratch-filesystem')
        v = statvfs(path)
        require(v.f_frsize > 0 and 0 <= v.f_bfree <= v.f_blocks,
                'ScratchMeasurement', 'scratch-filesystem', 'valid_statvfs')
        require(identity(safe(path, subject='scratch-filesystem')) == identity(before),
                'ScratchMeasurement', 'scratch-filesystem', 'stable_object')
        return (v.f_blocks - v.f_bfree) * v.f_frsize

    root_identity = account(root, True)
    for path in entries(root, ('context', 'fs', 'disk.img', 'image.id')):
        if path.name == 'fs' and mounted:
            require(safe(path, root, subject='scratch-fixture').st_dev != device,
                    'ScratchMeasurement', 'scratch-fixture', 'distinct_mounted_filesystem')
            continue
        before = account(path, path.name in ('context', 'fs'))
        if path.name == 'context':
            for child in entries(path, ('arxd-tests', 'Containerfile')):
                account(child, False, True)
        elif path.name == 'fs':
            for _ in entries(path, ()):
                pass
        require(identity(safe(path, directory=path.name in ('context', 'fs'))) == before,
                'ScratchMeasurement', 'scratch-tree', 'stable_object')
    require(identity(safe(root)) == root_identity,
            'ScratchMeasurement', 'scratch-root', 'stable_object')
    totals['host_volume_used_bytes'] = usage(root)
    totals['ext4_used_bytes'] = usage(root / 'fs') if mounted else None
    return totals


class BuildInput:
    """Trusted CI input, with retained no-follow object and bounded stable copy.

    Runner ownership and existing aliases are permitted for this role only.
    Pathname and descriptor identity, metadata and bytes must remain unchanged.
    """
    def __init__(self, path, parent, subject, record, checkpoint=lambda: None):
        self.path, self.parent, self.subject, self.record = path, parent, subject, record
        self.checkpoint = checkpoint
        self.fd = None

    def __enter__(self):
        try:
            m = safe(self.path, self.parent, False, input_file=True, subject=self.subject)
            self.record('input-object', subject=self.subject, uid=m.st_uid, gid=m.st_gid,
                        mode=stat.S_IMODE(m.st_mode), device=m.st_dev, inode=m.st_ino,
                        nlink=m.st_nlink, file_type=stat.S_IFMT(m.st_mode), size=m.st_size)
            require(m.st_size <= 256*MIB, 'CandidateBounds', self.subject, 'size_256_mib')
            self.fd = os.open(self.path, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) |
                              getattr(os, 'O_CLOEXEC', 0) | getattr(os, 'O_BINARY', 0))
            self.metadata = identity(m), m.st_size, m.st_mtime_ns, m.st_ctime_ns
            opened = os.fstat(self.fd)
            self.open_metadata = identity(opened), opened.st_size, opened.st_mtime_ns, opened.st_ctime_ns
            # Windows lstat ctime is birth time, while fstat can expose change
            # time. Retain both snapshots; object identity/size/mtime must agree.
            require(self.open_metadata[:3] == self.metadata[:3], 'StagingIdentity',
                    self.subject, 'opened_same_object')
            self.sha = self.hash()
            return self
        except Exception:
            self.__exit__(None, None, None)
            raise

    def check(self):
        self.checkpoint()
        for m, before in ((safe(self.path, self.parent, False, input_file=True, subject=self.subject), self.metadata),
                          (os.fstat(self.fd), self.open_metadata)):
            require((identity(m), m.st_size, m.st_mtime_ns, m.st_ctime_ns) == before,
                    'StagingIdentity', self.subject, 'same_open_and_route_object')

    def hash(self, output=None):
        self.check()
        os.lseek(self.fd, 0, os.SEEK_SET)
        h, size = hashlib.sha256(), 0
        while data := os.read(self.fd, MIB):
            self.checkpoint()
            size += len(data)
            require(size <= 256*MIB, 'CandidateBounds', self.subject, 'bounded_read')
            h.update(data)
            if output is not None:
                output.write(data)
        self.check()
        require(size == self.metadata[1], 'StagingIdentity', self.subject, 'exact_size')
        return h.hexdigest()

    def verify(self):
        require(self.hash() == self.sha, 'StagingHash', self.subject, 'stable_input_hash')

    def stage(self, destination):
        self.verify()
        with open(destination, 'xb') as out:
            copied = self.hash(out)
        self.verify()
        require(copied == self.sha == digest(destination), 'StagingHash', self.subject, 'staged_hash')

    def __exit__(self, *_):
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None


def counts(output):
    selected = re.findall(rb'^running ([0-9]{1,3}) tests?\r?$', output, re.M)
    results = re.findall(rb'^test result: (ok|FAILED)\. ([0-9]{1,3}) passed; ([0-9]{1,3}) failed; ([0-9]{1,3}) ignored; ([0-9]{1,3}) measured; ([0-9]{1,4}) filtered out(?:; finished in [0-9.]+s)?\r?$', output, re.M)
    if len(selected) != 1 or len(results) != 1:
        return {}
    status, passed, failed, ignored, measured, filtered = results[0]
    oracle = re.findall(rb'ARX_PASS selected=([0-9]{1,3}) passed=([0-9]{1,3})(?:\r?\n|$)', output)
    return dict(oracle_selected=int(oracle[0][0]) if len(oracle) == 1 else None,
                oracle_passed=int(oracle[0][1]) if len(oracle) == 1 else None,
                selected=int(selected[0]), passed=int(passed), failed=int(failed),
                ignored=int(ignored), measured=int(measured), filtered=int(filtered),
                libtest_ok=status == b'ok')


def protocol_records(output):
    # Never forward arbitrary output (including panic text or public context).
    records = []
    for line in output.splitlines():
        if b'ARX_' not in line:
            continue
        prefix, _, record = line.partition(b'ARX_')
        require(prefix in (b'', ('test ' + PROBE + ' ... ').encode()), 'UnknownRecord', 'protocol_records', "prefix in (b'', ('test ' + PROBE + ' ... ').encode())")
        record = b'ARX_' + record
        require(any(re.fullmatch(pattern, record) for pattern in (
            rb'ARX_PASS selected=[0-9]{1,3} passed=[0-9]{1,3}',
            rb'ARX_READY [0-9]{1,7} [0-9]{1,3},[0-9]{1,3},[0-9]{1,3} [a-f0-9]{256}',
            rb'ARX_FACTS uid=2100[12] caps=0 nnp=1 core=0',
            rb'ARX_MOUNT slot=/(tmp|workspace|control|dev/shm|dev) capacity=[0-9]{1,10} used=[0-9]{1,10}',
            rb'ARX_DENIAL errno=[0-9]{1,3}',
        )), 'UnknownRecord', 'protocol_records', "any((re.fullmatch(pattern, record) for pattern in (b'ARX_PASS selected=[0-9]{1,3} passed=[0-9]{1,3}', b'ARX_READY [0-9]{1,7} [0-9]{1,3},[0-9]{1,3},[0-9]{1,3} [a-f0-9]{256}', b'ARX_FACTS uid=2100[12] caps=0 nnp=1 core=0', b'ARX_MOUNT slot=/(tmp|workspace|con...")
        if record.startswith((b'ARX_FACTS ', b'ARX_MOUNT ', b'ARX_DENIAL ')):
            records.append(record.decode('ascii'))
    return records


def terminal(output):
    protocol_records(output)
    result = counts(output)
    require(output.count(b'ARX_PASS selected=1 passed=1') == 1 and
            all(result.get(key) == value for key, value in
                dict(oracle_selected=1, oracle_passed=1, selected=1, passed=1, failed=0,
                     ignored=0, measured=0, libtest_ok=True).items()), 'MissingOracle', 'terminal', "output.count(b'ARX_PASS selected=1 passed=1') == 1 and all((result.get(key) == value for key, value in dict(oracle_selected=1, oracle_passed=1, selected=1, passed=1, failed=0, ignored=0, measured=0, libtest_ok=True).items()))")
    require(len(re.findall(rb'ARX_PASS ', output)) == 1, 'MissingOracle', 'terminal', "len(re.findall(b'ARX_PASS ', output)) == 1")
    return result


def exact_listing(output):
    names = [line[:-6] for line in output.decode('ascii').splitlines() if line.endswith(': test')]
    return names == [PROBE] and output.count(b'1 test, 0 benchmarks') == 1


def command_role(argv):
    """Closed operation labels, never command text or caller paths."""
    if argv[0] == 'sudo':
        return 'sudo-' + argv[2] if len(argv) > 2 and argv[2] in TOOLS else 'sudo-invalid'
    if argv[0] == 'docker':
        return 'docker-' + argv[1] if len(argv) > 1 and argv[1] in (
            'build', 'image', 'create', 'inspect', 'start', 'exec', 'logs', 'rm', 'container') else 'docker-invalid'
    if argv[0] == 'git':
        return 'git-source' if len(argv) > 1 and argv[1] == 'show' else 'git-head'
    return argv[0] if argv[0] in TOOLS else 'candidate-list'


def choose(candidates):
    matches = [p for p, listing, before, after in candidates if exact_listing(listing) and before == after]
    require(len(matches) == 1 and all(before == after for _, _, before, after in candidates), 'ProbeSelection', 'choose', 'len(matches) == 1 and all((before == after for _, _, before, after in candidates))')
    return matches[0]


def create_argv(name, run_id, root, node, image):
    require(re.fullmatch(r'arx-[a-f0-9]{16}-(founder|learner1|learner2)', name) and
            node in ('founder', 'learner1', 'learner2') and re.fullmatch(r'[a-f0-9]{16}', run_id) and
            re.fullmatch(r'sha256:[a-f0-9]{64}', image), 'Arguments', 'create_argv', "re.fullmatch('arx-[a-f0-9]{16}-(founder|learner1|learner2)', name) and node in ('founder', 'learner1', 'learner2') and re.fullmatch('[a-f0-9]{16}', run_id) and re.fullmatch('sha256:[a-f0-9]{64}', image)")
    return ['docker', 'create', '--name', name, '--label', f'arxcovo.custody.run={run_id}',
            '--platform', 'linux/amd64', '--user', '21001:21001', '--workdir', '/', '--read-only',
            '--network', 'none', '--ipc', 'private', '--shm-size', '1m', '--cap-drop', 'ALL',
            '--security-opt', 'no-new-privileges=true', '--ulimit', 'core=0:0', '--ulimit', 'nofile=128:128',
            '--cpus', '1', '--memory', '256m', '--memory-swap', '256m', '--pids-limit', '64',
            '--log-driver', 'local', '--log-opt', 'max-size=1m', '--log-opt', 'max-file=1', '--log-opt', 'compress=false',
            '--tmpfs', '/tmp:rw,noexec,nosuid,nodev,size=8m,mode=1777',
            '--tmpfs', '/workspace:rw,noexec,nosuid,nodev,size=8m,mode=0700,uid=21002,gid=21002',
            '--tmpfs', '/control:rw,noexec,nosuid,nodev,size=1m,mode=0700,uid=21001,gid=21001',
            '--mount', f'type=bind,src={root}/fs/{node}-wrap,dst=/wrap,bind-propagation=rprivate',
            '--mount', f'type=bind,src={root}/fs/{node}-store,dst=/store,bind-propagation=rprivate', image]


def exec_argv(container, mode, context=None, target=None):
    require(mode in MODES and re.fullmatch(r'[a-f0-9]{64}', container), 'Arguments', 'exec_argv', "mode in MODES and re.fullmatch('[a-f0-9]{64}', container)")
    uid = '21002:21002' if mode == 'deny-build' else '21001:21001'
    argv = ['docker', 'exec', '--user', uid, '--workdir', '/', container, '/usr/bin/env', '-i',
            'PATH=/usr/bin:/bin', 'LANG=C', 'TZ=UTC', 'ARX_CUSTODY_HOSTED_MODE=' + mode]
    if context is not None:
        require(re.fullmatch(r'[a-f0-9]{256}', context), 'Protocol', 'exec_argv', "re.fullmatch('[a-f0-9]{256}', context)")
        argv += ['ARX_CUSTODY_PUBLIC_CONTEXT=' + context]
    if target is not None:
        pid, fds = target
        require(2 <= pid <= 4194304 and len(fds) == 3 and all(3 <= f <= 127 for f in fds), 'Protocol', 'exec_argv', '2 <= pid <= 4194304 and len(fds) == 3 and all((3 <= f <= 127 for f in fds))')
        argv += [f'ARX_CUSTODY_TARGET_PID={pid}', 'ARX_CUSTODY_TARGET_FDS=' + ','.join(map(str, fds))]
    return argv + ['/opt/arx/arxd-tests', '--exact', PROBE, '--nocapture', '--test-threads=1']


def inspect_node(data, run_id, image, roots):
    require(data['Config']['Labels'].get('arxcovo.custody.run') == run_id and data['Image'] == image, 'UnknownCleanup', 'inspect_node', "data['Config']['Labels'].get('arxcovo.custody.run') == run_id and data['Image'] == image")
    h = data['HostConfig']
    require(h['ShmSize'] == MIB and h['IpcMode'] == 'private' and h['Memory'] == 256*MIB and
            h['MemorySwap'] == 256*MIB and h['PidsLimit'] == 64 and h['NanoCpus'] == 1_000_000_000 and
            h['ReadonlyRootfs'] and h['NetworkMode'] == 'none' and h['CapDrop'] == ['ALL'], 'Limits', 'inspect_node', "h['ShmSize'] == MIB and h['IpcMode'] == 'private' and (h['Memory'] == 256 * MIB) and (h['MemorySwap'] == 256 * MIB) and (h['PidsLimit'] == 64) and (h['NanoCpus'] == 1000000000) and h['ReadonlyRootfs'] and (h['NetworkMode'] == 'none') and (h['CapDrop'] == ['...")
    binds = {m['Destination']: m['Source'] for m in data['Mounts'] if m['Type'] == 'bind'}
    require(binds == roots, 'MountOwnership', 'inspect_node', 'binds == roots')
    require(all(m['Type'] in ('bind', 'tmpfs') for m in data['Mounts']), 'MountOwnership', 'inspect_node', "all((m['Type'] in ('bind', 'tmpfs') for m in data['Mounts']))")
    require({m['Destination'] for m in data['Mounts'] if m['Type'] == 'tmpfs'} <= {'/tmp', '/workspace', '/control'}, 'MountOwnership', 'inspect_node', "{m['Destination'] for m in data['Mounts'] if m['Type'] == 'tmpfs'} <= {'/tmp', '/workspace', '/control'}")
    require(17 + 1 == 18 and 18 + 64 == 82 and 256 - 82 == 174, 'Accounting', 'inspect_node', '17 + 1 == 18 and 18 + 64 == 82 and (256 - 82 == 174)')


class Budget:
    CAPS = {'preflight': 20, 'image': 40, 'prepare': 15, 'ready': 15, 'probe': 15, 'restart': 15, 'evidence': 10}

    def __init__(self, clock=time.monotonic):
        self.clock = clock
        self.start = clock()
        self.used = dict.fromkeys(self.CAPS, 0.0)

    def deadline(self, phase, maximum):
        now = self.clock()
        require(now < self.start + 130, 'WorkTimeout', 'deadline', 'now < self.start + 130')
        left = self.CAPS[phase] - self.used[phase]
        require(left > 0, 'PhaseTimeout', 'deadline', 'left > 0')
        return min(now + maximum, now + left, self.start + 130)


class Child:
    def __init__(self, argv, deadline, clock):
        self.clock, self.deadline = clock, deadline
        self.p = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, start_new_session=True, shell=False, close_fds=True,
                                  env={'PATH': '/usr/bin:/bin', 'LANG': 'C', 'TZ': 'UTC'})
        self.selector = selectors.DefaultSelector()
        for stream in (self.p.stdout, self.p.stderr):
            os.set_blocking(stream.fileno(), False)
            self.selector.register(stream, selectors.EVENT_READ)
        self.out = bytearray()
        self.reaped = False

    def pump(self):
        for key, _ in self.selector.select(0):
            data = os.read(key.fileobj.fileno(), 4096)
            if data:
                require(len(self.out) + len(data) <= 65536, 'PipeOverflow', 'pump', 'len(self.out) + len(data) <= 65536')
                self.out.extend(data)
            else:
                self.selector.unregister(key.fileobj)
                key.fileobj.close()

    def alive(self):
        self.pump()
        require(self.clock() < self.deadline, 'HolderTimeout', 'alive', 'self.clock() < self.deadline')
        require(self.p.poll() is None, 'HolderExit', 'alive', 'self.p.poll() is None')

    def drain_and_reap(self):
        self.pump()
        if self.p.poll() is None or self.selector.get_map():
            return False
        self.p.wait(timeout=0)
        self.reaped = True
        self.selector.close()
        return True

    def finish(self):
        if not self.drain_and_reap():
            return False
        require(self.p.returncode == 0, 'CommandExit', 'finish', 'self.p.returncode == 0')
        return True

    def finish_discovery(self):
        if not self.drain_and_reap():
            return False
        require(isinstance(self.p.returncode, int) and self.p.returncode >= 0,
                'CommandExit', 'discovery-list', 'non_signal_completed_exit')
        return True

    def kill(self):
        if self.p.poll() is None:
            os.killpg(self.p.pid, signal.SIGKILL)
        self.p.wait(timeout=1)
        for stream in (self.p.stdout, self.p.stderr):
            if not stream.closed:
                stream.close()
        self.selector.close()
        self.reaped = True


class Harness:
    def __init__(self, clock=time.monotonic, spawn=Child):
        self.budget = Budget(clock)
        self.clock, self.spawn = clock, spawn
        self.first = None
        self.secondary = []
        self.children = []
        self.holder = None
        self.releasing = False
        self.holder_case = None
        self.holder_reap_start = None
        self.cleanup_seconds = 0.0
        self.nodes = {}
        self.root = self.loop = self.image = None
        self.mounted = False
        self.pending_node = None
        self.cleanup_deadline = None
        self.run_id = os.urandom(8).hex()
        self.evidence = 0
        self.evidence_exhausted = False
        self.tools = None
        self.case_results = set()
        self.scratch = None

    def record_scratch(self, checkpoint, node=None):
        if self.scratch is None:
            return
        require(checkpoint in ('root', 'staged', 'image', 'fixture', 'ready', 'probe', 'restart', 'node-removed', 'pre-cleanup') and
                node in (None, 'founder', 'learner1', 'learner2'),
                'ScratchMeasurement', 'scratch', 'closed_checkpoint')
        start = self.clock()
        try:
            self.budget.deadline('evidence', 10)
            try:
                measured = scratch_usage(self.root, self.mounted)
            except (OSError, AttributeError, ValueError, RuntimeError) as error:
                raise Refusal('ScratchMeasurement', 'scratch', 'metadata_error', error_type=type(error).__name__) from None
            state = self.scratch
            state['samples'] += 1
            state['peak_allocated_bytes'] = max(state['peak_allocated_bytes'], measured['allocated_bytes'])
            if measured['ext4_used_bytes'] is not None:
                state['peak_ext4_used_bytes'] = max(state['peak_ext4_used_bytes'], measured['ext4_used_bytes'])
            self.record('scratch-usage', checkpoint=checkpoint, node=node, **measured, **state)
        finally:
            self.budget.used['evidence'] += self.clock() - start
        self.budget.deadline('evidence', 10)

    def fail(self, error):
        code = str(error) if isinstance(error, Refusal) else type(error).__name__
        if not isinstance(error, Refusal):
            self.record('refusal', code=code, subject='launcher', predicate='operation_exception')
        if self.first is None:
            self.first = code
        else:
            self.secondary.append(code)

    def check_work(self):
        require(self.clock() < self.budget.start + 130, 'WorkTimeout', 'check_work', 'self.clock() < self.budget.start + 130')

    def record(self, event, **facts):
        line = json.dumps({'event': event, **facts}, sort_keys=True)
        size = len(line.encode('utf-8')) + 1
        # Reserve only diagnostic space inside the unchanged total evidence cap.
        if self.evidence + size > LIMIT - 256 or self.evidence_exhausted:
            if not self.evidence_exhausted:
                refusal = json.dumps(dict(event='refusal', code='EvidenceOverflow',
                    subject='evidence', predicate='bounded_sink', bytes=self.evidence, cap=LIMIT), sort_keys=True)
                self.evidence += len(refusal) + 1
                print(refusal, flush=True)
                self.evidence_exhausted = True
            raise Refusal('EvidenceOverflow')
        self.evidence += size
        print(line, flush=True)

    def bound_argv(self, argv):
        if self.tools is None:  # Pure lifecycle tests inject processes before qualify().
            return argv
        argv = list(argv)
        role = argv[0]
        if role in self.tools:
            argv[0] = self.tools[role].recheck()
            if role == 'sudo':
                require(len(argv) >= 3 and argv[1] == '-n' and argv[2] in
                        ('mkfs.ext4', 'losetup', 'mount', 'umount', 'findmnt', 'mkdir', 'chown'), 'Arguments', 'bound_argv', "len(argv) >= 3 and argv[1] == '-n' and (argv[2] in ('mkfs.ext4', 'losetup', 'mount', 'umount', 'findmnt', 'mkdir', 'chown'))")
                argv[2] = self.tools[argv[2]].recheck()
        else:
            # The sole non-tool host executable is the selected, hash-checked probe.
            require(Path(role).is_absolute(), 'Arguments', 'bound_argv', 'Path(role).is_absolute()')
        return argv

    def command(self, argv, phase, seconds=5, cleanup=False, observed=None, discovery=False):
        if discovery:
            require(not cleanup and phase == 'preflight' and seconds == 2 and len(argv) == 4 and
                    Path(argv[0]).is_absolute() and argv[1:] == ['--list', '--exact', PROBE],
                    'Arguments', 'discovery-list', 'fixed_host_listing_only')
        start = self.clock()
        deadline = min(self.cleanup_deadline or self.budget.start + 150, start + seconds) if cleanup else self.budget.deadline(phase, seconds)
        require(start < deadline, 'CleanupTimeout' if cleanup else 'WorkTimeout', 'command', 'start < deadline')
        child = None
        failure = None
        try:
            child = self.spawn(self.bound_argv(argv), deadline, self.clock)
            self.children.append(child)
            while True:
                if not cleanup and self.holder:
                    self.check_work()
                    if self.releasing:
                        self.holder.pump()
                        require(self.clock() < self.holder.deadline, 'HolderTimeout', 'command', 'self.clock() < self.holder.deadline')
                    else:
                        self.holder.alive()
                require(self.clock() < deadline, 'CommandTimeout', 'command', 'self.clock() < deadline')
                finished = child.finish_discovery() if discovery else child.finish()
                if finished:
                    output = bytes(child.out)
                    return (output, child.p.returncode) if discovery else output
                time.sleep(.01)
        except Exception as error:
            failure = error
            self.record('command-refusal', phase=phase, operation=command_role(argv),
                        predicate=str(error) if isinstance(error, Refusal) else type(error).__name__,
                        returncode=child.p.returncode if child is not None and hasattr(child, 'p') else None,
                        seconds=round(self.clock() - start, 6),
                        missing_glibc=sorted(set(v.decode('ascii') for v in re.findall(
                            rb"(GLIBC_[0-9]+\.[0-9]+)[^\r\n]{0,16}not found", bytes(child.out) if child else b''))))
            self.fail(error)
            raise
        finally:
            try:
                if child is not None and not child.reaped:
                    child.kill()
            except Exception as error:
                if self.first is None:
                    self.fail(Refusal('Cleanup'))
                self.fail(error)
                if failure is None:
                    raise
            finally:
                if observed is not None and child is not None:
                    observed['output'] = bytes(child.out)
                if not cleanup:
                    self.budget.used[phase] += self.clock() - start

    def case(self, node, mode, start, phase, output=None, error=None, **timings):
        self.case_results.add((node, mode))
        require(node in ('founder', 'learner1', 'learner2', 'unassigned') and mode in MODES, 'Arguments', 'case', "node in ('founder', 'learner1', 'learner2', 'unassigned') and mode in MODES")
        result = counts(output) if output is not None else {}
        if error is None:
            result = terminal(output)
            for facts in protocol_records(output):
                self.record('probe', node=node, mode=mode, facts=facts)
        self.record('case', node=node, mode=mode, phase=phase,
                    result='pass' if error is None else 'fail',
                    error=None if error is None else type(error).__name__,
                    counts=result, seconds=round(self.clock() - start, 6), **timings)

    def operational(self, container, mode, context=None, target=None, phase='probe', seconds=3):
        start, out = self.clock(), None
        observed = {}
        node = self.nodes.get(container, 'unassigned')
        try:
            out = self.command(exec_argv(container, mode, context, target), phase, seconds, observed=observed)
            terminal(out)
        except Exception as error:
            self.case(node, mode, start, phase, out if out is not None else observed.get('output'), error)
            raise
        self.case(node, mode, start, phase, out)

    def start_holder(self, argv, node='unassigned', mode='create-founder'):
        start = self.clock()
        self.holder_case = dict(node=node, mode=mode, start=start, phase='ready')
        self.holder_reap_start = None
        ready_deadline = self.budget.deadline('ready', 5)
        self.holder = self.spawn(self.bound_argv(argv), min(start + 30, self.budget.start + 130), self.clock)
        self.children.append(self.holder)
        try:
            while True:
                self.check_work()
                require(self.clock() < ready_deadline, 'ReadyTimeout', 'start_holder', 'self.clock() < ready_deadline')
                self.holder.alive()
                match = re.search(rb'^ARX_READY ([0-9]+) ([0-9]+),([0-9]+),([0-9]+) ([a-f0-9]{256})\r?$', self.holder.out, re.M)
                if match:
                    pid, *fds = map(int, match.groups()[:4])
                    require(2 <= pid <= 4194304 and all(3 <= fd <= 127 for fd in fds), 'Protocol', 'start_holder', '2 <= pid <= 4194304 and all((3 <= fd <= 127 for fd in fds))')
                    self.record('holder-ready', node=node, mode=mode, seconds=round(self.clock() - start, 6))
                    return match[5].decode(), (pid, fds)
                time.sleep(.01)
        finally:
            self.budget.used['ready'] += self.clock() - start

    def release(self, container):
        # A holder exit is legal only after deliberate release starts.
        self.check_work()
        self.holder.alive()
        self.releasing = True
        try:
            self.operational(container, 'release', phase='restart')
        finally:
            self.releasing = False
        start = self.clock()
        self.holder_reap_start = start
        deadline = min(self.budget.deadline('restart', 5), self.holder.deadline)
        try:
            while True:
                self.check_work()
                require(self.clock() < deadline, 'HolderTimeout', 'release', 'self.clock() < deadline')
                if self.holder.finish():
                    out = bytes(self.holder.out)
                    terminal(out)
                    if self.holder_case:
                        self.case(**self.holder_case, output=out,
                                  reap_seconds=round(self.clock() - start, 6),
                                  lifetime_seconds=round(self.clock() - self.holder_case['start'], 6))
                        self.holder_case = None
                    self.holder = None
                    return
                time.sleep(.01)
        finally:
            self.budget.used['restart'] += self.clock() - start

    def release_and_restart(self, container, role, public_context):
        self.release(container)
        self.operational(container, 'restart-' + role, public_context, phase='restart', seconds=5)

    def inspection(self, container, cleanup=False):
        out = self.command(['docker', 'inspect', container], 'evidence', cleanup=cleanup)
        data = json.loads(out)
        require(len(data) == 1 and data[0]['Id'] == container, 'UnknownCleanup', 'inspection', "len(data) == 1 and data[0]['Id'] == container")
        return data[0]

    def remove_node(self, container, cleanup=False):
        node = self.nodes[container]
        inspect_node(self.inspection(container, cleanup), self.run_id, self.image,
                     {f'/{slot}': str(self.root / 'fs' / f'{node}-{slot}') for slot in ('wrap', 'store')})
        self.command(['docker', 'rm', '--force', container], 'prepare', cleanup=cleanup)
        del self.nodes[container]

    def verify_loop(self, cleanup=False):
        data = json.loads(self.command(['sudo', '-n', 'losetup', '--json', '--list', '--output', 'NAME,BACK-FILE,MAJ:MIN', self.loop], 'prepare', cleanup=cleanup))['loopdevices']
        require(len(data) == 1 and data[0]['name'] == self.loop and data[0]['back-file'] == str(self.root / 'disk.img'), 'UnknownCleanup', 'verify_loop', "len(data) == 1 and data[0]['name'] == self.loop and (data[0]['back-file'] == str(self.root / 'disk.img'))")
        safe(self.root / 'disk.img', self.root, False)
        if self.mounted:
            mounted = json.loads(self.command(['findmnt', '--json', '--mountpoint', str(self.root / 'fs'), '--output', 'SOURCE,TARGET,FSTYPE,MAJ:MIN'], 'prepare', cleanup=cleanup))['filesystems']
            require(len(mounted) == 1 and mounted[0]['source'] == self.loop and mounted[0]['target'] == str(self.root / 'fs') and mounted[0]['fstype'] == 'ext4' and mounted[0]['maj:min'] == data[0]['maj:min'], 'UnknownCleanup', 'verify_loop', "len(mounted) == 1 and mounted[0]['source'] == self.loop and (mounted[0]['target'] == str(self.root / 'fs')) and (mounted[0]['fstype'] == 'ext4') and (mounted[0]['maj:min'] == data[0]['maj:min'])")

    def prepare_fixture(self):
        self.root.joinpath('fs').mkdir(mode=0o700)
        with open(self.root / 'disk.img', 'xb') as f:
            os.fchmod(f.fileno(), 0o600)
            f.truncate(64*MIB)
        self.command(['sudo', '-n', 'mkfs.ext4', '-q', '-F', '-m', '0', '-E', 'lazy_itable_init=0,lazy_journal_init=0', str(self.root / 'disk.img')], 'prepare')
        self.loop = self.command(['sudo', '-n', 'losetup', '--find', '--show', '--nooverlap', str(self.root / 'disk.img')], 'prepare').decode().strip()
        require(re.fullmatch(r'/dev/loop[0-9]{1,4}', self.loop), 'UnknownCleanup', 'prepare_fixture', "re.fullmatch('/dev/loop[0-9]{1,4}', self.loop)")
        self.verify_loop()
        # Mark before effect: a timed-out mount must still be attributed during cleanup.
        self.mounted = True
        self.command(['sudo', '-n', 'mount', '-t', 'ext4', '-o', 'nosuid,nodev,noexec', self.loop, str(self.root / 'fs')], 'prepare')
        self.verify_loop()
        for node in ('founder', 'learner1', 'learner2'):
            for slot in ('wrap', 'store'):
                path = str(self.root / 'fs' / f'{node}-{slot}')
                self.command(['sudo', '-n', 'mkdir', '-m', '0700', path], 'prepare')
                self.command(['sudo', '-n', 'chown', '21001:21001', path], 'prepare')

    def cleanup(self):
        start = self.clock()
        cleanup_result, cleanup_error = 'verified', None
        incomplete_case = self.holder_case
        incomplete_output = bytes(self.holder.out) if self.holder else None
        self.holder_case = None
        self.cleanup_deadline = min(self.clock() + 20, self.budget.start + 150)
        try:
            if self.pending_node:
                name, node = self.pending_node
                out = self.command(['docker', 'container', 'ls', '--all', '--no-trunc', '--filter', 'name=^/' + name + '$', '--format', '{{.ID}}'], 'prepare', cleanup=True)
                ids = out.decode().splitlines()
                require(len(ids) <= 1, 'UnknownCleanup', 'cleanup', 'len(ids) <= 1')
                if ids:
                    require(re.fullmatch(r'[a-f0-9]{64}', ids[0]), 'UnknownCleanup', 'cleanup', "re.fullmatch('[a-f0-9]{64}', ids[0])")
                    self.nodes[ids[0]] = node
                self.pending_node = None
            for container in list(self.nodes):
                self.remove_node(container, cleanup=True)
            for child in self.children:
                if not child.reaped:
                    child.kill()
            self.holder = None
            if self.root and not self.loop and (self.root / 'disk.img').exists():
                out = self.command(['sudo', '-n', 'losetup', '--json', '--associated', str(self.root / 'disk.img'), '--output', 'NAME,BACK-FILE,MAJ:MIN'], 'prepare', cleanup=True)
                loops = json.loads(out)['loopdevices']
                require(len(loops) <= 1, 'UnknownCleanup', 'cleanup', 'len(loops) <= 1')
                if loops:
                    self.loop = loops[0]['name']
                    require(re.fullmatch(r'/dev/loop[0-9]{1,4}', self.loop), 'UnknownCleanup', 'cleanup', "re.fullmatch('/dev/loop[0-9]{1,4}', self.loop)")
            if self.loop:
                self.verify_loop(cleanup=True)
                if self.mounted:
                    self.command(['sudo', '-n', 'umount', str(self.root / 'fs')], 'prepare', cleanup=True)
                    self.mounted = False
                self.verify_loop(cleanup=True)
                self.command(['sudo', '-n', 'losetup', '--detach', self.loop], 'prepare', cleanup=True)
                self.loop = None
            if self.root:
                safe(self.root)
                # Only these ordinary files and empty directories are attributable.
                for directory in (self.root / 'context', self.root / 'fs'):
                    if directory.exists():
                        safe(directory, self.root)
                        for path in directory.iterdir():
                            safe(path, directory, False)
                            require(path.name in ('Containerfile', 'arxd-tests'), 'UnknownCleanup', 'cleanup', "path.name in ('Containerfile', 'arxd-tests')")
                            path.unlink()
                        directory.rmdir()
                for path in self.root.iterdir():
                    safe(path, self.root, False)
                    require(path.name in ('disk.img', 'image.id'), 'UnknownCleanup', 'cleanup', "path.name in ('disk.img', 'image.id')")
                    path.unlink()
                self.root.rmdir()
        except Exception as error:
            self.fail(Refusal('Cleanup' if self.first is None else 'UnknownCleanup'))
            cleanup_result, cleanup_error = 'unknown', type(error).__name__
        finally:
            for child in self.children:
                if not child.reaped:
                    try:
                        child.kill()
                    except Exception as error:
                        self.fail(Refusal('UnknownCleanup'))
                        cleanup_result, cleanup_error = 'unknown', type(error).__name__
            self.cleanup_seconds = self.clock() - start
            if incomplete_case:
                self.case(**incomplete_case, output=incomplete_output, error=Refusal('IncompleteHolder'),
                          lifetime_seconds=round(start - incomplete_case['start'], 6),
                          reap_seconds=None if self.holder_reap_start is None else round(start - self.holder_reap_start, 6))
            self.record('cleanup', result=cleanup_result, error=cleanup_error, seconds=round(self.cleanup_seconds, 6))

    def run(self):
        token = DIAGNOSTIC.set(self.record)
        try:
            self.qualify()
        except Exception as error:
            if self.first is None:
                self.fail(error)
        finally:
            try:
                try:
                    self.record_scratch('pre-cleanup')
                except Exception as error:
                    self.fail(error)
                self.cleanup()
            finally:
                for tool in (self.tools or {}).values():
                    try:
                        tool.close()
                    except Refusal as error:
                        self.fail(error)
        DIAGNOSTIC.reset(token)
        for node in ('founder', 'learner1', 'learner2'):
            role = 'founder' if node == 'founder' else 'learner'
            modes = ['create-' + role, 'deny-build', 'release', 'restart-' + role]
            if node == 'founder':
                modes.append('same-uid-control')
            for mode in modes:
                if (node, mode) not in self.case_results:
                    self.record('not-run', node=node, mode=mode, reason='upstream-refusal')
        self.record('result', first_failure=self.first, secondary=self.secondary,
                    seconds=round(self.clock() - self.budget.start, 3), phases={**self.budget.used, 'cleanup': self.cleanup_seconds})
        return 1 if self.first else 0

    def qualify_tools(self, fs=os):
        self.tools = {}
        batch = {}
        statuses = []
        complete = False
        start = self.clock()
        try:
            deadline = self.budget.deadline('preflight', 20)
            hardlinks = False
            try:
                require(self.clock() < deadline, 'PhaseTimeout', 'qualify_tools', 'self.clock() < deadline')
                protected_hardlinks(self.record, fs)
                hardlinks = True
            except Refusal as error:
                self.fail(error)
            for tool in TOOLS:
                status = 'refused'
                try:
                    if self.clock() >= deadline or self.evidence >= LIMIT - 4096:
                        status = 'not-evaluated'
                        raise Refusal('PhaseTimeout' if self.clock() >= deadline else 'EvidenceOverflow')
                    batch[tool] = trusted_tool(tool, self.record, fs,
                        entry=sys.executable if tool == 'python3' else None, hardlinks=hardlinks)
                    require(self.clock() < deadline, 'PhaseTimeout', 'qualify_tools', 'self.clock() < deadline')
                    if tool == 'python3':
                        require(sys.version_info >= (3, 11), 'PythonVersion', 'qualify_tools', 'sys.version_info >= (3, 11)')
                    status = 'validated'
                except Refusal as error:
                    self.fail(error)
                statuses.append(status)
                self.record('tool-terminal', tool=tool, status=status)
            if sys.version_info < (3, 11) and self.first != 'PythonVersion':
                self.fail(Refusal('PythonVersion'))
            self.record('interpreter', version=list(sys.version_info[:3]),
                        bootstrap='trusted-ci', version_ok=sys.version_info >= (3, 11),
                        resolved_path=batch['python3'].path if 'python3' in batch else None)
            complete = True
        finally:
            self.budget.used['preflight'] += self.clock() - start
            if self.first is not None or not complete:
                for obj in batch.values():
                    try:
                        obj.close()
                    except Refusal as error:
                        self.fail(error)
            else:
                self.tools = batch
        self.record('tool-batch', status='refused' if self.first else 'validated',
                    required=len(TOOLS), validated=statuses.count('validated'),
                    refused=statuses.count('refused'), not_evaluated=statuses.count('not-evaluated'),
                    first_failure=self.first)
        require(self.first is None, self.first, 'qualify_tools', 'self.first is None')

    def qualify(self):
        require(len(sys.argv) == 1 and sys.platform == 'linux' and platform.machine() == 'x86_64' and
                os.environ.get('GITHUB_ACTIONS') == 'true' and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted', 'HostedOnly', 'qualify', "len(sys.argv) == 1 and sys.platform == 'linux' and (platform.machine() == 'x86_64') and (os.environ.get('GITHUB_ACTIONS') == 'true') and (os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted')")
        self.qualify_tools()
        self.command(['python3', '-I', '-B', '-m', 'unittest', 'discover', '-s', str(Path(__file__).parent), '-p', 'test_run.py'], 'preflight', 20)
        tmp = Path(os.environ['RUNNER_TEMP'])
        safe(tmp)
        # RUNNER_TEMP only. Staged context is at most 257 MiB and the ext4 fixture
        # is 64 MiB, 321 MiB together. 1 GiB free covers that scratch. Docker stores
        # the image on its data root, which this check does not measure; image Size
        # is capped separately at 512 MiB and is not added into this 1 GiB figure.
        require(shutil.disk_usage(tmp).free >= 1024*MIB, 'DiskCapacity', 'qualify', 'shutil.disk_usage(tmp).free >= 1024 * MIB')
        self.root = Path(tempfile.mkdtemp(prefix='arx-custody-', dir=tmp))
        os.chmod(self.root, 0o700)
        safe(self.root, tmp)
        context = self.root / 'context'
        context.mkdir(mode=0o700)
        self.scratch = dict(samples=0, peak_allocated_bytes=0, peak_ext4_used_bytes=0)
        self.record_scratch('root')
        source = self.command(['git', 'rev-parse', 'HEAD'], 'preflight').decode().strip()
        require(re.fullmatch(r'[a-f0-9]{40}', source), 'SourcePin', 'qualify', "re.fullmatch('[a-f0-9]{40}', source)")
        # Coordinator must persist adoption before hosted execution.
        # PINS is amendment A4, the applicable revision. ADOPTED_PINS is the
        # 2026-11-20 brief digest and stays in the ADR as the historical pin.
        note = Path('docs/adr/0037-acceptance-custody.md').read_text()
        require(ADOPTED_PINS in note and PINS in note, 'MissingAdoptedPins', 'qualify', 'ADOPTED_PINS in note and PINS in note')
        self.discover_and_stage(source, context)
        self.record_scratch('staged')
        self.command(['docker', 'build', '--platform', 'linux/amd64', '--network', 'none', '--target', 'custody-runtime', '--file', str(context / 'Containerfile'), '--iidfile', str(self.root / 'image.id'), str(context)], 'image', 40)
        self.image = (self.root / 'image.id').read_text().strip()
        require(re.fullmatch(r'sha256:[a-f0-9]{64}', self.image), 'ImageIdentity', 'qualify', "re.fullmatch('sha256:[a-f0-9]{64}', self.image)")
        image = json.loads(self.command(['docker', 'image', 'inspect', self.image], 'image'))[0]
        # image['Size'] is Docker's inspect-reported image size. A packed driver can
        # report less than the candidate (hosted evidence: context 84,499,396 bytes,
        # Size 50,089,568). An unpacked driver adds the pinned Ubuntu base to the
        # copied candidate. The candidate cap is 256 MiB; 512 MiB leaves 256 MiB
        # for that base and either accounting. The pinned base layer is 29,793,297
        # bytes compressed.
        require(image['Id'] == self.image and image['Architecture'] == 'amd64' and image['Os'] == 'linux' and image['Size'] <= 512*MIB, 'ImageIdentity', 'qualify', "image['Id'] == self.image and image['Architecture'] == 'amd64' and (image['Os'] == 'linux') and (image['Size'] <= 512 * MIB)")
        self.record('image', identity=self.image, bytes=image['Size'])
        self.record_scratch('image')
        self.run_nodes()

    def discover_and_stage(self, source, context):
        start, used = self.clock(), self.budget.used['preflight']
        def checkpoint():
            self.budget.used['preflight'] = used + self.clock() - start
            self.budget.deadline('preflight', 20)
        try:
            checkpoint()
            self._discover_and_stage(source, context, checkpoint)
            checkpoint()
        finally:
            # Include filesystem/hash/copy work once, together with commands.
            self.budget.used['preflight'] = used + self.clock() - start

    def _discover_and_stage(self, source, context, checkpoint):
        checkout = Path.cwd()
        safe(checkout, subject='checkout')
        target = Path(os.environ['CARGO_TARGET_DIR'])
        if not target.is_absolute():
            target = Path.cwd() / target
        deps = target / 'debug' / 'deps'
        safe(deps, subject='deps')
        candidates = []
        failed = None
        with ExitStack() as stack:
            objects = {}
            for path in deps.iterdir():
                if not path.name.startswith('arxd-') or not os.access(path, os.X_OK):
                    continue
                require(len(objects) < 16, 'CandidateBounds', 'candidate-set', 'count_16')
                subject = 'candidate-' + str(len(objects))
                objects[path] = None
                try:
                    obj = stack.enter_context(BuildInput(path, deps, subject, self.record, checkpoint))
                    objects[path] = obj
                    os.lseek(obj.fd, 0, os.SEEK_SET)
                    if os.read(obj.fd, 4) != b'\x7fELF':
                        continue
                    started = self.clock()
                    listing, returncode = self.command([str(path), '--list', '--exact', PROBE], 'preflight', 2, discovery=True)
                    obj.verify()
                    diagnostic = 'empty' if not listing else 'glibc-missing' if re.search(
                        rb'GLIBC_[0-9]+\.[0-9]+[^\r\n]{0,16}not found', listing) else 'other'
                    self.record('candidate-list', candidate=len(objects)-1, returncode=returncode,
                                seconds=round(self.clock() - started, 6), diagnostic=diagnostic)
                    require(returncode == 0 or (PROBE + ': test').encode() not in listing.splitlines(),
                            'ProbeSelection', subject, 'nonzero_probe_record_conflict')
                    if returncode == 0:
                        candidates.append((path, listing, obj.sha, obj.sha))
                except (Refusal, OSError) as error:
                    if failed is None:
                        failed = error
                    if not isinstance(error, Refusal):
                        self.record('refusal', subject=subject, predicate='input_filesystem_error', code=type(error).__name__)
            if failed is not None:
                raise failed
            selected = objects[choose(candidates)]
            selected.stage(context / 'arxd-tests')
            inputs = {}
            for relative in ('tools/ci/custody/Containerfile', 'tools/ci/custody/run.py',
                             'crates/arxd/src/cluster/pki/custody/hosted_probe.rs', '.github/workflows/ci.yml'):
                with BuildInput(checkout / relative, checkout, relative, self.record, checkpoint) as obj:
                    blob = self.command(['git', 'show', source + ':' + relative], 'preflight')
                    require(hashlib.sha256(blob).hexdigest() == obj.sha, 'SourcePin', relative, 'immutable_git_blob')
                    if relative.endswith('/Containerfile'):
                        obj.stage(context / 'Containerfile')
                    obj.verify()
                    inputs[relative] = obj.sha
            selected.verify()
            require(sum(p.stat().st_size for p in context.iterdir()) <= 257*MIB, 'ContextCapacity', 'discover_and_stage', 'sum((p.stat().st_size for p in context.iterdir())) <= 257 * MIB')
            elf = self.command(['readelf', '--program-headers', '--dynamic', '--version-info', str(context / 'arxd-tests')], 'preflight')
            require(b'/lib64/ld-linux-x86-64.so.2' in elf and b'NEEDED' in elf, 'ElfABI', 'discover_and_stage', "b'/lib64/ld-linux-x86-64.so.2' in elf and b'NEEDED' in elf")
            versions = sorted(set(v.decode('ascii') for v in re.findall(rb'GLIBC_[0-9]+\.[0-9]+', elf)))
            self.record('elf-abi', required_glibc=versions)
            self.record('inputs', source=source, hashes=inputs, executable=selected.sha, pins=PINS, adopted_pins=ADOPTED_PINS)

    def run_nodes(self):
        self.prepare_fixture()
        self.record_scratch('fixture')
        founder_context = None
        identities = set()
        for node in ('founder', 'learner1', 'learner2'):
            for slot in ('wrap', 'store'):
                m = (self.root / 'fs' / f'{node}-{slot}').stat()
                require((m.st_dev, m.st_ino) not in identities and m.st_uid == 21001 and stat.S_IMODE(m.st_mode) == 0o700, 'MountOwnership', 'run_nodes', '(m.st_dev, m.st_ino) not in identities and m.st_uid == 21001 and (stat.S_IMODE(m.st_mode) == 448)')
                identities.add((m.st_dev, m.st_ino))
            name = f'arx-{self.run_id}-{node}'
            self.pending_node = (name, node)
            container = self.command(create_argv(name, self.run_id, self.root, node, self.image), 'prepare').decode().strip()
            require(re.fullmatch(r'[a-f0-9]{64}', container), 'UnknownCleanup', 'run_nodes', "re.fullmatch('[a-f0-9]{64}', container)")
            self.nodes[container] = node
            self.pending_node = None
            inspect_node(self.inspection(container), self.run_id, self.image, {f'/{s}': str(self.root / 'fs' / f'{node}-{s}') for s in ('wrap', 'store')})
            self.command(['docker', 'start', container], 'prepare')
            try:
                listing = self.command(['docker', 'exec', '--user', '21001:21001', '--workdir', '/', container, '/usr/bin/env', '-i', 'PATH=/usr/bin:/bin', '/opt/arx/arxd-tests', '--list', '--exact', PROBE], 'image')
            except Refusal:
                # PID1 may itself fail in the loader before exec is possible.
                # This fixed pre-key diagnostic never forwards container text.
                logs = self.command(['docker', 'logs', container], 'image')
                self.record('image-loader', missing_glibc=sorted(set(v.decode('ascii') for v in re.findall(
                    rb"(GLIBC_[0-9]+\.[0-9]+)[^\r\n]{0,16}not found", logs))))
                raise
            require(exact_listing(listing), 'ImageABI', 'run_nodes', 'exact_listing(listing)')
            role = 'founder' if node == 'founder' else 'learner'
            public_context, target = self.start_holder(exec_argv(container, 'create-' + role, founder_context), node, 'create-' + role)
            self.record_scratch('ready', node)
            if node == 'founder':
                founder_context = public_context
            self.operational(container, 'deny-build', target=target)
            if node == 'founder':
                self.operational(container, 'same-uid-control', public_context)
            self.record_scratch('probe', node)
            self.release_and_restart(container, role, public_context)
            self.record_scratch('restart', node)
            self.remove_node(container)
            self.record_scratch('node-removed', node)


def main():
    harness = Harness()
    def interrupted(_signum, _frame):
        raise Refusal('Interrupted')
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    return harness.run()


if __name__ == '__main__':
    sys.exit(main())
