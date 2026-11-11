import contextlib
import io
import json
import subprocess
import unittest
from unittest.mock import patch

import observe_dns as diagnostic


class DnsObservationTests(unittest.TestCase):
    def test_fixed_targets_commands_and_bounds(self):
        calls = []

        def run(argv, **kwargs):
            calls.append((argv, kwargs))
            return subprocess.CompletedProcess(argv, 0, "private-address", "private-error")

        encoded = diagnostic.observe(run)
        value = json.loads(encoded)
        self.assertEqual(value["schemaVersion"], 1)
        self.assertEqual([r["target"] for r in value["observations"]],
                         [label for label, _ in diagnostic.TARGETS])
        self.assertEqual(len(calls), 5)
        self.assertLessEqual(len(encoded), 1024)
        self.assertNotIn("private", encoded)
        self.assertEqual({tuple(args) for args, _ in calls},
                         {("/usr/bin/getent", "ahosts", host) for _, host in diagnostic.TARGETS})
        for _, options in calls:
            self.assertEqual(options, dict(stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                           timeout=5, check=False, shell=False))

    def test_nonzero_is_not_resolution_success(self):
        def run(argv, **_):
            return subprocess.CompletedProcess(argv, 2)
        self.assertEqual(diagnostic.probe(diagnostic.TARGETS[0], run),
                         {"target": "ubuntu_azure", "status": "not_resolved"})

    def test_timeout_and_unavailable_emit_no_exception_data(self):
        for error, expected in [(subprocess.TimeoutExpired("private-command", 5, "private-output"), "timeout"),
                                (OSError("private-path"), "unavailable")]:
            def run(*_, **__):
                raise error
            row = diagnostic.probe(diagnostic.TARGETS[1], run)
            self.assertEqual(row, {"target": "ubuntu_archive", "status": expected})
            self.assertNotIn("private", json.dumps(row))

    def test_mixed_outcomes_stay_attached_to_their_fixed_labels(self):
        def run(argv, **_):
            return subprocess.CompletedProcess(argv, 0 if argv[-1] == "github.com" else 2)
        rows = json.loads(diagnostic.observe(run))["observations"]
        self.assertEqual([r["target"] for r in rows if r["status"] == "resolved"], ["github_control"])

    def test_diagnostics_do_not_override_existing_ci_gate(self):
        with patch.object(diagnostic.sys, "argv", ["observe_dns.py"]), \
             patch.object(diagnostic, "observe", return_value='{"observations":[]}'), \
             contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(diagnostic.main(), 0)
        self.assertEqual(json.loads(output.getvalue()), {"observations": []})

    def test_arguments_refuse_without_echo_or_observation(self):
        with patch.object(diagnostic.sys, "argv", ["observe_dns.py", "private-input"]), \
             patch.object(diagnostic, "observe") as observe, \
             contextlib.redirect_stderr(io.StringIO()) as output:
            self.assertEqual(diagnostic.main(), 2)
        observe.assert_not_called()
        self.assertEqual(output.getvalue(), "dns-observation-arguments\n")


if __name__ == "__main__":
    unittest.main()
