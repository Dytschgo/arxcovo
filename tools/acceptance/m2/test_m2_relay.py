"""The M2 client config must launch the acceptance relay, which records derived fields."""

import importlib.util
import io
import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from queue import Queue

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

    def poll(self):
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


def junction(test, link, target):
    result = subprocess.run(["cmd.exe", "/d", "/c", "mklink", "/J", str(link), str(target)],
                            capture_output=True, text=True, timeout=5)
    test.assertEqual(result.returncode, 0, result.stderr)
    test.addCleanup(link.rmdir)


@unittest.skipUnless(os.name == "nt", "Windows launcher")
class PrepareTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=os.environ.get("TMP") or None)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.python = str(self.root / "python.exe")
        self.relay = str(RELAY_PATH)
        self.arxd = str(self.root / "arxd.exe")
        self.client = str(self.root / "client.json")
        self.evidence = str(self.root / "m2-mcp.jsonl")
        self.out = self.root / ".mcp.json"
        for path in (self.python, self.arxd, self.client):
            Path(path).touch()

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


    def test_prepare_refuses_junctions_for_inputs_and_output(self):
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "config.json").write_bytes(b"original")
        alias = self.root / "alias"
        junction(self, alias, outside)
        for override in (
            {"OutFile": str(alias / ".mcp.json")},
            {"ClientConfig": str(alias / "config.json")},
            {"Evidence": str(alias / "evidence.jsonl")},
        ):
            with self.subTest(argument=next(iter(override))):
                completed = self.write_config(**override)
                self.assertNotEqual(completed.returncode, 0)
                self.assertFalse(self.out.exists())
        self.assertEqual({p.name: p.read_bytes() for p in outside.iterdir()}, {"config.json": b"original"})

    def test_prepare_refuses_hardlinks_and_preserves_existing_output(self):
        os.link(self.client, self.root / "second-name")
        self.assertNotEqual(self.write_config().returncode, 0)
        self.assertFalse(self.out.exists())
        (self.root / "second-name").unlink()
        self.out.write_bytes(b"original")
        self.assertNotEqual(self.write_config().returncode, 0)
        self.assertEqual(self.out.read_bytes(), b"original")


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
        child = ScriptedChild(session_handler)
        with relay.checked_file(evidence, create=True) as trace:
            self.assertTrue(relay.relay(child, trace, relay.client_frames(child, inbound), outbound))
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


# Test the production entry point with a synthetic child and real anonymous
# pipes. Neither the fixture nor its launcher invokes arxd.
CHILD = r"""
import json
import os
import sys
import time
from pathlib import Path

mode = sys.argv[1]
if mode in ("idle", "idle_zero"):
    time.sleep(0.2)
    sys.exit(0 if mode == "idle_zero" else 23)
if mode == "stall_write":
    time.sleep(5)
    sys.exit(24)
if mode == "clean_eof":
    sys.exit(0 if not sys.stdin.buffer.read() else 24)

raw = sys.stdin.buffer.readline(8193)
request = json.loads(raw)
if mode.startswith("boundary"):
    expected = "a" if mode == "boundary_ascii" else "é"
    if len(raw) != 8192 or not raw.endswith(b"\n") or expected not in request["params"]["text"]:
        sys.exit(24)
    if expected.encode() not in raw:
        sys.exit(25)
    sys.exit(23)
if mode == "outstanding":
    sys.exit(23)
if mode == "partial_reply":
    os.write(1, b'{"jsonrpc":"2.0","id":1,')
    os.close(1)
    # Make pipe EOF observable before the process handle signals.
    time.sleep(0.5)
    os._exit(23)
if mode == "stall_read":
    time.sleep(5)
    sys.exit(24)
if mode == "oversize":
    sys.exit(24)

result = {"content": [{"type": "text", "text": json.dumps({"hosts": []})}]}
print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
if mode == "after_answer":
    # Wait for the fixture to observe the sanitized reply while leaving stdin open.
    marker = Path(sys.argv[2])
    deadline = time.monotonic() + 10
    while not marker.exists() and time.monotonic() < deadline:
        time.sleep(0.02)
    sys.exit(23 if marker.exists() else 24)
sys.stdin.buffer.read()
"""

HARNESS = """
import runpy
import subprocess
import sys

source, fixture, mode, marker, arxd, client, evidence = sys.argv[1:]
entrypoint = runpy.run_path(source)["entrypoint"]
bridge = entrypoint.__globals__
bridge["RPC_TIMEOUT"] = 0.75 if mode in ("stall_read", "stall_write") else 5
bridge["open_child"] = lambda *_: subprocess.Popen(
    [sys.executable, "-I", "-B", "-c", fixture, mode, marker],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
)
sys.argv = [source, "--arxd", arxd, "--client-config", client, "--evidence", evidence]
sys.exit(entrypoint())
"""
PIPE_TEST_TIMEOUT = 12
FAILURE = b"arxcovo-acceptance-bridge-failed"
LIST_HOSTS = b'{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_hosts"}}\n'


class PipeTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(dir=os.environ.get("TMP") or None)
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.arxd = self.root / "arxd.exe"
        self.client = self.root / "client.json"
        self.evidence = self.root / "evidence.jsonl"
        self.marker = self.root / "reply-observed"
        self.arxd.touch()
        self.client.touch()

    def start_relay(self, mode, *, evidence=None):
        child = subprocess.Popen(
            [sys.executable, "-I", "-B", "-c", HARNESS, str(RELAY_PATH), CHILD, mode,
             str(self.marker), str(self.arxd), str(self.client), str(evidence or self.evidence)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(self.close_process, child)
        return child

    @staticmethod
    def close_process(process):
        if process.poll() is None:
            process.kill()
        process.wait(timeout=5)
        for pipe in (process.stdin, process.stdout, process.stderr):
            if pipe is not None:
                pipe.close()

    def request(self, child, frame):
        # Keep the input pipe open so a child failure cannot hide behind client EOF.
        child.stdin.write(frame)
        child.stdin.flush()

    def expect_failure(self, child, status, *, timeout=PIPE_TEST_TIMEOUT):
        self.assertEqual(child.wait(timeout=timeout), status)
        self.assertEqual(child.stdout.read(), b"")
        self.assertEqual(child.stderr.read().strip(), FAILURE)

    @staticmethod
    def boundary(character):
        template = b'{"jsonrpc":"2.0","id":1,"method":"ping","params":{"text":""}}\n'
        padding = (character * ((8192 - len(template)) // len(character.encode()))).encode()
        # Pad any unmatched byte with ASCII while retaining the UTF-8 test data.
        padding += b"a" * (8192 - len(template) - len(padding))
        return template.replace(b'""', b'"' + padding + b'"')

    def test_ascii_boundary_reaches_child(self):
        child = self.start_relay("boundary_ascii")
        self.request(child, self.boundary("a"))
        self.expect_failure(child, 23)

    def test_utf8_boundary_reaches_child(self):
        child = self.start_relay("boundary_utf8")
        self.request(child, self.boundary("é"))
        self.expect_failure(child, 23)

    def test_encoded_request_over_budget_is_refused_before_send(self):
        child = self.start_relay("oversize")
        # Re-encoding the exponent as a float grows the submitted frame.
        frame = self.boundary("a").replace(b'a"}}', b'"}}', 1)
        frame = frame.replace(b'"params":{', b'"params":{"number":1e2,').replace(b"a" * 13, b"", 1)
        self.assertLessEqual(len(frame), 8192)
        self.assertGreater(len(relay.encode(json.loads(frame))), 8192)
        self.request(child, frame)
        self.expect_failure(child, 1)

    def test_watchdog_covers_blocked_write(self):
        child = self.start_relay("stall_write")
        self.request(child, self.boundary("a"))
        self.expect_failure(child, 1, timeout=3)

    def test_unexpected_idle_exit_preserves_status(self):
        self.expect_failure(self.start_relay("idle"), 23)

    def test_unexpected_idle_exit_zero_still_fails(self):
        self.expect_failure(self.start_relay("idle_zero"), 1)

    def test_unexpected_exit_with_request_outstanding_preserves_status(self):
        child = self.start_relay("outstanding")
        self.request(child, LIST_HOSTS)
        self.expect_failure(child, 23)

    def test_partial_child_reply_is_discarded_and_exit_status_preserved(self):
        child = self.start_relay("partial_reply")
        self.request(child, LIST_HOSTS)
        self.expect_failure(child, 23)
        self.assertEqual(self.evidence.read_bytes(), b"")

    def test_list_hosts_then_exit_is_a_failure_with_client_idle(self):
        child = self.start_relay("after_answer")
        self.request(child, LIST_HOSTS)
        # The marker releases the synthetic child after the evidence is flushed
        # and the tool response is observed. The queue bounds that observation.
        output = Queue()
        threading.Thread(target=lambda: output.put(child.stdout.readline()), daemon=True).start()
        self.assertEqual(json.loads(output.get(timeout=PIPE_TEST_TIMEOUT))["id"], 1)
        self.marker.touch()
        self.expect_failure(child, 23)
        self.assertEqual(json.loads(self.evidence.read_bytes())["tool"], "list_hosts")

    def test_watchdog_covers_stalled_read(self):
        child = self.start_relay("stall_read")
        self.request(child, LIST_HOSTS)
        self.expect_failure(child, 1, timeout=3)

    def test_client_eof_allows_clean_shutdown_without_acceptance_calls(self):
        child = self.start_relay("clean_eof")
        stdout, stderr = child.communicate(timeout=PIPE_TEST_TIMEOUT)
        self.assertEqual((child.returncode, stdout, stderr), (0, b"", b""))

    def test_client_eof_with_partial_evidence_fails(self):
        child = self.start_relay("complete")
        stdout, stderr = child.communicate(LIST_HOSTS, timeout=PIPE_TEST_TIMEOUT)
        self.assertEqual((child.returncode, stderr.strip()), (1, FAILURE))
        self.assertEqual(json.loads(stdout)["id"], 1)

    @unittest.skipUnless(os.name == "nt", "Windows junction")
    def test_relay_refuses_junction_and_writes_nothing_outside(self):
        outside = self.root / "outside"
        outside.mkdir()
        alias = self.root / "alias"
        junction(self, alias, outside)
        child = self.start_relay("complete", evidence=alias / "evidence.jsonl")
        stdout, stderr = child.communicate(LIST_HOSTS, timeout=PIPE_TEST_TIMEOUT)
        self.assertEqual((child.returncode, stdout, stderr.strip()), (1, b"", FAILURE))
        self.assertEqual(list(outside.iterdir()), [])

    @unittest.skipUnless(os.name == "nt", "Windows handle sharing")
    def test_evidence_handle_excludes_other_writers_and_renames(self):
        with relay.checked_file(self.evidence, create=True) as trace:
            relay.send(trace, b"first\n")
            with self.assertRaises(PermissionError):
                self.evidence.open("ab").close()
            with self.assertRaises(PermissionError):
                self.evidence.rename(self.root / "renamed")
            relay.send(trace, b"second\n")
        self.assertEqual(self.evidence.read_bytes(), b"first\nsecond\n")

    def test_relay_preserves_existing_evidence_and_hardlinks(self):
        self.evidence.write_bytes(b"original")
        alias = self.root / "alias"
        os.link(self.evidence, alias)
        self.expect_failure(self.start_relay("complete", evidence=alias), 1)
        self.assertEqual(self.evidence.read_bytes(), b"original")


class VerifierTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(dir=os.environ.get("TMP") or None)
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.outside = self.root / "outside"
        self.outside.mkdir()
        self.db = self.outside / "activity.sqlite3"
        with sqlite3.connect(self.db) as db:
            db.execute("""CREATE TABLE activity (
                id INTEGER, tool, program, argc, outcome, identity_mode, exit_code,
                integrity_rid, elevation, observation_point, tree_boundary, stop_reason,
                identity_matches_pin, cleanup_confirmed, observed_id, observed_kind,
                observed_gid, rights, actor_kind, principal)""")
            db.execute("""INSERT INTO activity VALUES (
                2, 'run_command', 'cargo', 1, 'succeeded', 'current_user', 0,
                8192, 0, 'pre_exec', 'windows_job', 'exited', 1, 1, NULL, NULL,
                NULL, NULL, 'agent', 'claude-code')""")
        db.close()

    def verify(self, path):
        return subprocess.run([sys.executable, "-I", "-B", str(RELAY_PATH),
                               "--verify", "2", "--database", str(path)],
                              capture_output=True, timeout=3)

    def test_valid_stopped_database_is_verified(self):
        before = self.db.read_bytes()
        result = self.verify(self.db)
        self.assertEqual((result.returncode, result.stderr), (0, b""))
        self.assertTrue(result.stdout.startswith(b"PASS durable M2 row 2:"))
        self.assertEqual({p.name: p.read_bytes() for p in self.outside.iterdir()},
                         {self.db.name: before})

    @unittest.skipUnless(os.name == "nt", "Windows junction")
    def test_verifier_refuses_junction_without_reading_the_valid_row(self):
        alias = self.root / "alias"
        junction(self, alias, self.outside)
        before = self.db.read_bytes()
        result = self.verify(alias / self.db.name)
        self.assertEqual((result.returncode, result.stdout, result.stderr.strip()), (1, b"", FAILURE))
        self.assertEqual({p.name: p.read_bytes() for p in self.outside.iterdir()},
                         {self.db.name: before})

    def test_verifier_refuses_hardlink_and_journal(self):
        alias = self.root / "alias"
        os.link(self.db, alias)
        result = self.verify(self.db)
        self.assertEqual((result.returncode, result.stdout, result.stderr.strip()), (1, b"", FAILURE))
        alias.unlink()
        (self.outside / (self.db.name + "-journal")).touch()
        result = self.verify(self.db)
        self.assertEqual((result.returncode, result.stdout, result.stderr.strip()), (1, b"", FAILURE))


if __name__ == "__main__":
    unittest.main()
