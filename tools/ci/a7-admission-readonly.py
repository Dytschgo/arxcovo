"""D7j preparation: fixed alias/C2 diagnosis, never host qualification."""
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import types

_MODULE_SHA256 = '24dd0d6a2c8948f2092e54fff40bbb03c4364ab2a07e59c6e12d62517e5fe47f'
_MODULE_SIZE = 96767
_REFUSAL = '{"schema":1,"status":"refused"}'
_ROLES = {'/': 'root', '/usr': 'usr', '/usr/local': 'usr_local',
          '/usr/local/bin': 'usr_local_bin', '/usr/bin': 'usr_bin',
          '/usr/local/bin/sudo': 'c2_image'}
_FIELDS = ('st_dev', 'st_ino', 'st_mode', 'st_uid', 'st_gid', 'st_nlink',
           'st_size', 'st_mtime_ns', 'st_ctime_ns')


def _need(ok):
    if not ok:
        raise ValueError()


def _stable(info):
    return tuple(getattr(info, key) for key in _FIELDS)


def _gate():
    e = os.environ
    for key, value in {
        'CI': 'true', 'GITHUB_ACTIONS': 'true',
        'RUNNER_ENVIRONMENT': 'github-hosted', 'RUNNER_OS': 'Linux',
        'RUNNER_ARCH': 'X64', 'ImageOS': 'ubuntu24',
        'GITHUB_EVENT_NAME': 'workflow_dispatch',
        'GITHUB_REPOSITORY': 'Dytschgo/arxcovo',
        'GITHUB_ACTOR': 'Dytschgo', 'GITHUB_TRIGGERING_ACTOR': 'Dytschgo',
        'GITHUB_REF': 'refs/heads/main', 'GITHUB_RUN_ATTEMPT': '1',
        'GITHUB_WORKFLOW_REF': 'Dytschgo/arxcovo/.github/workflows/a7-admission-readonly.yml@refs/heads/main',
    }.items():
        _need(e.get(key) == value)
    sha = e.get('GITHUB_SHA', '')
    _need(re.fullmatch('[0-9a-f]{40}', sha) is not None)
    _need(sha == e.get('GITHUB_WORKFLOW_SHA') == e.get('A7_ADMISSION_REVIEWED_SHA'))
    _need(not e.get('UNIX_EXECUTION_REVIEWED_SHA') and not e.get('SUDO_PACKAGE_REVIEWED_SHA'))
    run = e.get('GITHUB_RUN_ID', '')
    _need(re.fullmatch('[1-9][0-9]{0,19}', run) is not None)
    _need(sys.platform == 'linux' and len(sys.argv) == 1)
    _need(os.getuid() == os.geteuid() == 0)
    workspace = e.get('GITHUB_WORKSPACE', '')
    _need(workspace.startswith('/') and '..' not in workspace.split('/'))
    root = Path(workspace)
    _need(str(root) == workspace)
    _need(Path(__file__) == root / 'tools/ci/a7-admission-readonly.py')
    return dict(source_sha=sha, workflow_sha=sha, run=run, attempt='1')


def _module_bytes():
    # Checkout is the reviewed source boundary, not protected host ancestry.
    path = Path(__file__).parent.parent / 'unix-exec-validation/driver.py'
    held = []
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
    try:
        parent = os.open('/', flags | os.O_DIRECTORY)
        held.append(parent)
        for part in path.parts[1:-1]:
            parent = os.open(part, flags | os.O_DIRECTORY, dir_fd=parent)
            held.append(parent)
        fd = os.open(path.name, flags, dir_fd=parent)
        held.append(fd)
        before = os.fstat(fd)
        _need(stat.S_ISREG(before.st_mode) and before.st_nlink == 1
              and before.st_size == _MODULE_SIZE)
        data = bytearray()
        for _ in range((_MODULE_SIZE + 4095) // 4096 + 1):
            block = os.read(fd, min(4096, _MODULE_SIZE + 1 - len(data)))
            _need(type(block) is bytes)
            if not block:
                break
            data.extend(block)
            _need(len(data) <= _MODULE_SIZE)
        _need(not block and len(data) == _MODULE_SIZE)
        _need(_stable(os.fstat(fd)) == _stable(before))
        _need(hashlib.sha256(data).hexdigest() == _MODULE_SHA256)
        return bytes(data)
    finally:
        failed = False
        for fd in reversed(held):
            try:
                os.close(fd)
            except Exception:
                failed = True
        _need(not failed)


def _load_driver():
    data = _module_bytes()
    module = types.ModuleType('_d7j_fixed_driver')
    exec(compile(data, '<fixed-driver>', 'exec'), module.__dict__)
    return module


class _Fault(Exception):
    pass


class _Port:
    """Finite read-only adapter. All ancestors stay held until final verification."""
    O_RDONLY = os.O_RDONLY
    O_NOFOLLOW = getattr(os, 'O_NOFOLLOW', 131072)
    O_CLOEXEC = getattr(os, 'O_CLOEXEC', 524288)
    O_NONBLOCK = getattr(os, 'O_NONBLOCK', 2048)
    O_DIRECTORY = getattr(os, 'O_DIRECTORY', 65536)
    SEEK_SET = os.SEEK_SET

    def __init__(self):
        self.held = {}
        self.rows = {}
        self.read_count = {}
        self.calls = 0

    def _flags(self, path, info):
        row = dict(role=_ROLES[path], owner_is_root=info.st_uid == 0,
                   type_accepted=(stat.S_ISREG(info.st_mode) if path.endswith('/sudo')
                                  else stat.S_ISDIR(info.st_mode)),
                   group_other_write_absent=not bool(info.st_mode & 0o022))
        self.rows[path] = row
        if not all(row[key] for key in row if key != 'role'):
            raise _Fault('E_PATH')

    def _verify(self):
        # In insertion order: validate each parent before any child path lookup.
        for fd, (path, parent, name, before) in self.held.items():
            _need(_stable(os.fstat(fd)) == _stable(before))
            now = os.stat(name, dir_fd=parent, follow_symlinks=False)
            _need(_stable(now) == _stable(before))

    def open(self, name, flags, dir_fd=None):
        self.calls += 1
        _need(self.calls <= 16)
        self._verify()
        _need((dir_fd is None and name == '/') or
              (dir_fd in self.held and type(name) is str and '/' not in name
               and name not in ('', '.', '..')))
        path = '/' if dir_fd is None and name == '/' else (
            self.held[dir_fd][0].rstrip('/') + '/' + name)
        _need(path in _ROLES)
        expected = self.O_RDONLY | self.O_NOFOLLOW | self.O_CLOEXEC | self.O_NONBLOCK
        _need(flags == expected | (0 if path.endswith('/sudo') else self.O_DIRECTORY))
        before = os.stat(name, dir_fd=dir_fd, follow_symlinks=False)
        self._flags(path, before)  # stop BEFORE opening an unsafe component
        if path.endswith('/sudo'):
            _need(before.st_nlink == 1 and before.st_size == 282032)
        fd = os.open(name, flags, dir_fd=dir_fd)
        self.held[fd] = (path, dir_fd, name, before)
        _need(_stable(os.fstat(fd)) == _stable(before))
        # Reopen against the same held parent, excluding atime only.
        again = os.open(name, flags, dir_fd=dir_fd)
        try:
            _need(_stable(os.fstat(again)) == _stable(before))
        finally:
            os.close(again)
        return fd

    def close(self, fd):
        _need(fd in self.held)
        self._verify()  # retain ancestors; finish owns actual closure

    def fstat(self, fd):
        self._verify()
        return os.fstat(fd)

    def stat(self, name, *, dir_fd, follow_symlinks=True):
        self._verify()
        parent = self.held[dir_fd][0]
        _need((parent, name) in (('/', 'bin'), ('/usr/local/bin', 'sudo')))
        if follow_symlinks:
            _need((parent, name) == ('/', 'bin'))
            # Driver already checked bounded readlinkat and unchanged alias lstat.
            # Resolve that fixed accepted alias via held target, never follow a link.
            return os.fstat(next(fd for fd, v in self.held.items() if v[0] == '/usr/bin'))
        info = os.stat(name, dir_fd=dir_fd, follow_symlinks=False)
        if name == 'sudo':
            self._flags('/usr/local/bin/sudo', info)
        return info

    def lseek(self, fd, offset, whence):
        _need(self.held[fd][0] == '/usr/local/bin/sudo' and offset == 0 and whence == self.SEEK_SET)
        return os.lseek(fd, offset, whence)

    def read(self, fd, count):
        self._verify()
        _need(self.held[fd][0] == '/usr/local/bin/sudo' and count == 65536)
        n = self.read_count.get(fd, 0)
        _need(n < 6)
        self.read_count[fd] = n + 1
        return os.read(fd, count)

    def finish(self):
        try:
            self._verify()
        finally:
            failed = False
            for fd in reversed(self.held):
                try:
                    os.close(fd)
                except Exception:
                    failed = True
            _need(not failed)


def _diagnose(driver):
    port = _Port()
    driver.os = port
    code = 'PATH_CHAIN_OBSERVED'
    try:
        driver.a7_alias_c2()
    except _Fault:
        code = 'E_PATH'
    except RuntimeError as error:
        code = error.args[0] if error.args in [('E_ALIAS',), ('E_C2',), ('E_ARTIFACT',)] else 'REFUSED'
    except OSError:
        code = 'E_OS'
    finally:
        port.finish()
    return dict(schema=1, status='read_only_diagnosis', code=code,
                predicates=list(port.rows.values()), qualified=False, native_credit=False)


def _main():
    try:
        binding = _gate()  # before source load/import OR root-down reads
        result = _diagnose(_load_driver())
        output = json.dumps(dict(schema=1, binding=binding, diagnosis=result),
                            sort_keys=True, separators=(',', ':'))
        code = 0 if result['code'] == 'PATH_CHAIN_OBSERVED' else 2
    except Exception:
        output, code = _REFUSAL, 2
    print(output)
    return code


if __name__ == '__main__':
    raise SystemExit(_main())
