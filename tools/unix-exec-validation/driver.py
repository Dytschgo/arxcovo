"""Manual disposable CI only. Import and guard/planning functions have no effects."""
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import time
import threading

TEST = 'exec::disposable::disposable_linux_cross_account'
TOOLCHAIN = '1.95.0-x86_64-unknown-linux-gnu'
SAFE_ENV = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C.UTF-8'}


def require(ok, code):
    if not ok:
        raise RuntimeError(code)


def identity(env):
    for key in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT'):
        require(re.fullmatch(r'[1-9][0-9]{0,19}', env.get(key, '')), 'E_ID')
    return env['GITHUB_RUN_ID'] + '-' + env['GITHUB_RUN_ATTEMPT']


def gate(env, platform):
    require(platform == 'linux', 'E_PLATFORM')
    expected = {'CI': 'true', 'GITHUB_ACTIONS': 'true',
                'RUNNER_ENVIRONMENT': 'github-hosted', 'RUNNER_OS': 'Linux',
                'GITHUB_EVENT_NAME': 'workflow_dispatch',
                'GITHUB_REPOSITORY': 'Dytschgo/arxcovo',
                'GITHUB_ACTOR': 'Dytschgo', 'GITHUB_TRIGGERING_ACTOR': 'Dytschgo',
                'GITHUB_REF': 'refs/heads/main'}
    for key, value in expected.items():
        require(env.get(key) == value, 'E_GATE_' + key)
    sha = env.get('ARX_REVIEWED_SHA', '')
    require(re.fullmatch('[0-9a-f]{40}', sha), 'E_SHA_FORMAT')
    require(env.get('GITHUB_SHA') == sha == env.get('ARX_WORKFLOW_SHA'), 'E_SHA')
    return identity(env)


def plan(run):
    require(re.fullmatch(r'[1-9][0-9]{0,19}-[1-9][0-9]{0,19}', run), 'E_ID')
    return Path('/opt/arx-ci-' + run), 'arx-ci-' + run + '.service'


# F0 mechanism only: no experiment is authorized by this instrumentation.
SITES = ('cg_preflight', 'cleanup_capture', 'cleanup_base', 'service_stage',
         'service_cg', 'service_runs', 'capture_base', 'harness_stage',
         'harness_binary', 'run_parent', 'run_artifact', 'run_cleanup', 'installed')
PHASES = ('initial', 'final')
MUTATIONS = ('none', 'staged', 'service', 'cgroup', 'account')
CLEANUPS = ('confirmed', 'unconfirmed', 'not_reached')
CODES = ('E_ACCOUNT_COLLISION', 'E_ARGS', 'E_ARTIFACT', 'E_ARTIFACT_PATH', 'E_BUILD', 'E_BUILD_LOG', 'E_CGROUP2', 'E_CGROUP_FIXTURE_OWNER', 'E_CGROUP_SCAN_BOUND', 'E_CGROUP_UNIT_OWNER', 'E_CGROUP_WRITABLE_HIERARCHY', 'E_CHECKOUT', 'E_CONTROLLERS', 'E_COPY_RACE', 'E_COPY_TYPE', 'E_DELEGATION', 'E_FIXTURE_COLLISION', 'E_FIXTURE_INODE', 'E_FIXTURE_UID', 'E_GROUP_ORPHAN', 'E_ID', 'E_INSTALLED', 'E_INTERNAL_TASKS', 'E_MODE', 'E_OUTPUT_DRAIN', 'E_PATH', 'E_PID', 'E_PLATFORM', 'E_QUARANTINE_CGROUP', 'E_QUARANTINE_PATH_UNCAPTURED', 'E_QUARANTINE_UID_UNCAPTURED', 'E_RECORD', 'E_RECORD_BOUND', 'E_RMTREE', 'E_ROOT', 'E_RUNTIME', 'E_SERVICE_OR_TEST', 'E_SHA', 'E_SHA_FORMAT', 'E_SITE', 'E_SYSTEMD', 'E_TEST', 'E_TEST_ARGS', 'E_TEST_LOG', 'E_TEST_TIMEOUT', 'E_UID_LIVE', 'E_UNIT_CGROUP', 'E_UNIT_COLLISION', 'E_GATE_CI', 'E_GATE_GITHUB_ACTIONS', 'E_GATE_RUNNER_ENVIRONMENT', 'E_GATE_RUNNER_OS', 'E_GATE_GITHUB_EVENT_NAME', 'E_GATE_GITHUB_REPOSITORY', 'E_GATE_GITHUB_ACTOR', 'E_GATE_GITHUB_TRIGGERING_ACTOR', 'E_GATE_GITHUB_REF', 'E_COMMAND_TIMEOUT', 'E_COMMAND_FAILED', 'E_OS')
MAX_TRIPLES = 32
MAX_LINE = 4096
feedback_phase = 'initial'
feedback_mutation = 'none'
feedback_cleanup = 'not_reached'
feedback_first = None
feedback_internal = None
internal_trusted = False


def triples_valid(triples):
    require(type(triples) is list and 1 <= len(triples) <= MAX_TRIPLES, 'E_RECORD')
    for t in triples:
        require(type(t) is list and len(t) == 3, 'E_RECORD')
        require(type(t[0]) is str and t[0] in ('dir', 'link', 'other') and
                type(t[1]) is bool and type(t[2]) is int and
                t[2] in (0, 2, 16, 18), 'E_RECORD')


def decide(triples):
    """Pure historical predicate; other (including regular files) stays allowed."""
    triples_valid(triples)
    for ordinal, (kind, owner_zero, write_bits) in enumerate(triples):
        for conjunct, failed in (('link', kind == 'link'),
                                   ('owner', not owner_zero), ('write', write_bits != 0)):
            if failed:
                return ordinal, conjunct
    return None


class Refusal(RuntimeError):
    def __init__(self, site, phase, triples):
        require(site in SITES and phase in PHASES, 'E_RECORD')
        result = decide(triples)
        require(result is not None and result[0] == len(triples) - 1, 'E_RECORD')
        self.record = dict(code='E_PROTECTION', site=site, phase=phase,
                           ordinal=result[0], kind=triples[-1][0],
                           owner_zero=triples[-1][1], write_bits=triples[-1][2],
                           predecessors=[list(t) for t in triples[:-1]])
        super().__init__('E_PROTECTION')


def exception_record(error):
    # Never stringify an arbitrary payload, even a RuntimeError payload.
    if type(error) is Refusal:
        return dict(error.record)
    if type(error) is RuntimeError and len(error.args) == 1:
        code = error.args[0]
        if type(code) is str and code in CODES:
            return dict(code=code)
    if type(error) is subprocess.TimeoutExpired:
        return dict(code='E_COMMAND_TIMEOUT')
    if type(error) is subprocess.CalledProcessError:
        return dict(code='E_COMMAND_FAILED')
    if isinstance(error, OSError):
        return dict(code='E_OS')
    return dict(code='E_RUNTIME')


def validate_refusal(e):
    require(set(e) == {'code', 'site', 'phase', 'ordinal', 'kind', 'owner_zero',
                       'write_bits', 'predecessors'}, 'E_RECORD')
    require(e['code'] == 'E_PROTECTION' and type(e['site']) is str and e['site'] in SITES and
            type(e['phase']) is str and e['phase'] in PHASES and
            type(e['ordinal']) is int and type(e['predecessors']) is list, 'E_RECORD')
    triples = e['predecessors'] + [[e['kind'], e['owner_zero'], e['write_bits']]]
    result = decide(triples)
    require(result is not None and result[0] == e['ordinal'] == len(triples) - 1, 'E_RECORD')
    return e


def validate_record(record):
    require(type(record) is dict and set(record) ==
            {'schema', 'version', 'provenance', 'first', 'final', 'mutation_lower_bound', 'cleanup', 'internal'}, 'E_RECORD')
    require(record['schema'] == 'unix-protection-f0' and type(record['schema']) is str and
            type(record['version']) is int and record['version'] == 2, 'E_RECORD')
    p = record['provenance']
    require(type(p) is dict and set(p) == {'source_sha', 'workflow_sha', 'run', 'attempt'}, 'E_RECORD')
    for key in ('source_sha', 'workflow_sha'):
        require(type(p[key]) is str and re.fullmatch('[0-9a-f]{40}', p[key]), 'E_RECORD')
    require(p['source_sha'] == p['workflow_sha'], 'E_RECORD')
    for key in ('run', 'attempt'):
        require(type(p[key]) is str and re.fullmatch('[1-9][0-9]{0,19}', p[key]), 'E_RECORD')
    require(type(record['mutation_lower_bound']) is str and record['mutation_lower_bound'] in MUTATIONS and
            type(record['cleanup']) is str and record['cleanup'] in CLEANUPS, 'E_RECORD')
    for key in ('first', 'final'):
        e = record[key]
        if e is None:
            continue
        require(type(e) is dict, 'E_RECORD')
        if set(e) == {'code'} and type(e['code']) is str and e['code'] in CODES:
            continue
        validate_refusal(e)
    first, final = record['first'], record['final']
    mutation, cleanup = record['mutation_lower_bound'], record['cleanup']
    internal = record['internal']
    require((internal is None) == (mutation in ('none', 'staged')), 'E_RECORD')
    if internal is not None:
        validate_internal_summary(internal)
    if final is None:
        require(first is None and cleanup == 'confirmed' and mutation in ('service', 'cgroup', 'account') and
                all(internal[m] == 'absent' for m in PRECEDENCE), 'E_RECORD')
    if cleanup == 'confirmed':
        require(first == final, 'E_RECORD')
    if mutation == 'none':
        require(cleanup == 'not_reached', 'E_RECORD')
    if cleanup == 'not_reached':
        require(first == final and final is not None, 'E_RECORD')
    if final is not None and first is None:
        require(cleanup == 'unconfirmed' and mutation != 'none', 'E_RECORD')
    if first and first.get('code') == 'E_PROTECTION':
        require(first['phase'] == 'initial', 'E_RECORD')
    if final and final.get('code') == 'E_PROTECTION':
        require((final['phase'] == 'final' and cleanup == 'unconfirmed') or
                (final['phase'] == 'initial' and first == final), 'E_RECORD')
    return record


def encode_record(record):
    validate_record(record)
    line = json.dumps(record, separators=(',', ':'), ensure_ascii=True) + '\n'
    require(len(line.encode('ascii')) <= MAX_LINE, 'E_RECORD_BOUND')
    return line


def replay(line):
    require(type(line) is str and len(line) <= MAX_LINE and line.isascii() and
            line.endswith('\n') and line.count('\n') == 1, 'E_RECORD_BOUND')
    def pairs(items):
        out = {}
        for key, value in items:
            require(key not in out, 'E_RECORD')
            out[key] = value
        return out
    try:
        record = json.loads(line, object_pairs_hook=pairs)
        validate_record(record)
    except (ValueError, TypeError, RecursionError, KeyError):
        raise RuntimeError('E_RECORD') from None
    return {key: decide(e['predecessors'] + [[e['kind'], e['owner_zero'], e['write_bits']]])
            for key in ('first', 'final') if (e := record[key]) and e['code'] == 'E_PROTECTION'}


CHANNELS = {'service': 'feedback-service', 'harness': 'feedback-harness',
            'fixture-check': 'feedback-fixture-check'}
INTERNAL_SITES = {'service': ('service_stage', 'service_cg', 'service_runs', 'capture_base'),
                  'harness': ('harness_stage', 'harness_binary'),
                  'fixture-check': ('capture_base',)}
PRECEDENCE = ('service', 'fixture-check', 'harness')
STATUSES = ('absent', 'malformed', 'captured', 'unknown')
MAX_INTERNAL = 1024


def canonical(value):
    return (json.dumps(value, separators=(',', ':'), ensure_ascii=True) + '\n').encode('ascii')


def unique_pairs(items):
    out = {}
    for key, value in items:
        require(key not in out, 'E_RECORD')
        out[key] = value
    return out


def internal_refusal(mode, refusal):
    validate_refusal(refusal)
    require(refusal['site'] in INTERNAL_SITES[mode] and refusal['phase'] == 'initial', 'E_RECORD')


def validate_internal(value, mode):
    require(type(value) is dict and set(value) == {'schema', 'version', 'mode', 'refusal'}, 'E_RECORD')
    require(type(value['schema']) is str and value['schema'] == 'unix-protection-f0-internal' and
            type(value['version']) is int and value['version'] == 1 and
            type(value['mode']) is str and value['mode'] == mode, 'E_RECORD')
    internal_refusal(mode, value['refusal'])
    return dict(schema=value['schema'], version=1, mode=mode,
                refusal={k: value['refusal'][k] for k in
                         ('code', 'site', 'phase', 'ordinal', 'kind', 'owner_zero', 'write_bits', 'predecessors')})


def validate_internal_summary(value):
    require(type(value) is dict and set(value) == {*PRECEDENCE, 'carried', 'refusal'}, 'E_RECORD')
    require(all(type(value[m]) is str and value[m] in STATUSES for m in PRECEDENCE), 'E_RECORD')
    require(all(value[m] == 'unknown' for m in PRECEDENCE) or
            all(value[m] != 'unknown' for m in PRECEDENCE), 'E_RECORD')
    carried = next((m for m in PRECEDENCE if value[m] == 'captured'), None)
    require(value['carried'] == carried and (value['refusal'] is None) == (carried is None), 'E_RECORD')
    if carried is not None:
        internal_refusal(carried, value['refusal'])


def open_stage(run_id):
    fd = None
    try:
        stage, _ = plan(run_id)
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        for name in ('/', 'opt', stage.name):
            child = os.open(name, flags, dir_fd=fd)
            parent, fd = fd, child
            if parent is not None:
                os.close(parent)
            info = os.fstat(fd)
            require(stat.S_ISDIR(info.st_mode) and info.st_uid == 0 and
                    info.st_mode & 0o022 == 0, 'E_RECORD')
        require(info.st_mode & 0o077 == 0, 'E_RECORD')
        result, fd = fd, None
        return result
    except Exception:
        return None
    finally:
        if fd is not None:
            try:
                os.close(fd)
            except Exception:
                pass


def create_channel(run_id, name, data):
    stage = fd = None
    try:
        stage = open_stage(run_id)
        if stage is None:
            return
        fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                     0o600, dir_fd=stage)
        os.fchmod(fd, 0o600)
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == 0 and
                info.st_nlink == 1 and info.st_size == 0, 'E_RECORD')
        if data is not None:
            require(os.write(fd, data) == len(data), 'E_RECORD')
    finally:
        try:
            if fd is not None:
                os.close(fd)
        finally:
            if stage is not None:
                os.close(stage)


def write_internal(error):
    try:
        mode = sys.argv[1]
        if not internal_trusted or mode not in CHANNELS or type(error) is not Refusal:
            return
        internal_refusal(mode, error.record)
        value = dict(schema='unix-protection-f0-internal', version=1, mode=mode, refusal=error.record)
        data = canonical(validate_internal(value, mode))
        require(len(data) <= MAX_INTERNAL, 'E_RECORD')
        create_channel(sys.argv[2], CHANNELS[mode], data)
    except Exception:
        pass


def mark_cgroup(run_id):
    try:
        create_channel(run_id, 'progress-cgroup', None)
    except Exception:
        pass


def read_internal(stage, mode):
    fd = None
    try:
        try:
            fd = os.open(CHANNELS[mode], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                         dir_fd=stage)
        except FileNotFoundError:
            return 'absent', None
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode), 'E_RECORD')
        require(info.st_uid == 0, 'E_RECORD')
        require(stat.S_IMODE(info.st_mode) == 0o600, 'E_RECORD')
        require(info.st_nlink == 1, 'E_RECORD')
        require(0 < info.st_size <= MAX_INTERNAL, 'E_RECORD')
        data = b''
        while len(data) < MAX_INTERNAL + 1:
            chunk = os.read(fd, MAX_INTERNAL + 1 - len(data))
            if not chunk:
                break
            data += chunk
        require(len(data) == info.st_size and len(data) <= MAX_INTERNAL, 'E_RECORD')
        line = data.decode('ascii')
        require(line.endswith('\n') and line.count('\n') == 1, 'E_RECORD')
        value = validate_internal(json.loads(line, object_pairs_hook=unique_pairs), mode)
        require(canonical(value) == data, 'E_RECORD')
        return 'captured', value['refusal']
    except Exception:
        return 'malformed', None
    finally:
        if fd is not None:
            os.close(fd)


def snapshot_internal(run_id, unit, stopped):
    # Conditional snapshot within the existing containment assumption only.
    global feedback_internal, feedback_mutation
    feedback_internal = dict.fromkeys(PRECEDENCE, 'unknown')
    feedback_internal.update(carried=None, refusal=None)
    stage = None
    try:
        cgroup_absent = False
        if stopped is not None:
            try:
                os.lstat(Path('/sys/fs/cgroup/system.slice') / unit)
            except FileNotFoundError:
                cgroup_absent = True
            except Exception:
                pass
        snapshot_available = stopped is not None and cgroup_absent
        stage = open_stage(run_id)
        if stage is None:
            return
        for name, bound in (('progress-cgroup', 'cgroup'), ('fixture-identity', 'account')):
            try:
                info = os.stat(name, dir_fd=stage, follow_symlinks=False)
                if stat.S_ISREG(info.st_mode) and info.st_uid == 0:
                    if MUTATIONS.index(bound) > MUTATIONS.index(feedback_mutation):
                        feedback_mutation = bound
            except Exception:
                pass
        if snapshot_available:
            value = {}
            for mode in PRECEDENCE:
                status, refusal = read_internal(stage, mode)
                value[mode] = status
                if status == 'captured' and 'carried' not in value:
                    value.update(carried=mode, refusal=refusal)
            value.setdefault('carried', None)
            value.setdefault('refusal', None)
            feedback_internal = value
    except Exception:
        pass
    finally:
        if stage is not None:
            try:
                os.close(stage)
            except Exception:
                pass


def protected(path, site):
    """Match library ancestor checks; never resolve away a link."""
    require(site in SITES, 'E_SITE')
    require(path.is_absolute() and '..' not in path.parts, 'E_PATH')
    items = [*reversed(path.parents), path]
    require(len(items) <= MAX_TRIPLES, 'E_RECORD_BOUND')
    triples = []
    for item in items:
        s = item.lstat()
        kind = 'link' if stat.S_ISLNK(s.st_mode) else 'dir' if stat.S_ISDIR(s.st_mode) else 'other'
        triples.append([kind, s.st_uid == 0, s.st_mode & 0o022])
        if decide(triples) is not None:
            raise Refusal(site, feedback_phase, triples)


def call(args, **kw):
    return subprocess.run(args, env=SAFE_ENV, check=True, timeout=30,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kw)


def copy_tree(src, dst, exclude=()):
    # Open every component relative to a pinned directory; never reopen a checked
    # pathname for copying. Destination is exclusively created private staging.
    def copy_dir(fd, target):
        target.mkdir(mode=0o700)
        for name in os.listdir(fd):
            if name in exclude:
                continue
            s = os.stat(name, dir_fd=fd, follow_symlinks=False)
            require(stat.S_ISDIR(s.st_mode) or stat.S_ISREG(s.st_mode), 'E_COPY_TYPE')
            flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
            if stat.S_ISDIR(s.st_mode):
                flags |= os.O_DIRECTORY
            child = os.open(name, flags, dir_fd=fd)
            try:
                opened = os.fstat(child)
                require((s.st_dev, s.st_ino, stat.S_IFMT(s.st_mode)) ==
                        (opened.st_dev, opened.st_ino, stat.S_IFMT(opened.st_mode)), 'E_COPY_RACE')
                if stat.S_ISDIR(opened.st_mode):
                    copy_dir(child, target / name)
                else:
                    with os.fdopen(os.dup(child), 'rb') as source, (target / name).open('xb') as out:
                        shutil.copyfileobj(source, out)
                    (target / name).chmod(0o700 if opened.st_mode & 0o111 else 0o600)
            finally:
                os.close(child)
    fd = os.open(src, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        copy_dir(fd, dst)
    finally:
        os.close(fd)


def cg_preflight():
    require(Path('/run/systemd/system').is_dir(), 'E_SYSTEMD')
    protected(Path('/sys/fs/cgroup'), 'cg_preflight')
    fs = subprocess.check_output(['/usr/bin/stat', '-f', '-c', '%T', '/sys/fs/cgroup'],
                                 env=SAFE_ENV, timeout=5, stderr=subprocess.DEVNULL).strip()
    require(fs == b'cgroup2fs', 'E_CGROUP2')
    require({'memory', 'pids'} <= set(Path('/sys/fs/cgroup/cgroup.controllers').read_text().split()),
            'E_CONTROLLERS')
    cg_scan()


def cg_scan(unit=None, uid=None, gid=None):
    # v2 migration needs write access to destination AND common ancestor procs.
    # Unrelated user delegation is safe; our ancestors/subtree remain root-owned.
    # ACL write masks appear in group mode bits and therefore refuse globally.
    count = 0
    for root, dirs, files in os.walk('/sys/fs/cgroup', followlinks=False):
        for name in dirs + files:
            count += 1
            require(count <= 20000, 'E_CGROUP_SCAN_BOUND')
            s = (Path(root) / name).lstat()
            path = Path(root) / name
            require(not s.st_mode & 0o022 and
                    not stat.S_ISLNK(s.st_mode), 'E_CGROUP_WRITABLE_HIERARCHY')
            if unit is not None and (path == unit or unit in path.parents or path in unit.parents):
                require(s.st_uid == 0, 'E_CGROUP_UNIT_OWNER')
            if uid is not None:
                require(s.st_uid != uid and s.st_gid != gid, 'E_CGROUP_FIXTURE_OWNER')


def cargo_env(stage):
    return {**SAFE_ENV, 'HOME': str(stage), 'CARGO_HOME': str(stage / 'cargo'),
            'CARGO_NET_OFFLINE': 'true', 'RUSTC': str(stage / 'rust/bin/rustc'),
            'RUSTDOC': str(stage / 'rust/bin/rustdoc')}


def command(stage):
    return [str(stage / 'rust/bin/cargo'), 'test', '-p', 'arxd', '--locked', '--lib', TEST]


def fixture_cleanup(stage):
    """Only the PID recorded by the root-owned Cargo runner, never a glob."""
    import pwd
    import grp
    capture = stage / 'harness-pid'
    if not capture.exists():
        return
    protected(capture, 'cleanup_capture')
    pid = capture.read_text()
    require(re.fullmatch('[1-9][0-9]{0,9}', pid), 'E_PID')
    name = 'arxexec' + pid
    base = Path('/var/lib') / name
    try:
        account = pwd.getpwnam(name)
    except KeyError:
        account = None
    if base.exists() or base.is_symlink():
        protected(base, 'cleanup_base')
        require((stage / 'fixture-directory').exists(), 'E_QUARANTINE_PATH_UNCAPTURED')
        captured = json.loads((stage / 'fixture-directory').read_text())
        require([base.stat().st_dev, base.stat().st_ino] == captured, 'E_FIXTURE_INODE')
        # Descendants can be build-user owned but must never be followed.
        require(shutil.rmtree.avoids_symlink_attacks, 'E_RMTREE')
    if account:
        require((stage / 'fixture-identity').exists(), 'E_QUARANTINE_UID_UNCAPTURED')
        captured = json.loads((stage / 'fixture-identity').read_text())
        group = grp.getgrnam(name)
        require(account.pw_uid == captured['uid'] >= 1000 and
                account.pw_gid == captured['gid'] == group.gr_gid >= 1000 and
                account.pw_shell == '/usr/sbin/nologin' and not group.gr_mem,
                'E_FIXTURE_UID')
        uid = account.pw_uid
        # Unit is already stopped. Any task outside it with this UID quarantines.
        for proc in Path('/proc').iterdir():
            if proc.name.isdigit():
                try:
                    require(proc.stat().st_uid != uid, 'E_UID_LIVE')
                except FileNotFoundError:
                    pass
        call(['/usr/sbin/userdel', name])
    if base.exists() or base.is_symlink():
        shutil.rmtree(base)
    try:
        group = grp.getgrnam(name)
    except KeyError:
        group = None
    if group:
        require(account is not None and group.gr_gid == account.pw_gid and not group.gr_mem,
                'E_GROUP_ORPHAN')
        call(['/usr/sbin/groupdel', name])


def drain_output(source, target, exceeded, limit=1024 * 1024):
    written = 0
    while chunk := source.read(65536):
        keep = min(len(chunk), limit - written)
        target.write(chunk[:keep])
        written += keep
        if keep < len(chunk):
            exceeded.set()


def service(stage, run):
    # Only entered from our protected transient unit, never a local CLI shortcut.
    protected(stage, 'service_stage')
    _, unit = plan(run)
    cg = Path('/sys/fs/cgroup') / Path('/proc/self/cgroup').read_text().strip().split('0::')[-1].lstrip('/')
    require(cg == Path('/sys/fs/cgroup/system.slice') / unit, 'E_UNIT_CGROUP')
    protected(cg, 'service_cg')
    require({'memory', 'pids'} <= set((cg / 'cgroup.controllers').read_text().split()), 'E_DELEGATION')
    control = cg / 'control'
    mark_cgroup(run)
    control.mkdir(mode=0o700)  # exclusive: never reuse another group's child
    (control / 'cgroup.procs').write_text(str(os.getpid()))
    require(not (cg / 'cgroup.procs').read_text().strip(), 'E_INTERNAL_TASKS')
    (cg / 'cgroup.subtree_control').write_text('+memory +pids')
    delegated = cg / 'runs'
    delegated.mkdir(mode=0o700)
    protected(delegated, 'service_runs')
    (delegated / 'cgroup.subtree_control').write_text('+memory +pids')
    require(not (delegated / 'cgroup.procs').read_text().strip(), 'E_INTERNAL_TASKS')
    env = {**cargo_env(stage), 'CI': 'true', 'GITHUB_ACTIONS': 'true',
           'RUNNER_ENVIRONMENT': 'github-hosted', 'ARX_DISPOSABLE_EXEC_TEST': '1',
           'ARX_EXEC_CGROUP_PARENT': str(delegated),
           'ARX_EXEC_GUARD_DRIVER': str(stage / 'source/tools/unix-exec-validation/driver.py'),
           'ARX_EXEC_RUN_ID': run,
           'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER': str(stage / 'runner')}
    with (stage / 'test-output').open('xb') as out:
        child = subprocess.Popen(['/usr/bin/timeout', '--signal=KILL', '120s'] +
                                 command(stage) + ['--', '--ignored', '--exact'],
                                 cwd=stage / 'source', env=env,
                                 stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        exceeded = threading.Event()
        reader = threading.Thread(target=drain_output, args=(child.stdout, out, exceeded), daemon=True)
        reader.start()
        deadline = time.monotonic() + 120
        try:
            while child.poll() is None:
                capture_fixture(stage)
                require(not exceeded.is_set(), 'E_TEST_LOG')
                require(time.monotonic() < deadline, 'E_TEST_TIMEOUT')
                time.sleep(0.01)
            capture_fixture(stage)
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=5)
            reader.join(timeout=5)
            require(not reader.is_alive(), 'E_OUTPUT_DRAIN')
            child.stdout.close()
        require(not exceeded.is_set(), 'E_TEST_LOG')
    verify_test_result(stage, child.returncode)
    (stage / 'passed').write_text('1')


def verify_test_result(stage, returncode):
    require((stage / 'test-output').stat().st_size <= 1024 * 1024, 'E_TEST_LOG')
    text = (stage / 'test-output').read_text(errors='replace')
    passed = f'test {TEST} ... ok' in text and '1 passed; 0 failed' in text
    require(returncode == 0 and passed and (stage / 'harness-pid').exists(), 'E_TEST')


def capture_fixture(stage):
    """Capture identities while alive; missing capture quarantines residuals."""
    import pwd
    capture = stage / 'harness-pid'
    if not capture.exists():
        return
    pid = capture.read_text()
    # Runner may have opened but not yet written its marker. Never guess a name.
    if not pid:
        return
    require(re.fullmatch('[1-9][0-9]{0,9}', pid), 'E_PID')
    name = 'arxexec' + pid
    base = Path('/var/lib') / name
    directory = stage / 'fixture-directory'
    if not directory.exists() and base.exists():
        protected(base, 'capture_base')
        s = base.stat()
        directory.write_text(json.dumps([s.st_dev, s.st_ino]))
    captured = stage / 'fixture-identity'
    if not captured.exists():
        try:
            account = pwd.getpwnam(name)
        except KeyError:
            return
        require(account.pw_uid >= 1000 and account.pw_gid >= 1000, 'E_FIXTURE_UID')
        captured.write_text(json.dumps({'uid': account.pw_uid, 'gid': account.pw_gid}))


def harness(stage, args):
    import pwd
    import grp
    protected(stage, 'harness_stage')
    binary = (stage / 'artifact').read_text()
    require(args == [binary, TEST, '--ignored', '--exact'], 'E_TEST_ARGS')
    protected(Path(binary), 'harness_binary')
    pid = str(os.getpid())
    name = 'arxexec' + pid
    require(not os.path.lexists('/var/lib/' + name), 'E_FIXTURE_COLLISION')
    for lookup in (pwd.getpwnam, grp.getgrnam):
        try:
            lookup(name)
        except KeyError:
            continue
        raise RuntimeError('E_ACCOUNT_COLLISION')
    with (stage / 'harness-pid').open('x') as f:
        f.write(pid)
    os.execve(binary, args, dict(os.environ))


def run(env):
    global feedback_phase, feedback_mutation, feedback_cleanup, feedback_first, feedback_internal
    feedback_internal = None
    feedback_phase, feedback_mutation, feedback_cleanup, feedback_first = 'initial', 'none', 'not_reached', None
    try:
        return run_body(env)
    except Exception as error:
        if feedback_cleanup == 'not_reached':
            feedback_first = exception_record(error)
        raise


def run_body(env):
    global feedback_phase, feedback_mutation, feedback_cleanup, feedback_first
    run_id = gate(env, sys.platform)  # before any mutation or privileged probe
    require(os.getuid() == os.geteuid() == 0, 'E_ROOT')
    cg_preflight()  # read-only; no retry or global controller writes
    stage, unit = plan(run_id)
    protected(stage.parent, 'run_parent')
    # Refuse unit collisions before taking ownership of this generated name.
    state = subprocess.check_output(['/usr/bin/systemctl', 'show', unit, '-p', 'LoadState', '--value'],
                                    env=SAFE_ENV, timeout=5, stderr=subprocess.DEVNULL).strip()
    require(state == b'not-found', 'E_UNIT_COLLISION')
    feedback_mutation = 'staged'  # conservative: mutation may begin
    stage.mkdir(mode=0o700)
    unit_created = False
    try:
        source = Path.cwd()
        require(subprocess.check_output(['/usr/bin/git', '-c', 'safe.directory=' + str(source), 'rev-parse', 'HEAD'], env=SAFE_ENV,
                                        timeout=5, stderr=subprocess.DEVNULL).decode().strip() == env['GITHUB_SHA'], 'E_CHECKOUT')
        # Fixed hosted-runner tool locations; no user supplied paths or config.
        copy_tree(source, stage / 'source', ('.git', 'target', 'node_modules', '__pycache__'))
        copy_tree(Path('/home/runner/.rustup/toolchains') / TOOLCHAIN, stage / 'rust')
        (stage / 'cargo').mkdir(mode=0o700)
        copy_tree(Path('/home/runner/.cargo/registry'), stage / 'cargo/registry')
        driver = stage / 'source/tools/unix-exec-validation/driver.py'
        (stage / 'runner').write_text('#!/bin/sh\nexec /usr/bin/python3 -I ' + str(driver) +
                                    ' harness ' + run_id + ' "$@"\n')
        (stage / 'runner').chmod(0o700)
        build_env = cargo_env(stage)
        # Build before the 120s harness bound, with no fixture account yet.
        with (stage / 'build-output').open('xb') as out:
            result = subprocess.run(command(stage) + ['--no-run', '--message-format=json'],
                                    cwd=stage / 'source', env=build_env, timeout=360,
                                    stdout=out, stderr=subprocess.DEVNULL)
        require(result.returncode == 0, 'E_BUILD')
        require((stage / 'build-output').stat().st_size <= 4 * 1024 * 1024, 'E_BUILD_LOG')
        artifacts = [j['executable'] for line in (stage / 'build-output').read_text().splitlines()
                     if (j := json.loads(line)).get('reason') == 'compiler-artifact'
                     and j.get('executable') and j.get('profile', {}).get('test')]
        require(len(artifacts) == 1, 'E_ARTIFACT')
        artifact = Path(artifacts[0])
        require(artifact.is_relative_to(stage / 'source/target/debug/deps'), 'E_ARTIFACT_PATH')
        protected(artifact, 'run_artifact')
        (stage / 'artifact').write_text(str(artifact))
        feedback_mutation = 'service'
        unit_created = True  # name reserved by absence; finally stops even start failure
        result = subprocess.run(['/usr/bin/systemd-run', '--quiet', '--wait', '--collect',
                                 '--unit=' + unit, '--property=Delegate=yes',
                                 '--slice=system.slice',
                                 '--property=RuntimeMaxSec=140', '--property=TimeoutStopSec=10',
                                 '--property=KillMode=control-group', '--property=UMask=0077',
                                 '/usr/bin/python3', '-I', str(driver), 'service', run_id],
                                env=SAFE_ENV, timeout=165, stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL)
        require(result.returncode == 0 and (stage / 'passed').exists(), 'E_SERVICE_OR_TEST')

    except Exception as error:
        feedback_first = exception_record(error)
        raise
    finally:
        feedback_phase = 'final'
        feedback_cleanup = 'unconfirmed'
        if unit_created:
            # Stopping our captured unit kills descendants; never kill host hierarchy.
            stopped = None
            try:
                stopped = subprocess.run(['/usr/bin/systemctl', 'stop', unit], env=SAFE_ENV, timeout=20,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            finally:
                snapshot_internal(run_id, unit, stopped)
            cg = Path('/sys/fs/cgroup/system.slice') / unit
            require(not cg.exists(), 'E_QUARANTINE_CGROUP')
        fixture_cleanup(stage)
        protected(stage, 'run_cleanup')
        require(shutil.rmtree.avoids_symlink_attacks, 'E_RMTREE')
        shutil.rmtree(stage)
        feedback_cleanup = 'confirmed'


def main():
    global internal_trusted
    internal_trusted = False
    require(len(sys.argv) >= 2, 'E_MODE')
    mode = sys.argv[1]
    if mode in ('guard', 'run'):
        require(len(sys.argv) == 2, 'E_ARGS')
        gate(os.environ, sys.platform)
        if mode == 'run':
            run(os.environ)
        return
    # Internal modes require root, protected installed path, numeric identity.
    require(sys.platform == 'linux', 'E_PLATFORM')
    require(os.getuid() == os.geteuid() == 0, 'E_ROOT')
    require(len(sys.argv) >= 3, 'E_ARGS')
    stage, _ = plan(sys.argv[2])
    require(Path(__file__) == stage / 'source/tools/unix-exec-validation/driver.py', 'E_INSTALLED')
    protected(Path(__file__), 'installed')
    internal_trusted = True
    if mode == 'service' and len(sys.argv) == 3:
        service(stage, sys.argv[2])
    elif mode == 'fixture-check' and len(sys.argv) == 3:
        capture_fixture(stage)
        captured = json.loads((stage / 'fixture-identity').read_text())
        _, unit = plan(sys.argv[2])
        cg_scan(Path('/sys/fs/cgroup/system.slice') / unit, captured['uid'], captured['gid'])
    elif mode == 'harness':
        harness(stage, sys.argv[3:])
    else:
        raise RuntimeError('E_MODE')


def feedback_line(env, first, final):
    # Gate validates the association; this envelope is association, not attestation.
    gate(env, sys.platform)
    return encode_record(dict(schema='unix-protection-f0', version=2,
        provenance=dict(source_sha=env['ARX_REVIEWED_SHA'], workflow_sha=env['ARX_WORKFLOW_SHA'],
                        run=env['GITHUB_RUN_ID'], attempt=env['GITHUB_RUN_ATTEMPT']),
        first=first, final=final, mutation_lower_bound=feedback_mutation, cleanup=feedback_cleanup, internal=feedback_internal))


def entry():
    error = None
    try:
        main()
    except Exception as caught:
        error = caught
    # Only the outer run owns public feedback. Internal modes remain suppressed
    # by the existing parent subprocess collectors; no extra public lines.
    if sys.argv[1:] == ['run']:
        try:
            gate(os.environ, sys.platform)
            final = exception_record(error) if error is not None else None
            print(feedback_line(os.environ, feedback_first, final), end='')
        except Exception as record_error:
            try:
                gate(os.environ, sys.platform)
            except Exception as gate_error:
                code = exception_record(gate_error)['code']
            else:
                code = 'E_RECORD'
            print('unix experiment refused/failed: ' + code, file=sys.stderr)
            return 1
    elif error is not None:
        write_internal(error)
        print('unix experiment refused/failed: ' + exception_record(error)['code'], file=sys.stderr)
    return 1 if error is not None else 0


if __name__ == '__main__':
    sys.exit(entry())
