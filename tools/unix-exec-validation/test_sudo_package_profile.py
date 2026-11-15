"""Pure/injected adapter tests. No real Linux read entry is ever invoked."""
import ast
import contextlib
import copy
import hashlib
import io
from pathlib import Path
import stat
import struct
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import sudo_package_profile as p


def elf(index):
    size = p._PINS[index][0]
    data = bytearray(size)
    data[:7] = b'\x7fELF\x02\x01\x01'
    struct.pack_into('<HHI', data, 16, 3, 62, 1)
    struct.pack_into('<Q', data, 32, 64)
    struct.pack_into('<HHH', data, 52, 64, 56, 2)
    struct.pack_into('<IIQQQQQQ', data, 64, 1, 4, 0, 0x1000, 0, size, size, 4096)
    strings = bytearray(b'\0')
    tags = []
    for name in p._NEEDED[index]:
        tags.append((1, len(strings)))
        strings.extend(name.encode() + b'\0')
    for tag, value in zip((15, 29), p._SEARCH[index]):
        if value is not None:
            tags.append((tag, len(strings)))
            strings.extend(value.encode() + b'\0')
    tags.extend(((5, 0x1400), (10, len(strings)), (0, 0)))
    dynsize = len(tags) * 16
    struct.pack_into('<IIQQQQQQ', data, 120, 2, 4, 256, 0x1100, 0, dynsize, dynsize, 8)
    for i, pair in enumerate(tags):
        struct.pack_into('<qQ', data, 256 + 16 * i, *pair)
    data[1024:1024 + len(strings)] = strings
    return bytes(data)


def info(mode, inode, size=0):
    return SimpleNamespace(st_mode=mode, st_uid=0, st_gid=0, st_nlink=1,
                           st_dev=1, st_ino=inode, st_size=size,
                           st_mtime_ns=1, st_ctime_ns=1)


class FakeOS:
    O_RDONLY = 0
    O_NOFOLLOW = 0x20000
    O_CLOEXEC = 0x80000
    O_NONBLOCK = 0x800
    O_DIRECTORY = 0x10000

    def __init__(self, payloads):
        self.nodes = {}
        for i, path in enumerate(((), ('usr',), ('usr', 'bin'),
                                  ('usr', 'libexec'), ('usr', 'libexec', 'sudo'))):
            self.nodes[path] = [info(stat.S_IFDIR | 0o755, i + 1), b'']
        for i, (path, data) in enumerate(zip(p._PATHS, payloads)):
            self.nodes[path] = [info(stat.S_IFREG | (0o4755 if i == 0 else 0o644),
                                     i + 20, len(data)), data]
        for i, name in enumerate(p._LINKS):
            self.nodes[p._PATHS[2][:-1] + (name,)] = [
                info(stat.S_IFLNK | 0o777, i + 30, len(p._TARGET)), p._TARGET]
        self.fds = {}
        self.nextfd = 100
        self.calls = []
        self.hook = lambda *args: None

    def path(self, parent, name):
        if parent is None:
            assert name == '/'
            return ()
        assert name and '/' not in name and name not in ('.', '..')
        return self.fds[parent][0] + (name,)

    def open(self, name, flags, *, dir_fd):
        path = self.path(dir_fd, name)
        self.calls.append(('open', path, flags))
        self.hook('open', path)
        node = self.nodes[path]
        required = self.O_NOFOLLOW | self.O_CLOEXEC | self.O_NONBLOCK
        assert flags & required == required and flags & 3 == self.O_RDONLY
        if stat.S_ISDIR(node[0].st_mode):
            assert flags & self.O_DIRECTORY
        if stat.S_ISLNK(node[0].st_mode):
            raise OSError('PRIVATE')
        fd = self.nextfd
        self.nextfd += 1
        self.fds[fd] = [path, node, 0]
        return fd

    def fstat(self, fd):
        self.hook('fstat', self.fds[fd][0])
        return copy.copy(self.fds[fd][1][0])

    def read(self, fd, count):
        path, node, pos = self.fds[fd]
        assert 1 <= count <= 4096
        self.calls.append(('read', path, count))
        self.hook('read', path)
        block = node[1][pos:pos + count]
        self.fds[fd][2] += len(block)
        return block

    def stat(self, name, *, dir_fd, follow_symlinks):
        assert follow_symlinks is False
        path = self.path(dir_fd, name)
        self.hook('stat', path)
        return copy.copy(self.nodes[path][0])

    def readlink(self, parent, name, limit):
        assert limit == len(p._TARGET) + 1
        path = self.path(parent, name)
        self.hook('readlink', path)
        return self.nodes[path][1]

    def close(self, fd):
        del self.fds[fd]
        self.hook('close', ())


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.payloads = tuple(elf(i) for i in range(3))

    def synthetic_pins(self):
        # Only success-path fixture setup: never a production digest selector.
        return patch.object(p, '_PINS', tuple((len(b), hashlib.sha256(b).hexdigest())
                                             for b in self.payloads))

    def collect(self, fake):
        result = p._collect(p._LinuxPorts(fake, fake.readlink))
        self.assertFalse(fake.fds, 'all held/reopened descriptors must close')
        return result

    def test_literals_do_not_admit_synthetic_or_caller_hashes(self):
        self.assertEqual(p.compare(self.payloads), p._result())
        for payloads in ((), list(self.payloads), self.payloads + (b'',),
                         (None,) * 3):
            self.assertEqual(p.compare(payloads), p._result())
        with self.synthetic_pins():
            self.assertTrue(p.compare(self.payloads)['candidate_match'])
            bad = bytearray(self.payloads[0]); bad[-1] ^= 1
            self.assertEqual(p.compare((bytes(bad),) + self.payloads[1:]), p._result())

    def test_parser_facts_and_mutants(self):
        for i, data in enumerate(self.payloads):
            self.assertEqual(p._elf(data), (p._NEEDED[i], p._SEARCH[i]))
        changes = [(4, b'\x01'), (5, b'\x02'), (18, b'\xb7\x00'),
                   (54, struct.pack('<H', 55)), (56, struct.pack('<H', 33)),
                   (32, struct.pack('<Q', 2**63)),
                   (120, struct.pack('<I', 1)),
                   (152, struct.pack('<Q', 4096)),
                   (1025, b'/evil'), (1024 + 1, b'X')]
        for off, value in changes:
            with self.subTest(off=off, value=value):
                bad = bytearray(self.payloads[0]); bad[off:off+len(value)] = value
                with self.assertRaises(Exception):
                    facts = p._elf(bytes(bad))
                    p._need(facts == (p._NEEDED[0], p._SEARCH[0]))
        for data in (b'', self.payloads[0][:63], self.payloads[0][:1025], b'X' * 588081):
            with self.assertRaises(Exception):
                p._elf(data)
        # Search directory drift, including empty/current-dir and $ORIGIN syntax.
        for search in ('/tmp', '', '$ORIGIN', '/usr/libexec/sudo:/tmp'):
            with patch.object(p, '_SEARCH', ((None, search),) + p._SEARCH[1:]):
                bad = elf(0)
            with self.synthetic_pins():
                self.assertEqual(p.compare((bad,) + self.payloads[1:]), p._result())

    def test_adapter_success_fixed_flags_reopens_and_budget(self):
        fake = FakeOS(self.payloads)
        with self.synthetic_pins():
            result = self.collect(fake)
        self.assertTrue(result['candidate_match'])
        self.assertFalse(result['qualified'])
        opens = [c for c in fake.calls if c[0] == 'open']
        self.assertEqual(len(opens), 16)
        self.assertEqual({c[1] for c in opens}, set(fake.nodes) - {
            p._PATHS[2][:-1] + (name,) for name in p._LINKS})
        self.assertLessEqual(sum(c[2] for c in fake.calls if c[0] == 'read'), p._TOTAL)

    def test_missing_type_owner_mode_nlink_size(self):
        cases = [('st_mode', stat.S_IFIFO | 0o644), ('st_mode', stat.S_IFDIR | 0o755),
                 ('st_mode', stat.S_IFREG | 0o666), ('st_uid', 1000),
                 ('st_nlink', 2), ('st_size', 588081), ('st_size', -1)]
        for field, value in cases:
            fake = FakeOS(self.payloads)
            setattr(fake.nodes[p._PATHS[0]][0], field, value)
            with self.subTest(field=field, value=value), self.synthetic_pins():
                self.assertEqual(self.collect(fake), p._result())
        for path in (p._PATHS[0], ('usr',), p._PATHS[2][:-1] + (p._LINKS[0],)):
            fake = FakeOS(self.payloads); del fake.nodes[path]
            with self.synthetic_pins():
                self.assertEqual(self.collect(fake), p._result())
        fake = FakeOS(self.payloads); fake.nodes[('usr',)][0].st_mode |= 0o020
        self.assertEqual(self.collect(fake), p._result())

    def test_links_dangling_redirected_oversize_owner_type_swap(self):
        for name in p._LINKS:
            path = p._PATHS[2][:-1] + (name,)
            for value in ('missing', '../sudoers.so', '/tmp/evil', 'x'*5000,
                          'libsudo_util.so.0', ''):
                fake = FakeOS(self.payloads); fake.nodes[path][1] = value
                with self.synthetic_pins():
                    self.assertEqual(self.collect(fake), p._result())
            for field, value in (('st_uid', 1), ('st_nlink', 2),
                                 ('st_mode', stat.S_IFREG | 0o644), ('st_size', 999)):
                fake = FakeOS(self.payloads); setattr(fake.nodes[path][0], field, value)
                self.assertEqual(self.collect(fake), p._result())
            fake = FakeOS(self.payloads)
            def swap(event, at):
                if event == 'readlink' and at == path:
                    fake.nodes[path][0].st_ino += 1
            fake.hook = swap
            self.assertEqual(self.collect(fake), p._result())

    def test_content_truncation_overread_and_metadata_swap(self):
        for value in (self.payloads[0][:-1], self.payloads[0] + b'x',
                      b'Z' + self.payloads[0][1:]):
            fake = FakeOS(self.payloads); fake.nodes[p._PATHS[0]][1] = value
            with self.synthetic_pins():
                self.assertEqual(self.collect(fake), p._result())
        for field in ('st_dev', 'st_ino', 'st_mtime_ns', 'st_ctime_ns', 'st_gid'):
            fake = FakeOS(self.payloads)
            def drift(event, path):
                if event == 'read' and path == p._PATHS[0]:
                    node = fake.nodes[path][0]; setattr(node, field, getattr(node, field) + 1)
            fake.hook = drift
            with self.synthetic_pins():
                self.assertEqual(self.collect(fake), p._result())
        fake = FakeOS(self.payloads)
        def replace(event, path):
            if event == 'read' and path == p._PATHS[0]:
                fake.nodes[('usr',)] = copy.deepcopy(fake.nodes[('usr',)])
                fake.nodes[('usr',)][0].st_ino += 1
        fake.hook = replace
        with self.synthetic_pins():
            self.assertEqual(self.collect(fake), p._result())

    def test_budget_errors_and_private_leak(self):
        with patch.object(p, '_TOTAL', 1), self.synthetic_pins():
            self.assertEqual(self.collect(FakeOS(self.payloads)), p._result())
        for event in ('open', 'fstat', 'read', 'stat', 'readlink', 'close'):
            fake = FakeOS(self.payloads)
            def fail(kind, path):
                if kind == event:
                    raise OSError('PRIVATE /owner/data uid=123 inode=456')
            fake.hook = fail
            with self.synthetic_pins(), contextlib.redirect_stdout(io.StringIO()) as out:
                result = self.collect(fake)
            self.assertEqual(result, p._result())
            self.assertEqual(out.getvalue(), '')
            self.assertNotIn('PRIVATE', str(result))

    def test_required_flags_and_elf_table_bounds(self):
        for flag in ('O_NOFOLLOW', 'O_CLOEXEC', 'O_NONBLOCK', 'O_DIRECTORY'):
            fake = FakeOS(self.payloads)
            setattr(fake, flag, 0)
            with self.assertRaises(Exception):
                p._LinuxPorts(fake, fake.readlink)
        data = self.payloads[0]
        # Oversized dynamic table, missing terminator, repeated string table,
        # out-of-range string reference and unbounded dynamic string table.
        changes = [(152, struct.pack('<Q', 2064)),
                   (256 + 8 * 16, struct.pack('<qQ', 1, 1)),
                   (256, struct.pack('<qQ', 5, 0x1400)),
                   (264, struct.pack('<Q', 2**63)),
                   (256 + 7 * 16 + 8, struct.pack('<Q', 8193))]
        for offset, value in changes:
            bad = bytearray(data); bad[offset:offset + len(value)] = value
            with self.subTest(offset=offset), self.assertRaises(Exception):
                p._elf(bytes(bad))

    def test_native_readlink_buffer_with_fake_libc(self):
        # ctypes allocates only an in-memory buffer. No CDLL or real syscall.
        import ctypes
        class Call:
            def __call__(self, parent, name, buffer, limit):
                self.seen = (parent, name, len(buffer), limit)
                buffer.raw = p._TARGET.encode() + b'X'
                return self.length
        call = Call()
        link = p._ReadlinkAt(ctypes, SimpleNamespace(readlinkat=call))
        call.length = len(p._TARGET)
        self.assertEqual(link(99, p._LINKS[0], len(p._TARGET) + 1), p._TARGET)
        self.assertEqual(call.seen, (99, p._LINKS[0].encode(),
                                     len(p._TARGET) + 1, len(p._TARGET) + 1))
        for length in (-1, len(p._TARGET) + 1, 999):
            call.length = length
            with self.assertRaises(Exception):
                link(99, p._LINKS[0], len(p._TARGET) + 1)
        with self.assertRaises(Exception):
            link(99, '../evil', len(p._TARGET) + 1)
        with self.assertRaises(Exception):
            link(99, p._LINKS[0], 4096)

    def test_cli_and_import_callgraph_have_no_read_edge(self):
        source = Path(p.__file__).read_text(encoding='utf-8-sig')
        tree = ast.parse(source)
        cli = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == '_cli')
        calls = {ast.unparse(n.func) for n in ast.walk(cli) if isinstance(n, ast.Call)}
        self.assertEqual(calls, {'print', 'json.dumps', '_result'})
        for node in tree.body:
            if isinstance(node, (ast.FunctionDef, ast.ClassDef, ast.If)):
                continue
            self.assertFalse(any(isinstance(n, ast.Call) and
                                 ast.unparse(n.func) in ('_collect', '_read_linux', '_LinuxPorts')
                                 for n in ast.walk(node)))
        self.assertNotIn('import subprocess', source)
        with patch.object(p, '_read_linux', side_effect=AssertionError('called')):
            with contextlib.redirect_stdout(io.StringIO()) as out:
                self.assertEqual(p._cli(), 2)
        import json
        self.assertEqual(json.loads(out.getvalue()), p._result('root_reads_not_authorized'))


if __name__ == '__main__':
    unittest.main()
