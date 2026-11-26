"""Local acceptance relay.

The owner-supplied arxd, client config, and evidence paths are arguments.
This process launches `arxd mcp-stdio` and appends sanitized tool results to
the evidence file. It does not embed a machine path or a credential.
"""

import json
import os
import re
import sqlite3
import subprocess
import sys
import threading
from contextlib import closing
from pathlib import Path

TOOLS = {"list_hosts", "run_command", "list_activity"}
LIMIT = 65536


def absolute_path(value):
    if not isinstance(value, str) or value == "" or value != value.strip() or value.startswith("-"):
        raise ValueError("usage")
    path = Path(value)
    if not path.is_absolute() or ".." in path.parts:
        raise ValueError("usage")
    if os.name == "nt":
        drive = re.match(r"^[A-Za-z]:[\\/]", value) is not None
        unc = value.startswith("\\\\")
        if not drive and not unc:
            raise ValueError("usage")
    return value


def take_flags(argv, required):
    values = {}
    index = 0
    while index < len(argv):
        flag = argv[index]
        if flag not in required or flag in values or index + 1 >= len(argv):
            raise ValueError("usage")
        values[flag] = absolute_path(argv[index + 1])
        index += 2
    if set(values) != set(required):
        raise ValueError("usage")
    return values


def parse_invocation(argv):
    if not argv:
        raise ValueError("usage")
    if argv[0] == "--verify":
        if len(argv) != 4 or argv[2] != "--database":
            raise ValueError("usage")
        if not re.fullmatch(r"[1-9][0-9]{0,15}", argv[1]):
            raise ValueError("id")
        return {"mode": "verify", "id": argv[1], "database": absolute_path(argv[3])}
    if argv[0] == "--discover":
        flags = take_flags(argv[1:], {"--arxd", "--client-config"})
        return {"mode": "discover", "arxd": flags["--arxd"], "client": flags["--client-config"]}
    flags = take_flags(argv, {"--arxd", "--client-config", "--evidence"})
    return {
        "mode": "relay",
        "arxd": flags["--arxd"],
        "client": flags["--client-config"],
        "evidence": flags["--evidence"],
    }


def child_argv(arxd, client):
    return [arxd, "mcp-stdio", "--client-config", client]


def read_frame(stream, limit=LIMIT):
    line = stream.readline(limit + 2)
    if not line:
        return None
    if len(line) > limit or not line.endswith(b"\n"):
        raise ValueError("frame")
    value = json.loads(line)
    if not isinstance(value, dict):
        raise ValueError("envelope")
    return value


def send(stream, value):
    stream.write(json.dumps(value, separators=(",", ":")).encode("utf-8") + b"\n")
    stream.flush()


def payload(response):
    result = response["result"]
    blocks = result["content"]
    if result.get("isError") or len(blocks) != 1 or blocks[0]["type"] != "text":
        raise ValueError("tool result")
    return json.loads(blocks[0]["text"])


def version(text):
    # Cargo's fixed version line only. A different output is recorded as unavailable.
    text = text.strip() if isinstance(text, str) else ""
    pattern = (
        r"cargo [0-9]+\.[0-9]+\.[0-9]+(?:-[a-z0-9.]+)?"
        r"(?: \([a-f0-9]{7,40} [0-9]{4}-[0-9]{2}-[0-9]{2}\))?"
    )
    return text if re.fullmatch(pattern, text) else None


def safe_result(name, data):
    if name == "list_hosts":
        safe = []
        for host in data["hosts"]:
            execution = host["execution"]
            safe.append({
                "os": "windows" if host["os"] == "windows" else "unexpected",
                "inventory_observed": all(type(host.get(key)) is int for key in
                    ("logical_cpus", "ram_bytes", "disk_total_bytes", "disk_available_bytes")),
                "execution": {key: execution[key] if type(execution.get(key)) is bool else None
                    for key in ("available", "dedicated", "isolation", "matches_current_user", "elevated")},
            })
            safe[-1]["execution"]["mode"] = "current_user" if execution["mode"] == "current_user" else "unexpected"
            safe[-1]["execution"]["integrity"] = "medium" if execution["integrity"] == "medium" else "unexpected"
            safe[-1]["execution"]["rights_is_null"] = execution.get("rights", False) is None
        return {"hosts": safe}
    if name == "run_command":
        return {
            "execution_mode": "current_user" if data.get("execution_mode") == "current_user" else "unexpected",
            "stop_reason": "exited" if data.get("stop_reason") == "exited" else "unexpected",
            "exit_code": data["exit_code"] if type(data.get("exit_code")) is int else None,
            "matches_current_user": data.get("matches_current_user") is True,
            "cleanup_confirmed": data.get("cleanup_confirmed") is True,
            "cargo_version": version(data.get("stdout")),
        }
    rows = []
    for row in data["activities"]:
        if row["tool"] != "run_command" or row["program"] != "cargo":
            continue
        safe = {key: row[key] for key in ("id", "argc", "exit_code", "integrity_rid")
                if type(row.get(key)) is int}
        for key, expected in {
            "tool": "run_command", "program": "cargo", "outcome": "succeeded",
            "identity_mode": "current_user", "stop_reason": "exited",
            "observation_point": "pre_exec", "tree_boundary": "windows_job",
        }.items():
            safe[key] = expected if row.get(key) == expected else "unexpected"
        for key in ("cleanup_confirmed", "identity_matches_pin", "elevation"):
            safe[key] = row[key] if type(row.get(key)) is bool else None
        safe["raw_identity_and_rights_are_null"] = all(
            key in row and row[key] is None for key in ("observed_id", "observed_kind", "observed_gid", "rights"))
        rows.append(safe)
    return {"activities": rows}


def call_allowed(name, args):
    if name == "list_hosts":
        return args == {}
    if name == "run_command":
        return args == {"tool": "cargo", "args": ["--version"], "timeout_s": 10, "output_bytes": 4096}
    if name == "list_activity":
        return args == {"limit": 20} or (
            set(args) == {"activity_id"} and type(args["activity_id"]) is int
            and 1 <= args["activity_id"] <= 2**53 - 1)
    return False


def rpc(child, request):
    send(child.stdin, request)
    if "id" not in request:
        return None
    timer = threading.Timer(65, child.kill)
    timer.daemon = True
    timer.start()
    try:
        result = read_frame(child.stdout)
        if result is None or result.get("id") != request["id"]:
            raise ValueError("response")
        return result
    finally:
        timer.cancel()


def error(request, code, message):
    return {"jsonrpc": "2.0", "id": request["id"], "error": {"code": code, "message": message}}


def relay(child, trace, inbound, outbound):
    ran = False
    trace_path = Path(trace)
    while (request := read_frame(inbound, 8192)) is not None:
        method = request.get("method")
        if "id" not in request:
            if method in ("notifications/initialized", "notifications/cancelled"):
                rpc(child, request)
            continue
        if method in ("initialize", "tools/list", "ping"):
            reply = rpc(child, request)
            if method == "tools/list" and "result" in reply:
                reply["result"] = {"tools": [t for t in reply["result"]["tools"] if t["name"] in TOOLS]}
        elif method == "tools/call":
            params = request.get("params", {})
            name = params.get("name")
            args = params.get("arguments", {})
            if not isinstance(args, dict) or not call_allowed(name, args) or (name == "run_command" and ran):
                reply = error(request, -32602, "Outside the acceptance tool arguments.")
            else:
                ran |= name == "run_command"
                exact = {"jsonrpc": "2.0", "id": request["id"], "method": "tools/call",
                         "params": {"name": name, "arguments": args}}
                response = rpc(child, exact)
                if "error" in response:
                    reply = error(request, -32603, "Arxcovo refused or failed the acceptance call.")
                else:
                    clean = safe_result(name, payload(response))
                    reply = {"jsonrpc": "2.0", "id": request["id"],
                             "result": {"content": [{"type": "text", "text": json.dumps(clean)}]}}
                    with trace_path.open("a", encoding="utf-8") as handle:
                        handle.write(json.dumps({"tool": name, "arguments": args, "result": clean}) + "\n")
        else:
            reply = error(request, -32601, "Method unavailable.")
        send(outbound, reply)


def discover(child):
    def request(method, ident, params):
        return rpc(child, {"jsonrpc": "2.0", "method": method, "id": ident, "params": params})
    hello = request("initialize", 1, {"protocolVersion": "2025-03-26", "capabilities": {},
                                      "clientInfo": {"name": "arxcovo-owner-local", "version": "1"}})
    if "result" not in hello:
        raise ValueError("initialize")
    rpc(child, {"jsonrpc": "2.0", "method": "notifications/initialized"})
    result = payload(request("tools/call", 2, {"name": "discover_tools", "arguments": {}}))
    if result["outcome"] != "succeeded" or result["execution_mode"] != "current_user" or not any(
        item.get("name") == "cargo" and item.get("found") is True for item in result["tools"]):
        raise ValueError("discovery")
    print("Local discovery succeeded. Cargo found under current_user.")


def verify_database(ident, database):
    if not re.fullmatch(r"[1-9][0-9]{0,15}", ident):
        raise ValueError("id")
    db = Path(database)
    with closing(sqlite3.connect(db.as_uri() + "?mode=ro", uri=True)) as connection:
        row = connection.execute("""
            SELECT id, tool, program, argc, outcome, identity_mode, exit_code,
                   integrity_rid, elevation, observation_point, tree_boundary,
                   stop_reason, identity_matches_pin, cleanup_confirmed,
                   observed_id IS NULL AND observed_kind IS NULL AND
                   observed_gid IS NULL AND rights IS NULL
            FROM activity WHERE id=? AND actor_kind='agent' AND principal='claude-code'
        """, (int(ident),)).fetchone()
    if row != (int(ident), "run_command", "cargo", 1, "succeeded", "current_user",
               0, 8192, 0, "pre_exec", "windows_job", "exited", 1, 1, 1):
        raise ValueError("database evidence")
    print(f"PASS durable M2 row {ident}: succeeded, current_user, Medium, unelevated, "
          "pre_exec, windows_job, matching identity, cleanup confirmed, identity/rights null.")


def open_child(arxd, client):
    return subprocess.Popen(
        child_argv(arxd, client),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )


def main(argv=None):
    parsed = parse_invocation(list(sys.argv[1:] if argv is None else argv))
    if parsed["mode"] == "verify":
        verify_database(parsed["id"], parsed["database"])
        return
    with open_child(parsed["arxd"], parsed["client"]) as child:
        try:
            if parsed["mode"] == "discover":
                discover(child)
            else:
                relay(child, parsed["evidence"], sys.stdin.buffer, sys.stdout.buffer)
        finally:
            child.stdin.close()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=5)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        sys.exit("arxcovo-acceptance-bridge-failed")
