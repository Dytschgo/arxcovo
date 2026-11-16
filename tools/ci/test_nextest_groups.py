"""Keep the CI resource limit narrow and prevent it becoming a test bypass."""
import copy
from pathlib import Path
import tomllib
import unittest

CONFIG = Path(__file__).resolve().parents[2] / ".config" / "nextest.toml"
GROUP = "arxd-three-node-cluster"
FILTER = "package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/)"


def validate(config):
    # Exact schema excludes skips, retries, timeout changes and global scheduling.
    assert set(config) == {"test-groups", "profile"}
    assert config["test-groups"] == {GROUP: {"max-threads": 1}}
    assert config["profile"] == {"default": {"overrides": [
        {"filter": FILTER, "test-group": GROUP}
    ]}}


class NextestResourceTests(unittest.TestCase):
    def setUp(self):
        self.config = tomllib.loads(CONFIG.read_text(encoding="utf-8"))

    def test_repository_resource_contract(self):
        validate(self.config)

    def test_rejects_weakened_or_broadened_resource_limits(self):
        for key, value in [("max-threads", 2), ("max-threads", 0), ("retries", 1)]:
            with self.subTest(key=key, value=value):
                candidate = copy.deepcopy(self.config)
                candidate["test-groups"][GROUP][key] = value
                with self.assertRaises(AssertionError):
                    validate(candidate)
        for key, value in [("filter", "all()"), ("test-group", "@global"),
                           ("retries", 1), ("slow-timeout", "60s"),
                           ("threads-required", 4)]:
            with self.subTest(key=key):
                candidate = copy.deepcopy(self.config)
                candidate["profile"]["default"]["overrides"][0][key] = value
                with self.assertRaises(AssertionError):
                    validate(candidate)
        candidate = copy.deepcopy(self.config)
        candidate["profile"]["default"]["default-filter"] = "none()"
        with self.assertRaises(AssertionError):
            validate(candidate)


if __name__ == "__main__":
    unittest.main()
