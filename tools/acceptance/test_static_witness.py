import errno
import hashlib
import unittest

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
            network.measure(inventory, "static", connect, lambda target: (True, True))
        with self.assertRaises(ValueError):
            network.measure(inventory, "test", lambda target: False, lambda target: (True, False))

    def test_public_witness_records_real_bytes_and_requires_cleanup_inspection(self):
        request = {"version": 1, "phase": "republished", "challenge": "fixture",
                   "reference": {"hosts": [{"name": name, "path": "/marker", "marker": name} for name in ("one", "two")],
                                 "media_path": "/media", "start": 1, "end": 2, "total": 4}}
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
        truncated_flag = reply(b"\xc0\x0c" + a_rest)
        truncated_flag = truncated_flag[:2] + bytes([truncated_flag[2] | 0x02]) + truncated_flag[3:]
        self.assertFalse(network.dns_answered(truncated_flag, query))

    @staticmethod
    def _fake_connection(now, read_advance, body=b"x", header_advance=0.0):
        class Response:
            status, headers = 200, {}

            def __init__(self):
                self.sent = False

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
                pass

            def getresponse(self):
                now[0] += header_advance
                return Response()

            def close(self):
                pass

        return Connection

    def test_public_fetch_enforces_a_total_deadline(self):
        now = [0.0]
        clock = lambda: now[0]
        # A read that returns late (here: EOF at 61 s) is still refused.
        with self.assertRaises(TimeoutError):
            public.get("example.invalid", "/", end=60, clock=clock,
                       connection_factory=self._fake_connection(now, 61, body=b""))
        # Late response headers are refused before any body is read.
        now[0] = 0.0
        with self.assertRaises(TimeoutError):
            public.get("example.invalid", "/", end=60, clock=clock,
                       connection_factory=self._fake_connection(now, 0, header_advance=61))
        # One deadline spans every request of a phase.
        now[0] = 0.0
        fetch = public.phase_fetch(clock=clock, deadline_s=60,
                                   connection_factory=self._fake_connection(now, 20))
        self.assertEqual(fetch("example.invalid", "/")[0], 200)
        with self.assertRaises(TimeoutError):
            fetch("example.invalid", "/")
