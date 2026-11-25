"""Owner-side network measurements for a disposable Design section 2 qualification fixture."""

import argparse
import errno
import ipaddress
import json
import secrets
import socket
import sys
from pathlib import Path


def endpoint(value):
    ip, port = value.rsplit(":", 1)
    return str(ipaddress.IPv4Address(ip)), int(port)


def denied(error):
    if isinstance(error, TimeoutError) or error.errno in {
        errno.ECONNREFUSED, errno.ETIMEDOUT, errno.EHOSTUNREACH, errno.ENETUNREACH
    }:
        return
    raise error


def tcp(target):
    try:
        with socket.create_connection(endpoint(target), timeout=2):
            return True
    except OSError as error:
        denied(error)
        return False


def udp(target):
    # An ordinary A query allows the owner to use a DNS responder as the
    # positive control. For a denial, any returned datagram disproves isolation.
    tag = secrets.token_bytes(2)
    question = tag + bytes.fromhex("01000001000000000000") + b"\x07example\x03org\x00\x00\x01\x00\x01"
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.settimeout(2)
        try:
            sock.connect(endpoint(target))
            sock.send(question)
            reply = sock.recv(4096)
        except OSError as error:
            denied(error)
            return False, False
    return True, dns_answered(reply, question)


def dns_answered(reply, question):
    """True only for a matching NOERROR reply that carries a complete IN A record for the query."""
    if len(reply) < len(question) or reply[:2] != question[:2]:
        return False
    if not reply[2] & 128 or reply[3] & 15 != 0:
        return False
    if int.from_bytes(reply[4:6], "big") != 1 or int.from_bytes(reply[6:8], "big") < 1:
        return False
    # The question section must echo the query exactly.
    if reply[12:len(question)] != question[12:]:
        return False
    offset = len(question)
    for _ in range(int.from_bytes(reply[6:8], "big")):
        if offset + 2 > len(reply):
            return False
        if reply[offset] & 0xC0 == 0xC0:
            offset += 2
        else:
            while True:
                if offset >= len(reply):
                    return False
                length = reply[offset]
                offset += 1
                if length == 0:
                    break
                if length & 0xC0:
                    return False
                offset += length
        if offset + 10 > len(reply):
            return False
        rtype = int.from_bytes(reply[offset:offset + 2], "big")
        rclass = int.from_bytes(reply[offset + 2:offset + 4], "big")
        rdlength = int.from_bytes(reply[offset + 8:offset + 10], "big")
        offset += 10
        if offset + rdlength > len(reply):
            return False
        if rtype == 1 and rclass == 1 and rdlength == 4:
            return True
        offset += rdlength
    return False


def measure(inventory, role, connect=tcp, query=udp):
    targets = inventory["denied_targets"]
    if not 4 <= len(targets) <= 32 or len(set(targets)) != len(targets):
        raise ValueError("denial inventory")
    tcp_target, dns_target = inventory["public_tcp_target"], inventory["public_dns_target"]
    dns_replied, answered = query(dns_target)
    result = dict(tcp_target=tcp_target, dns_target=dns_target,
                  public_tcp_connected=connect(tcp_target), udp_dns_answer=answered)
    if role == "test":
        if result["public_tcp_connected"] or dns_replied:
            raise ValueError("TEST egress observed")
        return dict(result, network_class="restricted")
    if not result["public_tcp_connected"] or not answered:
        raise ValueError("positive controls unavailable")
    denied = []
    for target in targets:
        connected = connect(target)
        replied, _ = query(target)
        if connected or replied:
            raise ValueError("static denial failed")
        denied.append(dict(target=target, tcp_connected=connected, udp_replied=replied))
    return dict(result, denied=denied)


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--owner-qualification-fixture", required=True, action="store_true")
    cli.add_argument("--role", choices=["static", "test"], required=True)
    cli.add_argument("inventory", type=Path)
    cli.add_argument("output", type=Path)
    args = cli.parse_args()
    with args.inventory.open("rb") as source:
        raw = source.read(65537)
    if len(raw) > 65536:
        raise ValueError("inventory bound")
    result = measure(json.loads(raw), args.role)
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(result, output)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError):
        sys.exit("Qualification measurement failed. Inspect the controlled fixture privately.")
