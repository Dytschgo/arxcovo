import errno
import io
import json
import pathlib
import socket as real_socket
import subprocess
import sys
import types


def build_dns_query(nonce):
    label = b"arx-" + nonce.hex().encode("ascii")
    return (
        nonce[:2]
        + bytes.fromhex("01000001000000000000")
        + bytes([len(label)])
        + label
        + b"\x07invalid\x00\x00\x01\x00\x01"
    )


def make_fake_socket_module(nonce, tcp, udp, dns, overrides, attempts):
    dns_query = build_dns_query(nonce)
    expected_dns_targets = [dns, ("10.0.2.3", 53)]
    so_bindtodevice = getattr(real_socket, "SO_BINDTODEVICE", 25)

    class FakeSocket:
        def __init__(self, family, mode):
            self.family = family
            self.mode = mode
            self.timeout = None
            self.bound_device = None
            self.bound_addr = None
            self.connected = None

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb):
            return False

        def fileno(self):
            return 3

        def settimeout(self, value):
            self.timeout = value

        def setsockopt(self, level, optname, value):
            if level == real_socket.SOL_SOCKET and optname == so_bindtodevice:
                self.bound_device = value

        def bind(self, addr):
            self.bound_addr = addr

        def connect(self, target):
            self.connected = target
            if self.mode == real_socket.SOCK_STREAM:
                assert self.bound_device == b"eth0\0", f"missing bindtodevice: {self.bound_device!r}"
                if ":" not in target[0]:
                    assert self.bound_addr == ("10.0.2.15", 0), f"bad IPv4 bind: {self.bound_addr!r}"
                else:
                    assert self.bound_addr is None, f"unexpected IPv6 bind: {self.bound_addr!r}"
                attempts.append(("tcp", target))
                action = overrides.get(("tcp", target))
                if action is None:
                    if target == tcp[0]:
                        raise TimeoutError("fixture timeout")
                    if target == tcp[1]:
                        raise OSError(errno.ENETUNREACH, "fixture no v6")
                    if target == tcp[2]:
                        raise OSError(errno.ECONNREFUSED, "fixture refused")
                    raise AssertionError(f"unexpected tcp target: {target!r}")
                if action == "connect":
                    return
                if isinstance(action, Exception):
                    raise action
                raise AssertionError(f"unknown tcp action: {action!r}")

        def sendto(self, payload, target):
            assert self.mode == real_socket.SOCK_DGRAM
            assert self.bound_device == b"eth0\0", f"missing bindtodevice: {self.bound_device!r}"
            if ":" not in target[0]:
                assert self.bound_addr == ("10.0.2.15", 0), f"bad IPv4 bind: {self.bound_addr!r}"
            else:
                assert self.bound_addr is None, f"unexpected IPv6 bind: {self.bound_addr!r}"
            assert payload == nonce, f"unexpected udp payload: {payload!r}"
            attempts.append(("udp", target))
            action = overrides.get(("udp", target))
            if action is None:
                if target == udp[1]:
                    raise OSError(errno.EADDRNOTAVAIL, "fixture no v6")
                if target in (udp[0], udp[2]):
                    return len(payload)
                raise AssertionError(f"unexpected udp target: {target!r}")
            if action == "short":
                return len(payload) - 1
            if isinstance(action, Exception):
                raise action
            raise AssertionError(f"unknown udp action: {action!r}")

        def send(self, payload):
            assert self.mode == real_socket.SOCK_DGRAM
            assert self.connected in expected_dns_targets, f"unexpected dns target: {self.connected!r}"
            assert self.bound_device == b"eth0\0", f"missing bindtodevice: {self.bound_device!r}"
            assert self.bound_addr == ("10.0.2.15", 0), f"bad dns bind: {self.bound_addr!r}"
            assert payload == dns_query, f"unexpected dns query: {payload!r}"
            attempts.append(("dns", self.connected))
            action = overrides.get(("dns_send", self.connected))
            if isinstance(action, Exception):
                raise action
            if action == "short":
                return len(payload) - 1
            return len(payload)

        def recv(self, size):
            assert size == 512
            action = overrides.get(("dns_recv", self.connected))
            if action is None:
                raise TimeoutError("fixture dns timeout")
            if isinstance(action, bytes):
                return action
            if isinstance(action, Exception):
                raise action
            raise AssertionError(f"unknown dns recv action: {action!r}")

    mod = types.ModuleType("socket")
    for attr in ("AF_INET", "AF_INET6", "SOCK_STREAM", "SOCK_DGRAM", "SOL_SOCKET", "inet_ntoa"):
        setattr(mod, attr, getattr(real_socket, attr))
    mod.SO_BINDTODEVICE = so_bindtodevice
    mod.socket = FakeSocket
    return mod


def run_arxnet3_case(
    script_source,
    nonce,
    tcp,
    udp,
    dns,
    *,
    overrides=None,
    has_nic=True,
    ipv4="10.0.2.15",
    has_default_route=True,
    global_v6=False,
    neigh_stdout=b"10.0.2.2 lladdr 52:55:0a:00:02:02 REACHABLE\n",
    neigh_fail=False,
    port_exists=True,
):
    overrides = overrides or {}
    attempts = []
    neigh_calls = []
    wire = io.BytesIO()

    fake_sock = make_fake_socket_module(nonce, tcp, udp, dns, overrides, attempts)

    fake_fcntl = types.ModuleType("fcntl")

    def fake_ioctl(fd, req, arg):
        assert req == 0x8915
        if ipv4 is None:
            raise OSError("no addr")
        return bytes(20) + real_socket.inet_aton(ipv4) + bytes(232)

    fake_fcntl.ioctl = fake_ioctl

    class FakePath:
        def __init__(self, path):
            self._path = str(path)
            self.name = self._path.rsplit("/", 1)[-1]

        def __truediv__(self, other):
            return FakePath(self._path.rstrip("/") + "/" + str(other))

        def resolve(self):
            if self._path == "/sys/class/net/eth0/device/driver":
                return FakePath("/sys/bus/virtio/drivers/virtio_net")
            return self

        def iterdir(self):
            if self._path == "/sys/class/net":
                items = [FakePath("/sys/class/net/lo")]
                if has_nic:
                    items.append(FakePath("/sys/class/net/eth0"))
                return iter(items)
            raise OSError(f"unexpected iterdir: {self._path}")

        def read_text(self):
            if self._path == "/sys/class/net/eth0/operstate":
                return "up\n"
            if self._path == "/proc/net/route":
                header = "Iface\tDestination\tGateway\tFlags\n"
                if has_default_route:
                    return header + "eth0\t00000000\t0202000A\t0003\n"
                return header
            if self._path == "/proc/net/if_inet6":
                if global_v6:
                    return "20010db8000000000000000000000001 02 40 00 80 eth0\n"
                return "fe800000000000000000000000000001 02 40 20 80 eth0\n"
            if self._path == "/proc/net/ipv6_route":
                return ""
            raise OSError(f"unexpected read_text: {self._path}")

        def exists(self):
            if self._path == "/dev/virtio-ports/org.arxcovo.agent.0":
                return port_exists
            return False

        def open(self, mode="r", buffering=-1):
            assert self._path == "/dev/virtio-ports/org.arxcovo.agent.0"
            assert mode == "wb" and buffering == 0

            class CloseSafeBytesIO:
                def __enter__(self_inner):
                    return wire

                def __exit__(self_inner, exc_type, exc, tb):
                    return False

            return CloseSafeBytesIO()

    fake_pathlib = types.ModuleType("pathlib")
    fake_pathlib.Path = FakePath

    fake_subprocess = types.ModuleType("subprocess")
    fake_subprocess.SubprocessError = subprocess.SubprocessError
    fake_subprocess.TimeoutExpired = subprocess.TimeoutExpired

    def fake_run(args, **kwargs):
        if args == ["ip", "-4", "neigh", "get", "10.0.2.2", "dev", "eth0"]:
            neigh_calls.append(len(attempts))
            if neigh_fail:
                raise subprocess.CalledProcessError(1, args)
            # Require the host-alias UDP probe (UDP[2]) to have been sent before
            # the gateway neighbor becomes REACHABLE.
            if ("udp", udp[2]) not in attempts:
                return subprocess.CompletedProcess(
                    args, 0, stdout=b"10.0.2.2 lladdr 52:55:0a:00:02:02 STALE\n"
                )
            return subprocess.CompletedProcess(args, 0, stdout=neigh_stdout)
        if args == ["ip", "-4", "route", "add", "default", "via", "10.0.2.2", "dev", "eth0"]:
            return subprocess.CompletedProcess(args, 1, stdout=b"")
        raise AssertionError(f"unexpected subprocess.run: {args!r}")

    fake_subprocess.run = fake_run

    fake_time = types.ModuleType("time")
    fake_time.sleep = lambda _s: None

    saved = {}
    injected = {
        "socket": fake_sock,
        "fcntl": fake_fcntl,
        "pathlib": fake_pathlib,
        "subprocess": fake_subprocess,
        "time": fake_time,
    }
    try:
        for name, mod in injected.items():
            saved[name] = sys.modules.get(name)
            sys.modules[name] = mod
        ns = {"__name__": "__main__"}
        exec(compile(script_source, "/run/arxcovo-network-probe.py", "exec"), ns, ns)
    finally:
        for name, old in saved.items():
            if old is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = old

    return wire.getvalue().decode("ascii"), attempts, neigh_calls


def run_installed_case(
    script_source,
    nonce,
    tcp,
    udp,
    dns,
    *,
    overrides=None,
    ipv4_addr_out=b"2: eth0 inet 10.0.2.15/24 scope global eth0\n",
    ipv4_route_out=b"default via 10.0.2.2 dev eth0\n",
    ipv6_addr_out=b"",
    ipv6_route_out=b"",
    neigh_stdout=b"10.0.2.2 lladdr 52:55:0a:00:02:02 REACHABLE\n",
    neigh_fail=False,
):
    overrides = overrides or {}
    attempts = []
    neigh_calls = []

    fake_sock = make_fake_socket_module(nonce, tcp, udp, dns, overrides, attempts)

    class FakePath:
        def __init__(self, path):
            self._path = str(path)
            self.name = self._path.rsplit("/", 1)[-1]

        def __truediv__(self, other):
            return FakePath(self._path.rstrip("/") + "/" + str(other))

        def resolve(self):
            if self._path == "/sys/class/net/eth0/device/driver":
                return FakePath("/sys/bus/virtio/drivers/virtio_net")
            return self

        def iterdir(self):
            if self._path == "/sys/class/net":
                return iter([FakePath("/sys/class/net/lo"), FakePath("/sys/class/net/eth0")])
            raise OSError(f"unexpected iterdir: {self._path}")

        def read_text(self):
            if self._path == "/sys/class/net/eth0/operstate":
                return "up\n"
            raise OSError(f"unexpected read_text: {self._path}")

    fake_pathlib = types.ModuleType("pathlib")
    fake_pathlib.Path = FakePath

    fake_subprocess = types.ModuleType("subprocess")
    fake_subprocess.SubprocessError = subprocess.SubprocessError
    fake_subprocess.TimeoutExpired = subprocess.TimeoutExpired

    def fake_run(args, **kwargs):
        if args == ["/usr/sbin/ip", "-4", "addr", "show", "dev", "eth0"]:
            return subprocess.CompletedProcess(args, 0, stdout=ipv4_addr_out)
        if args == ["/usr/sbin/ip", "-4", "route", "show", "default", "dev", "eth0"]:
            return subprocess.CompletedProcess(args, 0, stdout=ipv4_route_out)
        if args == ["/usr/sbin/ip", "-6", "addr", "show", "dev", "eth0", "scope", "global"]:
            return subprocess.CompletedProcess(args, 0, stdout=ipv6_addr_out)
        if args == ["/usr/sbin/ip", "-6", "route", "show", "default", "dev", "eth0"]:
            return subprocess.CompletedProcess(args, 0, stdout=ipv6_route_out)
        if args == ["ip", "-4", "neigh", "get", "10.0.2.2", "dev", "eth0"]:
            neigh_calls.append(len(attempts))
            if neigh_fail:
                raise subprocess.CalledProcessError(1, args)
            if ("udp", udp[2]) not in attempts:
                return subprocess.CompletedProcess(
                    args, 0, stdout=b"10.0.2.2 lladdr 52:55:0a:00:02:02 STALE\n"
                )
            return subprocess.CompletedProcess(args, 0, stdout=neigh_stdout)
        raise AssertionError(f"unexpected subprocess.run: {args!r}")

    fake_subprocess.run = fake_run

    fake_time = types.ModuleType("time")
    fake_time.sleep = lambda _s: None

    saved = {}
    injected = {
        "socket": fake_sock,
        "pathlib": fake_pathlib,
        "subprocess": fake_subprocess,
        "time": fake_time,
    }
    exc_name = None
    try:
        for name, mod in injected.items():
            saved[name] = sys.modules.get(name)
            sys.modules[name] = mod
        ns = {"__name__": "__main__"}
        exec(compile(script_source, "installed_witness_script.py", "exec"), ns, ns)
    except BaseException as err:
        exc_name = type(err).__name__
    finally:
        for name, old in saved.items():
            if old is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = old

    return exc_name, attempts, neigh_calls


def main():
    mode = sys.argv[1]
    meta = json.loads(sys.argv[2])
    script_source = sys.stdin.read()
    nonce = bytes.fromhex(meta["nonce"])
    tcp = [tuple(x) for x in meta["tcp"]]
    udp = [tuple(x) for x in meta["udp"]]
    dns = tuple(meta["dns"])
    query = build_dns_query(nonce)
    good_dns = query[:2] + b"\x81\x83" + query[4:]
    tc_dns = query[:2] + b"\x83\x80" + query[4:]
    expected_attempts = [
        ("udp", udp[2]),
        ("tcp", tcp[0]),
        ("tcp", tcp[1]),
        ("tcp", tcp[2]),
        ("udp", udp[0]),
        ("udp", udp[1]),
        ("dns", dns),
        ("dns", ("10.0.2.3", 53)),
    ]

    if mode == "arxnet3":
        frames = {}

        # 1. Reachable control and full restricted attempt order.
        frame, attempts, neigh_calls = run_arxnet3_case(script_source, nonce, tcp, udp, dns)
        assert attempts == expected_attempts, f"wrong attempt sequence: {attempts!r}"
        assert neigh_calls == [1], f"gateway check must occur right after UDP[2]: {neigh_calls!r}"
        frames["reachable"] = frame

        # 2. Gateway control failures (proves gateway result is not hard-coded).
        for label, kw in [
            ("gateway_stale", {"neigh_stdout": b"10.0.2.2 lladdr 52:55:0a:00:02:02 STALE\n"}),
            ("gateway_wrong_ip", {"neigh_stdout": b"10.0.2.1 lladdr 52:55:0a:00:02:02 REACHABLE\n"}),
            ("gateway_no_lladdr", {"neigh_stdout": b"10.0.2.2 REACHABLE\n"}),
            ("gateway_cmd_fail", {"neigh_fail": True}),
        ]:
            frame, attempts, _ = run_arxnet3_case(script_source, nonce, tcp, udp, dns, **kw)
            assert attempts == expected_attempts, f"{label}: wrong attempts {attempts!r}"
            assert frame.split(" ")[3] == "0", f"{label}: expected control=0, got {frame!r}"
            frames[label] = frame

        # 3. Local IPv4 errors must classify as 'E'.
        for label, ov, idx in [
            ("ipv4_tcp_local_err", {("tcp", tcp[0]): OSError(errno.ENETUNREACH, "local")}, 0),
            ("ipv4_udp_local_err", {("udp", udp[0]): OSError(errno.ENETUNREACH, "local")}, 3),
            ("ipv4_udp_short_send", {("udp", udp[2]): "short"}, 5),
            ("ipv4_dns_local_err", {("dns_send", dns): OSError(errno.ENETUNREACH, "local")}, 6),
        ]:
            frame, attempts, _ = run_arxnet3_case(script_source, nonce, tcp, udp, dns, overrides=ov)
            assert attempts == expected_attempts, f"{label}: wrong attempts {attempts!r}"
            assert frame.split(" ")[4][idx] == "E", f"{label}: expected E at {idx}, got {frame!r}"
            frames[label] = frame

        # 4. Absent setup / malformed DNS / unexpected reachability / absent virtio port.
        for label, kw in [
            ("absent_no_lease", {"ipv4": None}),
            ("absent_no_route", {"has_default_route": False}),
            ("absent_no_nic", {"has_nic": False}),
            ("absent_global_v6", {"global_v6": True}),
        ]:
            frame, attempts, neigh_calls = run_arxnet3_case(script_source, nonce, tcp, udp, dns, **kw)
            assert attempts == [], f"{label}: expected zero attempts, got {attempts!r}"
            assert neigh_calls == [], f"{label}: expected zero gateway checks, got {neigh_calls!r}"
            frames[label] = frame

        for label, ov, idx, code in [
            ("malformed_dns_short", {("dns_recv", dns): good_dns[:-1]}, 6, "M"),
            (
                "malformed_dns_wrong_id",
                {("dns_recv", dns): bytes([good_dns[0] ^ 0xFF]) + good_dns[1:]},
                6,
                "M",
            ),
            ("malformed_dns_tc", {("dns_recv", ("10.0.2.3", 53)): tc_dns}, 7, "M"),
            ("valid_dns_reply", {("dns_recv", dns): good_dns}, 6, "R"),
            ("unexpected_tcp_connect", {("tcp", tcp[0]): "connect"}, 0, "R"),
        ]:
            frame, attempts, _ = run_arxnet3_case(script_source, nonce, tcp, udp, dns, overrides=ov)
            assert attempts == expected_attempts, f"{label}: wrong attempts {attempts!r}"
            assert frame.split(" ")[4][idx] == code, f"{label}: expected {code}, got {frame!r}"
            frames[label] = frame

        frame, attempts, _ = run_arxnet3_case(script_source, nonce, tcp, udp, dns, port_exists=False)
        assert attempts == expected_attempts
        assert frame == "", f"expected empty output when port absent, got {frame!r}"
        frames["absent_port"] = frame

        print(json.dumps(frames))
        return

    if mode == "installed":
        # 1. Reachable control and all 8 attempts in required order.
        exc, attempts, neigh_calls = run_installed_case(script_source, nonce, tcp, udp, dns)
        assert exc is None, f"happy path failed with {exc}"
        assert attempts == expected_attempts, f"wrong attempt sequence: {attempts!r}"
        assert neigh_calls == [1], f"gateway check must occur right after UDP[2]: {neigh_calls!r}"

        # 2. Gateway control failures must fail closed with SystemExit.
        for label, kw in [
            ("gateway_stale", {"neigh_stdout": b"10.0.2.2 lladdr 52:55:0a:00:02:02 STALE\n"}),
            ("gateway_cmd_fail", {"neigh_fail": True}),
        ]:
            exc, attempts, _ = run_installed_case(script_source, nonce, tcp, udp, dns, **kw)
            assert exc == "SystemExit", f"{label}: expected SystemExit, got {exc!r}"
            assert attempts == [("udp", udp[2])], f"{label}: stopped before later probes"

        # 3. Local IPv4 errors must fail the script.
        for label, ov in [
            ("ipv4_tcp_local_err", {("tcp", tcp[0]): OSError(errno.ENETUNREACH, "local")}),
            ("ipv4_udp_local_err", {("udp", udp[0]): OSError(errno.ENETUNREACH, "local")}),
            ("ipv4_dns_local_err", {("dns_send", dns): OSError(errno.ENETUNREACH, "local")}),
        ]:
            exc, _, _ = run_installed_case(script_source, nonce, tcp, udp, dns, overrides=ov)
            assert exc == "AssertionError", f"{label}: expected AssertionError, got {exc!r}"

        # 4. Absent or malformed route/address/DNS/TCP evidence must fail.
        for label, kw in [
            ("absent_route", {"ipv4_route_out": b""}),
            ("wrong_gateway", {"ipv4_route_out": b"default via 10.0.2.1 dev eth0\n"}),
            ("absent_ipv4", {"ipv4_addr_out": b"2: eth0 inet 10.0.2.99/24 scope global eth0\n"}),
            ("global_v6_addr", {"ipv6_addr_out": b"inet6 2001:db8::1/64 scope global\n"}),
            ("v6_default_route", {"ipv6_route_out": b"default via fe80::1 dev eth0\n"}),
            ("unexpected_tcp_connect", {"overrides": {("tcp", tcp[0]): "connect"}}),
            ("ipv6_tcp_timeout", {"overrides": {("tcp", tcp[1]): TimeoutError("v6 timeout")}}),
            ("valid_dns_reply", {"overrides": {("dns_recv", dns): good_dns}}),
            ("malformed_dns_reply", {"overrides": {("dns_recv", dns): good_dns[:-1]}}),
        ]:
            exc, _, _ = run_installed_case(script_source, nonce, tcp, udp, dns, **kw)
            assert exc == "AssertionError", f"{label}: expected AssertionError, got {exc!r}"

        return

    raise AssertionError(f"unknown mode: {mode!r}")


if __name__ == "__main__":
    main()
