import errno
import hashlib
import random
import socket
import struct
import time
import unittest
from concurrent.futures import Future
from queue import SimpleQueue
from threading import Event, Thread
from unittest.mock import MagicMock, patch

import static_network_witness as network
import static_public_witness as public


class WitnessTests(unittest.TestCase):
    def test_network_denials_keep_unexpected_socket_errors_visible(self):
        network.denied(OSError(errno.ENETUNREACH, "fixture"))
        with self.assertRaises(OSError):
            network.denied(OSError(errno.EBADF, "fixture"))

    def test_each_denial_requires_both_transports_and_test_keeps_restriction(self):
        denied = ["10.0.2.2:80", "10.0.2.3:53", "100.64.0.1:53", "169.254.1.1:53"]
        inventory = {"public_tcp_target": "public-tcp", "public_dns_target": "public-dns", "denied_targets": denied}
        calls = []
        def connect(target):
            calls.append(("tcp", target))
            return target == "public-tcp"
        def query(target):
            calls.append(("udp", target))
            return (target == "public-dns", target == "public-dns")
        rows = network.measure(inventory, "static", connect, query)["denied"]
        self.assertEqual([row["target"] for row in rows], denied)
        for target in denied:
            self.assertIn(("tcp", target), calls)
            self.assertIn(("udp", target), calls)
        with self.assertRaises(ValueError):
            network.measure(inventory, "static", connect, lambda target: (True, target == "public-dns"))
        with self.assertRaises(ValueError):
            network.measure(inventory, "test", lambda target: False, lambda target: (True, False))

    def test_udp_reports_even_an_empty_or_malformed_datagram(self):
        for packet in (b"", b"invalid", bytes.fromhex("123481800001000100000000")):
            with self.subTest(packet=packet), patch.object(network.socket, "socket") as create:
                fake = MagicMock()
                fake.recv.return_value = packet
                create.return_value.__enter__.return_value = fake
                with patch.object(network.secrets, "token_bytes", return_value=b"\x12\x34"):
                    self.assertEqual(network.udp("192.0.2.1:53"), (True, False))

    @staticmethod
    def _challenge(phase):
        return {"version": 1, "phase": phase, "challenge": "fixture",
                "reference": {"hosts": [{"name": name, "path": "/marker", "marker": name} for name in ("one", "two")],
                              "media_path": "/media", "start": 1, "end": 2, "total": 4}}

    def test_public_witness_records_real_bytes_and_requires_cleanup_inspection(self):
        request = self._challenge("republished")
        def fetch(host, path, interval=None):
            if path == "/marker":
                return 200, {}, host.encode()
            if interval == "bytes=1-2":
                return 206, {"Content-Length": "2", "Content-Range": "bytes 1-2/4", "Accept-Ranges": "bytes"}, b"bc"
            if interval == "bytes=4-":
                return 416, {"Content-Range": "bytes */4"}, b""
            return 200, {}, b"abcd"
        measured = public.measure(request, False, fetch)
        self.assertEqual(measured["matching_markers"], 2)
        self.assertEqual(measured["partial_sha256"], hashlib.sha256(b"bc").hexdigest())
        self.assertEqual(measured["unsatisfiable_range"], "bytes */4")
        request["phase"] = "cleanup"
        with self.assertRaises(ValueError):
            public.measure(request, False, fetch)
        with self.assertRaises(ValueError):
            public.measure(request, True, fetch)
        self.assertEqual(public.measure(request, True, lambda *_: (404, {}, b""))["names_no_longer_serve"], 2)

    @staticmethod
    def _query():
        return b"\x12\x34" + bytes.fromhex("01000001000000000000") + b"\x07example\x03org\x00\x00\x01\x00\x01"

    @staticmethod
    def _rr(owner, kind=1, data=b"\xc0\x00\x02\x01"):
        return owner + struct.pack("!HHIH", kind, 1, 60, len(data)) + data

    def _reply(self, *answers):
        return struct.pack("!6H", 0x1234, 0x8180, 1, len(answers), 0, 0) + self._query()[12:] + b"".join(answers)

    def test_dns_answer_requires_a_complete_matching_a_record(self):
        query = self._query()
        header = bytes.fromhex("123481800001000100000000")
        answer = bytes.fromhex("c00c000100010000003c00045db8d822")
        self.assertTrue(network.dns_answered(header + query[12:] + answer, query))
        # Header that claims one answer but carries none (review reproduction).
        self.assertFalse(network.dns_answered(header, query))
        self.assertFalse(network.dns_answered(header + query[12:], query))
        # Truncated record, wrong id, mismatched question, non-A record, error rcode.
        self.assertFalse(network.dns_answered(header + query[12:] + answer[:-2], query))
        self.assertFalse(network.dns_answered(b"\x00\x00" + (header + query[12:] + answer)[2:], query))
        other = query[:13] + b"x" + query[14:]
        self.assertFalse(network.dns_answered(header + other[12:] + answer, query))
        aaaa = bytes.fromhex("c00c001c00010000003c000400000000")
        self.assertFalse(network.dns_answered(header + query[12:] + aaaa, query))
        nxdomain = header[:3] + b"\x83" + header[4:]
        self.assertFalse(network.dns_answered(nxdomain + query[12:] + answer, query))

    def test_dns_rejects_malformed_names_unrelated_owners_and_short_answer_sections(self):
        query = self._query()
        header = bytes.fromhex("123481800001000100000000")
        a_rest = bytes.fromhex("000100010000003c00045db8d822")  # TYPE A, IN, TTL, RDLENGTH 4, address

        def reply(answers_bytes, count=1):
            h = header[:6] + count.to_bytes(2, "big") + header[8:]
            return h + query[12:] + answers_bytes

        self_pointer = len(query).to_bytes(2, "big")
        self_pointer = bytes([0xC0 | self_pointer[0], self_pointer[1]])
        self.assertFalse(network.dns_answered(reply(self_pointer + a_rest), query))
        self.assertFalse(network.dns_answered(reply(b"\xcf\xff" + a_rest), query))
        self.assertFalse(network.dns_answered(reply(b"\xc0\x04" + a_rest), query))
        overlong = b"\x40" + b"a" * 64 + b"\x00"
        self.assertFalse(network.dns_answered(reply(overlong + a_rest), query))
        unrelated = b"\x05other\x03org\x00"
        self.assertFalse(network.dns_answered(reply(unrelated + a_rest), query))
        self.assertFalse(network.dns_answered(reply(b"\xc0\x0c" + a_rest, count=2), query))
        literal_owner = b"\x07EXAMPLE\x03org\x00"
        self.assertTrue(network.dns_answered(reply(literal_owner + a_rest), query))
        # CNAME example.org -> www.example.org, then A for the alias.
        cname_rdata = b"\x03www\xc0\x0c"
        cname = b"\xc0\x0c" + bytes.fromhex("000500010000003c") + len(cname_rdata).to_bytes(2, "big") + cname_rdata
        alias_offset = len(query) + 12
        alias_ptr = bytes([0xC0 | (alias_offset >> 8), alias_offset & 0xFF])
        self.assertTrue(network.dns_answered(reply(cname + alias_ptr + a_rest, count=2), query))
        self.assertFalse(network.dns_answered(reply(cname + unrelated + a_rest, count=2), query))
        valid = reply(b"\xc0\x0c" + a_rest)
        for flags in (0x0180, 0x8380, 0x8980, 0x8183):
            with self.subTest(flags=flags):
                self.assertFalse(network.dns_answered(valid[:2] + struct.pack("!H", flags) + valid[4:], query))
        for valid in (valid, reply(cname + alias_ptr + a_rest, count=2)):
            for end in range(len(valid)):
                self.assertFalse(network.dns_answered(valid[:end], query))

    def test_dns_compression_requires_a_previously_decoded_name_boundary(self):
        query_owner = b"\xc0\x0c"
        alias = b"\x03www\x00"
        cname = self._rr(query_owner, 5, alias)
        other_owner = b"\x05other\x00"
        previous = self._rr(other_owner, 16, b"\x05" + alias)
        offset = len(self._query()) + len(cname) + len(other_owner) + 10
        address_inside_txt = self._rr(struct.pack("!H", 0xC000 | (offset + 1)))
        self.assertFalse(network.dns_answered(self._reply(cname, previous, address_inside_txt), self._query()))
        previous = self._rr(b"\x05" + alias + b"\x00", 16, b"\x00")
        offset = len(self._query()) + len(cname) + 1
        address_inside_label = self._rr(struct.pack("!H", 0xC000 | offset))
        self.assertFalse(network.dns_answered(self._reply(cname, previous, address_inside_label), self._query()))

        # The suffix of a genuine name in NS RDATA, including a reference to
        # its root octet, remains available for compression.
        ns = self._rr(other_owner, 2, b"\x02ns" + alias)
        offset = len(self._query()) + len(other_owner) + 10 + 3
        cname = self._rr(query_owner, 5, struct.pack("!H", 0xC000 | offset))
        root = struct.pack("!H", 0xC000 | (offset + len(alias) - 1))
        self.assertTrue(network.dns_answered(self._reply(ns, cname, self._rr(alias[:-1] + root)), self._query()))

    def test_dns_rejects_alias_conflicts_and_loops(self):
        wanted = b"\xc0\x0c"
        alias = b"\x03www\x00"
        cname = self._rr(wanted, 5, alias)
        competing = self._rr(wanted, 5, b"\x03alt\x00")
        direct = self._rr(wanted)
        alias_address = self._rr(alias)
        for answers in ((direct, cname, alias_address), (cname, direct, alias_address),
                        (direct, cname, competing), (cname, competing, alias_address),
                        (cname, cname, alias_address), (cname, self._rr(wanted, 16, b"\x00"), alias_address),
                        (cname, self._rr(alias, 5, wanted))):
            with self.subTest(answers=answers):
                self.assertFalse(network.dns_answered(self._reply(*answers), self._query()))

    def test_dns_counts_the_root_in_expanded_name_length(self):
        wanted = b"\xc0\x0c"
        for length in (255, 256):
            alias = (b"\x3f" + b"a" * 63) * 3
            last = length - len(alias) - 2
            alias += bytes([last]) + b"b" * last + b"\x00"
            cname = self._rr(wanted, 5, alias)
            address = self._rr(struct.pack("!H", 0xC000 | (len(self._query()) + 12)))
            with self.subTest(length=length):
                self.assertEqual(len(alias), length)
                self.assertEqual(network.dns_answered(self._reply(cname, address), self._query()), length == 255)
                self.assertFalse(network.dns_answered(self._reply(self._rr(alias)), self._query()))
        suffix = (b"\x3f" + b"a" * 63) * 3 + b"\x3b" + b"b" * 59 + b"\x00"
        other = b"\x05other\x00"
        ns = self._rr(other, 2, suffix)
        pointer = struct.pack("!H", 0xC000 | (len(self._query()) + len(other) + 10))
        for label in (b"\x01x", b"\x02xx"):
            address = self._rr(label + pointer)
            cname = self._rr(wanted, 5, label + pointer)
            self.assertEqual(network.dns_answered(self._reply(ns, cname, address), self._query()), len(label) == 2)

    def test_dns_follows_up_to_eight_alias_links(self):
        for length in (7, 8, 9):
            names = [b"\xc0\x0c"] + [b"\x01" + bytes([ord("a") + i]) + b"\x00" for i in range(length)]
            answers = [self._rr(owner, 5, target) for owner, target in zip(names, names[1:])]
            answers.append(self._rr(names[-1]))
            with self.subTest(length=length):
                self.assertEqual(network.dns_answered(self._reply(*answers), self._query()), length <= 8)

    def test_dns_deterministic_mutations_match_a_direct_answer_reference(self):
        result = Future()

        def run():
            try:
                self._check_dns_mutations()
            except Exception as error:
                result.set_exception(error)
            else:
                result.set_result(None)

        Thread(target=run, daemon=True).start()
        result.result(timeout=2)

    def _check_dns_mutations(self):
        query = self._query()
        seed = self._reply(self._rr(b"\xc0\x0c"))
        rng = random.Random(341)
        accepted = 0
        for _ in range(2000):
            packet = bytearray(seed)
            for _ in range(rng.randrange(1, 5)):
                packet[rng.randrange(len(packet))] ^= rng.randrange(1, 256)
            if rng.randrange(3) == 0:
                del packet[rng.randrange(len(packet)):]
            # At this size only a compressed owner can encode example.org
            # while leaving room for the RR header and four-byte address.
            expected = False
            if len(packet) == len(seed):
                tag, flags, questions, answers = struct.unpack_from("!4H", packet)
                owner, kind, rrclass, _, size = struct.unpack_from("!HHHIH", packet, len(query))
                expected = (tag == 0x1234 and flags & ~0x05B0 == 0x8000
                            and questions == answers == 1 and packet[12:len(query)] == query[12:]
                            and (owner, kind, rrclass, size) == (0xC00C, 1, 1, 4))
            actual = network.dns_answered(bytes(packet), query)
            self.assertEqual(actual, expected, packet.hex())
            accepted += actual
        self.assertGreater(accepted, 0)
        self.assertLess(accepted, 1000)

    @staticmethod
    def _fake_connection(now, read_advance, body=b"x", header_advance=0.0, request_advance=0.0):
        class Response:
            status, headers = 200, {}

            def __init__(self):
                self.sent = False

            def __enter__(self):
                return self

            def __exit__(self, *_):
                pass

            def read(self, _size):
                now[0] += read_advance
                if self.sent:
                    return b""
                self.sent = True
                return body

        class Connection:
            sock = None

            def __init__(self, timeout):
                self.timeout = timeout

            def request(self, *args, **kwargs):
                now[0] += request_advance

            def getresponse(self):
                now[0] += header_advance
                return Response()

            def close(self):
                pass

        return Connection

    def test_public_fetch_rejects_late_request_headers_and_eof(self):
        for options in ({"request_advance": 61}, {"header_advance": 61}, {"body": b""}):
            with self.subTest(options=options):
                now = [0.0]
                read_advance = 61 if "body" in options else 0
                with self.assertRaises(TimeoutError):
                    public.get("example.invalid", "/", end=60, clock=lambda: now[0],
                               connection_factory=self._fake_connection(now, read_advance, **options))

    def test_public_fetch_shares_the_deadline_across_requests(self):
        now = [0.0]
        fetch = public.phase_fetch(clock=lambda: now[0], deadline_s=60,
                                   connection_factory=self._fake_connection(now, 11))
        self.assertEqual(fetch("example.invalid", "/")[0], 200)
        self.assertEqual(fetch("example.invalid", "/")[0], 200)
        with self.assertRaises(TimeoutError):
            fetch("example.invalid", "/")

    def test_public_cleanup_counts_only_timely_name_errors(self):
        codes = {socket.EAI_NONAME, getattr(socket, "EAI_NODATA", socket.EAI_NONAME)}
        for dns_code in codes:
            with self.subTest(dns_code=dns_code):
                waiting = Event()
                now = [0.0]

                class SignalQueue(SimpleQueue):
                    def get(self, *args, **kwargs):
                        waiting.set()
                        return super().get(*args, **kwargs)

                class MissingName:
                    def request(self, *args, **kwargs):
                        raise socket.gaierror(dns_code, "fixture")

                    def close(self):
                        pass

                class LateMissingName(MissingName):
                    def request(self, *args, **kwargs):
                        if not waiting.wait(1):
                            raise AssertionError("fetch did not start waiting")
                        now[0] = 61
                        super().request(*args, **kwargs)

                cleanup = self._challenge("cleanup")
                factory = MagicMock(side_effect=[MissingName(), MissingName()])
                fetch = public.phase_fetch(clock=lambda: now[0], connection_factory=factory)
                self.assertEqual(public.measure(cleanup, True, fetch)["names_no_longer_serve"], 2)
                factory = MagicMock(side_effect=[MissingName(), LateMissingName()])
                fetch = public.phase_fetch(clock=lambda: now[0], connection_factory=factory)
                with patch.object(public, "SimpleQueue", side_effect=[SimpleQueue(), SignalQueue()]):
                    with self.assertRaises(TimeoutError):
                        public.measure(cleanup, True, fetch)
                self.assertEqual(factory.call_count, 2)

    def test_public_fetch_abandons_a_blocked_worker(self):
        for step in ("request", "getresponse", "read"):
            with self.subTest(step=step):
                started, release, closed = Event(), Event(), Event()

                def block(*args, **kwargs):
                    started.set()
                    release.wait(2)
                    return b""

                response = MagicMock()
                response.__enter__.return_value = response
                connection = MagicMock(sock=None)
                connection.getresponse.return_value = response
                connection.close.side_effect = closed.set
                blocked = response if step == "read" else connection
                getattr(blocked, step).side_effect = block
                start = time.monotonic()
                try:
                    with self.assertRaises(TimeoutError):
                        public.get("example.invalid", "/", end=start + 0.10,
                                   connection_factory=lambda _: connection)
                    self.assertTrue(started.is_set())
                    self.assertFalse(closed.is_set())
                    self.assertLess(time.monotonic() - start, 1.0)
                finally:
                    release.set()
                    self.assertTrue(closed.wait(1))
