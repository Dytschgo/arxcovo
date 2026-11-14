"""Pure manifest/ELF rejection tests. No waiting image or native adapter runs."""

import copy
import json
import struct
import sys
import unittest
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
