"""Measure one private challenge from the owner-run tunnel acceptance client."""

import argparse
import hashlib
import http.client
import json
import socket
import ssl
import sys
import time
from pathlib import Path

TOTAL_DEADLINE_S = 60
RESPONSE_CAP = 32 * 1024 * 1024


def _remaining(end, clock):
    left = end - clock()
    if left <= 0:
        raise TimeoutError("total deadline")
    return left


def get(host, path, byte_range=None, end=None, clock=time.monotonic, connection_factory=None):
    """Fetch within a shared absolute deadline `end` (monotonic seconds).

    Every blocking step (connect and TLS, request, headers, each body read) is
    bounded by the time left, and the deadline is rechecked after each step, so
    a slow step that returns late is still refused.
    """
    if end is None:
        end = clock() + TOTAL_DEADLINE_S
    timeout = min(15.0, _remaining(end, clock))
    factory = connection_factory or (
        lambda t: http.client.HTTPSConnection(host, timeout=t, context=ssl.create_default_context()))
    connection = factory(timeout)
    try:
        headers = {"Cache-Control": "no-cache", "Accept-Encoding": "identity"}
        if byte_range:
            headers["Range"] = byte_range
        connection.request("GET", path, headers=headers)
        _remaining(end, clock)
        response = connection.getresponse()
        _remaining(end, clock)
        chunks, size = [], 0
        while True:
            sock = getattr(connection, "sock", None)
            if sock is not None:
                sock.settimeout(min(15.0, _remaining(end, clock)))
            chunk = response.read(65536)
            _remaining(end, clock)
            if not chunk:
                break
            size += len(chunk)
            if size > RESPONSE_CAP:
                raise ValueError("response bound")
            chunks.append(chunk)
        return response.status, response.headers, b"".join(chunks)
    finally:
        connection.close()


def phase_fetch(clock=time.monotonic, deadline_s=TOTAL_DEADLINE_S, connection_factory=None):
    """One deadline shared by every request of a measurement phase."""
    end = clock() + deadline_s

    def fetch(host, path, byte_range=None):
        return get(host, path, byte_range, end=end, clock=clock, connection_factory=connection_factory)

    return fetch


def measure(request, cleanup_confirmed, fetch=None):
    if fetch is None:
        fetch = phase_fetch()
    reference = request["reference"]
    hosts = reference["hosts"]
    if request["version"] != 1 or len(hosts) != 2 or hosts[0]["name"] == hosts[1]["name"]:
        raise ValueError("challenge")
    result = {key: request[key] for key in ("version", "phase", "challenge")}
    if request["phase"] == "cleanup":
        if not cleanup_confirmed:
            raise ValueError("independent cleanup inspection required")
        for host in hosts:
            try:
                status, _, body = fetch(host["name"], host["path"])
            except socket.gaierror as error:
                if error.errno not in {socket.EAI_NONAME, getattr(socket, "EAI_NODATA", socket.EAI_NONAME)}:
                    raise
                continue
            if status < 400 or host["marker"].encode() in body:
                raise ValueError("route still served")
        return dict(result, connector_stopped=True, owned_dns_absent=2, names_no_longer_serve=2)
    if request["phase"] not in {"first", "republished"}:
        raise ValueError("unknown phase")
    digests = []
    for host in hosts:
        status, _, body = fetch(host["name"], host["path"])
        if status != 200 or host["marker"].encode() not in body:
            raise ValueError("marker")
        digests.append(hashlib.sha256(body).hexdigest())
    host = hosts[0]["name"]
    path = reference["media_path"]
    status, _, full = fetch(host, path)
    code, headers, partial = fetch(host, path, f"bytes={reference['start']}-{reference['end']}")
    unsat, rejected, _ = fetch(host, path, f"bytes={reference['total']}-")
    return dict(result, matching_markers=2, marker_body_sha256=digests,
                full_status=status, full_sha256=hashlib.sha256(full).hexdigest(),
                partial_status=code, partial_sha256=hashlib.sha256(partial).hexdigest(),
                content_range=headers.get("Content-Range"), content_length=int(headers["Content-Length"]),
                accept_ranges=headers.get("Accept-Ranges"), unsatisfiable_status=unsat,
                unsatisfiable_range=rejected.get("Content-Range"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--owner-live-public-https", required=True, action="store_true")
    parser.add_argument("--confirmed-connector-stopped-and-both-owned-records-absent", action="store_true")
    parser.add_argument("request", type=Path)
    parser.add_argument("response", type=Path)
    args = parser.parse_args()
    with args.request.open("rb") as source:
        raw = source.read(65537)
    if len(raw) > 65536:
        raise ValueError("challenge bound")
    result = measure(json.loads(raw), args.confirmed_connector_stopped_and_both_owned_records_absent)
    with args.response.open("x", encoding="utf-8") as dest:
        json.dump(result, dest)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, http.client.HTTPException):
        sys.exit("Public witness failed. Inspect the enrolled services privately.")
