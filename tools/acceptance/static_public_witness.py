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


def get(host, path, byte_range=None, deadline_s=TOTAL_DEADLINE_S, clock=time.monotonic,
        connection_factory=None):
    """Fetch with a per-socket inactivity timeout and a total deadline for the whole exchange."""
    end = clock() + deadline_s
    factory = connection_factory or (
        lambda: http.client.HTTPSConnection(host, timeout=15, context=ssl.create_default_context()))
    connection = factory()
    try:
        headers = {"Cache-Control": "no-cache", "Accept-Encoding": "identity"}
        if byte_range:
            headers["Range"] = byte_range
        connection.request("GET", path, headers=headers)
        response = connection.getresponse()
        chunks, size = [], 0
        while True:
            if clock() > end:
                raise TimeoutError("total deadline")
            chunk = response.read(65536)
            if not chunk:
                break
            size += len(chunk)
            if size > RESPONSE_CAP:
                raise ValueError("response bound")
            chunks.append(chunk)
        return response.status, response.headers, b"".join(chunks)
    finally:
        connection.close()


def measure(request, cleanup_confirmed, fetch=get):
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
