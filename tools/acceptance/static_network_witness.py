"""Owner-side network measurements for a disposable Design section 2 qualification fixture."""

import argparse
import errno
import ipaddress
import json
import secrets
import socket
import sys
from pathlib import Path

MAX_CNAME_LINKS = 8


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


class _Malformed(Exception):
    pass


def _read_name(message, offset, limit, suffixes):
    """Decode one name and remember its label boundaries for subsequent names."""
    labels, starts = [], []
    while True:
        if offset >= limit:
            raise _Malformed("name runs past the message")
        starts.append(offset)
        length = message[offset]
        if length & 0xC0 == 0xC0:
            if offset + 1 >= limit:
                raise _Malformed("truncated pointer")
            target = ((length & 0x3F) << 8) | message[offset + 1]
            if target not in suffixes:
                raise _Malformed("pointer must target a prior name boundary")
            tail = suffixes[target]
            offset += 2
            break
        if length & 0xC0:
            raise _Malformed("reserved label type")
        offset += 1
        if length == 0:
            tail = ()
            break
        if offset + length > limit:
            raise _Malformed("label runs past the name field")
        labels.append(message[offset:offset + length].lower())
        offset += length
    name = tuple(labels) + tail
    if 1 + sum(len(label) + 1 for label in name) > 255:
        raise _Malformed("name too long")
    suffixes.update((start, name[index:]) for index, start in enumerate(starts))
    return name, offset


def _read_answer_names(message, rtype, start, end, suffixes):
    """Decode RFC 1035 name fields in RDATA. Other RDATA stays opaque."""
    if rtype in {2, 3, 4, 5, 7, 8, 9, 12}:  # NS, MD, MF, CNAME, MB, MG, MR, PTR
        name, offset = _read_name(message, start, end, suffixes)
    elif rtype == 15:  # MX preference, then exchange
        name, offset = _read_name(message, start + 2, end, suffixes)
    elif rtype in {6, 14}:  # SOA and MINFO start with two names
        name, offset = _read_name(message, start, end, suffixes)
        _, offset = _read_name(message, offset, end, suffixes)
        if rtype == 6:
            offset += 20  # serial, refresh, retry, expire, minimum
    else:
        return None
    if offset != end:
        raise _Malformed("invalid name-bearing RDATA length")
    return name


def dns_answered(reply, question):
    """True only for a well-formed NOERROR reply to exactly this query whose answer
    section is complete and resolves the queried name (directly or through CNAME
    aliases) to an IN A record with a four-byte address."""
    try:
        if len(question) < 17 or len(reply) < len(question) or reply[:2] != question[:2]:
            return False
        # Require QR, opcode 0, TC clear, the reserved bit clear and NOERROR.
        if reply[2] & 0xFA != 0x80 or reply[3] & 0x4F:
            return False
        if int.from_bytes(reply[4:6], "big") != 1:
            return False
        answers = int.from_bytes(reply[6:8], "big")
        if answers < 1:
            return False
        if reply[12:len(question)] != question[12:]:
            return False
        suffixes = {}
        wanted, question_end = _read_name(reply, 12, len(question) - 4, suffixes)
        if question[question_end:] != b"\x00\x01\x00\x01":
            return False
        offset = question_end + 4
        owners, addresses, aliases = set(), set(), {}
        for _ in range(answers):
            owner, offset = _read_name(reply, offset, len(reply), suffixes)
            if offset + 10 > len(reply):
                return False
            rtype = int.from_bytes(reply[offset:offset + 2], "big")
            rclass = int.from_bytes(reply[offset + 2:offset + 4], "big")
            rdlength = int.from_bytes(reply[offset + 8:offset + 10], "big")
            data = offset + 10
            if data + rdlength > len(reply):
                return False
            end = data + rdlength
            target = _read_answer_names(reply, rtype, data, end, suffixes)
            if owner in aliases or (rtype == 5 and owner in owners):
                return False
            if rtype == 5:
                aliases[owner] = (rclass, target)
            if rtype == 1 and rclass == 1:
                if rdlength != 4:
                    return False
                addresses.add(owner)
            owners.add(owner)
            offset = end
        name = wanted
        for _ in range(MAX_CNAME_LINKS):
            if name in addresses:
                return True
            alias = aliases.get(name)
            if alias is None or alias[0] != 1:
                return False
            name = alias[1]
        return name in addresses
    except _Malformed:
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
