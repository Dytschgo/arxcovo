"""Local acceptance relay.

The owner-supplied arxd, client config, and evidence paths are arguments.
This process launches `arxd mcp-stdio` and appends sanitized tool results to
the evidence file. It does not embed a machine path or a credential.
"""

import io
import json
import os
import queue
import re
import stat
import sqlite3
import subprocess
import sys
import threading
from contextlib import ExitStack, closing, contextmanager
from time import monotonic
from pathlib import Path

TOOLS = {"list_hosts", "run_command", "list_activity"}
RESPONSE_LIMIT = 65536
REQUEST_LIMIT = 8192
RPC_TIMEOUT = 65
POLL_INTERVAL = 0.05
DATABASE_LIMIT = 64 * 1024 * 1024


def absolute_path(value):
    if not isinstance(value, str) or value == "" or value != value.strip() or value.startswith("-"):
        raise ValueError("usage")
    path = Path(value)
    if not path.is_absolute() or ".." in path.parts:
        raise ValueError("usage")
    if os.name == "nt":
        if not re.match(r"^[A-Za-z]:[\\/]", value) or any(
            part.endswith((" ", ".")) or Path(part).is_reserved()
            or any(ord(char) < 32 or char in '<>:"|?*' for char in part)
            for part in path.parts[1:]
        ):
            raise ValueError("usage")
    return value


if os.name == "nt":
    import ctypes
    import msvcrt
    from ctypes import wintypes

    kernel = ctypes.WinDLL("kernel32", use_last_error=True)

    def bind(name, result, *args):
        function = getattr(kernel, name)
        function.argtypes = args
        function.restype = result
        return function

    HANDLE = wintypes.HANDLE
    DWORD = wintypes.DWORD
    create_file = bind("CreateFileW", HANDLE, wintypes.LPCWSTR, DWORD, DWORD,
                       HANDLE, DWORD, DWORD, HANDLE)
    file_type = bind("GetFileType", DWORD, HANDLE)
    close_handle = bind("CloseHandle", wintypes.BOOL, HANDLE)

    class FileInfo(ctypes.Structure):
        _fields_ = [("attributes", DWORD), ("created", wintypes.FILETIME),
                    ("accessed", wintypes.FILETIME), ("written", wintypes.FILETIME),
                    ("volume", DWORD), ("size_high", DWORD), ("size_low", DWORD),
                    ("links", DWORD), ("index_high", DWORD), ("index_low", DWORD)]

    file_info = bind("GetFileInformationByHandle", wintypes.BOOL, HANDLE,
                     ctypes.POINTER(FileInfo))
    INVALID_HANDLE = ctypes.c_void_p(-1).value
    OPEN_REPARSE_POINT = 0x00200000
    BACKUP_SEMANTICS = 0x02000000
    DIRECTORY = 0x10
    REPARSE_POINT = 0x400
    SHARE_READ = 1
    OPEN_EXISTING = 3
    CREATE_NEW = 1
    GENERIC_READ = 0x80000000
    GENERIC_WRITE = 0x40000000

    def check_handle(handle, directory=False):
        info = FileInfo()
        if (not file_info(handle, ctypes.byref(info)) or file_type(handle) != 1
                or info.attributes & REPARSE_POINT
                or bool(info.attributes & DIRECTORY) != directory
                or (not directory and info.links != 1)):
            raise ValueError("unsafe file")

    def open_handle(path, access, share, disposition, directory=False):
        flags = OPEN_REPARSE_POINT | (BACKUP_SEMANTICS if directory else 0)
        handle = create_file(str(path), access, share, None, disposition, flags, None)
        if handle == INVALID_HANDLE:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            check_handle(handle, directory)
        except BaseException:
            close_handle(handle)
            raise
        return handle


def check_file(stream):
    if os.name == "nt":
        check_handle(msvcrt.get_osfhandle(stream.fileno()))
    else:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise ValueError("unsafe file")


@contextmanager
def checked_file(value, *, create=False, share_read=False):
    path = Path(absolute_path(os.fspath(value)))
    with ExitStack() as stack:
        if os.name == "nt":
            # Open prefixes from the volume root down. Retain each directory
            # without write/delete sharing so later path operations stay under it.
            for parent in reversed(path.parents):
                handle = open_handle(parent, 1, SHARE_READ, OPEN_EXISTING, directory=True)
                stack.callback(close_handle, handle)
            handle = open_handle(
                path, GENERIC_WRITE if create else GENERIC_READ, SHARE_READ if share_read else 0,
                CREATE_NEW if create else OPEN_EXISTING,
            )
            try:
                fd = msvcrt.open_osfhandle(handle, os.O_BINARY | (os.O_WRONLY if create else os.O_RDONLY))
            except BaseException:
                close_handle(handle)
                raise
        else:
            parent_fd = None
            for parent in reversed(path.parents):
                parent_fd = os.open(parent.name or parent.anchor, os.O_DIRECTORY | os.O_NOFOLLOW,
                                    dir_fd=parent_fd)
                stack.callback(os.close, parent_fd)
            flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL if create else os.O_RDONLY | os.O_NONBLOCK
            fd = os.open(path.name, flags | os.O_NOFOLLOW, 0o600, dir_fd=parent_fd)
        with os.fdopen(fd, "wb" if create else "rb") as stream:
            check_file(stream)
            yield stream


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


def read_frame(stream, limit=RESPONSE_LIMIT):
    line = stream.readline(limit + 1)
    if not line:
        return None
    if len(line) > limit:
        raise ValueError("frame")
    if not line.endswith(b"\n"):
        # A short read without a newline reached EOF mid-frame.
        raise EOFError("unterminated frame")
    value = json.loads(line)
    if not isinstance(value, dict):
        raise ValueError("envelope")
    return value


def encode(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode("utf-8") + b"\n"


def send(stream, frame):
    stream.write(frame)
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


class ChildExit(Exception):
    def __init__(self, status):
        self.status = status


def require_running(child):
    status = child.poll()
    if status is not None:
        raise ChildExit(status)


def receive(child, results, deadline=None):
    while True:
        require_running(child)
        delay = POLL_INTERVAL
        if deadline is not None:
            remaining = deadline - monotonic()
            if remaining <= 0:
                raise TimeoutError("bridge")
            delay = min(delay, remaining)
        try:
            result, failure = results.get(timeout=delay)
        except queue.Empty:
            continue
        require_running(child)
        if failure is not None:
            raise failure
        return result


def rpc(child, request):
    frame = encode(request)
    if len(frame) > REQUEST_LIMIT:
        raise ValueError("request frame")
    results = queue.Queue(1)

    def exchange():
        try:
            send(child.stdin, frame)
            reply = None
            if "id" in request:
                reply = read_frame(child.stdout)
                if reply is None:
                    raise EOFError("child stdout")
                if reply.get("id") != request["id"]:
                    raise ValueError("response")
            results.put((reply, None))
        except Exception as failure:
            results.put((None, failure))

    # The deadline covers the complete pipe exchange, including notifications.
    deadline = monotonic() + RPC_TIMEOUT
    threading.Thread(target=exchange, daemon=True).start()
    try:
        return receive(child, results, deadline)
    except (EOFError, BrokenPipeError):
        # Windows can deliver pipe EOF before the process handle signals exit.
        status = child.wait(timeout=max(0, deadline - monotonic()))
        raise ChildExit(status) from None


def client_frames(child, inbound):
    results = queue.Queue(1)

    def read():
        try:
            with inbound:
                while (request := read_frame(inbound, REQUEST_LIMIT)) is not None:
                    results.put((request, None))
            results.put((None, None))
        except Exception as failure:
            results.put((None, failure))

    threading.Thread(target=read, daemon=True).start()
    while (request := receive(child, results)) is not None:
        yield request


def error(request, code, message):
    return {"jsonrpc": "2.0", "id": request["id"], "error": {"code": code, "message": message}}


def relay(child, trace, requests, outbound):
    recorded = set()
    ran = False
    for request in requests:
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
                    check_file(trace)
                    send(trace, encode({"tool": name, "arguments": args, "result": clean}))
                    recorded.add("activity_lookup" if "activity_id" in args else name)
        else:
            reply = error(request, -32601, "Method unavailable.")
        send(outbound, encode(reply))
    return not recorded or recorded == TOOLS | {"activity_lookup"}


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


def verify_database(ident, database):
    if not re.fullmatch(r"[1-9][0-9]{0,15}", ident):
        raise ValueError("id")
    # The stopped daemon uses rollback journaling. Read a bounded snapshot
    # from the checked handle and give SQLite no filesystem path or sidecars.
    with checked_file(database) as source:
        for suffix in ("-journal", "-wal", "-shm"):
            try:
                os.lstat(database + suffix)
            except FileNotFoundError:
                continue
            raise ValueError("database sidecar")
        snapshot = source.read(DATABASE_LIMIT + 1)
        check_file(source)
    if len(snapshot) > DATABASE_LIMIT or snapshot[18:20] != b"\x01\x01":
        raise ValueError("database snapshot")
    with closing(sqlite3.connect(":memory:")) as connection:
        connection.deserialize(snapshot)
        connection.execute("PRAGMA trusted_schema=OFF")
        connection.execute("PRAGMA query_only=ON")
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
    with ExitStack() as stack:
        stack.enter_context(checked_file(parsed["arxd"], share_read=True))
        stack.enter_context(checked_file(parsed["client"], share_read=True))
        is_relay = parsed["mode"] == "relay"
        if is_relay:
            trace = stack.enter_context(checked_file(parsed["evidence"], create=True))
            # A daemon reader may still be waiting for the client after an
            # unexpected child exit. Keep its lock off Python's standard input.
            inbound = io.open(os.dup(sys.stdin.fileno()), "rb")
        child = open_child(parsed["arxd"], parsed["client"])
        try:
            complete = True
            if is_relay:
                complete = relay(child, trace, client_frames(child, inbound), sys.stdout.buffer)
            else:
                discover(child)
            require_running(child)
            child.stdin.close()
            status = child.wait(timeout=5)
        except ChildExit:
            raise
        except Exception:
            status = child.poll()
            if status is None:
                child.kill()
                child.wait(timeout=5)
                raise
            raise ChildExit(status) from None
        child.stdout.close()
        if status != 0:
            raise ChildExit(status)
        if not complete:
            raise ValueError("partial evidence")
        if not is_relay:
            print("Local discovery succeeded. Cargo found under current_user.")


def entrypoint():
    try:
        main()
    except ChildExit as failure:
        print("arxcovo-acceptance-bridge-failed", file=sys.stderr)
        status = ctypes.c_int(failure.status).value if os.name == "nt" else failure.status
        return status or 1
    except Exception:
        print("arxcovo-acceptance-bridge-failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(entrypoint())
