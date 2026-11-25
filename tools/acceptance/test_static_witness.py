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
