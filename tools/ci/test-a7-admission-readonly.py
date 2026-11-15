"""No host ports: source files only; every OS/ctypes operation below is fake."""
import ast
from contextlib import ExitStack, redirect_stdout
import hashlib
import importlib.util
import io
import json
from pathlib import Path, PurePosixPath
import stat
import types
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).absolute().parents[2]
SOURCE = (ROOT / 'tools/unix-exec-validation/driver.py').read_bytes()
WRAPPER = (ROOT / 'tools/ci/a7-admission-readonly.py').read_text()
SPEC = importlib.util.spec_from_file_location('d7j', ROOT / 'tools/ci/a7-admission-readonly.py')
W = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(W)
ENV = dict(CI='true', GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted',
           RUNNER_OS='Linux', RUNNER_ARCH='X64', ImageOS='ubuntu24',
           GITHUB_EVENT_NAME='workflow_dispatch', GITHUB_REPOSITORY='Dytschgo/arxcovo',
           GITHUB_ACTOR='Dytschgo', GITHUB_TRIGGERING_ACTOR='Dytschgo',
           GITHUB_REF='refs/heads/main', GITHUB_RUN_ATTEMPT='1',
           GITHUB_SHA='a'*40, GITHUB_WORKFLOW_SHA='a'*40, A7_ADMISSION_REVIEWED_SHA='a'*40,
           GITHUB_WORKFLOW_REF='Dytschgo/arxcovo/.github/workflows/a7-admission-readonly.yml@refs/heads/main',
           GITHUB_RUN_ID='123', GITHUB_WORKSPACE='/repo')


def info(path, **changes):
    fields = dict(st_dev=1, st_ino=sum(path.encode()), st_mode=stat.S_IFDIR | 0o755,
                  st_uid=0, st_gid=0, st_nlink=1, st_size=0,
                  st_atime_ns=0, st_mtime_ns=0, st_ctime_ns=0)
    if path == '/bin':
        fields.update(st_mode=stat.S_IFLNK | 0o777, st_size=7)
    if path.endswith('/sudo'):
        fields.update(st_mode=stat.S_IFREG | 0o755, st_size=282032)
    fields.update(changes)
    return types.SimpleNamespace(**fields)


class FakeOS:
    def __init__(self, c2=False):
        self.metadata = {p: info(p) for p in W._ROLES}
        self.metadata['/bin'] = info('/bin')
        if not c2:
            del self.metadata['/usr/local/bin/sudo']
        self.fds = {}
        self.opened = []
        self.closed = []
        self.next = 10
        self.link = b'usr/bin'

    def path(self, name, dir_fd):
        return name if dir_fd is None else self.fds[dir_fd].rstrip('/') + '/' + name

    def stat(self, name, dir_fd=None, follow_symlinks=True):
        path = self.path(name, dir_fd)
        if path not in self.metadata:
            raise FileNotFoundError('PRIVATE')
        return self.metadata[path]

    def open(self, name, flags, dir_fd=None):
        path = self.path(name, dir_fd)
        self.opened.append((path, flags))
        fd = self.next
        self.next += 1
        self.fds[fd] = path
        return fd

    def fstat(self, fd):
        return self.metadata[self.fds[fd]]

    def close(self, fd):
        assert fd not in self.closed
        self.closed.append(fd)

    def lseek(self, *args):
        return 0

    def read(self, fd, count):
        raise AssertionError('digest must be mocked')

    def ctypes(self):
        def readlink(root, name, buffer, count):
            assert self.fds[root] == '/' and name == b'bin' and count == 9
            buffer.raw = self.link + b'\0' * (9-len(self.link))
            return len(self.link)
        call = Mock(side_effect=readlink)
        return types.SimpleNamespace(CDLL=lambda _: types.SimpleNamespace(readlinkat=call),
                                     c_int=int, c_char_p=bytes, c_void_p=object, c_size_t=int,
                                     c_ssize_t=int,
                                     create_string_buffer=lambda n: types.SimpleNamespace(raw=b'\0'*n))


class Tests(unittest.TestCase):
    def driver(self):
        with patch.object(W, '_module_bytes', return_value=SOURCE):
            driver = W._load_driver()
        driver.Path = PurePosixPath
        return driver

    def run_fake(self, fake, driver=None):
        driver = driver or self.driver()
        with patch.object(W, 'os', fake), patch.dict('sys.modules', {'ctypes': fake.ctypes()}):
            try:
                return W._diagnose(driver)
            finally:
                self.assertCountEqual(fake.closed, fake.fds)

    def test_source_pin_and_callgraph(self):
        self.assertEqual(len(SOURCE), W._MODULE_SIZE)
        self.assertEqual(hashlib.sha256(SOURCE).hexdigest(), W._MODULE_SHA256)
        tree = ast.parse(WRAPPER)
        diagnose = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == '_diagnose')
        calls = [n.func.attr for n in ast.walk(diagnose) if isinstance(n, ast.Call) and isinstance(n.func, ast.Attribute) and isinstance(n.func.value, ast.Name) and n.func.value.id == 'driver']
        self.assertEqual(calls, ['a7_alias_c2'])
        # Imports define functions only; no invocation of native main.
        self.assertNotIn('subprocess', WRAPPER)

    def test_workflow_transport_and_closed_output(self):
        workflow = (ROOT / '.github/workflows/a7-admission-readonly.yml').read_text()
        self.assertIn('on:\n  workflow_dispatch:\npermissions:\n  contents: read', workflow)
        self.assertIn('timeout-minutes: 3', workflow)
        self.assertIn('persist-credentials: false', workflow)
        self.assertIn('checkout@11d5960a326750d5838078e36cf38b85af677262', workflow)
        self.assertLess(workflow.index('python3 -I -B tools/ci/test-'), workflow.index('sudo -n /usr/bin/env -i'))
        self.assertNotIn('secrets.', workflow)
        for key in ('UNIX_EXECUTION_REVIEWED_SHA', 'SUDO_PACKAGE_REVIEWED_SHA'):
            self.assertIn("vars." + key + " == ''", workflow)
        result = self.run_fake(FakeOS())
        with self.gate_context(), patch.object(W, '_load_driver'), patch.object(W, '_diagnose', return_value=result), redirect_stdout(io.StringIO()) as out:
            self.assertEqual(W._main(), 0)
        value = json.loads(out.getvalue())
        self.assertEqual(set(value), {'schema', 'binding', 'diagnosis'})
        self.assertEqual(set(value['diagnosis']), {'schema', 'status', 'code', 'predicates', 'qualified', 'native_credit'})
        text = json.dumps(value)
        for private in ('st_uid', 'st_gid', 'st_mode', 'st_ino', '/usr', 'sha256', 'traceback'):
            self.assertNotIn(private, text)

    def gate_context(self):
        stack = ExitStack()
        stack.enter_context(patch.dict(W.os.environ, ENV, clear=True))
        stack.enter_context(patch.object(W, 'Path', PurePosixPath))
        stack.enter_context(patch.object(W, '__file__', '/repo/tools/ci/a7-admission-readonly.py'))
        stack.enter_context(patch.object(W.sys, 'platform', 'linux'))
        stack.enter_context(patch.object(W.sys, 'argv', ['fixed']))
        for name in ('getuid', 'geteuid'):
            stack.enter_context(patch.object(W.os, name, return_value=0, create=True))
        return stack

    def test_gate_before_import_or_read(self):
        with self.gate_context(), patch.object(W, '_load_driver') as load, patch.object(W.os, 'open', create=True) as opened:
            for key in ENV:
                with self.subTest(key=key), patch.dict(W.os.environ, {key: 'PRIVATE'}), redirect_stdout(io.StringIO()) as out:
                    self.assertEqual(W._main(), 2)
                    self.assertEqual(out.getvalue().strip(), W._REFUSAL)
            for key in ('UNIX_EXECUTION_REVIEWED_SHA', 'SUDO_PACKAGE_REVIEWED_SHA'):
                with patch.dict(W.os.environ, {key: 'a'*40}):
                    self.assertRaises(ValueError, W._gate)
            for name in ('getuid', 'geteuid'):
                with patch.object(W.os, name, return_value=1000):
                    self.assertRaises(ValueError, W._gate)
            with patch.object(W.sys, 'argv', ['fixed', 'override']):
                self.assertRaises(ValueError, W._gate)
            load.assert_not_called()
            opened.assert_not_called()

    def test_gate_positive_and_bounds(self):
        with self.gate_context():
            self.assertEqual(W._gate()['attempt'], '1')
            for key, values in {'GITHUB_RUN_ID': ['0', '01', '1'*21, '1\n'], 'GITHUB_RUN_ATTEMPT': ['2', '01'], 'GITHUB_WORKSPACE': ['repo', '/repo/', '/repo/../repo']}.items():
                for value in values:
                    with patch.dict(W.os.environ, {key: value}):
                        self.assertRaises(ValueError, W._gate)

    def test_fixed_chain_and_c2(self):
        for present in (False, True):
            fake = FakeOS(present)
            driver = self.driver()
            driver.sudo_nnp_digest = Mock(return_value=dict(size=282032, sha256='6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff'))
            result = self.run_fake(fake, driver)
            self.assertEqual(result['code'], 'PATH_CHAIN_OBSERVED')
            self.assertFalse(result['qualified'])
            self.assertFalse(result['native_credit'])
            self.assertEqual({p for p, flags in fake.opened}, set(W._ROLES) - (set() if present else {'/usr/local/bin/sudo'}))
            self.assertEqual(driver.sudo_nnp_digest.call_count, int(present))
            for _, flags in fake.opened:
                for bit in (W._Port.O_NOFOLLOW, W._Port.O_CLOEXEC, W._Port.O_NONBLOCK):
                    self.assertTrue(flags & bit)

    def test_first_fault_every_directory_and_predicate(self):
        for path in list(W._ROLES)[:-1]:
            for changes, predicate in [(dict(st_uid=1000), 'owner_is_root'), (dict(st_mode=stat.S_IFREG | 0o755), 'type_accepted'), (dict(st_mode=stat.S_IFDIR | 0o775), 'group_other_write_absent'), (dict(st_mode=stat.S_IFDIR | 0o757), 'group_other_write_absent'), (dict(st_mode=stat.S_IFLNK | 0o755), 'type_accepted')]:
                with self.subTest(path=path, changes=changes):
                    fake = FakeOS()
                    fake.metadata[path] = info(path, **changes)
                    result = self.run_fake(fake)
                    self.assertEqual(result['code'], 'E_PATH')
                    row = result['predicates'][-1]
                    self.assertEqual(row['role'], W._ROLES[path])
                    self.assertIs(row[predicate], False)
                    self.assertNotIn(path, [p for p, _ in fake.opened])
                    self.assertFalse(any(p.startswith(path.rstrip('/') + '/') for p, _ in fake.opened if p != path))
                    self.assertEqual(set(row), {'role', 'owner_is_root', 'type_accepted', 'group_other_write_absent'})

    def test_alias_c2_refusals_private(self):
        fake = FakeOS()
        fake.link = b'PRIVATE'
        result = self.run_fake(fake)
        self.assertEqual(result['code'], 'E_ALIAS')
        self.assertNotIn('PRIVATE', json.dumps(result))
        fake = FakeOS(True)
        driver = self.driver()
        driver.sudo_nnp_digest = Mock(return_value=dict(size=282032, sha256='PRIVATE'))
        result = self.run_fake(fake, driver)
        self.assertEqual(result['code'], 'E_C2')
        self.assertNotIn('PRIVATE', json.dumps(result))

    def test_atime_only_and_every_other_field_drift(self):
        for field in ('st_atime_ns',) + W._FIELDS:
            fake = FakeOS()
            original = fake.fstat
            count = 0
            def changed(fd):
                nonlocal count
                count += 1
                value = original(fd)
                if count > 1:
                    value = types.SimpleNamespace(**vars(value))
                    setattr(value, field, getattr(value, field)+1)
                return value
            fake.fstat = changed
            if field == 'st_atime_ns':
                self.assertEqual(self.run_fake(fake)['code'], 'PATH_CHAIN_OBSERVED')
            else:
                with self.subTest(field=field):
                    self.assertRaises(ValueError, self.run_fake, fake)

    def test_untrusted_source_constant(self):
        with self.gate_context(), patch.object(W, '_module_bytes', side_effect=ValueError('PRIVATE')), redirect_stdout(io.StringIO()) as out:
            self.assertEqual(W._main(), 2)
            self.assertEqual(out.getvalue().strip(), W._REFUSAL)

    def test_bounded_pinned_loader(self):
        for variant in ('good', 'atime', 'hash', 'size', 'link', 'type', 'drift', 'extra'):
            with self.subTest(variant=variant):
                fake = FakeOS()
                for key in ('O_RDONLY', 'O_NOFOLLOW', 'O_CLOEXEC', 'O_NONBLOCK', 'O_DIRECTORY'):
                    setattr(fake, key, getattr(W._Port, key))
                source_info = info('/source', st_mode=stat.S_IFREG | 0o644, st_size=len(SOURCE))
                if variant == 'size':
                    source_info.st_size += 1
                if variant == 'link':
                    source_info.st_nlink = 2
                if variant == 'type':
                    source_info.st_mode = stat.S_IFIFO | 0o644
                calls = 0
                def fstat(fd):
                    nonlocal calls
                    calls += 1
                    result = types.SimpleNamespace(**vars(source_info))
                    if calls > 1 and variant in ('atime', 'drift'):
                        setattr(result, 'st_atime_ns' if variant == 'atime' else 'st_ctime_ns', 1)
                    return result
                fake.fstat = fstat
                stream = io.BytesIO((b'x' + SOURCE[1:]) if variant == 'hash' else SOURCE + (b'x' if variant == 'extra' else b''))
                fake.read = lambda fd, count: stream.read(count)
                with patch.object(W, 'os', fake), patch.object(W, 'Path', PurePosixPath), patch.object(W, '__file__', '/repo/tools/ci/a7-admission-readonly.py'):
                    if variant in ('good', 'atime'):
                        self.assertEqual(W._module_bytes(), SOURCE)
                    else:
                        self.assertRaises(ValueError, W._module_bytes)
                self.assertCountEqual(fake.closed, fake.fds)
                for _, flags in fake.opened:
                    self.assertTrue(flags & W._Port.O_NOFOLLOW)

    def test_digest_bound_and_final_stability(self):
        fake = FakeOS(True)
        fake.O_RDONLY = W._Port.O_RDONLY
        for key in ('O_NOFOLLOW', 'O_CLOEXEC', 'O_NONBLOCK', 'O_DIRECTORY', 'SEEK_SET'):
            setattr(fake, key, getattr(W._Port, key))
        fake.read = Mock(return_value=b'x'*65536)
        with patch.object(W, 'os', fake):
            port = W._Port()
            fd = port.open('/', W._Port.O_NOFOLLOW | W._Port.O_CLOEXEC | W._Port.O_NONBLOCK | W._Port.O_DIRECTORY)
            self.assertRaises(ValueError, port.read, fd, 65536)
            self.assertRaises(ValueError, port.lseek, fd, 0, W._Port.SEEK_SET)
            port.finish()
        # Actual digest verifies ELF/count/hash while all read bytes remain fake.
        fake = FakeOS(True)
        data = b'\x7fELF' + b'x'*(282032-4)
        stream = io.BytesIO(data)
        fake.read = lambda fd, count: stream.read(count)
        self.assertEqual(self.run_fake(fake)['code'], 'E_C2')
        fake = FakeOS(True)
        fake.metadata['/usr/local/bin/sudo'].st_size = 282033
        self.assertRaises(ValueError, self.run_fake, fake)

    def test_mutant_evidence(self):
        global W
        original = W
        mutants = [
            ("owner_is_root=info.st_uid == 0", "owner_is_root=True", 'test_first_fault_every_directory_and_predicate'),
            ("group_other_write_absent=not bool(info.st_mode & 0o022)", "group_other_write_absent=True", 'test_first_fault_every_directory_and_predicate'),
            ("if not all(row[key] for key in row if key != 'role'):", "if False:", 'test_first_fault_every_directory_and_predicate'),
            ("_need(flags == expected | (0 if path.endswith('/sudo') else self.O_DIRECTORY))", "pass", 'test_arbitrary_path_and_flags_rejected'),
            ("_need(e.get(key) == value)", "pass", 'test_gate_before_import_or_read'),
            ("_need(sha == e.get('GITHUB_WORKFLOW_SHA') == e.get('A7_ADMISSION_REVIEWED_SHA'))", "pass", 'test_gate_before_import_or_read'),
            ("_need(hashlib.sha256(data).hexdigest() == _MODULE_SHA256)", "pass", 'test_bounded_pinned_loader'),
        ]
        try:
            for old, new, test in mutants:
                self.assertEqual(WRAPPER.count(old), 1)
                module = types.ModuleType('mutant')
                module.__file__ = original.__file__
                exec(compile(WRAPPER.replace(old, new), '<mutant>', 'exec'), module.__dict__)
                W = module
                result = unittest.TestResult()
                Tests(test).run(result)
                self.assertTrue(result.failures, old)
                self.assertFalse(result.errors, old)
        finally:
            W = original

    def test_arbitrary_path_and_flags_rejected(self):
        fake = FakeOS()
        with patch.object(W, 'os', fake):
            port = W._Port()
            self.assertRaises(Exception, port.open, '/private', 0)
            self.assertRaises(ValueError, port.open, '/', 0)
            self.assertEqual(fake.opened, [])


if __name__ == '__main__':
    unittest.main()
