"""Pure manifest/ELF rejection tests. No waiting image or native adapter runs."""

import copy
import contextlib
import io
import json
import struct
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import build


def elf():
    # One static executable with a bounded symbol table containing _start.
    data = bytearray(512)
    data[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HHIQQQIHHHHHH", data, 16,
                     2, 62, 1, 4096, 64, 128, 0, 64, 56, 1, 64, 4, 1)
    struct.pack_into("<IIQQQQQQ", data, 64, 1, 5, 0, 4096, 4096, 512, 512, 4096)
    names = b"\0.shstrtab\0.strtab\0.symtab\0"
    data[400:400 + len(names)] = names
    data[440:448] = b"\0_start\0"
    struct.pack_into("<IIQQQQIIQQ", data, 192, 1, 3, 0, 0, 400, len(names), 0, 0, 1, 0)
    struct.pack_into("<IIQQQQIIQQ", data, 256, 11, 3, 0, 0, 440, 8, 0, 0, 1, 0)
    struct.pack_into("<IIQQQQIIQQ", data, 320, 19, 2, 0, 0, 448, 48, 2, 0, 8, 24)
    struct.pack_into("<IBBHQQ", data, 472, 1, 18, 0, 1, 4096, 1)
    return data


class InventoryTests(unittest.TestCase):
    def test_refusal_labels_distinguish_gates_without_exception_text(self):
        self.assertEqual(build.refusal_label(ValueError("compiler identity mismatch")), "compiler-identity")
        self.assertEqual(build.refusal_label(ValueError("Linux candidate differs from independently reviewed image")), "image-pin")
        for error in (ValueError("PRIVATE-MARKER/path"), OSError("PRIVATE-MARKER/path"),
                      build.subprocess.CalledProcessError(1, ["PRIVATE-MARKER/path"], output=b"PRIVATE-MARKER")):
            label = build.refusal_label(error)
            self.assertIn(label, ("format", "io", "compiler-command"))
            self.assertNotIn("PRIVATE-MARKER", label)

    def test_root_refuses_before_any_compiler_or_source_read(self):
        for uid, euid in ((0, 1000), (1000, 0), (0, 0)):
            with patch.object(sys, 'argv', ['build.py', 'build']), \
                 patch.object(build, 'os', SimpleNamespace(name='posix', getuid=lambda: uid, geteuid=lambda: euid)), \
                 patch.object(build, 'command') as command, patch.object(build, 'read') as read:
                with self.assertRaisesRegex(ValueError, 'unprivileged'):
                    build.main()
                command.assert_not_called()
                read.assert_not_called()

    def test_linux_candidate_compares_independent_pin_never_updates_it(self):
        recipe = build.read(build.ROOT / 'recipe.json')
        source = build.read(build.ROOT / 'helper.rs')
        pin = json.loads(build.read(build.ROOT / 'image-pin.json'))
        identity = pin['compiler_identity'].replace('x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu')
        data = b'synthetic static image'
        # Synthetic authorized identity stands in for the independently held pin.
        pin['image_sha256'], pin['image_size'] = build.sha(data), len(data)
        for differs in (False, True):
            def read(path):
                return {'recipe.json': recipe, 'helper.rs': source,
                        'image-pin.json': json.dumps(pin).encode(),
                        'a4-helper': data + (b'!' if differs else b'')}[path.name]
            with patch.object(sys, 'argv', ['build.py', 'linux-candidate']), \
                 patch.object(sys, 'platform', 'linux'), \
                 patch.object(build, 'os', SimpleNamespace(name='posix', getuid=lambda: 1000, geteuid=lambda: 1000)), \
                 patch.object(build, 'read', side_effect=read), \
                 patch.object(build, 'command', side_effect=[identity, '/fixed/sysroot', '']) as command, \
                 patch.object(build, 'regular'), patch.object(Path, 'mkdir'), \
                 patch.object(Path, 'write_text') as write, \
                 patch.object(build, 'tool_sha', return_value='a' * 64), \
                 patch.object(build, 'inventory', return_value=pin['symbols']):
                if differs:
                    output = io.StringIO()
                    with contextlib.redirect_stdout(output), self.assertRaisesRegex(ValueError, 'independently reviewed image'):
                        build.main()
                    report = json.loads(output.getvalue())
                    self.assertEqual(set(report), {"kind", "matches", "image_sha256", "image_size", "symbol_count", "symbol_names_match"})
                    self.assertEqual(report["kind"], "a4-compile-comparison")
                    self.assertEqual(report["image_size"], len(data) + 1)
                    self.assertFalse(report["matches"]["image_sha256"])
                    self.assertFalse(report["matches"]["image_size"])
                    self.assertTrue(report["matches"]["source_sha256"])
                    self.assertTrue(report["symbol_names_match"])
                else:
                    build.main()
                self.assertEqual(command.call_count, 3)
                self.assertIn('helper.rs', command.call_args.args[0])
                write.assert_called_once()
                record = json.loads(write.call_args.args[0])
                self.assertEqual(record['compiler_identity'], identity)
                self.assertEqual(record['compiler_sha256'], 'a' * 64)
                self.assertEqual(record['source_sha256'], pin['source_sha256'])

    def test_fixed_static_entry(self):
        self.assertEqual(build.inventory(elf()), [
            {"name": "_start", "type": 18, "value": 4096, "size": 1}])

    def test_dynamic_tls_and_writable_executable_refused(self):
        for kind, flags in [(2, 4), (3, 4), (7, 4), (1, 7)]:
            with self.subTest(kind=kind, flags=flags):
                data = elf()
                struct.pack_into("<II", data, 64, kind, flags)
                with self.assertRaises(ValueError):
                    build.inventory(data)

    def test_undefined_and_forbidden_symbols_refused(self):
        data = elf()
        struct.pack_into("<H", data, 478, 0)
        with self.assertRaises(ValueError):
            build.inventory(data)
        data = elf()
        data[441:447] = b"forkxx"
        with self.assertRaises(ValueError):
            build.inventory(data)

    def test_truncation_bad_ranges_and_entry_refused(self):
        for length in [0, 6, 63, 127, 400, 490]:
            with self.assertRaises(ValueError):
                build.inventory(elf()[:length])
        data = elf()
        struct.pack_into("<Q", data, 24, 9999)
        with self.assertRaises(ValueError):
            build.inventory(data)
        data = elf()
        struct.pack_into("<Q", data, 40, 2**64 - 1)
        with self.assertRaises(ValueError):
            build.inventory(data)

    def test_image_hash_cannot_authorize_unknown_source_or_flags(self):
        recipe = json.loads(build.read(build.ROOT / "recipe.json"))
        for field, value in [("source_sha256", "0" * 64), ("flags", [])]:
            altered = copy.deepcopy(recipe)
            altered[field] = value
            def read(path):
                if path.name == "recipe.json":
                    return json.dumps(altered).encode()
                if path.name == "helper.rs":
                    return (build.ROOT / "helper.rs").read_bytes()
                self.fail("must refuse before reading any image pin")
            with patch.object(sys, "argv", ["build.py", "verify"]), \
                 patch.object(build, "read", side_effect=read), \
                 patch.object(build, "command") as command:
                with self.assertRaises(ValueError):
                    build.main()
                command.assert_not_called()

    def test_compiler_identity_refused_before_compile(self):
        with patch.object(sys, "argv", ["build.py", "build"]), \
             patch.object(build, "command", return_value="unknown compiler") as command:
            with self.assertRaisesRegex(ValueError, "compiler identity"):
                build.main()
            self.assertEqual(command.call_count, 1)

    def test_every_image_provenance_field_is_verified(self):
        pin = json.loads(build.read(build.ROOT / "image-pin.json"))
        pin["image_sha256"] = build.sha(b"synthetic image")
        pin["image_size"] = len(b"synthetic image")
        original_read = build.read
        for field, replacement in [("image_sha256", "0" * 64), ("image_size", 999),
                                   ("symbols", []), ("compiler_sha256", "1" * 64),
                                   ("linker_sha256", "2" * 64)]:
            altered = copy.deepcopy(pin)
            altered[field] = replacement
            def read(path):
                if path.name == "image-pin.json":
                    return json.dumps(altered).encode()
                if path.name == "a4-helper":
                    return b"synthetic image"
                return original_read(path)
            def tool_sha(path):
                return pin["linker_sha256" if "lld" in path.name else "compiler_sha256"]
            with self.subTest(field=field), \
                 patch.object(sys, "argv", ["build.py", "verify"]), \
                 patch.object(build, "read", side_effect=read), \
                 patch.object(build, "tool_sha", side_effect=tool_sha), \
                 patch.object(build, "inventory", return_value=pin["symbols"]), \
                 patch.object(build, "command", side_effect=[pin["compiler_identity"], str(build.ROOT)]):
                with self.assertRaisesRegex(ValueError, "build pin mismatch"):
                    build.main()


if __name__ == "__main__":
    unittest.main()
