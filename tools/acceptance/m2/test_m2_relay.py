"""The M2 client config must launch the acceptance relay, which records derived fields."""

import importlib.util
import io
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
PREPARE = HERE / "prepare-owner.ps1"
RELAY_PATH = HERE / "acceptance-stdio.py"

spec = importlib.util.spec_from_file_location("acceptance_stdio", RELAY_PATH)
relay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(relay)


class ScriptedChild:
    def __init__(self, handler):
        self.handler = handler
        self.stdin = self._Stdin(self)
        self.stdout = self._Stdout(self)
        self.lines = []

    def kill(self):
        return None

    def accept(self, raw):
        request = json.loads(raw.decode())
        reply = self.handler(request)
        if reply is not None:
            self.lines.append(json.dumps(reply, separators=(",", ":")).encode() + b"\n")

    class _Stdin:
        def __init__(self, child):
            self.child = child

        def write(self, data):
            self.child.accept(data)
            return len(data)

        def flush(self):
            return None

        def close(self):
            return None

    class _Stdout:
        def __init__(self, child):
            self.child = child

        def readline(self, limit=-1):
            if not self.child.lines:
                return b""
            line = self.child.lines.pop(0)
            if isinstance(limit, int) and 0 <= limit < len(line):
                raise AssertionError("fixture frame exceeds the relay limit")
            return line


def tool_result(request, body):
    return {
        "jsonrpc": "2.0",
        "id": request["id"],
        "result": {"content": [{"type": "text", "text": json.dumps(body)}]},
    }


HOST = {
    "id": "secret-host",
    "os": "windows",
    "logical_cpus": 8,
    "ram_bytes": 1024,
    "disk_total_bytes": 2048,
    "disk_available_bytes": 512,
    "execution": {
        "available": True,
        "dedicated": False,
        "isolation": False,
        "matches_current_user": True,
        "elevated": False,
        "mode": "current_user",
        "integrity": "medium",
        "rights": None,
    },
}
RUN = {
    "execution_mode": "current_user",
    "stop_reason": "exited",
    "exit_code": 0,
    "matches_current_user": True,
    "cleanup_confirmed": True,
    "stdout": "cargo 1.95.0 (abcdef0 2026-01-01)\n",
    "stderr": "secret-stderr",
}
ACTIVITY = {
    "id": 2,
    "tool": "run_command",
    "program": "cargo",
    "argc": 1,
    "exit_code": 0,
    "integrity_rid": 8192,
    "outcome": "succeeded",
    "identity_mode": "current_user",
    "stop_reason": "exited",
    "observation_point": "pre_exec",
    "tree_boundary": "windows_job",
    "cleanup_confirmed": True,
    "identity_matches_pin": True,
    "elevation": False,
    "observed_id": None,
    "observed_kind": None,
    "observed_gid": None,
    "rights": None,
    "raw_sid": "S-1-5-21-SECRET",
}


def session_handler(request):
    name = request["params"]["name"]
    arguments = request["params"]["arguments"]
    if name == "list_hosts":
        body = {"hosts": [HOST]}
    elif name == "run_command":
        body = RUN
    elif name == "list_activity" and arguments == {"limit": 20}:
        body = {"activities": [ACTIVITY, {"tool": "discover_tools", "program": "secret"}]}
    elif name == "list_activity" and arguments == {"activity_id": 2}:
        body = {"activities": [ACTIVITY]}
    else:
        raise AssertionError(name)
    return tool_result(request, body)


class PrepareTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=os.environ.get("TMP") or None)
        self.root = Path(self.temp.name)
        self.python = str(self.root / "python.exe")
        self.relay = str(RELAY_PATH)
        self.arxd = str(self.root / "arxd.exe")
        self.client = str(self.root / "client.json")
        self.evidence = str(self.root / "m2-mcp.jsonl")
        self.out = self.root / ".mcp.json"

    def tearDown(self):
        self.temp.cleanup()

    def write_config(self, **overrides):
        values = {
            "Python": self.python,
            "Relay": self.relay,
            "ArxExe": self.arxd,
            "ClientConfig": self.client,
            "Evidence": self.evidence,
            "OutFile": str(self.out),
        }
        values.update(overrides)
        completed = subprocess.run(
            ["pwsh", "-NoProfile", "-File", str(PREPARE), "-ConfigOnly",
             "-Python", values["Python"], "-ArxRelay", values["Relay"], "-ArxExe", values["ArxExe"],
             "-ClientConfig", values["ClientConfig"], "-Evidence", values["Evidence"],
             "-OutFile", values["OutFile"]],
            capture_output=True, text=True, check=False)
        return completed

    def test_prepare_script_has_no_direct_bridge_literal(self):
        source = PREPARE.read_text(encoding="utf-8")
        self.assertNotIn("mcp-stdio", source)

    def test_prepare_routes_through_the_relay(self):
        completed = self.write_config()
        self.assertEqual(completed.returncode, 0, completed.stderr)
        text = self.out.read_text(encoding="utf-8")
        self.assertNotIn("mcp-stdio", text)
        config = json.loads(text)
        server = config["mcpServers"]["arxcovo-owner"]
        self.assertEqual(server["type"], "stdio")
        self.assertEqual(server["command"], self.python)
        self.assertNotEqual(server["command"], self.arxd)
        self.assertEqual(server["args"], [
            "-I", "-B", self.relay,
            "--arxd", self.arxd,
            "--client-config", self.client,
            "--evidence", self.evidence,
        ])
        self.assertNotEqual(server["args"][:2], ["mcp-stdio", "--client-config"])
        parsed = relay.parse_invocation(server["args"][3:])
        self.assertEqual(parsed, {"mode": "relay", "arxd": self.arxd, "client": self.client, "evidence": self.evidence})
        self.assertEqual(relay.child_argv(parsed["arxd"], parsed["client"]),
                         [self.arxd, "mcp-stdio", "--client-config", self.client])

    def test_prepare_rejects_arxd_as_the_server_command(self):
        completed = self.write_config(Python=self.arxd)
        self.assertNotEqual(completed.returncode, 0)
        self.assertFalse(self.out.exists())
        self.assertIn("acceptance relay", completed.stderr)

    def test_prepare_rejects_a_relative_evidence_path(self):
        completed = self.write_config(Evidence="m2-mcp.jsonl")
        self.assertNotEqual(completed.returncode, 0)
        self.assertFalse(self.out.exists())
        self.assertIn("absolute", completed.stderr)


class RelayTests(unittest.TestCase):
    def test_relay_requires_absolute_evidence(self):
        with self.assertRaises(ValueError):
            relay.parse_invocation([
                "--arxd", r"D:\acceptance\arxd.exe",
                "--client-config", r"D:\acceptance\client.json",
                "--evidence", "m2-mcp.jsonl",
            ])

    def test_recorded_session_derives_inventory_rights_and_identity_fields(self):
        root = Path(tempfile.mkdtemp(dir=os.environ.get("TMP") or None))
        self.addCleanup(lambda: __import__("shutil").rmtree(root))
        evidence = root / "m2-mcp.jsonl"
        calls = [
            {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
             "params": {"name": "list_hosts", "arguments": {}}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
             "params": {"name": "run_command", "arguments": {
                 "tool": "cargo", "args": ["--version"], "timeout_s": 10, "output_bytes": 4096}}},
            {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
             "params": {"name": "list_activity", "arguments": {"limit": 20}}},
            {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
             "params": {"name": "list_activity", "arguments": {"activity_id": 2}}},
        ]
        inbound = io.BytesIO("".join(json.dumps(call) + "\n" for call in calls).encode())
        outbound = io.BytesIO()
        relay.relay(ScriptedChild(session_handler), evidence, inbound, outbound)
        rows = [json.loads(line) for line in evidence.read_text(encoding="utf-8").splitlines()]
        self.assertEqual([row["tool"] for row in rows],
                         ["list_hosts", "run_command", "list_activity", "list_activity"])
        host = rows[0]["result"]["hosts"][0]
        self.assertTrue(host["inventory_observed"])
        self.assertTrue(host["execution"]["rights_is_null"])
        self.assertEqual(host["execution"]["mode"], "current_user")
        self.assertEqual(rows[1]["result"]["cargo_version"], "cargo 1.95.0 (abcdef0 2026-01-01)")
        self.assertTrue(rows[2]["result"]["activities"][0]["raw_identity_and_rights_are_null"])
        self.assertEqual(rows[3]["result"]["activities"][0]["id"], 2)
        text = evidence.read_text(encoding="utf-8")
        self.assertNotIn("secret-host", text)
        self.assertNotIn("S-1-5-21-SECRET", text)
        self.assertNotIn("secret-stderr", text)
        self.assertEqual(len(rows[2]["result"]["activities"]), 1)

    def test_missing_inventory_and_present_rights_stay_false(self):
        observed = relay.safe_result("list_hosts", {"hosts": [{
            "os": "windows",
            "logical_cpus": 1,
            "ram_bytes": 1,
            "disk_total_bytes": 1,
            "execution": {"available": True, "dedicated": False, "isolation": False,
                          "matches_current_user": True, "elevated": False,
                          "mode": "current_user", "integrity": "medium", "rights": {}},
        }]})
        host = observed["hosts"][0]
        self.assertFalse(host["inventory_observed"])
        self.assertFalse(host["execution"]["rights_is_null"])
        raw = relay.safe_result("list_activity", {"activities": [{
            "id": 2, "tool": "run_command", "program": "cargo", "argc": 1, "exit_code": 0,
            "integrity_rid": 8192, "outcome": "succeeded", "identity_mode": "current_user",
            "stop_reason": "exited", "observation_point": "pre_exec", "tree_boundary": "windows_job",
            "cleanup_confirmed": True, "identity_matches_pin": True, "elevation": False,
            "observed_id": "secret", "observed_kind": None, "observed_gid": None, "rights": None,
        }]})
        self.assertFalse(raw["activities"][0]["raw_identity_and_rights_are_null"])


if __name__ == "__main__":
    unittest.main()
