"""First-attempt D5 qualification on the reviewed hosted Linux runner."""
import contextlib
import email.utils
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import socket
import sqlite3
import ssl
import stat
import struct
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = Path(__file__).resolve()
ENV = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LC_ALL": "C"}
OWNER, DAEMON, AGENT, BUILD = range(21901, 21905)
TTL = 900_000
SKEW = 30_000
MAX_TIME = 253_402_300_799_999
DWELL_NS = 900_000_000_000
TOTAL = 1030
CLEANUP = 25
FINAL = 55
ORIGINS = (
    ("www.google.com", "/generate_204", 204),
    ("www.cloudflare.com", "/cdn-cgi/trace", 200),
)
DOMAIN = b"arxcovo-d5-query-v1\0"


class Failure(Exception):
    """A fixed, redacted reason suitable for first-attempt evidence."""


def require(condition, reason):
    if not condition:
        raise Failure(reason)


def remaining(deadline, cap):
    available = deadline - time.monotonic()
    if available <= 0:
        raise TimeoutError
    return min(available, cap)


def emit(event, **data):
    print(json.dumps({"event": event, **data}, sort_keys=True), flush=True)


def digest(domain, data):
    return hashlib.sha256(domain + data).digest()


def c13_tools():
    spec = importlib.util.spec_from_file_location("c13", ROOT / "tools/ci/m3-c13/run.py")
    c13 = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(c13)
    return c13


def checked_blob(blob, magic, cap):
    require(isinstance(blob, bytes) and len(magic) + 32 <= len(blob) <= cap
            and blob.startswith(magic) and hashlib.sha256(blob[:-32]).digest() == blob[-32:],
            "invalid-store-blob")
    return blob[:-32]


def regular(path):
    metadata = path.lstat()
    require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1
            and metadata.st_uid == DAEMON, "fixture-file")
    return metadata


def bounded_file(path, cap):
    original = regular(path)
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), "rb") as f:
        current = os.fstat(f.fileno())
        require((current.st_dev, current.st_ino) == (original.st_dev, original.st_ino),
                "fixture-file-replaced")
        data = f.read(cap + 1)
    require(0 < len(data) <= cap, "fixture-file-bound")
    return data


@contextlib.contextmanager
def database(path):
    regular(path)
    # Fixed fixture namespace, root-owned parent and a single attributed writer.
    with contextlib.closing(sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=.2)) as db:
        db.execute("PRAGMA query_only=ON")
        yield db


class Reader:
    def __init__(self, data):
        self.data = data

    def take(self, size):
        require(len(self.data) >= size, "truncated-store")
        part, self.data = self.data[:size], self.data[size:]
        return part

    def number(self, size=8):
        return int.from_bytes(self.take(size), "big")

    def position(self):
        return (self.number(), self.number(), self.number())


@dataclass(frozen=True)
class View:
    applied: tuple
    membership: tuple
    voters: bytes
    cluster: bytes
    allocation_floor: int
    floor: int
    records: dict
    members: dict


def read_view(node):
    with database(node / "state/cluster-state.sqlite3") as db:
        row = db.execute("SELECT snapshot FROM sm WHERE singleton=1 AND length(snapshot)<=16384").fetchone()
    require(row is not None, "no-applied-view")
    prefix = b"ARXSM1\0\0\0\x03\x01"
    body = checked_blob(row[0], prefix, 16384)
    r = Reader(body[len(prefix):])
    applied = r.position()
    require(r.number(1) == 1, "no-membership")
    membership = r.position()
    require(covers(applied, membership), "unapplied-membership")
    voters = r.take(r.number(2))
    cluster, allocation_floor, floor = r.take(16), r.number(), r.number()
    count = r.number(2)
    require(count <= 2 and floor <= MAX_TIME and allocation_floor in (2, 3), "applied-bounds")
    records = {}
    for _ in range(count):
        code = r.take(16)
        r.take(64)  # actor, Mint request and verifier never enter the result
        expiry, consumed = r.number(), r.number(1)
        require(code not in records and TTL <= expiry <= floor + TTL and consumed in (0, 1),
                "invalid-code-record")
        record = (r.take(16), r.take(32), r.number(), r.take(32), r.position(), r.number()) if consumed else None
        records[code] = (expiry, record)
    require(r.number(1) == 1, "not-founded")
    members_count = r.number(1)
    require(1 <= members_count <= 2, "member-count")
    members = {r.number(): r.take(32) for _ in range(members_count)}
    require(not r.data and len(members) == members_count and 1 in members, "member-view")
    return View(applied, membership, voters, cluster, allocation_floor, floor, records, members)


def read_log(node):
    with database(node / "state/cluster-log.sqlite3") as db:
        rows = db.execute(
            "SELECT idx, CASE WHEN length(body)<=4163 THEN body END FROM log ORDER BY idx LIMIT 65"
        ).fetchall()
    require(0 < len(rows) < 65, "log-bound")
    result = []
    for index, blob in rows:
        r = Reader(checked_blob(blob, b"ARXLE2\0\0", 4163)[8:])
        position, tag = r.position(), r.number(1)
        require(position[2] == index and tag in (0, 1, 3, 4, 6), "unexpected-log-entry")
        # Read only fields that correlate the selected commands.
        if tag in (1, 6):
            cluster = r.take(16)
            r.take(32 if tag == 1 else 16)
            query = cluster + r.take(16)
        else:
            query = None
        result.append((position, tag, query))
    return result


def covers(applied, position):
    return applied == position or applied[2] > position[2]


@dataclass(frozen=True, repr=False)
class Paste:
    query: bytes
    anchor: bytes
    secret: bytes


def parse_paste(wire, endpoint, root, *, keep_secret=False):
    require(len(wire) <= 8800 and re.fullmatch(b"arxje1-[0-9a-f]+", wire), "mint-paste")
    raw = bytes.fromhex(wire[7:].decode("ascii"))
    require(len(raw) >= 283 and raw[0] == 4, "mint-envelope")
    require((socket.inet_ntoa(raw[1:5]), int.from_bytes(raw[5:7], "big")) == endpoint,
            "mint-route")
    root_size = int.from_bytes(raw[7:9], "big")
    require(0 < root_size <= 4096 and len(raw) == 9 + root_size + 270 + 4
            and raw[9:9 + root_size] == root
            and digest(b"arxcovo/join/transfer/v1", raw[:-4])[:4] == raw[-4:], "mint-root")
    inner = raw[9 + root_size:-4]
    require(re.fullmatch(b"arxj2-[0-9a-f]{264}", inner), "mint-code")
    code = bytes.fromhex(inner[6:].decode("ascii"))
    require(digest(b"arxcovo/join/check/v2", code[:128])[:4] == code[128:]
            and hashlib.sha256(root).digest() == code[64:96], "mint-pin")
    return Paste(code[:32], code[96:128], code[32:64] if keep_secret else b"")


def mint_view(node, paste):
    view, entries = read_view(node), read_log(node)
    positions = [pos for pos, tag, query in entries if tag == 1 and query == paste.query]
    record = view.records.get(paste.query[16:])
    require(view.cluster == paste.query[:16] and len(positions) == 1 and record is not None
            and covers(view.applied, positions[0]) and record == (view.floor + TTL, None),
            "mint-commit")
    return view, entries, positions[0], record[0]


def recv_exact(read, size):
    data = bytearray()
    while len(data) < size:
        part = read(size - len(data))
        require(part, "truncated-frame")
        data.extend(part)
    return bytes(data)


def owner_peer(path, pid, tag):
    sizes = {1: (11, 8800), 4: (15, 29), 5: (16, 53)}
    reply, maximum = sizes[tag]
    with socket.socket(socket.AF_UNIX) as s:
        ended = time.monotonic() + (19 if tag == 1 else 2)
        s.settimeout(remaining(ended, 2))
        s.connect(path)
        credentials = struct.unpack("3i", s.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        require(credentials[:2] == (pid, DAEMON), "owner-peer")
        s.sendall(b"ARXO" + bytes([tag, 0, 0]))
        s.shutdown(socket.SHUT_WR)

        def read(size):
            s.settimeout(remaining(ended, 19))
            return s.recv(size)

        header = recv_exact(read, 7)
        size = int.from_bytes(header[5:], "big")
        require(header[:5] == b"ARXO" + bytes([reply])
                and (0 < size <= maximum if tag == 1 else size == maximum), "owner-reply")
        body = recv_exact(read, size)
        require(not read(1), "owner-trailing")
        return body


@dataclass(frozen=True, repr=False)
class Observation:
    ordinal: int
    result: int
    query_ms: int | None
    checkpoint_ms: int
    handle: bytes


def sample(status, data, mandatory):
    value = int.from_bytes(data, "big")
    if status == 1:
        require(0 < value <= MAX_TIME, "invalid-sample")
        return value
    require(status in (0, 2) and value == 0, "invalid-sample-status")
    require(not mandatory, "sample-missing" if status == 0 else "sample-failed")
    return None


def observation(body):
    require(len(body) == 53 and body[0] == 1, "observer-format")
    ordinal, result, query_status, checkpoint_status = body[1:5]
    query_ms = sample(query_status, body[5:13], False)
    checkpoint = sample(checkpoint_status, body[13:21], True)
    handle = body[21:]
    require((ordinal == 0 and result == 0 and query_status == 0 and handle == bytes(32))
            or (0 < ordinal < 255 and result in (1, 2, 3, 4) and handle != bytes(32)),
            "observer-slot")
    return Observation(ordinal, result, query_ms, checkpoint, handle)


def query_evidence(entry, paste, ordinal, result, bracket):
    require(entry.ordinal == ordinal and entry.result == result
            and entry.handle == digest(DOMAIN, paste.query) and entry.query_ms is not None
            and bracket[0] <= entry.query_ms <= entry.checkpoint_ms <= bracket[1],
            "query-evidence")
    return entry.query_ms


def check_elapsed(start, finish, expiry, checked):
    require(finish - start >= DWELL_NS, "elapsed-short")
    require(checked > expiry, "checked-not-past-expiry")
    return finish - start


def date_evidence(headers):
    require(len(headers) <= 48 and sum(len(k) + len(v) for k, v in headers) <= 8192,
            "utc-header-bound")
    date = []
    for key, value in headers:
        key, value = key.lower(), value.strip()
        if key == "date":
            date.append(value)
        if key == "age":
            raise Failure("utc-cached")
        if key in ("cf-cache-status", "x-cache", "cache-status", "warning"):
            require((key == "cf-cache-status" and value.upper() in {"DYNAMIC", "BYPASS", "MISS"})
                    or (key == "x-cache" and value.upper() == "MISS"), "utc-cache-evidence")
    require(len(date) == 1, "utc-missing-or-ambiguous")
    parsed = email.utils.parsedate_to_datetime(date[0])
    require(parsed.tzinfo is not None and email.utils.format_datetime(parsed, usegmt=True) == date[0],
            "utc-date")
    return int(parsed.timestamp() * 1000)


def utc_bounds(before, after, reference):
    require(type(reference) is int and 0 < reference <= MAX_TIME
            and 0 < before <= after <= MAX_TIME and after - before <= 6000, "utc-bracket")
    # Date resolution +/-1 s and coordinator-approved source accuracy +/-1 s.
    low, high = before - (reference + 2000), after - (reference - 2000)
    require(low >= -SKEW and high <= SKEW, "utc-outside-premise")
    return low, high


def reference_request(index):
    require(index in (0, 1), "utc-source")
    host, path, expected = ORIGINS[index]
    connection = http.client.HTTPSConnection(host, timeout=3, context=ssl.create_default_context())
    try:
        connection.request("HEAD", path, headers={"User-Agent": "Time check",
                                                  "Cache-Control": "no-cache, no-store",
                                                  "Connection": "close"})
        response = connection.getresponse()
        require(response.status == expected, "utc-http-status")
        return date_evidence(response.getheaders())
    finally:
        connection.close()



class EnrollmentTls:
    """Keep reading after sending the client's TLS close_notify."""

    def __init__(self, raw, endpoint_cert, root, deadline):
        self.raw, self.deadline = raw, deadline
        self.incoming, self.outgoing = ssl.MemoryBIO(), ssl.MemoryBIO()
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        context.minimum_version = context.maximum_version = ssl.TLSVersion.TLSv1_3
        # The enrollment issuer has a URI identity. Verify the CA and exact
        # retained founder leaf instead of a DNS name.
        context.check_hostname = False
        context.load_verify_locations(cadata=ssl.DER_cert_to_PEM_cert(root))
        self.tls = context.wrap_bio(self.incoming, self.outgoing, server_hostname="arxcovo.invalid")
        self.drive(self.tls.do_handshake)
        require(not self.tls.session_reused and self.tls.getpeercert(binary_form=True) == endpoint_cert,
                "wrong-enrollment-authority")

    def flush(self):
        while self.outgoing.pending:
            self.raw.settimeout(remaining(self.deadline, 12))
            self.raw.sendall(self.outgoing.read())

    def drive(self, operation):
        while True:
            try:
                result = operation()
            except ssl.SSLWantReadError:
                self.flush()
                self.raw.settimeout(remaining(self.deadline, 12))
                data = self.raw.recv(16384)
                require(data, "tls-unclean-eof")
                self.incoming.write(data)
            else:
                self.flush()
                return result

    def write_frame(self, tag, body):
        frame = b"ARXE\x01" + bytes([tag]) + len(body).to_bytes(2, "big") + body
        require(self.drive(lambda: self.tls.write(frame)) == len(frame), "tls-short-write")

    def read(self, size):
        try:
            return self.drive(lambda: self.tls.read(size))
        except ssl.SSLZeroReturnError:
            return b""  # Authenticated close_notify. Raw TCP EOF still fails in drive.

    def read_frame(self, tag, maximum):
        head = recv_exact(self.read, 8)
        length = int.from_bytes(head[6:], "big")
        require(head[:6] == b"ARXE\x01" + bytes([tag]) and 0 < length <= maximum,
                "enrollment-frame")
        return recv_exact(self.read, length)

    def send_close(self):
        try:
            self.tls.unwrap()
        except ssl.SSLWantReadError:
            pass  # Result still belongs to the read side.
        self.flush()


def anchor_exchange(tls, paste, founder_key, late):
    tls.write_frame(1, paste.query)
    answer = tls.read_frame(2, 658)
    if late:
        require(answer == b"\x01" and not tls.read(1), "late-uniform-refusal")
        return None
    voters = b"\x01" + (1).to_bytes(8, "big") + founder_key
    require(len(answer) == 58 and answer[0] == 0 and answer[17:] == voters
            and digest(b"arxcovo/join/anchor/v1", paste.query[:16] + voters) == paste.anchor,
            "positive-anchor")
    return answer[1:17]


def run_helper(arguments, deadline, cap, *, identity=None, data=None, limit=4096):
    uid = {"user": identity, "group": identity, "extra_groups": []} if identity is not None else {}
    result = subprocess.run(arguments, input=data, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                            env=ENV, timeout=remaining(deadline, cap), **uid)
    require(len(result.stdout) <= limit, "helper-output")
    return result.returncode, result.stdout


def openssl(arguments, deadline, data=None):
    code, output = run_helper(["/usr/bin/openssl", *arguments], deadline, 3, data=data)
    require(code == 0 and output, "control-certificate")
    return output


def control_csr(deadline):
    key = openssl(["genpkey", "-algorithm", "ED25519"], deadline)
    csr = openssl(["req", "-new", "-subj", "/", "-outform", "DER", "-key", "/dev/stdin"], deadline, key)
    spki = openssl(["pkey", "-pubout", "-outform", "DER"], deadline, key)
    return csr, spki


def redeem(tls, paste, challenge, csr, request):
    proof = digest(b"arxcovo/join/proof/v2", paste.query + paste.secret + paste.anchor)
    tls.write_frame(3, request + paste.query + paste.anchor + challenge + proof
                    + len(csr).to_bytes(2, "big") + csr)
    tls.send_close()
    result = tls.read_frame(4, 4131)
    require(len(result) >= 36 and result[0] == 0 and not tls.read(1), "control-redeem")
    length = int.from_bytes(result[33:35], "big")
    require(length == len(result[35:]) and 1 <= length <= 4096, "result-leaf-size")
    node, *position = struct.unpack(">QQQQ", result[1:33])
    return node, tuple(position), result[35:]


def verify_issued(leaf, spki, root_pem, cluster, node, deadline):
    pem = ssl.DER_cert_to_PEM_cert(leaf).encode("ascii")
    openssl(["verify", "-purpose", "sslserver", "-CAfile", str(root_pem), "/dev/stdin"], deadline, pem)
    san = openssl(["x509", "-noout", "-ext", "subjectAltName"], deadline, pem)
    expected = f"URI:arxcovo://cluster/{cluster.hex()}/node/{node}".encode("ascii")
    require(san.splitlines()[-1].strip() == expected, "control-issued-identity")
    pubkey = openssl(["x509", "-pubkey", "-noout"], deadline, pem)
    require(openssl(["pkey", "-pubin", "-outform", "DER"], deadline, pubkey) == spki,
            "control-issued-key")


def configure(node, peer, enrollment):
    relative = ("workspace", "state", "credentials", "wrap", "private/custody", "history", "control/owner.sock")
    config = dict(zip(("workspace", "state", "credentials", "wrap", "custody", "history", "owner_socket"),
                      [str(node / path) for path in relative], strict=True))
    config.update(peer="%s:%d" % peer, enrollment="%s:%d" % enrollment, issuer_peer=None,
                  founder_cluster=[13] * 16, owner_uid=OWNER, daemon_uid=DAEMON, daemon_gid=DAEMON,
                  agent_uid=AGENT, build_uid=BUILD, owner_gid=OWNER)
    with (node / "install.json").open("w", encoding="ascii") as target:
        json.dump(config, target)
        target.flush()
        os.fsync(target.fileno())


def start_founder(binary, node, ip):
    import fcntl
    with contextlib.ExitStack() as owned:
        listeners = [owned.enter_context(socket.socket()) for _ in range(2)]
        for listener in listeners:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind((ip, 0))
            listener.listen(128)
        peer, enrollment = (listener.getsockname() for listener in listeners)
        configure(node, peer, enrollment)
        descriptors = []
        for listener in listeners:
            fd = fcntl.fcntl(listener, fcntl.F_DUPFD_CLOEXEC, 6)
            owned.callback(os.close, fd)
            descriptors.append(fd)
        actions = [(os.POSIX_SPAWN_DUP2, src, dst)
                   for src, dst in zip(descriptors, (3, 4), strict=True)]
        actions += [(os.POSIX_SPAWN_OPEN, fd, "/dev/null", mode, 0)
                    for fd, mode in enumerate((os.O_RDONLY, os.O_WRONLY, os.O_WRONLY))]
        command = ["/usr/bin/setpriv", f"--reuid={DAEMON}", f"--regid={DAEMON}", "--clear-groups",
                   str(binary), "--cluster-config", str(node / "install.json"),
                   "--cluster-intent", "create-founder", "--inherit-listeners"]
        pid = os.posix_spawn(command[0], command, ENV, file_actions=actions, setpgroup=0)
    return Founder(pid, node, peer, enrollment)



class Founder:
    def __init__(self, pid, node, peer, enrollment):
        self.pid, self.node, self.peer, self.enrollment = pid, node, peer, enrollment
        self.reaped = None
        self.signalled = False
        self.last_clock = 0
        self.status = None

    def poll(self):
        if self.reaped is None:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.reaped = os.waitstatus_to_exitcode(status)
        return self.reaped

    def owner(self, tag, deadline, starting=False):
        require(self.poll() is None, "founder-exited")
        code, body = run_helper(
            ["/usr/bin/python3", "-I", "-B", str(self.node.parent / "tools/owner/run.py"), "--owner",
             str(self.node / "control/owner.sock"),
             str(self.pid), str(tag)], deadline, 20 if tag == 1 else 3, identity=OWNER, limit=8800,
        )
        if starting and code == 3:
            return None
        require(code == 0, "owner-request")
        if tag == 4:
            self.status = tuple(body[:3])
        return body

    def ready(self, deadline):
        while True:
            status = self.owner(4, deadline, starting=True)
            if status is not None:
                require(status[0] != 6, "founder-refused")
                if status[0] == 4:
                    require(status[1:3] == b"\0\0", "nonvirgin-ingress")
                    return
            time.sleep(remaining(deadline, .05))

    def counts(self, anchors, redeems, deadline):
        status = self.owner(4, deadline)
        require(status[:3] == bytes((4, anchors, redeems)), "ingress-counts")

    def evidence(self, deadline):
        entry = observation(self.owner(5, deadline))
        require(entry.checkpoint_ms >= self.last_clock, "checked-rollback")
        self.last_clock = entry.checkpoint_ms
        return entry

    def close(self, deadline):
        if self.poll() is None:
            self.signalled = True
            os.kill(self.pid, signal.SIGINT)
            while self.poll() is None:
                time.sleep(remaining(deadline, .04))
        require(self.signalled and self.reaped == 0, "unconfirmed-owned-close")
        require(not os.path.lexists(self.node / "control/owner.sock")
                and released_locks(self.node)
                and closed_ports((self.peer, self.enrollment)), "unconfirmed-release")


def released_locks(node):
    import fcntl
    files = ("state/cluster-owner.lock", "state/cluster-state.lock", "state/cluster-log.lock",
             "private/custody/custody-lock")
    with contextlib.ExitStack() as observed:
        for relative in files:
            path = node / relative
            regular(path)
            fd = os.open(path, os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK)
            observed.callback(os.close, fd)
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    return True


def closed_ports(endpoints):
    targets = set()
    for host, port in endpoints:
        ip = int.from_bytes(socket.inet_aton(host), "little")
        targets.update((f"{ip:08X}:{port:04X}".encode(), f"00000000:{port:04X}".encode()))
    with open("/proc/net/tcp", "rb") as table:
        data = table.read(1024 * 1024 + 1)
    require(len(data) <= 1024 * 1024, "listener-inventory-bound")
    return not any(address in targets and state == b"0A"
                   for _, address, _, state, *_ in map(bytes.split, data.splitlines()[1:]))


def utc_witness(founder, stage, deadline):
    first = last = None
    for index, (host, _, _) in enumerate(ORIGINS):
        emit("utc-source", stage=stage, source=host)
        before = founder.evidence(deadline).checkpoint_ms
        code, data = run_helper(
            ["/usr/bin/python3", "-I", "-B", str(SCRIPT), "--reference", str(index)],
            deadline, 4, limit=16,
        )
        require(code == 0 and re.fullmatch(rb"[0-9]{1,15}\n", data), "utc-reference-failed")
        after = founder.evidence(deadline).checkpoint_ms
        low, high = utc_bounds(before, after, int(data))
        emit("utc-pass", stage=stage, source=host, bracket_ms=after - before,
             resolution_ms=1000, assumed_accuracy_ms=1000, offset_low_ms=low, offset_high_ms=high)
        first = before if first is None else first
        last = after
    return first, last


def open_exchange(founder, leaf, root, deadline, paste, founder_key, late=False, fresh=None):
    end = time.monotonic() + remaining(deadline, 12)
    with socket.create_connection(founder.enrollment, timeout=remaining(end, 12)) as raw:
        tls = EnrollmentTls(raw, leaf, root, end)
        challenge = anchor_exchange(tls, paste, founder_key, late)
        if fresh is not None:
            return redeem(tls, paste, challenge, *fresh)
        if not late:
            tls.send_close()
    return None


def wait_dwell(founder, start_ns, deadline):
    end = start_ns + DWELL_NS + 1_000_000_000
    require(time.monotonic_ns() + int(remaining(deadline, TOTAL) * 1e9) >= end, "no-dwell-reserve")
    while time.monotonic_ns() < end:
        require(founder.poll() is None, "founder-exited-during-dwell")
        time.sleep(min(.25, max(0, (end - time.monotonic_ns()) / 1e9)))


def entry_tags(entries):
    return [tag for _, tag, _ in entries if tag != 0]


def emit_view(stage, view, time_ms, expiry, **other):
    emit("view", stage=stage, applied=view.applied, membership=view.membership, ttl_ms=TTL,
         floor_from_mint_ms=view.floor - (expiry - TTL), sample_after_floor_ms=time_ms - view.floor,
         expiry_margin_ms=expiry - time_ms, **other)



def campaign(founder, deadline, progress):
    node = founder.node
    progress["phase"] = "founder-readiness"
    initial_deadline = min(deadline, time.monotonic() + 45)
    founder.ready(min(initial_deadline, time.monotonic() + 18))
    genesis, genesis_log = read_view(node), read_log(node)
    require(entry_tags(genesis_log) == [3, 4] and genesis.allocation_floor == 2
            and not genesis.records and set(genesis.members) == {1}, "founder-inventory")
    progress.update(engine_starts=1, found=1)
    emit("founder-ready", stage=4, found_position=next(pos for pos, tag, _ in genesis_log if tag == 4))
    root = bounded_file(node / "credentials/ca.der", 4096)
    identity = json.loads(bounded_file(node / "credentials/identity.json", 32768))
    leaf = bytes(identity["leaf"])
    require(identity["node"] == 1 and bytes(identity["ca"]) == hashlib.sha256(root).digest()
            and bytes(identity["cluster"]) == genesis.cluster and bytes(identity["key"]) == genesis.members[1]
            and 0 < len(leaf) <= 4096, "founder-identity")

    progress["phase"] = "initial-utc-and-mint"
    _, before = utc_witness(founder, "before-initial-mint", initial_deadline)
    progress["owner_mint_requests"] = 1
    a = parse_paste(founder.owner(1, initial_deadline), founder.enrollment, root)
    view, entries, mint_a, expiry = mint_view(node, a)
    marked = time.monotonic_ns()
    require(entry_tags(entries) == [3, 4, 1] and view.applied == mint_a
            and view.floor >= before and view.allocation_floor == 2, "first-mint-inventory")
    progress.update(committed_mints=1, enrollment_proposals=2)
    emit("commit", stage="initial-mint", position=mint_a, ttl_ms=TTL)

    progress["phase"] = "initial-query"
    progress["client_queries"] = 1
    open_exchange(founder, leaf, root, initial_deadline, a, view.members[1])
    first = founder.evidence(initial_deadline)
    founder.counts(1, 0, initial_deadline)
    after, _ = utc_witness(founder, "after-initial-query-before-dwell", initial_deadline)
    checked = query_evidence(first, a, 1, 1, (before, after))
    require(view.floor <= checked <= expiry - 4 * SKEW and read_view(node) == view
            and read_log(node) == entries, "initial-usable-window")
    emit_view("initial-query", view, checked, expiry, stage_code=4, proof_sent=False)

    progress["phase"] = "elapsed-dwell"
    wait_dwell(founder, marked, deadline - FINAL)

    progress["phase"] = "late-utc-and-query"
    late_deadline = min(deadline, time.monotonic() + 24)
    _, before = utc_witness(founder, "after-dwell-before-late-query", late_deadline)
    require(before > expiry and read_view(node) == view and read_log(node) == entries,
            "late-precondition")
    founder.counts(1, 0, late_deadline)
    late_mark = time.monotonic_ns()
    progress["client_queries"] = 2
    open_exchange(founder, leaf, root, late_deadline, a, view.members[1], late=True)
    refused = founder.evidence(late_deadline)
    founder.counts(2, 0, late_deadline)
    require(read_view(node) == view and read_log(node) == entries, "late-effect")
    after, positive_before = utc_witness(founder, "after-late-query-before-fresh-mint", late_deadline)
    checked = query_evidence(refused, a, 2, 2, (before, after))
    elapsed = check_elapsed(marked, late_mark, expiry, checked)
    emit_view("late-refusal", view, checked, expiry, elapsed_ns=elapsed, stage_code=4,
              outcome="policy-refused", pending=True, proof_sent=False, allocations=0, redeems=0)

    progress["phase"] = "fresh-mint-and-redeem"
    csr, spki = control_csr(deadline)
    progress["owner_mint_requests"] = 2
    b = parse_paste(founder.owner(1, deadline), founder.enrollment, root, keep_secret=True)
    fresh_view, fresh_log, mint_b, fresh_expiry = mint_view(node, b)
    require(b.query != a.query and entry_tags(fresh_log) == [3, 4, 1, 1]
            and fresh_view.applied == mint_b and fresh_view.floor >= positive_before
            and fresh_view.allocation_floor == 2 and fresh_view.membership == view.membership
            and fresh_view.voters == view.voters and fresh_view.records[a.query[16:]] == (expiry, None),
            "fresh-mint-inventory")
    progress.update(committed_mints=2, enrollment_proposals=3, client_queries=3)
    emit("commit", stage="fresh-mint", position=mint_b, ttl_ms=TTL)
    request = os.urandom(16)
    allocated, e, issued = open_exchange(founder, leaf, root, deadline, b, view.members[1],
                                         fresh=(csr, request))
    final_query = founder.evidence(deadline)
    founder.counts(3, 1, deadline)
    final, final_log = read_view(node), read_log(node)
    consumed = final.records.get(b.query[16:])
    key = hashlib.sha256(spki).digest()
    require(allocated == 2 and final.allocation_floor == 3 and final.applied == e
            and final.members == {1: view.members[1], 2: key}
            and final.membership == view.membership and final.voters == view.voters
            and final.records.get(a.query[16:]) == (expiry, None)
            and consumed == (fresh_expiry, (request, key, 2, b.anchor, e, final.floor))
            and entry_tags(final_log) == [3, 4, 1, 1, 6]
            and final_log[-1] == (e, 6, b.query), "control-allocation")
    progress.update(enrollment_proposals=4, allocations=1)
    root_pem = node.parent / "public-root.pem"
    with root_pem.open("x", encoding="ascii") as pem:
        pem.write(ssl.DER_cert_to_PEM_cert(root))
    verify_issued(issued, spki, root_pem, final.cluster, allocated, deadline)
    after, _ = utc_witness(founder, "after-fresh-redeem", deadline)
    checked = query_evidence(final_query, b, 3, 1, (positive_before, after))
    require(fresh_view.floor <= checked < fresh_expiry - 2 * SKEW, "fresh-query-window")
    emit_view("fresh-positive", final, checked, fresh_expiry, redeem_position=e,
              stage_code=4, outcome="issued", allocations=1, anchor_queries=3, redeems=1)


def category(error):
    if isinstance(error, (TimeoutError, subprocess.TimeoutExpired)):
        return "timeout"
    return str(error) if isinstance(error, Failure) else type(error).__name__


def remember(first, stage, error):
    current = (stage, category(error))
    emit("first-failure" if first is None else "additional-failure", stage=current[0], category=current[1])
    return first or current


def provision(c13, roles, deadline):
    require(all(c13.unused(name, uid) for name, uid in c13.ROLES), "role-collision")
    for name, uid in c13.ROLES:
        roles.append(("group", name, uid))
        c13.command(["/usr/sbin/groupadd", "--gid", str(uid), name], deadline)
        roles.append(("user", name, uid))
        c13.command(["/usr/sbin/useradd", "--uid", str(uid), "--gid", name, "--no-create-home",
                     "--no-log-init", "--shell", "/bin/sh", "--password", "!", name], deadline)


def retained_effects(founder):
    view = read_view(founder.node)
    entries = read_log(founder.node)
    return {
        "applied": view.applied,
        "membership": view.membership,
        "allocations": view.allocation_floor - 2,
        "retained_positions": {
            label: [pos for pos, tag, _ in entries if tag == expected]
            for label, expected in (("found", 4), ("mint", 1), ("redeem", 6))
        },
    }


def privileged(stage, expected_sha, deadline):
    import shutil
    c13 = c13_tools()
    c13.qualify_runner()
    require(os.geteuid() == 0 and stage.parent == Path("/tmp")
            and re.fullmatch(r"arx-d5-[a-z0-9_]{8}", stage.name), "staging-path")
    metadata = stage.lstat()
    require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == int(os.environ["SUDO_UID"])
            and metadata.st_mode & 0o777 == 0o700, "staging-owner")
    deadline = min(deadline, time.monotonic() + TOTAL)
    cleanup_deadline = deadline - 6
    work_deadline = cleanup_deadline - CLEANUP
    setup_deadline = min(work_deadline, time.monotonic() + 20)
    roles, fixture, founder = [], None, None
    first, clean = None, False
    progress = dict(phase="setup", engine_starts=None, found=None, owner_mint_requests=0,
                    client_queries=0, committed_mints=None, enrollment_proposals=None, allocations=None)
    try:
        provision(c13, roles, setup_deadline)
        fixture = Path(tempfile.mkdtemp(prefix="arx-d5-fixture-", dir="/tmp"))
        c13.stage_tree(fixture)
        # The installed owner UID may be unable to traverse the checkout.
        owner_helper = fixture / "tools/owner/run.py"
        owner_helper.parent.mkdir(parents=True, mode=0o755)
        shutil.copyfile(SCRIPT, owner_helper)
        owner_helper.chmod(0o644)
        sha = c13.stage_binary(stage / "arxd", fixture / "arxd", setup_deadline)
        ip = c13.primary_ip(setup_deadline)
        require(sha == expected_sha, "artifact-pin")
        emit("executable", sha256=sha)
        progress["phase"] = "founder-start"
        founder = start_founder(fixture / "arxd", fixture / "a", ip)
        emit("process-started", count=1)
        campaign(founder, work_deadline, progress)
    except Exception as error:
        first = remember(first, progress["phase"], error)
    finally:
        effects = None
        if founder is not None:
            try:
                if first is not None and founder.poll() is None:
                    try:
                        founder.owner(4, min(cleanup_deadline - 15, time.monotonic() + 3))
                    except Exception as error:
                        remember(first, "final-owner-status", error)
                try:
                    remaining(cleanup_deadline - 14, 1)
                    effects = retained_effects(founder)
                except Exception as error:
                    first = remember(first, "final-applied-evidence", error)
                founder.close(cleanup_deadline)
                clean = True
            except Exception as error:
                first = remember(first, "owned-close", error)
        status = founder.status if founder else None
        emit("counters", **progress, status=status, owned_closes=int(clean), retained=effects,
             anchor_dispatches=status[1] if status else None, redeem_dispatches=status[2] if status else None)
        # Preserve failed/ambiguous fixtures until the disposable runner exits.
        if first is None or founder is None:
            try:
                for kind, name, uid in reversed(roles):
                    require(c13.remove_role(kind, name, uid, deadline) == "removed", "role-disposition")
                if fixture is not None:
                    remaining(deadline, 6)
                    require(fixture.parent == Path("/tmp") and stat.S_ISDIR(fixture.lstat().st_mode),
                            "fixture-disposition")
                    shutil.rmtree(fixture)
                    fixture = None
            except Exception as error:
                first = remember(first, "fixture-cleanup", error)
        emit("cleanup", close_signalled=bool(founder and founder.signalled),
             child_reaped=bool(founder and founder.reaped is not None), owned_close=clean,
             fixture_retained=fixture is not None)
    return first is None


def checked_run(env):
    required = {
        "GITHUB_EVENT_NAME": "workflow_dispatch",
        "GITHUB_REPOSITORY": "Dytschgo/arxcovo",
        "GITHUB_ACTOR": "Dytschgo",
        "GITHUB_TRIGGERING_ACTOR": "Dytschgo",
        "GITHUB_REF": "refs/heads/main",
        "GITHUB_RUN_ATTEMPT": "1",
        "GITHUB_JOB": "qualification",
        "GITHUB_WORKFLOW_REF": "Dytschgo/arxcovo/.github/workflows/m3-time-qualification.yml@refs/heads/main",
        "RUNNER_ARCH": "X64",
        "ImageOS": "ubuntu24",
    }
    require(all(env.get(key) == value for key, value in required.items()), "run-source")
    head, image = env.get("GITHUB_SHA", ""), env.get("ImageVersion", "")
    require(re.fullmatch("[a-f0-9]{40}", head)
            and head == env.get("GITHUB_WORKFLOW_SHA") == env.get("M3_TIME_REVIEWED_SHA"), "source-pin")
    require(re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,3}", image)
            and image == env.get("M3_TIME_REVIEWED_IMAGE"), "image-pin")
    require(re.fullmatch(r"[1-9][0-9]{0,19}", env.get("GITHUB_RUN_ID", "")), "run-number")
    return head, image, env["GITHUB_RUN_ID"]


def qualified_source(c13, deadline):
    c13.qualify_runner()
    require(os.geteuid() != 0 and Path.cwd().resolve() == ROOT, "launcher-entry")
    identity = checked_run(os.environ)
    code, output = run_helper(["/usr/bin/git", "rev-parse", "HEAD"], deadline, 3, limit=64)
    require(code == 0 and output.strip() == identity[0].encode("ascii"), "checked-out-head")
    return identity


def launch():
    c13 = c13_tools()
    deadline = time.monotonic() + TOTAL
    head, image, run = qualified_source(c13, deadline)
    target = Path(os.environ["CARGO_TARGET_DIR"])
    require(target.resolve() == ROOT / "target", "target-path")
    emit("run", head=head, image=image, run=run, attempt=1, job="qualification")
    with tempfile.TemporaryDirectory(prefix="arx-d5-", dir="/tmp") as directory:
        stage = Path(directory)
        sha = c13.stage_binary(target / "debug/arxd", stage / "arxd", deadline - TOTAL + 10)
        emit("build", sha256=sha)
        facts = [f"{key}={value}" for key, value in c13.HOSTED_ENV.items()]
        result = subprocess.run(
            ["/usr/bin/sudo", "-n", "/usr/bin/env", *facts, "/usr/bin/python3", "-I", "-B",
             str(SCRIPT), "--staged", str(stage), sha, str(int((deadline - 3) * 1_000_000))],
            env=ENV, stdin=subprocess.DEVNULL, timeout=remaining(deadline, TOTAL),
        )
    return result.returncode == 0


def main(args):
    if len(args) == 4 and args[0] == "--owner":
        _, path, pid, tag = args
        require(re.fullmatch(r"/tmp/arx-d5-fixture-[a-z0-9_]{8}/a/control/owner\.sock", path)
                and re.fullmatch("[1-9][0-9]{0,9}", pid)
                and tag in ("1", "4", "5") and os.geteuid() == OWNER, "owner-helper")
        try:
            body = owner_peer(path, int(pid), int(tag))
        except (FileNotFoundError, ConnectionRefusedError):
            return 3
        sys.stdout.buffer.write(body)
        return 0
    if len(args) == 2 and args[0] == "--reference":
        require(args[1] in ("0", "1") and sys.platform == "linux", "reference-helper")
        print(reference_request(int(args[1])))
        return 0
    if len(args) == 4 and args[0] == "--staged":
        require(re.fullmatch(r"[a-f0-9]{64}", args[2]) and re.fullmatch(r"[0-9]{1,20}", args[3]),
                "staging-pin")
        return 0 if privileged(Path(args[1]), args[2], int(args[3]) / 1_000_000) else 1
    if args == ["--preflight"]:
        head, image, run = qualified_source(c13_tools(), time.monotonic() + 3)
        emit("preflight", head=head, image=image, run=run, attempt=1, job="qualification")
        return 0
    require(not args, "launcher-arguments")
    return 0 if launch() else 1


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Exception as failure:
        if not sys.argv[1:] or sys.argv[1] in ("--staged", "--preflight"):
            remember(None, "launcher", failure)
        sys.exit(1)
