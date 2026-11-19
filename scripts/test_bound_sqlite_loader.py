"""Pure loader-boundary tests; no Cargo, compiler or native loading."""
import importlib.util
import os
from pathlib import Path
import stat
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('bound_sqlite', Path(__file__).with_name('check-bound-sqlite.py'))
bound = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bound)

class LoaderBoundary(unittest.TestCase):
    def test_clean_parent_and_benign_names(self):
        bound.loader_environment({'PATH': 'ordinary', 'CARGO_HOME': 'ordinary'})
        self.assertFalse(bound.loader_key('MY_DYLD_PATH'))

    def test_parent_loader_keys_refuse_regardless_of_value(self):
        for key in ['DYLD_FALLBACK_LIBRARY_PATH', 'DYLD_INSERT_LIBRARIES',
                    'DYLD_LIBRARY_PATH', 'DYLD_FRAMEWORK_PATH',
                    'DYLD_FALLBACK_FRAMEWORK_PATH', 'DYLD_UNKNOWN',
                    'dyld_fallback_library_path', 'LD_PRELOAD', 'ld_preload']:
            for value in ['', '0', 'synthetic-path']:
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    bound.loader_environment({key: value})

    def setUp(self):
        # Explicit scratch, including hosted runner tests; never default owner Temp.
        self.folder = tempfile.TemporaryDirectory(dir=os.environ['TMP'])
        self.base = Path(self.folder.name)
        self.root = self.base / 'workspace'
        self.home = self.base / 'cargo-home'
        self.root.mkdir(); self.home.mkdir()

    def tearDown(self):
        self.folder.cleanup()

    def check_config(self, location, text):
        folder = {'workspace': self.root / '.cargo',
                  'ancestor': self.base / '.cargo', 'home': self.home}[location]
        folder.mkdir(exist_ok=True)
        path = folder / 'config.toml'
        path.write_text(text, encoding='utf-8')
        try:
            with patch.dict(os.environ, {'CARGO_HOME': str(self.home)}, clear=True):
                bound.configs(self.root, loader=True)
        finally:
            path.unlink()

    def test_config_string_force_and_relative_refuse_at_every_location(self):
        for location in ['workspace', 'ancestor', 'home']:
            for value in ['""', '"0"', '{ value="synthetic", force=true }',
                          '{ value="synthetic", relative=true }']:
                with self.subTest(location=location, value=value), self.assertRaises(ValueError):
                    self.check_config(location, '[env]\nDYLD_FALLBACK_LIBRARY_PATH = '+value+'\n')

    def test_config_other_loader_and_include_refuse(self):
        for text in ['[env]\nLD_PRELOAD=""\n', '[env]\ndyld_unknown="0"\n',
                     'include=[]\n', 'include="unread.toml"\n']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                self.check_config('home', text)

    def test_benign_config_passes(self):
        for location in ['workspace', 'ancestor', 'home']:
            self.check_config(location, '[env]\nBENIGN={ value="ordinary", force=true }\n')

    def test_malformed_and_non_table_config_refuse(self):
        for text in ['[env\n', 'env=[]\n']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                self.check_config('home', text)

    def test_reparse_metadata_refuses_before_read(self):
        fake = SimpleNamespace(parents=[], exists=lambda: True,
                               lstat=lambda: SimpleNamespace(st_mode=stat.S_IFDIR,
                                                             st_file_attributes=1024))
        with self.assertRaises(ValueError):
            bound.safe(fake)

if __name__ == '__main__':
    unittest.main()
