"""Attributed hosted-only resources for the zero-daemon R2 exercise."""

import hashlib
import json
import os
from pathlib import Path
import re
import select
import shutil
import socket
import stat
import subprocess
import time

from fixture import (INNER, NETWORK, OUTER, Refusal, allocate, bounded_read, data_layout,
                     filesystem_facts, host_command, image_stat, private_directory,
                     remaining, require, unique_pairs)


def dev_majmin(dev):
    if hasattr(os, "major") and hasattr(os, "minor"):
        return f"{os.major(dev)}:{os.minor(dev)}"
    major = ((dev >> 8) & 0xfff) | ((dev >> 32) & ~0xfff)
    minor = (dev & 0xff) | ((dev >> 12) & ~0xff)
    return f"{major}:{minor}"


def mountinfo_entries(target, contents=None):
    if contents is None:
        contents = bounded_read(Path("/proc/self/mountinfo"), 256*1024).decode("ascii")
    expected = target.as_posix() if isinstance(target, Path) else str(target)
    matches = []
    for line in contents.splitlines():
        left, separator, right = line.partition(" - ")
        fields = left.split()
        tail = right.split()
        require(separator and len(fields) >= 6 and len(tail) >= 3 and fields[0].isdecimal(), "mount-info")
        if fields[4] == expected:
            matches.append((int(fields[0]), fields[3], tail[0], tail[1]))
    return matches


def json_rows(value):
    data = json.loads(value, object_pairs_hook=unique_pairs)
    require(isinstance(data, list) and len(data) <= 128, "readback-inventory")
    return data


def require_same(observed, expected, category):
    require(observed == expected, category)


def token(receipt):
    raw = "|".join(receipt[key] for key in ("repo", "head", "run", "attempt"))
    return hashlib.sha256(raw.encode("ascii")).hexdigest()[:16]


class Resources:
    def __init__(self, receipt, pins, tools=None):
        self.id = token(receipt)
        self.root = Path("/var/lib") / f"arx-r2-{self.id}"
        self.bridge = "r2" + self.id[:11]
        self.table = "r2_" + self.id
        self.namespaces = {node: f"r2-{self.id}-{node}" for node in "abc"}
        self.links = {node: f"{self.bridge}{node}" for node in "abc"}
        self.budget = pins["rootfs"]
        self.tools = tools
        self.cleanup_stack = []
        self.facts = {}

    def cmd(self, tool, end, *args, **options):
        return host_command(tool, end, *args, tools=self.tools, **options)

    def remember(self, label, cleanup):
        self.cleanup_stack.append((label, cleanup))

    def cleanup(self, end):
        errors = []
        for label, action in reversed(self.cleanup_stack):
            try:
                action(end)
            except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError):
                errors.append(label)
        return errors

    def arena(self):
        parent = self.root.parent.lstat()
        require(stat.S_ISDIR(parent.st_mode) and parent.st_uid == 0 and not parent.st_mode & 0o022,
                "arena-parent")
        private_directory(self.root, 0o700)
        identity = self.root.stat()
        self.remember("arena", lambda end: self.remove_arena(identity, end))

    def remove_arena(self, identity, end):
        remaining(end)
        current = self.root.lstat()
        require(stat.S_ISDIR(current.st_mode) and current.st_uid == 0
                and (current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino), "arena-identity")
        views = bounded_read(Path("/proc/self/mountinfo"), 256*1024).decode("ascii").splitlines()
        require(all(not Path(line.split()[4]).is_relative_to(self.root) for line in views), "mount-residue")
        require(not self.unit_groups(), "unit-residue")
        require(not self.loop_rows(end), "loop-residue")
        require(not self.network_names(end), "network-residue")
        # No filesystem tree is removed if an attributed child remains.
        shutil.rmtree(self.root)
        require(not self.root.exists(), "arena-residue")

    def unit_groups(self):
        parent = Path("/sys/fs/cgroup/system.slice")
        return list(parent.glob(f"arx-r2-{self.id}-?.service")) + list(parent.glob(f"arx-r2-{self.id}-??.service"))

    def loop_rows(self, end, file=None):
        args = ["-J", "-l", "-O", "NAME,BACK-FILE,BACK-INO,BACK-MAJ:MIN,RO,SIZELIMIT,OFFSET"]
        if file is not None:
            args.extend(("-j", str(file)))
        data = json.loads(self.cmd("loop", end, *args), object_pairs_hook=unique_pairs)
        rows = data["loopdevices"]
        require(isinstance(rows, list) and len(rows) <= 128, "loop-inventory")
        return [row for row in rows if row.get("back-file") and Path(row["back-file"]).is_relative_to(self.root)]

    def volume(self, name, source, end):
        require(name in ("root", "a", "b", "c") and source.parent == self.root, "volume-name")
        sealed = name == "root"
        size = self.budget["bytes" if sealed else "data_bytes"]
        image = self.root / f"{name}.ext4"
        mount = self.root / name
        allocate(image, size)
        flags = ["-t", "ext4", "-q", "-F", "-b", "4096", "-m", "0",
                 "-E", "nodiscard,lazy_itable_init=0,lazy_journal_init=0"]
        if not sealed:
            flags.extend(("-N", str(self.budget["data_inodes"])))
        self.cmd("mkfs", end, *flags, "-d", source, image)
        if sealed:
            image.chmod(0o400)
        img_info = image.lstat()
        image_stat(img_info, size)
        backing = (img_info.st_dev, img_info.st_ino)
        require(not self.loop_rows(end, image), "loop-already-attached")
        args = ["--find", "--show", "--nooverlap"]
        if sealed:
            args.append("--read-only")
        loop_box = [None]
        self.remember(f"loop-{name}", lambda deadline, b=backing, lb=loop_box:
                      self.release_loop(image, size, sealed, b, lb[0], deadline))
        loop_box[0] = self.cmd("loop", end, *args, image).strip()
        loop = loop_box[0]
        self.check_loop(image, loop, size, sealed, backing, end)
        private_directory(mount, 0o755)
        require(not mountinfo_entries(mount) and not self.mounts(mount, end), "mount-preexisting")
        mount_box = [None]
        self.remember(f"mount-{name}", lambda deadline, box=mount_box, b=backing, img=image, sz=size, sl=sealed:
                      self.release_mount(mount, loop, box[0], deadline, unobserved_loop=(img, sz, sl, b)))
        self.cmd("mount", end, "-t", "ext4", "-o",
                 "ro,nodev,nosuid" if sealed else "rw,nodev,nosuid,noexec", loop, mount)
        mount_box[0] = self.check_mount(mount, loop, end)
        stats = os.statvfs(mount)
        require(bool(stats.f_flag & os.ST_RDONLY) == sealed, "volume-readonly")
        self.facts[name] = filesystem_facts(stats, self.budget, writable=not sealed)
        return mount

    def check_loop(self, image, loop, size, sealed, backing, end):
        require(re.fullmatch(r"/dev/loop[0-9]{1,4}", loop), "loop-name")
        rows = self.loop_rows(end, image)
        require(len(rows) == 1, "ambiguous-loop")
        row = rows[0]
        require(row["name"] == loop and row["back-file"] == image.as_posix() and row["ro"] is sealed
                and row["offset"] == 0 and row["sizelimit"] == 0, "loop-identity")
        require(type(row.get("back-ino")) is int and row["back-ino"] == backing[1]
                and row.get("back-maj:min") == dev_majmin(backing[0]), "loop-identity")
        current = image.lstat()
        image_stat(current, size)
        require((current.st_dev, current.st_ino) == backing, "loop-identity")
        actual = bounded_read(Path("/sys/class/block") / Path(loop).name / "size", 32)
        require(int(actual) * 512 == size, "loop-size")

    def release_loop(self, image, size, sealed, backing, expected_loop, end):
        rows = self.loop_rows(end, image)
        if rows:
            require(len(rows) == 1, "ambiguous-loop")
            loop = rows[0]["name"]
            if expected_loop is not None:
                require(loop == expected_loop, "loop-identity")
            self.check_loop(image, loop, size, sealed, backing, end)
            self.cmd("loop", end, "-d", loop)
        require(not self.loop_rows(end, image), "loop-still-attached")

    def mounts(self, path, end):
        return self.cmd("findmnt", end, "-nr", "-o", "ID,TARGET,SOURCE,FSTYPE,FSROOT",
                        "--mountpoint", path, statuses=(0, 1)).splitlines()

    def check_mount(self, path, loop, end, expected_id=None):
        entries = mountinfo_entries(path)
        require(len(entries) == 1, "mount-identity")
        mount_id, fsroot, fstype, source = entries[0]
        require(fsroot == "/" and fstype == "ext4" and source == loop, "mount-identity")
        if expected_id is not None:
            require(mount_id == expected_id, "mount-identity")
        require_same(self.mounts(path, end), [f"{mount_id} {path.as_posix()} {loop} ext4 /"], "mount-identity")
        return mount_id

    def release_mount(self, path, loop, expected_id, end, *, unobserved_loop=None):
        entries = mountinfo_entries(path)
        if entries or self.mounts(path, end):
            if expected_id is not None:
                self.check_mount(path, loop, end, expected_id=expected_id)
            else:
                require(unobserved_loop is not None and path.parent == self.root, "mount-identity")
                img, sz, sl, backing = unobserved_loop
                self.check_loop(img, loop, sz, sl, backing, end)
                self.check_mount(path, loop, end)
            self.cmd("umount", end, path)
        require(not mountinfo_entries(path) and not self.mounts(path, end), "mount-still-active")

    def stage_data(self, end):
        for node in self.namespaces:
            source = self.root / f"data-{node}"
            private_directory(source, 0o755)
            data_layout(source, node)
            for role, uid in (("daemon", 21071), ("build", 21072)):
                private_directory(source / "probe" / role, 0o755, uid, uid)
            self.volume(node, source, end)

    def ip(self, end, *args, **options):
        return self.cmd("ip", end, *args, **options)

    def ns(self, end, node, *args):
        return self.ip(end, "-n", self.namespaces[node], *args)

    def links_at(self, end, name):
        output = self.ip(end, "-j", "-d", "link", "show", "dev", name, statuses=(0, 1))
        if output.startswith("Device ") and output.rstrip().endswith("does not exist."):
            return []
        return json_rows(output)

    def add_link(self, name, end, *add_args, kind, mac=None):
        state = {"index": None, "tagged": False}
        self.remember("link", lambda deadline, n=name, s=state, k=kind, m=mac:
                      self.remove_link(n, s["index"], deadline, tagged=s["tagged"],
                                       expected_kind=k, expected_mac=m))
        self.ip(end, "link", "add", "name", name, *add_args)
        rows = self.links_at(end, name)
        require(len(rows) == 1 and type(rows[0].get("ifindex")) is int, "link-attribution")
        state["index"] = rows[0]["ifindex"]
        self.ip(end, "link", "set", "dev", name, "alias", self.table)
        rows = self.links_at(end, name)
        require(len(rows) == 1 and rows[0]["ifindex"] == state["index"]
                and rows[0].get("ifalias") == self.table, "link-attribution")
        state["tagged"] = True

    def remove_link(self, name, index, end, *, tagged=True, expected_kind=None, expected_mac=None):
        rows = self.links_at(end, name)
        if rows:
            require(len(rows) == 1, "link-attribution")
            row = rows[0]
            if index is not None:
                require(row.get("ifindex") == index
                        and (not tagged or row.get("ifalias") == self.table), "link-attribution")
            else:
                require(name in (self.bridge, *self.links.values()) and expected_kind in ("bridge", "veth"),
                        "link-attribution")
                require(type(row.get("ifindex")) is int and row.get("ifname", name) == name, "link-attribution")
                require(row.get("linkinfo", {}).get("info_kind") == expected_kind, "link-attribution")
                if expected_mac is not None:
                    require(row.get("address") == expected_mac, "link-attribution")
                require(row.get("ifalias") in (None, "", self.table), "link-attribution")
            self.ip(end, "link", "del", "dev", name)
        require(not self.links_at(end, name), "link-residue")

    def network_names(self, end):
        namespaces = self.ip(end, "netns", "list").splitlines()
        tables = self.cmd("nft", end, "list", "tables").splitlines()
        return (any(line.split()[0] in self.namespaces.values() for line in namespaces)
                or any(line.split()[-1] == self.table for line in tables)
                or any(self.links_at(end, name) for name in (self.bridge, *self.links.values())))

    def add_namespace(self, node, end):
        namespace = self.namespaces[node]
        path = Path("/run/netns") / namespace
        require(not path.exists(), "network-preexisting")
        state = {"identity": None}
        self.remember("namespace", lambda deadline, n=node, s=state:
                      self.remove_namespace(n, s["identity"], deadline, allow_unobserved=True))
        self.ip(end, "netns", "add", namespace)
        info = path.stat()
        state["identity"] = (info.st_dev, info.st_ino)

    def remove_namespace(self, node, identity, end, *, allow_unobserved=False):
        name = self.namespaces[node]
        path = Path("/run/netns") / name
        if not path.exists():
            return
        if identity is not None:
            current = path.stat()
            require((current.st_dev, current.st_ino) == identity, "netns-attribution")
        else:
            require(allow_unobserved, "netns-attribution")
            linfo = path.lstat()
            require(not stat.S_ISLNK(linfo.st_mode) and linfo.st_uid == 0, "netns-attribution")
            listed = [line.split()[0] for line in self.ip(end, "netns", "list").splitlines() if line.strip()]
            require(name in listed, "netns-attribution")
            entries = mountinfo_entries(path)
            require(len(entries) == 1 and entries[0][2] == "nsfs", "netns-attribution")
        require(not self.ip(end, "netns", "pids", name).strip(), "netns-children")
        self.ip(end, "netns", "del", name)
        require(not path.exists(), "netns-residue")

    def make_links(self, end):
        require(not self.network_names(end), "network-preexisting")
        self.add_link(self.bridge, end, "address", NETWORK["local"][1], "type", "bridge",
                      "stp_state", "0", "mcast_snooping", "0", kind="bridge", mac=NETWORK["local"][1])
        self.ip(end, "link", "set", "dev", self.bridge, "addrgenmode", "none")
        for node, (address, mac) in NETWORK["nodes"].items():
            namespace = self.namespaces[node]
            self.add_namespace(node, end)
            link = self.links[node]
            self.add_link(link, end, "type", "veth", "peer", "name", "eth0", "netns", namespace, kind="veth")
            self.ip(end, "link", "set", "dev", link, "master", self.bridge, "addrgenmode", "none")
            self.ns(end, node, "link", "set", "dev", "eth0", "address", mac, "addrgenmode", "none")
            self.ns(end, node, "address", "add", f"{address}/24", "dev", "eth0")
            for other, (peer, peer_mac) in NETWORK["nodes"].items():
                if other != node:
                    self.ns(end, node, "neigh", "add", peer, "lladdr", peer_mac, "nud", "permanent", "dev", "eth0")

    def nft(self, end, *args):
        return self.cmd("nft", end, *args)

    def apply_policy(self, end):
        rules = policy(self.bridge, self.links)
        lines = []
        for family, entries in rules.items():
            lines.append(f"table {family} {self.table} {{")
            for chain, (hook, body) in entries.items():
                layer = "filter"
                lines.append(f" chain {chain} {{")
                if hook:
                    lines.append(f"  type {layer} hook {hook} priority -310; policy accept;")
                lines.extend(f'  {expression} counter {action} comment "r2-{key}"'
                             for key, expression, action in body)
                lines.append(" }")
            lines.append("}")
        config = self.root / "policy.nft"
        config.write_text("\n".join(lines) + "\n", encoding="ascii")
        first_link = next(index for index, (label, _) in enumerate(self.cleanup_stack) if label == "link")
        handles = {}
        for family in rules:
            expected_chains = set(rules[family])
            self.cleanup_stack.insert(
                first_link,
                ("nft", lambda deadline, f=family, h=handles, c=expected_chains:
                 self.remove_table(f, h.get(f), deadline, expected_chains=c)),
            )
        self.nft(end, "-f", config)
        for family in rules:
            handles[family] = table_handle(self.nft_table(family, end), family, self.table)
        self.rule_graph = rules
        self.rule_receipt = self.counters(end)

    def nft_table(self, family, end):
        return json.loads(self.nft(end, "-j", "-a", "list", "table", family, self.table),
                          object_pairs_hook=unique_pairs)["nftables"]

    def remove_table(self, family, handle, end, *, expected_chains=None):
        if f"table {family} {self.table}" not in self.nft(end, "list", "tables").splitlines():
            return
        require(not any(self.links_at(end, name) for name in (self.bridge, *self.links.values())),
                "nft-live-link")
        rows = self.nft_table(family, end)
        current_handle = table_handle(rows, family, self.table)
        if handle is not None:
            require(current_handle == handle, "nft-attribution")
        else:
            require(expected_chains is not None, "nft-attribution")
            for row in rows:
                if "metainfo" in row or "table" in row:
                    continue
                if "chain" in row:
                    chain = row["chain"]
                    require(chain.get("family") == family and chain.get("table") == self.table
                            and chain.get("name") in expected_chains and chain.get("type") == "filter"
                            and chain.get("prio") == -310 and chain.get("policy") == "accept",
                            "nft-attribution")
                elif "rule" in row:
                    rule = row["rule"]
                    require(rule.get("family") == family and rule.get("table") == self.table
                            and rule.get("chain") in expected_chains
                            and isinstance(rule.get("comment"), str) and rule["comment"].startswith("r2-"),
                            "nft-attribution")
                else:
                    raise Refusal("nft-attribution")
        self.nft(end, "delete", "table", family, self.table)
        require(f"table {family} {self.table}" not in self.nft(end, "list", "tables").splitlines(),
                "nft-residue")

    def counters(self, end):
        return {key: value for family, chains in self.rule_graph.items()
                for key, value in read_counters(self.nft_table(family, end), chains, family, self.table).items()}

    def activate(self, end):
        self.ip(end, "link", "set", "dev", self.bridge, "up")
        for node, link in self.links.items():
            self.ip(end, "link", "set", "dev", link, "up")
            self.ns(end, node, "link", "set", "dev", "lo", "up")
            self.ns(end, node, "link", "set", "dev", "eth0", "up")

    def synthetic(self, end):
        """Endpoints exist only on C or on the fixture bridge during the probes."""
        node = "b"
        c_address, c_mac = NETWORK["nodes"]["c"]
        local, mac = NETWORK["local"]
        self.ip(end, "address", "add", f"{local}/32", "dev", self.bridge)
        self.ns(end, node, "neigh", "add", local, "lladdr", mac, "nud", "permanent", "dev", "eth0")
        self.ip(end, "neigh", "add", c_address, "lladdr", c_mac, "nud", "permanent", "dev", self.bridge)
        self.ip(end, "route", "add", f'{NETWORK["outside"]}/32', "via", c_address, "dev", self.bridge, "onlink")
        self.forwarding = Path("/proc/sys/net/ipv4/conf") / self.bridge / "forwarding"
        self.original_forwarding = bounded_read(self.forwarding, 8)
        require(self.original_forwarding in (b"0\n", b"1\n"), "bridge-forwarding")
        self.forwarding.write_bytes(b"1\n")
        require(bounded_read(self.forwarding, 8) == b"1\n", "bridge-forwarding")
        self.ns(end, "c", "address", "add", f'{NETWORK["ipv6"]}/64', "dev", "eth0", "nodad")
        self.ns(end, node, "address", "add", "fd42:72::12/64", "dev", "eth0", "nodad")
        self.ns(end, node, "-6", "neigh", "add", NETWORK["ipv6"], "lladdr", c_mac, "nud", "permanent", "dev", "eth0")
        for key in ("outside", "link_local"):
            target = NETWORK[key]
            self.ns(end, "c", "address", "add", f"{target}/32", "dev", "lo")
            self.ns(end, node, "route", "add", f"{target}/32", "via",
                    local if key == "outside" else c_address, "dev", "eth0")

    def clear_synthetic(self, end):
        local, _ = NETWORK["local"]
        c_address, _ = NETWORK["nodes"]["c"]
        self.forwarding.write_bytes(self.original_forwarding)
        require(bounded_read(self.forwarding, 8) == self.original_forwarding, "bridge-forwarding-restore")
        self.ip(end, "route", "del", f'{NETWORK["outside"]}/32', "via", c_address, "dev", self.bridge, "onlink")
        self.ip(end, "neigh", "del", c_address, "dev", self.bridge)
        self.ns(end, "b", "neigh", "del", local, "dev", "eth0")
        self.ip(end, "address", "del", f"{local}/32", "dev", self.bridge)
        self.ns(end, "b", "-6", "neigh", "del", NETWORK["ipv6"], "dev", "eth0")
        self.ns(end, "c", "address", "del", f'{NETWORK["ipv6"]}/64', "dev", "eth0")
        self.ns(end, "b", "address", "del", "fd42:72::12/64", "dev", "eth0")
        for key in ("outside", "link_local"):
            target = NETWORK[key]
            self.ns(end, "b", "route", "del", f"{target}/32", "via",
                    local if key == "outside" else c_address, "dev", "eth0")
            self.ns(end, "c", "address", "del", f"{target}/32", "dev", "lo")
        self.endpoint_inventory(end, temporary=False)

    def endpoint_inventory(self, end, *, temporary):
        bridge_ports = json_rows(self.ip(end, "-j", "link", "show", "master", self.bridge))
        require_same({row["ifname"] for row in bridge_ports}, set(self.links.values()), "bridge-endpoints")
        bridge_address = json_rows(self.ip(end, "-j", "address", "show", "dev", self.bridge))
        require(len(bridge_address) == 1, "bridge-address")
        require_same([row["local"] for row in bridge_address[0]["addr_info"]],
                     [NETWORK["local"][0]] if temporary else [], "numbered-bridge")
        for node, (address, mac) in NETWORK["nodes"].items():
            links = json_rows(self.ns(end, node, "-j", "address", "show"))
            ipv4 = json_rows(self.ns(end, node, "-j", "-4", "route", "show", "table", "main"))
            ipv6 = json_rows(self.ns(end, node, "-j", "-6", "route", "show", "table", "main"))
            endpoint_inventory(node, address, mac, links, ipv4, ipv6, temporary)

    def unit_info(self, unit, end):
        keys = ("Id", "LoadState", "ActiveState", "ControlGroup", "MainPID", "InvocationID",
                "MemoryMax", "MemorySwapMax", "TasksMax", "CPUQuotaPerSecUSec", "LimitNOFILE", "LimitCORE")
        result = self.cmd("systemctl", end, "show", unit, "--property=" + ",".join(keys), statuses=(0, 1))
        rows = [line.split("=", 1) for line in result.splitlines()]
        require(all(len(row) == 2 for row in rows) and len(dict(rows)) == len(rows), "unit-fields")
        return dict(rows)

    def stop_unit(self, unit, invocation, end, *, crash=False):
        facts = self.unit_info(unit, end)
        if facts.get("LoadState") != "not-found":
            require(facts.get("Id") == unit and facts.get("InvocationID") == invocation
                    and facts.get("ControlGroup") == f"/system.slice/{unit}", "unit-attribution")
            if crash:
                self.cmd("systemctl", end, "kill", "--kill-whom=all", "--signal=SIGKILL", unit)
            self.cmd("systemctl", end, "stop", unit)
        require(not (Path("/sys/fs/cgroup/system.slice") / unit).exists(), "unit-cgroup-residue")

    def start(self, instance, end):
        require(instance in ("a0", "b0", "b1", "c0"), "boot-instance")
        unit = f"arx-r2-{self.id}-{instance}.service"
        require(self.unit_info(unit, end).get("LoadState") == "not-found", "unit-preexisting")
        properties = {
            "Type": "exec", "Delegate": "memory pids", "CPUQuota": "100%", "MemoryMax": "512M",
            "MemorySwapMax": "0", "TasksMax": "128", "LimitNOFILE": "128", "LimitCORE": "0",
            "RuntimeMaxSec": "34s", "TimeoutStopSec": "1s", "KillMode": "control-group", "Restart": "no",
            "StandardInput": "null", "StandardOutput": "journal", "StandardError": "journal",
        }
        args = [f"--property={name}={value}" for name, value in properties.items()]
        try:
            self.cmd("unit", end, "--quiet", "--unit", unit, *args,
                     self.root / "bootstrap", "boot", self.id, instance)
        finally:
            # An incomplete launch still owns any attributable invocation under the
            # freshly reserved name, which must go through the same stop checks.
            facts = self.unit_info(unit, end)
            if facts.get("LoadState") != "not-found":
                invocation = facts.get("InvocationID", "")
                require(re.fullmatch(r"[0-9a-f]{32}", invocation), "unit-invocation")
                self.remember("unit", lambda deadline: self.stop_unit(unit, invocation, deadline))
        validate_unit(unit, facts)
        group = Path("/sys/fs/cgroup") / facts["ControlGroup"].lstrip("/")
        observe_limits(group)
        return unit, invocation, int(facts["MainPID"])

    def event(self, node, path, expected, end, *, process=None):
        file = self.root / node / "probe" / path
        while not file.exists():
            remaining(end)
            if process is not None:
                require(Path(f"/proc/{process}").exists(), "probe-process-exited")
            time.sleep(0.01)
        require(stat.S_ISREG(file.lstat().st_mode), "probe-receipt-type")
        raw = bounded_read(file, 256)
        require(raw == expected, "probe-receipt")

    def contain(self, end):
        self.make_links(end)
        self.apply_policy(end)
        self.activate(end)
        self.synthetic(end)
        self.endpoint_inventory(end, temporary=True)
        a = self.start("a0", end)
        c = self.start("c0", end)
        b = self.start("b0", end)
        for node, (_unit, _invocation, pid) in zip("acb", (a, c, b)):
            self.event(node, "boot0", b"r2-init-v1\n", end, process=pid)
            self.check_namespace(node, pid, end)
        for node in "ac":
            self.event(node, "daemon/listeners", b"r2-listeners-v1\n", end)
        marker = self.root / "b/probe/persistent"
        before = marker.stat()
        with_pid = os.pidfd_open(b[2], 0)
        try:
            self.stop_unit(*b[:2], end, crash=True)
            watched = select.poll()
            watched.register(with_pid, select.POLLIN)
            events = watched.poll(0)
            require(len(events) == 1 and events[0][0] == with_pid and events[0][1] & select.POLLIN,
                    "restart-pidfd")
            require(not Path(f"/proc/{b[2]}").exists(), "restart-host-pid")
            require(not self.ip(end, "netns", "pids", self.namespaces["b"]).strip(), "restart-netns-pids")
        finally:
            os.close(with_pid)
        restarted = self.start("b1", end)
        self.event("b", "boot1", b"r2-init-v1\n", end, process=restarted[2])
        self.event("b", "daemon/listeners1", b"r2-listeners-v1\n", end, process=restarted[2])
        after = marker.stat()
        require((after.st_dev, after.st_ino, after.st_mtime_ns) == (before.st_dev, before.st_ino, before.st_mtime_ns),
                "restart-image-identity")
        self.check_namespace("b", restarted[2], end)
        require(restarted[2] != b[2], "restart-process-identity")
        self.facts["restart"] = "same-data-fresh-run"

    def check_namespace(self, node, pid, end):
        remaining(end)
        outer = Path(f"/proc/{pid}")
        children = bounded_read(outer / f"task/{pid}/children", 1024).decode("ascii").split()
        require(len(children) == 1 and re.fullmatch(r"[1-9][0-9]{0,9}", children[0]), "namespace-init")
        init = Path("/proc") / children[0]
        status = bounded_read(init / "status", 8192).decode("ascii")
        require(re.search(r"^NSpid:\s+" + children[0] + r"\s+1$", status, re.M) is not None,
                "namespace-init-pid")
        check_security_status(status, 0, 0)
        check_mount_inventory(bounded_read(init / "mountinfo", 8192).decode("ascii"), node == "b")
        for namespace in ("mnt", "uts", "ipc", "pid", "net", "cgroup"):
            actual = (init / "ns" / namespace).stat().st_ino
            require(actual != (Path("/proc/self/ns") / namespace).stat().st_ino, "namespace-host-alias")
            if namespace == "net":
                require(actual == (Path("/run/netns") / self.namespaces[node]).stat().st_ino, "namespace-net-alias")
        require(re.search(r"^Seccomp:\s+[012]$", status, re.M), "seccomp-readback")
        lsm = bounded_read(init / "attr/current", 256).decode("ascii").strip()
        require(re.fullmatch(r"[\w./ ()-]{1,100}", lsm), "lsm-readback")
        self.facts[node]["seccomp"] = re.search(r"^Seccomp:\s+([012])$", status, re.M)[1]
        self.facts[node]["lsm"] = lsm
        if node == "b":
            cgroup = bounded_read(outer / "cgroup", 256).decode("ascii").strip()
            require(re.fullmatch(r"0::/system\.slice/arx-r2-[0-9a-f]{16}-b[01]\.service/init", cgroup),
                    "broker-host-membership")
            unit = Path("/sys/fs/cgroup") / cgroup[4:].removesuffix("/init") / "system.slice/arxd.service"
            self.facts[node]["broker"] = self.check_delegate(unit, init)

    @staticmethod
    def check_delegate(unit, init):
        require(os.getxattr(unit, "user.delegate", follow_symlinks=False) == b"1", "delegate-xattr")
        require(bounded_read(unit / "cgroup.type", 32) == b"domain\n"
                and bounded_read(unit / "cgroup.procs", 32) == b"", "delegate-domain")
        for parent in (unit, unit / "runs"):
            require({b"memory", b"pids"} <= set(bounded_read(parent / "cgroup.subtree_control", 256).split()),
                    "delegate-controllers")
        entries = sorted(path.name for path in (unit / "runs").iterdir() if path.is_dir())
        require(entries == ["fixture"], "runs-inventory")
        leaf = unit / "runs/fixture"
        for name, key in (("memory.max", "memory"), ("memory.swap.max", "swap"), ("pids.max", "pids")):
            require(bounded_read(leaf / name, 32).strip() == str(INNER[key]).encode("ascii"), "inner-limit")
        for group, caps, uid in ((unit / "supervisor", 193, 0), (leaf, 0, 21072)):
            raw = bounded_read(group / "cgroup.procs", 128)
            require(re.fullmatch(rb"[1-9][0-9]{0,9}\n", raw), "delegate-sole-pid")
            process = Path("/proc") / raw.strip().decode("ascii")
            facts = bounded_read(process / "status", 8192).decode("ascii")
            check_security_status(facts, caps, uid)
            require((process / "ns/pid").stat().st_ino == (init / "ns/pid").stat().st_ino, "delegate-pid-namespace")
            require(sorted(path.name for path in (process / "fd").iterdir()) == ["0", "1", "2"], "extra-fd")
        return {"caps": 193, "nnp": 1, "memory": INNER["memory"], "swap": INNER["swap"], "pids": INNER["pids"]}

    def network(self, end):
        counters = self.counters(end)
        stable_rules(self.rule_receipt, counters)
        trigger = self.root / "a/probe/daemon/build-go"
        with trigger.open("xb") as output:
            output.write(b"r2-network-v1\n")
        trigger.chmod(0o644)
        self.event("a", "daemon/build-result", b"r2-network-v1\n", end)
        latest = self.counters(end)
        self.facts["build_route"] = counter_delta(counters, latest, required={"build", "build-return"})
        counters = latest
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            address = (NETWORK["local"][0], NETWORK["ports"]["forbidden"])
            listener.bind(address)
            listener.listen(4)
            with socket.create_connection(address, timeout=remaining(end)) as control:
                with listener.accept()[0] as accepted:
                    control.sendall(b"r2")
                    require(accepted.recv(2) == b"r2", "local-input-positive-control")
            for role in ("daemon", "build"):
                trigger = self.root / "b/probe" / role / "go"
                with trigger.open("xb") as output:
                    output.write(b"r2-network-v1\n")
                trigger.chmod(0o644)
                self.event("b", f"{role}/result", b"r2-network-v1\n", end)
                latest = self.counters(end)
                self.facts[role] = counter_delta(counters, latest)
                counters = latest
        trigger = self.root / "c/probe/daemon/finish"
        with trigger.open("xb") as output:
            output.write(b"r2-network-v1\n")
        trigger.chmod(0o644)
        self.event("c", "daemon/closed", b"r2-network-v1\n", end)
        self.clear_synthetic(end)
        stable_rules(counters, self.counters(end))


def endpoint_inventory(node, address, mac, links, ipv4, ipv6, temporary):
    require(len(links) == 2 and {row["ifname"] for row in links} == {"eth0", "lo"}, "node-endpoints")
    eth = next(row for row in links if row["ifname"] == "eth0")
    loopback = next(row for row in links if row["ifname"] == "lo")
    require(eth["address"] == mac, "node-source-mac")
    expected_eth = {(address, 24)}
    expected_lo = {("127.0.0.1", 8), ("::1", 128)}
    if temporary and node in "bc":
        expected_eth.add(("fd42:72::12" if node == "b" else NETWORK["ipv6"], 64))
    if temporary and node == "c":
        expected_lo.update((NETWORK[key], 32) for key in ("outside", "link_local"))
    for interface, expected in ((eth, expected_eth), (loopback, expected_lo)):
        observed = [(row["local"], row["prefixlen"]) for row in interface["addr_info"]]
        require(len(observed) == len(expected) and set(observed) == expected, "node-addresses")
    routes = [("198.18.72.0/24", "eth0", None)]
    if temporary and node == "b":
        routes.extend(((NETWORK["outside"], "eth0", NETWORK["local"][0]),
                       (NETWORK["link_local"], "eth0", NETWORK["nodes"]["c"][0])))
    actual = [(row["dst"].removesuffix("/32"), row.get("dev"), row.get("gateway")) for row in ipv4]
    require(sorted(actual) == sorted(routes), "node-ipv4-routes")
    expected_v6 = [("fd42:72::/64", "eth0", None)] if temporary and node in "bc" else []
    require([(row["dst"], row.get("dev"), row.get("gateway")) for row in ipv6] == expected_v6, "node-ipv6-routes")


def validate_unit(unit, facts):
    expected = {
        "Id": unit, "LoadState": "loaded", "ActiveState": "active",
        "ControlGroup": f"/system.slice/{unit}", "MemoryMax": str(OUTER["memory"]),
        "MemorySwapMax": str(OUTER["swap"]), "TasksMax": str(OUTER["pids"]),
        "CPUQuotaPerSecUSec": "1s", "LimitNOFILE": str(OUTER["nofile"]), "LimitCORE": str(OUTER["core"]),
    }
    require(all(facts.get(key) == value for key, value in expected.items())
            and re.fullmatch(r"[1-9][0-9]{0,9}", facts.get("MainPID", ""))
            and re.fullmatch(r"[0-9a-f]{32}", facts.get("InvocationID", "")), "unit-envelope")


def observe_limits(group, root=Path("/sys/fs/cgroup")):
    require(group.is_relative_to(root) and group != root, "cgroup-root")
    controllers = bounded_read(root / "cgroup.controllers", 512).decode("ascii").split()
    require({"memory", "pids", "cpu"} <= set(controllers), "cgroup-root")
    files = {"memory.max": str(OUTER["memory"]), "memory.swap.max": str(OUTER["swap"]),
             "pids.max": str(OUTER["pids"])}
    for name, expected in files.items():
        try:
            value = bounded_read(group / name, 64).decode("ascii").strip()
        except FileNotFoundError:
            raise Refusal("cgroup-outer-limit") from None
        require(value == expected, "cgroup-outer-limit")
    try:
        parts = bounded_read(group / "cpu.max", 64).decode("ascii").split()
    except FileNotFoundError:
        raise Refusal("cgroup-cpu") from None
    require(len(parts) == 2, "cgroup-cpu")
    quota, period = parts
    require(quota.isdecimal() and period.isdecimal() and 0 < int(quota) == int(period), "cgroup-cpu")
    ancestors = []
    cursor = group.parent
    while cursor != root:
        ancestors.append(cursor)
        cursor = cursor.parent
    require(len(ancestors) >= 1, "cgroup-ancestor-limit")
    # Linux cgroup v2 exposes memory.max, memory.swap.max, pids.max and cpu.max
    # only on non-root cgroups, never on the host hierarchy root.
    for parent in ancestors:
        for name, ceiling in files.items():
            try:
                value = bounded_read(parent / name, 64).decode("ascii").strip()
            except FileNotFoundError:
                raise Refusal("cgroup-ancestor-limit") from None
            require(value == "max" or value.isdecimal() and int(value) >= int(ceiling), "cgroup-ancestor-limit")
        try:
            a_parts = bounded_read(parent / "cpu.max", 64).decode("ascii").split()
        except FileNotFoundError:
            raise Refusal("cgroup-ancestor-cpu") from None
        require(len(a_parts) == 2 and a_parts[1].isdecimal() and int(a_parts[1]) > 0, "cgroup-ancestor-cpu")
        if a_parts[0] != "max":
            require(a_parts[0].isdecimal() and int(a_parts[0]) * int(period) >= int(quota) * int(a_parts[1]),
                    "cgroup-ancestor-cpu")


def policy(bridge, links):
    nodes = NETWORK["nodes"]
    for node, link in links.items():
        require(node in nodes and re.fullmatch(r"r2[0-9a-f]{11}[abc]", link), "policy-interface")
    require(set(links) == set(nodes) and re.fullmatch(r"r2[0-9a-f]{11}", bridge), "policy-bridge")
    def port(node, direction):
        return f'{direction}ifname "{links[node]}"'
    def pair(source, target):
        source_ip, source_mac = nodes[source]
        target_ip, target_mac = nodes[target]
        return (f"{port(source, 'i')} {port(target, 'o')} ether saddr {source_mac} ether daddr {target_mac}"
                f" ip saddr {source_ip} ip daddr {target_ip}")
    forward = []
    flows = (("peer", "b", "a"), ("enrollment", "b", "c"), ("build", "a", "b"))
    for name, source, target in flows:
        service = NETWORK["ports"][name]
        forward.extend(((name, f"{pair(source, target)} tcp dport {service}", "accept"),
                        (name + "-return", f"{pair(target, source)} tcp sport {service} ct state established", "accept")))
    b_ip, b_mac = nodes["b"]
    c_ip, c_mac = nodes["c"]
    identity = f"{port('b', 'i')} {port('c', 'o')} ether saddr {b_mac} ether daddr {c_mac}"
    exceptional = [
        ("forbidden", f"{identity} ip saddr {b_ip} ip daddr {c_ip} tcp dport 22174"),
        ("stray-outside", f'{identity} ip saddr {b_ip} ip daddr {NETWORK["outside"]} tcp dport 22174'),
        ("link-local", f'{identity} ip saddr {b_ip} ip daddr {NETWORK["link_local"]} tcp dport 22174'),
        ("ipv6", f'{identity} ip6 saddr fd42:72::12 ip6 daddr {NETWORK["ipv6"]} tcp dport 22174'),
        ("dns-tcp", f"{identity} ip saddr {b_ip} ip daddr {c_ip} tcp dport 53"),
        ("dns-udp", f"{identity} ip saddr {b_ip} ip daddr {c_ip} udp dport 53"),
    ]
    forward.extend((key, match, "drop") for key, match in exceptional)
    for source, (source_ip, source_mac) in nodes.items():
        for target, (target_ip, _target_mac) in nodes.items():
            if source != target:
                forward.append((f"arp-{source}-{target}",
                                f"{port(source, 'i')} {port(target, 'o')} ether saddr {source_mac} "
                                f"arp saddr ether {source_mac} arp saddr ip {source_ip} arp daddr ip {target_ip}", "accept"))
    local, local_mac = NETWORK["local"]
    local_probe = (f"{port('b', 'i')} ether saddr {b_mac} ether daddr {local_mac}"
                   f" ip saddr {b_ip} ip daddr {local} tcp dport 22174")
    outside_probe = local_probe.replace(f"ip daddr {local}", f'ip daddr {NETWORK["outside"]}')
    input_rules = [("synthetic-local", local_probe, "accept"), ("outside-to-host", outside_probe, "accept")]
    for node in nodes:
        forward.extend(((f"forward-in-{node}", port(node, "i"), "drop"),
                        (f"forward-out-{node}", port(node, "o"), "drop")))
        input_rules.append((f"bridge-input-{node}", port(node, "i"), "drop"))
    scoped = [bridge, *links.values()]
    return {
        "bridge": {"r2_forward": ("forward", forward), "r2_input": ("input", input_rules)},
        "inet": {
            "r2_input": ("input", [("local", f'iifname "{bridge}" ip saddr {b_ip} ip daddr {local} tcp dport 22174', "drop")]
                         + [(f"host-input-{i}", f'iifname "{name}"', "drop") for i, name in enumerate(scoped)]),
            "r2_forward": ("forward", [
                ("outside", f'iifname "{bridge}" oifname "{bridge}" ip saddr {b_ip} '
                 f'ip daddr {NETWORK["outside"]} tcp dport 22174', "drop"),
            ] + [(f"host-forward-{d}-{i}", f'{d}ifname "{name}"', "drop") for i, name in enumerate(scoped) for d in "io"]),
        },
    }


def table_handle(rows, family, name):
    tables = [row["table"] for row in rows if "table" in row]
    require(len(tables) == 1 and tables[0]["family"] == family and tables[0]["name"] == name
            and type(tables[0]["handle"]) is int and tables[0]["handle"] > 0, "nft-table")
    return tables[0]["handle"]


def read_counters(rows, chains, family, name):
    handle = table_handle(rows, family, name)
    entries = {key: [] for key in chains}
    found = set()
    handles = set()
    facts = {}
    for row in rows:
        require(len(row) == 1, "nft-row")
        if "table" in row or "metainfo" in row:
            continue
        if "chain" in row:
            chain = row["chain"]
            key = chain["name"]
            require(key in chains and key not in found and chain["family"] == family and chain["table"] == name
                    and chain["type"] == "filter" and chain["hook"] == chains[key][0] and chain["prio"] == -310
                    and chain["policy"] == "accept", "nft-chain")
            found.add(key)
            continue
        require("rule" in row, "nft-rule-type")
        rule = row["rule"]
        key = rule["chain"]
        ident = rule["handle"]
        comment = rule["comment"]
        expr = rule["expr"]
        require(key in entries and rule["family"] == family and rule["table"] == name
                and type(ident) is int and ident > 0 and ident not in handles and comment.startswith("r2-")
                and len(expr) >= 3, "nft-rule-identity")
        index = len(entries[key])
        require(index < len(chains[key][1]), "nft-extra-rule")
        label, source, verdict = chains[key][1][index]
        require(comment == "r2-" + label and expr[-1] == {verdict: None}, "nft-rule-order-verdict")
        rules_readback(expr[:-2], source, family)
        counter = expr[-2].get("counter", {})
        packets, total = counter.get("packets"), counter.get("bytes")
        require(type(packets) is int and type(total) is int and packets >= 0 and total >= 0, "nft-counter")
        entries[key].append(label)
        handles.add(ident)
        facts[label] = (handle, ident, packets, total)
    require(found == chains.keys() and all(len(entries[key]) == len(rules) for key, (_, rules) in chains.items()),
            "nft-missing-rule")
    return facts


def source_protocols(source):
    tokens = source.split()
    protocols = set()
    index = 0
    while index < len(tokens):
        head = tokens[index]
        if head in ("iifname", "oifname"):
            require(index + 1 < len(tokens), "nft-source")
            index += 2
        elif head == "ct":
            require(index + 2 < len(tokens), "nft-source")
            index += 3
        elif head == "arp":
            require(index + 3 < len(tokens) and tokens[index + 1] in ("saddr", "daddr")
                    and tokens[index + 2] in ("ether", "ip"), "nft-source")
            protocols.add("arp")
            index += 4
        elif head in ("ether", "ip", "ip6", "tcp", "udp"):
            require(index + 2 < len(tokens), "nft-source")
            protocols.add(head)
            index += 3
        else:
            raise Refusal("nft-source")
    return protocols


def allowed_nft_dependencies(source, family):
    protocols = source_protocols(source)
    has_ip = "ip" in protocols
    has_ip6 = "ip6" in protocols
    has_arp = "arp" in protocols
    has_tcp = "tcp" in protocols
    has_udp = "udp" in protocols
    deps = set()
    if family == "bridge":
        if has_ip:
            deps.add(("payload", "ether", "type", "ip"))
        if has_ip6:
            deps.add(("payload", "ether", "type", "ip6"))
        if has_arp:
            deps.update((
                ("payload", "ether", "type", "arp"),
                ("payload", "arp", "htype", 1),
                ("payload", "arp", "ptype", "ip"),
                ("payload", "arp", "hlen", 6),
                ("payload", "arp", "plen", 4),
            ))
    elif family == "inet":
        if has_ip:
            deps.add(("meta", "nfproto", "ipv4"))
        if has_ip6:
            deps.add(("meta", "nfproto", "ipv6"))
    if has_tcp:
        deps.add(("meta", "l4proto", "tcp"))
        if has_ip:
            deps.add(("payload", "ip", "protocol", "tcp"))
        if has_ip6:
            deps.add(("payload", "ip6", "nexthdr", "tcp"))
    if has_udp:
        deps.add(("meta", "l4proto", "udp"))
        if has_ip:
            deps.add(("payload", "ip", "protocol", "udp"))
        if has_ip6:
            deps.add(("payload", "ip6", "nexthdr", "udp"))
    return deps


def rules_readback(expressions, source, family="bridge"):
    # Match the expressions the kernel returned, including each protocol, field
    # and literal operand, while permitting only exact implicit protocol
    # dependencies implied by this rule's family and explicit predicates.
    allowed_deps = allowed_nft_dependencies(source, family)
    seen_deps = set()
    rendered = []
    for expr in expressions:
        match = expr.get("match", {})
        left, right = match.get("left", {}), match.get("right")
        require((match.get("op") == "==" or "ct" in left and match.get("op") == "in")
                and len(left) == 1, "nft-match")
        if "meta" in left:
            key = left["meta"]["key"]
            dep = ("meta", key, right)
            if dep in allowed_deps:
                require(dep not in seen_deps, "nft-match-drift")
                seen_deps.add(dep)
                continue
            require(key in ("iifname", "oifname") and isinstance(right, str), "nft-expression")
            rendered.append(f'{key} "{right}"')
        elif "payload" in left:
            field = left["payload"]
            dep = ("payload", field["protocol"], field["field"], right)
            if dep in allowed_deps:
                require(dep not in seen_deps, "nft-match-drift")
                seen_deps.add(dep)
                continue
            rendered.append(f'{field["protocol"]} {field["field"]} {right}')
        elif "ct" in left:
            value = right[0] if isinstance(right, list) and len(right) == 1 else right
            require(isinstance(value, str), "nft-match")
            rendered.append(f'ct {left["ct"]["key"]} {value}')
        else:
            raise Refusal("nft-expression")
    require(" ".join(rendered) == source, "nft-match-drift")


def stable_rules(before, after):
    require(before.keys() == after.keys() and all(before[key][:2] == after[key][:2]
                                                for key in before), "nft-handle-drift")


def counter_delta(before, after, *, required=None):
    stable_rules(before, after)
    if required is None:
        required = {"peer", "peer-return", "enrollment", "enrollment-return", "forbidden", "outside-to-host",
                    "outside", "link-local", "ipv6", "dns-tcp", "dns-udp", "synthetic-local", "local"}
    require(required <= before.keys(), "nft-missing-counter")
    result = {}
    for name, (_table, handle, packets, size) in after.items():
        delta = packets - before[name][2]
        byte_delta = size - before[name][3]
        require(0 <= delta <= 32 and (byte_delta == 0 if delta == 0 else 20*delta <= byte_delta <= 2000*delta),
                "nft-counter-drift")
        if name in required:
            require(delta > 0, "nft-missing-hit")
            result[name] = {"handle": handle, "packets": delta}
    return result


def check_security_status(status, caps, uid):
    fields = dict(line.split(":", 1) for line in status.splitlines() if ":" in line)
    caps_value = f"{caps:016x}"
    for key in ("CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"):
        require(fields.get(key, "").strip() == caps_value, "process-capabilities")
    for key in ("Uid", "Gid"):
        require(fields.get(key, "").split() == [str(uid)] * 4, "process-role")
    require(fields.get("NoNewPrivs", "").strip() == "1" and fields.get("Umask", "").strip() == "0077"
            and "Groups" in fields and not fields["Groups"].strip(), "process-locks")


def check_mount_inventory(contents, broker):
    cgroup = "/sys/fs/cgroup"
    expected = {
        "/": ("ext4", "/", "ro"), "/data": ("ext4", "/", "rw"), "/proc": ("proc", "/", "rw"),
        "/run": ("tmpfs", "/", "rw"), "/tmp": ("tmpfs", "/", "rw"), "/dev": ("tmpfs", "/", "rw"),
        "/dev/pts": ("devpts", "/", "rw"), cgroup: ("cgroup2", "/", "ro"),
    }
    if broker:
        expected[cgroup + "/system.slice/arxd.service"] = ("cgroup2", "/system.slice/arxd.service", "rw")
    observed = {}
    ids = set()
    for line in contents.splitlines():
        left, separator, right = line.partition(" - ")
        fields = left.split()
        tail = right.split()
        require(separator and len(fields) == 6 and len(tail) == 3, "mount-propagation")
        ident, _, _, source, target, options = fields
        require(ident.isdecimal() and ident not in ids and target not in observed, "mount-instance")
        ids.add(ident)
        observed[target] = (tail[0], source, options.split(",")[0])
    require_same(observed, expected, "mount-inventory")
