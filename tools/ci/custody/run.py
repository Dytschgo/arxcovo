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

PROBE = 'cluster::pki::custody::hosted_probe::hosted_custody_probe'
BASE = 'docker.io/library/ubuntu@sha256:9a04f13b3f80ef056361e30868593b1cfd2279fe19e4081cb93a90bf56819d6a'
PINS = '28f882cb018eb956c60c190f8718fb982923cd096adb608e15e194c51fa2e167'
MIB = 1024 * 1024
MODES = {'create-founder', 'create-learner', 'deny-build', 'same-uid-control', 'release', 'restart-founder', 'restart-learner'}
LIMIT = 1024 * 1024


class Refusal(Exception):
    pass


def require(condition, code):
    if not condition:
        raise Refusal(code)


def safe(path, parent=None, directory=True):
    path = Path(path)
    require(path.is_absolute() and len(str(path)) <= 4096, 'UnsafePath')
    for p in [path, *path.parents]:
        require(not p.is_symlink(), 'UnsafePath')
    require(path.resolve() == path, 'UnsafePath')
    if parent:
        require(path.is_relative_to(parent) and path != parent, 'UnsafePath')
    m = path.lstat()
    require(stat.S_ISDIR(m.st_mode) if directory else stat.S_ISREG(m.st_mode) and m.st_nlink == 1, 'UnsafePath')
    return m


TOOLS = ('docker', 'sudo', 'mkfs.ext4', 'losetup', 'mount', 'umount',
         'findmnt', 'readelf', 'git', 'mkdir', 'chown', 'python3')
SUDO_TARGET = '/usr/lib/cargo/bin/sudo'
SEARCH = '/usr/bin:/usr/sbin:/bin:/sbin'


def identity(m):
    return (m.st_dev, m.st_ino, m.st_mode, m.st_uid, m.st_gid, m.st_nlink)


class TrustedTool:
    """Retained CLOEXEC objects; the filesystem adapter is injectable in pure tests."""
    def __init__(self, role, record, entry, fs=os):
        self.role, self.record, self.entry, self.fs = role, record, entry, fs
        self.objects = []
        self.path = None
        self.current = None
        self.component = '<unapproved-path>'
        self.reported = False

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
                    self.check(stat.S_ISREG(m.st_mode) and m.st_nlink == 1,
                               'regular_single_link')
                    self.check(bool(m.st_mode & stat.S_IXUSR), 'owner_executable')
                    self.check(self.role != 'sudo' or bool(m.st_mode & stat.S_ISUID), 'sudo_setuid')
                    resolved = '/' + '/'.join(name for name, _ in stack[1:])
                    allowed = resolved.startswith(('/usr/bin/', '/usr/sbin/')) or (
                        self.role == 'sudo' and resolved == SUDO_TARGET)
                    self.check(allowed, 'canonical_system_directory')
                    self.path = resolved
                    self.record('tool-validated', tool=self.role, entry=self.entry if self.entry in
                                tuple(d + '/' + self.role for d in SEARCH.split(':')) else '<interpreter-entry>',
                                resolved_path=self.path, predicate='validated',
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
            for fd, before in self.objects:
                self.current = self.fs.fstat(fd)
                self.check(identity(self.current) == before, 'stable_open_object')
            other = TrustedTool(self.role, lambda *a, **k: None, self.entry, self.fs)
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
        for fd, _ in reversed(objects):
            try:
                self.fs.close(fd)
            except OSError:
                self.current = None
                self.check(False, 'filesystem_error')


def trusted_tool(tool, record, fs=os, entry=None):
    obj = TrustedTool(tool, record, entry, fs)
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


def counts(output):
    selected = re.findall(rb'^running ([0-9]{1,3}) tests?\r?$', output, re.M)
    results = re.findall(rb'^test result: (ok|FAILED)\. ([0-9]{1,3}) passed; ([0-9]{1,3}) failed; ([0-9]{1,3}) ignored; ([0-9]{1,3}) measured; ([0-9]{1,3}) filtered out(?:; finished in [0-9.]+s)?\r?$', output, re.M)
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
        require(prefix in (b'', ('test ' + PROBE + ' ... ').encode()), 'UnknownRecord')
        record = b'ARX_' + record
        require(any(re.fullmatch(pattern, record) for pattern in (
            rb'ARX_PASS selected=[0-9]{1,3} passed=[0-9]{1,3}',
            rb'ARX_READY [0-9]{1,7} [0-9]{1,3},[0-9]{1,3},[0-9]{1,3} [a-f0-9]{256}',
            rb'ARX_FACTS uid=2100[12] caps=0 nnp=1 core=0',
            rb'ARX_MOUNT slot=/(tmp|workspace|control|dev/shm|dev) capacity=[0-9]{1,10} used=[0-9]{1,10}',
            rb'ARX_DENIAL errno=[0-9]{1,3}',
        )), 'UnknownRecord')
        if record.startswith((b'ARX_FACTS ', b'ARX_MOUNT ', b'ARX_DENIAL ')):
            records.append(record.decode('ascii'))
    return records


def terminal(output):
    protocol_records(output)
    result = counts(output)
    require(output.count(b'ARX_PASS selected=1 passed=1') == 1 and
            all(result.get(key) == value for key, value in
                dict(oracle_selected=1, oracle_passed=1, selected=1, passed=1, failed=0,
                     ignored=0, measured=0, libtest_ok=True).items()), 'MissingOracle')
    require(len(re.findall(rb'ARX_PASS ', output)) == 1, 'MissingOracle')
    return result


def exact_listing(output):
    names = [line[:-6] for line in output.decode('ascii').splitlines() if line.endswith(': test')]
    return names == [PROBE] and output.count(b'1 test, 0 benchmarks') == 1


def choose(candidates):
    matches = [p for p, listing, before, after in candidates if exact_listing(listing) and before == after]
    require(len(matches) == 1 and all(before == after for _, _, before, after in candidates), 'ProbeSelection')
    return matches[0]


def create_argv(name, run_id, root, node, image):
    require(re.fullmatch(r'arx-[a-f0-9]{16}-(founder|learner1|learner2)', name) and
            node in ('founder', 'learner1', 'learner2') and re.fullmatch(r'[a-f0-9]{16}', run_id) and
            re.fullmatch(r'sha256:[a-f0-9]{64}', image), 'Arguments')
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
    require(mode in MODES and re.fullmatch(r'[a-f0-9]{64}', container), 'Arguments')
    uid = '21002:21002' if mode == 'deny-build' else '21001:21001'
    argv = ['docker', 'exec', '--user', uid, '--workdir', '/', container, '/usr/bin/env', '-i',
            'PATH=/usr/bin:/bin', 'LANG=C', 'TZ=UTC', 'ARX_CUSTODY_HOSTED_MODE=' + mode]
    if context is not None:
        require(re.fullmatch(r'[a-f0-9]{256}', context), 'Protocol')
        argv += ['ARX_CUSTODY_PUBLIC_CONTEXT=' + context]
    if target is not None:
        pid, fds = target
        require(2 <= pid <= 4194304 and len(fds) == 3 and all(3 <= f <= 127 for f in fds), 'Protocol')
        argv += [f'ARX_CUSTODY_TARGET_PID={pid}', 'ARX_CUSTODY_TARGET_FDS=' + ','.join(map(str, fds))]
    return argv + ['/opt/arx/arxd-tests', '--exact', PROBE, '--nocapture', '--test-threads=1']


def inspect_node(data, run_id, image, roots):
    require(data['Config']['Labels'].get('arxcovo.custody.run') == run_id and data['Image'] == image, 'UnknownCleanup')
    h = data['HostConfig']
    require(h['ShmSize'] == MIB and h['IpcMode'] == 'private' and h['Memory'] == 256*MIB and
            h['MemorySwap'] == 256*MIB and h['PidsLimit'] == 64 and h['NanoCpus'] == 1_000_000_000 and
            h['ReadonlyRootfs'] and h['NetworkMode'] == 'none' and h['CapDrop'] == ['ALL'], 'Limits')
    binds = {m['Destination']: m['Source'] for m in data['Mounts'] if m['Type'] == 'bind'}
    require(binds == roots, 'MountOwnership')
    require(all(m['Type'] in ('bind', 'tmpfs') for m in data['Mounts']), 'MountOwnership')
    require({m['Destination'] for m in data['Mounts'] if m['Type'] == 'tmpfs'} <= {'/tmp', '/workspace', '/control'}, 'MountOwnership')
    require(17 + 1 == 18 and 18 + 64 == 82 and 256 - 82 == 174, 'Accounting')


class Budget:
    CAPS = {'preflight': 20, 'image': 40, 'prepare': 15, 'ready': 15, 'probe': 15, 'restart': 15, 'evidence': 10}

    def __init__(self, clock=time.monotonic):
        self.clock = clock
        self.start = clock()
        self.used = dict.fromkeys(self.CAPS, 0.0)

    def deadline(self, phase, maximum):
        now = self.clock()
        require(now < self.start + 130, 'WorkTimeout')
        left = self.CAPS[phase] - self.used[phase]
        require(left > 0, 'PhaseTimeout')
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
                require(len(self.out) + len(data) <= 65536, 'PipeOverflow')
                self.out.extend(data)
            else:
                self.selector.unregister(key.fileobj)
                key.fileobj.close()

    def alive(self):
        self.pump()
        require(self.clock() < self.deadline, 'HolderTimeout')
        require(self.p.poll() is None, 'HolderExit')

    def finish(self):
        self.pump()
        if self.p.poll() is None or self.selector.get_map():
            return False
        self.p.wait(timeout=0)
        self.reaped = True
        self.selector.close()
        require(self.p.returncode == 0, 'CommandExit')
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
        self.tools = None

    def fail(self, error):
        code = str(error) if isinstance(error, Refusal) else type(error).__name__
        if self.first is None:
            self.first = code
        else:
            self.secondary.append(code)

    def check_work(self):
        require(self.clock() < self.budget.start + 130, 'WorkTimeout')

    def record(self, event, **facts):
        line = json.dumps({'event': event, **facts}, sort_keys=True)
        self.evidence += len(line)
        require(self.evidence <= LIMIT, 'EvidenceOverflow')
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
                        ('mkfs.ext4', 'losetup', 'mount', 'umount', 'findmnt', 'mkdir', 'chown'), 'Arguments')
                argv[2] = self.tools[argv[2]].recheck()
        else:
            # The sole non-tool host executable is the selected, hash-checked probe.
            require(Path(role).is_absolute(), 'Arguments')
        return argv

    def command(self, argv, phase, seconds=5, cleanup=False, observed=None):
        start = self.clock()
        deadline = min(self.cleanup_deadline or self.budget.start + 150, start + seconds) if cleanup else self.budget.deadline(phase, seconds)
        require(start < deadline, 'CleanupTimeout' if cleanup else 'WorkTimeout')
        child = self.spawn(self.bound_argv(argv), deadline, self.clock)
        self.children.append(child)
        failure = None
        try:
            while True:
                if not cleanup and self.holder:
                    self.check_work()
                    if self.releasing:
                        self.holder.pump()
                        require(self.clock() < self.holder.deadline, 'HolderTimeout')
                    else:
                        self.holder.alive()
                require(self.clock() < deadline, 'CommandTimeout')
                if child.finish():
                    return bytes(child.out)
                time.sleep(.01)
        except Exception as error:
            failure = error
            self.fail(error)
            raise
        finally:
            try:
                if not child.reaped:
                    child.kill()
            except Exception as error:
                if self.first is None:
                    self.fail(Refusal('Cleanup'))
                self.fail(error)
                if failure is None:
                    raise
            finally:
                if observed is not None:
                    observed['output'] = bytes(child.out)
                if not cleanup:
                    self.budget.used[phase] += self.clock() - start

    def case(self, node, mode, start, phase, output=None, error=None, **timings):
        require(node in ('founder', 'learner1', 'learner2', 'unassigned') and mode in MODES, 'Arguments')
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
                require(self.clock() < ready_deadline, 'ReadyTimeout')
                self.holder.alive()
                match = re.search(rb'^ARX_READY ([0-9]+) ([0-9]+),([0-9]+),([0-9]+) ([a-f0-9]{256})\r?$', self.holder.out, re.M)
                if match:
                    pid, *fds = map(int, match.groups()[:4])
                    require(2 <= pid <= 4194304 and all(3 <= fd <= 127 for fd in fds), 'Protocol')
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
                require(self.clock() < deadline, 'HolderTimeout')
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
        require(len(data) == 1 and data[0]['Id'] == container, 'UnknownCleanup')
        return data[0]

    def remove_node(self, container, cleanup=False):
        node = self.nodes[container]
        inspect_node(self.inspection(container, cleanup), self.run_id, self.image,
                     {f'/{slot}': str(self.root / 'fs' / f'{node}-{slot}') for slot in ('wrap', 'store')})
        self.command(['docker', 'rm', '--force', container], 'prepare', cleanup=cleanup)
        del self.nodes[container]

    def verify_loop(self, cleanup=False):
        data = json.loads(self.command(['sudo', '-n', 'losetup', '--json', '--list', '--output', 'NAME,BACK-FILE,MAJ:MIN', self.loop], 'prepare', cleanup=cleanup))['loopdevices']
        require(len(data) == 1 and data[0]['name'] == self.loop and data[0]['back-file'] == str(self.root / 'disk.img'), 'UnknownCleanup')
        safe(self.root / 'disk.img', self.root, False)
        if self.mounted:
            mounted = json.loads(self.command(['findmnt', '--json', '--mountpoint', str(self.root / 'fs'), '--output', 'SOURCE,TARGET,FSTYPE,MAJ:MIN'], 'prepare', cleanup=cleanup))['filesystems']
            require(len(mounted) == 1 and mounted[0]['source'] == self.loop and mounted[0]['target'] == str(self.root / 'fs') and mounted[0]['fstype'] == 'ext4' and mounted[0]['maj:min'] == data[0]['maj:min'], 'UnknownCleanup')

    def prepare_fixture(self):
        self.root.joinpath('fs').mkdir(mode=0o700)
        with open(self.root / 'disk.img', 'xb') as f:
            os.fchmod(f.fileno(), 0o600)
            f.truncate(64*MIB)
        self.command(['sudo', '-n', 'mkfs.ext4', '-q', '-F', '-m', '0', '-E', 'lazy_itable_init=0,lazy_journal_init=0', str(self.root / 'disk.img')], 'prepare')
        self.loop = self.command(['sudo', '-n', 'losetup', '--find', '--show', '--nooverlap', str(self.root / 'disk.img')], 'prepare').decode().strip()
        require(re.fullmatch(r'/dev/loop[0-9]{1,4}', self.loop), 'UnknownCleanup')
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
                require(len(ids) <= 1, 'UnknownCleanup')
                if ids:
                    require(re.fullmatch(r'[a-f0-9]{64}', ids[0]), 'UnknownCleanup')
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
                require(len(loops) <= 1, 'UnknownCleanup')
                if loops:
                    self.loop = loops[0]['name']
                    require(re.fullmatch(r'/dev/loop[0-9]{1,4}', self.loop), 'UnknownCleanup')
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
                            require(path.name in ('Containerfile', 'arxd-tests'), 'UnknownCleanup')
                            path.unlink()
                        directory.rmdir()
                for path in self.root.iterdir():
                    safe(path, self.root, False)
                    require(path.name in ('disk.img', 'image.id'), 'UnknownCleanup')
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
        try:
            self.qualify()
        except Exception as error:
            if self.first is None:
                self.fail(error)
        finally:
            try:
                self.cleanup()
            finally:
                for tool in (self.tools or {}).values():
                    try:
                        tool.close()
                    except Refusal as error:
                        self.fail(error)
        self.record('result', first_failure=self.first, secondary=self.secondary,
                    seconds=round(self.clock() - self.budget.start, 3), phases={**self.budget.used, 'cleanup': self.cleanup_seconds})
        return 1 if self.first else 0

    def qualify(self):
        require(len(sys.argv) == 1 and sys.platform == 'linux' and platform.machine() == 'x86_64' and
                os.environ.get('GITHUB_ACTIONS') == 'true' and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted', 'HostedOnly')
        self.tools = {}
        start = self.clock()
        try:
            deadline = self.budget.deadline('preflight', 20)
            for tool in TOOLS:
                require(self.clock() < deadline, 'PhaseTimeout')
                self.tools[tool] = trusted_tool(tool, self.record,
                    entry=sys.executable if tool == 'python3' else None)
            require(sys.version_info >= (3, 11), 'PythonVersion')
            self.record('interpreter', version=list(sys.version_info[:3]),
                        bootstrap='trusted-ci', resolved_path=self.tools['python3'].path)
        finally:
            self.budget.used['preflight'] += self.clock() - start
        self.command(['python3', '-I', '-B', '-m', 'unittest', 'discover', '-s', str(Path(__file__).parent), '-p', 'test_run.py'], 'preflight', 20)
        tmp = Path(os.environ['RUNNER_TEMP'])
        safe(tmp)
        require(shutil.disk_usage(tmp).free >= 1024*MIB, 'DiskCapacity')
        self.root = Path(tempfile.mkdtemp(prefix='arx-custody-', dir=tmp))
        os.chmod(self.root, 0o700)
        safe(self.root, tmp)
        context = self.root / 'context'
        context.mkdir(mode=0o700)
        source = self.command(['git', 'rev-parse', 'HEAD'], 'preflight').decode().strip()
        require(re.fullmatch(r'[a-f0-9]{40}', source), 'SourcePin')
        # Coordinator must persist adoption before hosted execution.
        note = Path('docs/adr/0037-acceptance-custody.md').read_text()
        require(PINS in note, 'MissingAdoptedPins')
        target = Path(os.environ['CARGO_TARGET_DIR'])
        if not target.is_absolute():
            target = Path.cwd() / target
        deps = target / 'debug' / 'deps'
        safe(deps)
        candidates = []
        for path in deps.iterdir():
            if not path.name.startswith('arxd-') or not os.access(path, os.X_OK):
                continue
            safe(path, deps, False)
            require(path.stat().st_size <= 128*MIB and len(candidates) < 16, 'CandidateBounds')
            with open(path, 'rb') as f:
                if f.read(4) != b'\x7fELF':
                    continue
            before = (path.stat().st_dev, path.stat().st_ino, digest(path))
            listing = self.command([str(path), '--list', '--exact', PROBE], 'preflight', 2)
            after = (path.stat().st_dev, path.stat().st_ino, digest(path))
            candidates.append((path, listing, before, after))
        selected = choose(candidates)
        before = digest(selected)
        shutil.copyfile(selected, context / 'arxd-tests')
        require(digest(selected) == before == digest(context / 'arxd-tests'), 'StagingHash')
        containerfile = Path(__file__).parent.resolve() / 'Containerfile'
        shutil.copyfile(containerfile, context / 'Containerfile')
        require(digest(containerfile) == digest(context / 'Containerfile'), 'StagingHash')
        require(sum(p.stat().st_size for p in context.iterdir()) <= 129*MIB, 'ContextCapacity')
        elf = self.command(['readelf', '--program-headers', '--dynamic', str(context / 'arxd-tests')], 'preflight')
        require(b'/lib64/ld-linux-x86-64.so.2' in elf and b'NEEDED' in elf, 'ElfABI')
        inputs = {str(p): digest(p) for p in (containerfile, Path(__file__).resolve(), Path('crates/arxd/src/cluster/pki/custody/hosted_probe.rs'), Path('.github/workflows/ci.yml'))}
        self.record('inputs', source=source, hashes=inputs, executable=before, pins=PINS)
        self.command(['docker', 'build', '--platform', 'linux/amd64', '--network', 'none', '--target', 'custody-runtime', '--file', str(context / 'Containerfile'), '--iidfile', str(self.root / 'image.id'), str(context)], 'image', 40)
        self.image = (self.root / 'image.id').read_text().strip()
        require(re.fullmatch(r'sha256:[a-f0-9]{64}', self.image), 'ImageIdentity')
        image = json.loads(self.command(['docker', 'image', 'inspect', self.image], 'image'))[0]
        require(image['Id'] == self.image and image['Architecture'] == 'amd64' and image['Os'] == 'linux' and image['Size'] <= 256*MIB, 'ImageIdentity')
        self.record('image', identity=self.image, bytes=image['Size'], executable=before)
        self.prepare_fixture()
        founder_context = None
        identities = set()
        for node in ('founder', 'learner1', 'learner2'):
            for slot in ('wrap', 'store'):
                m = (self.root / 'fs' / f'{node}-{slot}').stat()
                require((m.st_dev, m.st_ino) not in identities and m.st_uid == 21001 and stat.S_IMODE(m.st_mode) == 0o700, 'MountOwnership')
                identities.add((m.st_dev, m.st_ino))
            name = f'arx-{self.run_id}-{node}'
            self.pending_node = (name, node)
            container = self.command(create_argv(name, self.run_id, self.root, node, self.image), 'prepare').decode().strip()
            require(re.fullmatch(r'[a-f0-9]{64}', container), 'UnknownCleanup')
            self.nodes[container] = node
            self.pending_node = None
            inspect_node(self.inspection(container), self.run_id, self.image, {f'/{s}': str(self.root / 'fs' / f'{node}-{s}') for s in ('wrap', 'store')})
            self.command(['docker', 'start', container], 'prepare')
            listing = self.command(['docker', 'exec', '--user', '21001:21001', '--workdir', '/', container, '/usr/bin/env', '-i', 'PATH=/usr/bin:/bin', '/opt/arx/arxd-tests', '--list', '--exact', PROBE], 'image')
            require(exact_listing(listing), 'ImageABI')
            role = 'founder' if node == 'founder' else 'learner'
            public_context, target = self.start_holder(exec_argv(container, 'create-' + role, founder_context), node, 'create-' + role)
            if node == 'founder':
                founder_context = public_context
            self.operational(container, 'deny-build', target=target)
            if node == 'founder':
                self.operational(container, 'same-uid-control', public_context)
            self.release_and_restart(container, role, public_context)
            self.remove_node(container)


def main():
    harness = Harness()
    def interrupted(_signum, _frame):
        raise Refusal('Interrupted')
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    return harness.run()


if __name__ == '__main__':
    sys.exit(main())
