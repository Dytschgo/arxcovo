"""Pure R2 mount, process, policy, route and teardown controls."""

from copy import deepcopy
import json
from pathlib import Path
import stat
import subprocess
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from fixture import INNER, MIB, NETWORK, OUTER, Refusal, TOOLS, load_pins, filesystem_facts
import isolation
import run


PORTS = {node: f"r2{'1'*11}{node}" for node in "abc"}
BRIDGE = "r2" + "1"*11
TABLE = "r2_" + "1"*16


def mounts():
    data = [
        ("/", "/", "ro", "ext4"), ("/", "/data", "rw", "ext4"),
        ("/", "/proc", "rw", "proc"), ("/", "/run", "rw", "tmpfs"),
        ("/", "/tmp", "rw", "tmpfs"), ("/", "/dev", "rw", "tmpfs"),
        ("/", "/dev/pts", "rw", "devpts"), ("/", "/sys/fs/cgroup", "ro", "cgroup2"),
        ("/system.slice/arxd.service", "/sys/fs/cgroup/system.slice/arxd.service", "rw", "cgroup2"),
    ]
    return "\n".join(f"{i} 1 0:1 {src} {dest} {flag},nosuid - {kind} none rw"
                     for i, (src, dest, flag, kind) in enumerate(data, 2))


def status(caps=193, uid=0):
    return "\n".join([*(f"{name}:\t{caps:016x}" for name in ("CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb")),
                      f"Uid:\t{uid}\t{uid}\t{uid}\t{uid}", f"Gid:\t{uid}\t{uid}\t{uid}\t{uid}",
                      "Umask:\t0077", "Groups:\t", "NoNewPrivs:\t1"])


class Namespaces(unittest.TestCase):
    def test_mount_instance_propagation_and_only_exact_delegate_writable(self):
        good = mounts()
        isolation.check_mount_inventory(good, True)
        faults = [
            good.replace("/proc rw,nosuid", "/proc rw,nosuid shared:44"),
            good.replace("3 1 0:1 / /data", "2 1 0:1 / /data"),
            good.replace("/sys/fs/cgroup ro", "/sys/fs/cgroup rw"),
            good.replace("arxd.service rw", "arxd.service ro"),
            good.replace(" /system.slice/arxd.service /sys", " / /sys"),
            good + "\n30 1 0:1 / /data/workspace/ordinary rw - ext4 none rw",
            good + "\n31 1 0:1 / /dev/mem rw - tmpfs none rw",
        ]
        for candidate in faults:
            with self.subTest(candidate=candidate):
                with self.assertRaisesRegex(Refusal, "mount"):
                    isolation.check_mount_inventory(candidate, True)
        with self.assertRaisesRegex(Refusal, "mount-inventory"):
            isolation.check_mount_inventory(good, False)

    def test_exact_caps_nnp_uid_gid_groups_and_umask(self):
        good = status()
        isolation.check_security_status(good, 193, 0)
        isolation.check_security_status(status(0, 21072), 0, 21072)
        for key, value in (("CapBnd", "00000000000000c0"), ("NoNewPrivs", "0"), ("Gid", "0 0 0 1"),
                           ("Groups", "21073"), ("Umask", "0022")):
            changed = "\n".join(f"{key}:\t{value}" if line.startswith(key + ":") else line
                                for line in good.splitlines())
            with self.subTest(key=key):
                with self.assertRaisesRegex(Refusal, "process"):
                    isolation.check_security_status(changed, 193, 0)

    def test_delegate_bad_domain_marker_controllers_and_stale_runs(self):
        unit = Path("arxd.service")
        init = Path("init")
        evidence = {unit / "cgroup.type": b"domain\n", unit / "cgroup.procs": b"",
                    unit / "cgroup.subtree_control": b"memory pids\n",
                    unit / "runs/cgroup.subtree_control": b"pids memory\n"}
        with patch.object(isolation.os, "getxattr", return_value=b"1\n", create=True):
            with self.assertRaisesRegex(Refusal, "delegate-xattr"):
                isolation.Resources.check_delegate(unit, init)
        for file, value, refusal in ((unit / "cgroup.type", b"threaded\n", "delegate-domain"),
                                     (unit / "cgroup.procs", b"17\n", "delegate-domain"),
                                     (unit / "cgroup.subtree_control", b"memory\n", "delegate-controllers")):
            with self.subTest(file=file):
                with patch.object(isolation.os, "getxattr", return_value=b"1", create=True):
                    with patch.object(isolation, "bounded_read", side_effect=lambda path, _: {**evidence, file: value}[path]):
                        with self.assertRaisesRegex(Refusal, refusal):
                            isolation.Resources.check_delegate(unit, init)
        stale = SimpleNamespace(name="stale", is_dir=lambda: True)
        with patch.object(isolation.os, "getxattr", return_value=b"1", create=True):
            with patch.object(isolation, "bounded_read", side_effect=lambda path, _: evidence[path]):
                with patch.object(Path, "iterdir", return_value=iter([stale])):
                    with self.assertRaisesRegex(Refusal, "runs-inventory"):
                        isolation.Resources.check_delegate(unit, init)

    def test_unit_memory_swap_pid_and_outer_identity(self):
        unit = "arx-r2-test-b0.service"
        good = {
            "Id": unit, "LoadState": "loaded", "ActiveState": "active", "ControlGroup": f"/system.slice/{unit}",
            "InvocationID": "2"*32, "MainPID": "1234", "MemoryMax": str(OUTER["memory"]),
            "MemorySwapMax": "0", "TasksMax": "128", "CPUQuotaPerSecUSec": "1s",
            "LimitNOFILE": "128", "LimitCORE": "0",
        }
        isolation.validate_unit(unit, good)
        for key, value in (("MemoryMax", "infinity"), ("MemorySwapMax", "1"), ("TasksMax", "129"),
                           ("MainPID", "0"), ("ControlGroup", "/"), ("CPUQuotaPerSecUSec", "2s")):
            with self.subTest(key=key):
                with self.assertRaisesRegex(Refusal, "unit-envelope"):
                    isolation.validate_unit(unit, {**good, key: value})

    def test_observe_limits_root_without_limit_files_and_non_root_ancestors(self):
        root = Path("/sys/fs/cgroup")
        parent = root / "system.slice"
        unit = parent / "arx-r2-test-a0.service"
        # Realistic Linux cgroup v2: host root has cgroup.controllers, NOT memory.max/memory.swap.max/pids.max/cpu.max.
        base = {
            root / "cgroup.controllers": b"cpuset cpu io memory hugetlb pids rdma misc\n",
            parent / "memory.max": b"max\n",
            parent / "memory.swap.max": b"max\n",
            parent / "pids.max": b"max\n",
            parent / "cpu.max": b"max 100000\n",
            unit / "memory.max": f"{OUTER['memory']}\n".encode("ascii"),
            unit / "memory.swap.max": b"0\n",
            unit / "pids.max": f"{OUTER['pids']}\n".encode("ascii"),
            unit / "cpu.max": b"100000 100000\n",
        }

        def read_from(mapping):
            def reader(path, _limit):
                if path not in mapping:
                    raise FileNotFoundError(path)
                return mapping[path]
            return reader

        with patch.object(isolation, "bounded_read", side_effect=read_from(base)):
            isolation.observe_limits(unit, root=root)

        # Numeric non-root ancestor CPU limit >= unit quota (100% and 200%) passes.
        for ancestor_cpu in (b"100000 100000\n", b"200000 100000\n"):
            with patch.object(isolation, "bounded_read", side_effect=read_from({**base, parent / "cpu.max": ancestor_cpu})):
                isolation.observe_limits(unit, root=root)

        # Missing limit file on non-root ancestor (system.slice) must fail.
        for missing, pattern in (
            (parent / "memory.max", "cgroup-ancestor-limit"),
            (parent / "memory.swap.max", "cgroup-ancestor-limit"),
            (parent / "pids.max", "cgroup-ancestor-limit"),
            (parent / "cpu.max", "cgroup-ancestor-cpu"),
        ):
            reduced = {k: v for k, v in base.items() if k != missing}
            with self.subTest(missing=missing.name):
                with patch.object(isolation, "bounded_read", side_effect=read_from(reduced)):
                    with self.assertRaisesRegex(Refusal, pattern):
                        isolation.observe_limits(unit, root=root)

        # Tighter ancestor CPU ceiling (50% < 100%) or tighter memory/pids limit must fail.
        for key, bad_val, pattern in (
            (parent / "cpu.max", b"50000 100000\n", "cgroup-ancestor-cpu"),
            (parent / "memory.max", f"{OUTER['memory'] - 1}\n".encode("ascii"), "cgroup-ancestor-limit"),
            (parent / "pids.max", b"64\n", "cgroup-ancestor-limit"),
        ):
            with self.subTest(bad=key.name):
                with patch.object(isolation, "bounded_read", side_effect=read_from({**base, key: bad_val})):
                    with self.assertRaisesRegex(Refusal, pattern):
                        isolation.observe_limits(unit, root=root)

    def test_data_image_limits(self):
        pins = load_pins()["rootfs"]
        stats = dict(f_frsize=4096, f_bavail=60000, f_blocks=63000, f_favail=16380, f_files=16384)
        self.assertEqual(filesystem_facts(SimpleNamespace(**stats), pins, writable=True)["blocks_available"], 60000)
        for field, value in (("f_files", 16383), ("f_bavail", 1000), ("f_favail", 1000), ("f_frsize", 1024)):
            with self.subTest(field=field):
                with self.assertRaises(Refusal):
                    filesystem_facts(SimpleNamespace(**{**stats, field: value}), pins, writable=True)


class Network(unittest.TestCase):
    def test_exact_routes_and_an_extra_endpoint(self):
        address, mac = NETWORK["nodes"]["b"]
        links = [
            {"ifname": "eth0", "address": mac, "addr_info": [{"local": address, "prefixlen": 24}]},
            {"ifname": "lo", "addr_info": [{"local": "127.0.0.1", "prefixlen": 8}, {"local": "::1", "prefixlen": 128}]},
        ]
        ipv4 = [{"dst": "198.18.72.0/24", "dev": "eth0"}]
        isolation.endpoint_inventory("b", address, mac, links, ipv4, [], False)
        with self.assertRaisesRegex(Refusal, "node-endpoints"):
            isolation.endpoint_inventory("b", address, mac, [*links, links[0]], ipv4, [], False)
        with self.assertRaisesRegex(Refusal, "node-ipv4-routes"):
            isolation.endpoint_inventory("b", address, mac, links, [*ipv4, {"dst": "default"}], [], False)
        with self.assertRaisesRegex(Refusal, "node-ipv6-routes"):
            isolation.endpoint_inventory("b", address, mac, links, ipv4, [{"dst": "default"}], False)

    def test_policy_contains_exact_hook_and_denials(self):
        rules = isolation.policy(BRIDGE, PORTS)
        forward = dict((name, (predicate, verdict)) for name, predicate, verdict in rules["bridge"]["r2_forward"][1])
        self.assertEqual(forward["enrollment"][1], "accept")
        self.assertIn(f'iifname "{PORTS["a"]}" oifname "{PORTS["b"]}"', forward["build"][0])
        for key, value in (("stray-outside", "192.0.2.72"), ("link-local", "169.254.254.72"),
                           ("dns-tcp", "tcp dport 53"), ("dns-udp", "udp dport 53"), ("ipv6", "ip6 saddr")):
            self.assertIn(value, forward[key][0])
            self.assertEqual(forward[key][1], "drop")
        self.assertIn("host-forward-i-0", [row[0] for row in rules["inet"]["r2_forward"][1]])
        with self.assertRaisesRegex(Refusal, "policy-interface"):
            isolation.policy(BRIDGE, {**PORTS, "b": "r2b; delete"})

    def test_policy_readback_rejects_removed_and_substituted_rules(self):
        rules = {"r2_forward": ("forward", [("link-local", "ip daddr 169.254.254.72", "drop")])}
        table = {"table": {"family": "bridge", "name": TABLE, "handle": 1}}
        chain = {"chain": {"family": "bridge", "table": TABLE, "name": "r2_forward",
                           "type": "filter", "hook": "forward", "prio": -310, "policy": "accept"}}
        expression = {"match": {"op": "==", "left": {"payload": {"protocol": "ip", "field": "daddr"}},
                                "right": "169.254.254.72"}}
        rule = {"rule": {"family": "bridge", "table": TABLE, "chain": "r2_forward", "handle": 3,
                         "comment": "r2-link-local", "expr": [expression, {"counter": {"packets": 1, "bytes": 60}},
                                                             {"drop": None}]}}
        rows = [table, chain, rule]
        self.assertEqual(isolation.read_counters(rows, rules, "bridge", TABLE), {"link-local": (1, 3, 1, 60)})
        changed = deepcopy(rows)
        changed[-1]["rule"]["expr"][0]["match"]["right"] = "169.254.169.254"
        for candidate in (rows[:2], changed, [*rows, rule]):
            with self.subTest(candidate=candidate):
                with self.assertRaisesRegex(Refusal, "nft-"):
                    isolation.read_counters(candidate, rules, "bridge", TABLE)

    def test_policy_readback_accepts_representative_bridge_and_inet_dependencies(self):
        graph = isolation.policy(BRIDGE, PORTS)
        b_ip, b_mac = NETWORK["nodes"]["b"]
        a_ip, a_mac = NETWORK["nodes"]["a"]
        c_ip, c_mac = NETWORK["nodes"]["c"]
        local_ip, _ = NETWORK["local"]

        def m_meta(key, value):
            return {"match": {"op": "==", "left": {"meta": {"key": key}}, "right": value}}

        def m_pay(proto, field, value):
            return {"match": {"op": "==", "left": {"payload": {"protocol": proto, "field": field}}, "right": value}}

        def rule_row(family, chain, handle, label, exprs, verdict):
            return {"rule": {"family": family, "table": TABLE, "chain": chain, "handle": handle,
                             "comment": f"r2-{label}",
                             "expr": [*exprs, {"counter": {"packets": 2, "bytes": 120}}, {verdict: None}]}}

        bridge_subset = {
            "r2_forward": ("forward", [
                entry for entry in graph["bridge"]["r2_forward"][1]
                if entry[0] in ("peer", "peer-return", "ipv6", "dns-udp", "arp-a-b", "forward-in-b")
            ]),
        }
        bridge_rows = [
            {"metainfo": {"version": "1.0.9", "json_schema_version": 1}},
            {"table": {"family": "bridge", "name": TABLE, "handle": 10}},
            {"chain": {"family": "bridge", "table": TABLE, "name": "r2_forward",
                       "type": "filter", "hook": "forward", "prio": -310, "policy": "accept"}},
            rule_row("bridge", "r2_forward", 11, "peer", [
                m_meta("iifname", PORTS["b"]), m_meta("oifname", PORTS["a"]),
                m_pay("ether", "saddr", b_mac), m_pay("ether", "daddr", a_mac),
                m_pay("ether", "type", "ip"),
                m_pay("ip", "saddr", b_ip), m_pay("ip", "daddr", a_ip),
                m_pay("ip", "protocol", "tcp"),
                m_pay("tcp", "dport", 22171),
            ], "accept"),
            rule_row("bridge", "r2_forward", 12, "peer-return", [
                m_meta("iifname", PORTS["a"]), m_meta("oifname", PORTS["b"]),
                m_pay("ether", "saddr", a_mac), m_pay("ether", "daddr", b_mac),
                m_pay("ether", "type", "ip"),
                m_pay("ip", "saddr", a_ip), m_pay("ip", "daddr", b_ip),
                m_meta("l4proto", "tcp"),
                m_pay("tcp", "sport", 22171),
                {"match": {"op": "in", "left": {"ct": {"key": "state"}}, "right": "established"}},
            ], "accept"),
            rule_row("bridge", "r2_forward", 13, "ipv6", [
                m_meta("iifname", PORTS["b"]), m_meta("oifname", PORTS["c"]),
                m_pay("ether", "saddr", b_mac), m_pay("ether", "daddr", c_mac),
                m_pay("ether", "type", "ip6"),
                m_pay("ip6", "saddr", "fd42:72::12"), m_pay("ip6", "daddr", NETWORK["ipv6"]),
                m_pay("ip6", "nexthdr", "tcp"),
                m_pay("tcp", "dport", 22174),
            ], "drop"),
            rule_row("bridge", "r2_forward", 14, "dns-udp", [
                m_meta("iifname", PORTS["b"]), m_meta("oifname", PORTS["c"]),
                m_pay("ether", "saddr", b_mac), m_pay("ether", "daddr", c_mac),
                m_pay("ether", "type", "ip"),
                m_pay("ip", "saddr", b_ip), m_pay("ip", "daddr", c_ip),
                m_pay("ip", "protocol", "udp"),
                m_pay("udp", "dport", 53),
            ], "drop"),
            rule_row("bridge", "r2_forward", 15, "arp-a-b", [
                m_meta("iifname", PORTS["a"]), m_meta("oifname", PORTS["b"]),
                m_pay("ether", "saddr", a_mac),
                m_pay("ether", "type", "arp"),
                m_pay("arp", "saddr ether", a_mac), m_pay("arp", "saddr ip", a_ip), m_pay("arp", "daddr ip", b_ip),
            ], "accept"),
            rule_row("bridge", "r2_forward", 16, "forward-in-b", [
                m_meta("iifname", PORTS["b"]),
            ], "drop"),
        ]
        result = isolation.read_counters(bridge_rows, bridge_subset, "bridge", TABLE)
        self.assertEqual(set(result), {"peer", "peer-return", "ipv6", "dns-udp", "arp-a-b", "forward-in-b"})

        # Representative inet table with meta nfproto ipv4 + meta l4proto tcp / ip protocol tcp dependencies.
        inet_subset = {
            "r2_input": ("input", [graph["inet"]["r2_input"][1][0], graph["inet"]["r2_input"][1][1]]),
            "r2_forward": ("forward", [graph["inet"]["r2_forward"][1][0]]),
        }
        inet_rows = [
            {"table": {"family": "inet", "name": TABLE, "handle": 20}},
            {"chain": {"family": "inet", "table": TABLE, "name": "r2_input",
                       "type": "filter", "hook": "input", "prio": -310, "policy": "accept"}},
            rule_row("inet", "r2_input", 21, "local", [
                m_meta("iifname", BRIDGE),
                m_meta("nfproto", "ipv4"),
                m_pay("ip", "saddr", b_ip), m_pay("ip", "daddr", local_ip),
                m_meta("l4proto", "tcp"),
                m_pay("tcp", "dport", 22174),
            ], "drop"),
            rule_row("inet", "r2_input", 22, "host-input-0", [
                m_meta("iifname", BRIDGE),
            ], "drop"),
            {"chain": {"family": "inet", "table": TABLE, "name": "r2_forward",
                       "type": "filter", "hook": "forward", "prio": -310, "policy": "accept"}},
            rule_row("inet", "r2_forward", 23, "outside", [
                m_meta("iifname", BRIDGE), m_meta("oifname", BRIDGE),
                m_meta("nfproto", "ipv4"),
                m_pay("ip", "saddr", b_ip), m_pay("ip", "daddr", NETWORK["outside"]),
                m_pay("ip", "protocol", "tcp"),
                m_pay("tcp", "dport", 22174),
            ], "drop"),
        ]
        self.assertEqual(set(isolation.read_counters(inet_rows, inet_subset, "inet", TABLE)),
                         {"local", "host-input-0", "outside"})

        # Wrong-protocol, wrong-family, duplicate, or catch-all extra dependencies must be refused.
        for label, family, subset, base_rows, idx, bad_expr in (
            ("wrong-l4", "bridge", bridge_subset, bridge_rows, 3, m_meta("l4proto", "udp")),
            ("wrong-family-dep", "bridge", bridge_subset, bridge_rows, 3, m_meta("nfproto", "ipv4")),
            ("duplicate-dep", "bridge", bridge_subset, bridge_rows, 3, m_pay("ether", "type", "ip")),
            ("extra-on-catchall", "bridge", bridge_subset, bridge_rows, 8, m_pay("ether", "type", "ip")),
            ("ether-in-inet", "inet", inet_subset, inet_rows, 2, m_pay("ether", "type", "ip")),
        ):
            with self.subTest(case=label):
                tampered = deepcopy(base_rows)
                tampered[idx]["rule"]["expr"].insert(0, bad_expr)
                with self.assertRaisesRegex(Refusal, "nft-"):
                    isolation.read_counters(tampered, subset, family, TABLE)

    def test_counter_requires_allow_and_deny_hits_with_same_handles(self):
        graph = isolation.policy(BRIDGE, PORTS)
        labels = [rule[0] for chains in graph.values() for _, rules in chains.values() for rule in rules]
        initial = {name: (1, i, 10, 600) for i, name in enumerate(labels, 1)}
        later = {name: (tid, ident, 11, 660) for name, (tid, ident, _, _) in initial.items()}
        result = isolation.counter_delta(initial, later)
        self.assertEqual(result["ipv6"]["packets"], 1)
        self.assertEqual(set(isolation.counter_delta(initial, later, required={"build", "build-return"})),
                         {"build", "build-return"})
        for label in ("peer", "dns-udp", "local", "link-local"):
            with self.subTest(label=label):
                with self.assertRaisesRegex(Refusal, "nft-missing-hit"):
                    isolation.counter_delta(initial, {**later, label: initial[label]})
        with self.assertRaisesRegex(Refusal, "nft-handle-drift"):
            isolation.counter_delta(initial, {**later, "ipv6": (99, 1, 11, 660)})
        with self.assertRaisesRegex(Refusal, "nft-counter-drift"):
            isolation.counter_delta(initial, {**later, "ipv6": (*initial["ipv6"][:2], 9, 660)})


class Cleanup(unittest.TestCase):
    @staticmethod
    def make_resources():
        return isolation.Resources(
            {"repo": "org/repo", "head": "a"*40, "run": "1", "attempt": "1"},
            load_pins(),
            {**TOOLS, "ip": "/usr/bin/ip"},
        )

    def test_cleanup_keeps_trying_and_refuses_ambiguous_mount(self):
        resources = self.make_resources()
        calls = []
        def fail(_end):
            calls.append("mount")
            raise Refusal("ambiguous")
        resources.remember("arena", lambda _: calls.append("arena"))
        resources.remember("mount", fail)
        resources.remember("unit", lambda _: calls.append("unit"))
        self.assertEqual(resources.cleanup(0), ["mount"])
        self.assertEqual(calls, ["unit", "mount", "arena"])
        mount_path = resources.root / "root"
        dup_info = (f"142 29 7:0 / {mount_path.as_posix()} ro,nosuid - ext4 /dev/loop0 ro\n"
                    f"143 29 7:1 / {mount_path.as_posix()} ro,nosuid - ext4 /dev/loop1 ro\n")
        with patch.object(isolation, "bounded_read", return_value=dup_info.encode("ascii")), \
             patch.object(resources, "mounts", return_value=["one", "two"]):
            with self.assertRaisesRegex(Refusal, "mount-identity"):
                resources.release_mount(mount_path, "/dev/loop0", 142, 0)

    def test_cleanup_timeout_runs_later_callbacks_and_preserves_first_failure(self):
        resources = self.make_resources()
        calls = []
        resources.remember("arena", lambda _end: calls.append("arena"))
        resources.remember("mount-b", lambda _end: (calls.append("mount-b"),
                                                    (_ for _ in ()).throw(subprocess.TimeoutExpired(["umount"], 1))))
        resources.remember("unit-b", lambda _end: calls.append("unit-b"))

        def fail_contain(_end):
            calls.append("contain")
            raise Refusal("mount-instance")

        resources.contain = fail_contain
        receipt = run.Qualification().run(lambda _end: ({}, {}, resources), lambda *_: {})
        self.assertEqual(calls, ["contain", "unit-b", "mount-b", "arena"])
        self.assertEqual(receipt["first"], {"phase": "contain", "category": "mount-instance"})
        self.assertEqual(receipt["cleanup"], "incomplete")
        self.assertEqual(receipt["cleanup_errors"], ["mount-b"])

    def test_cleanup_refuses_substituted_mount_id_and_loop_backing_identity(self):
        resources = self.make_resources()
        mount_path = resources.root / "root"
        # Substituted mount_id (143 instead of recorded 142) with identical target/source/fstype/fsroot must refuse umount.
        substituted_info = f"143 29 7:0 / {mount_path.as_posix()} ro,nosuid - ext4 /dev/loop0 ro\n".encode("ascii")
        with patch.object(isolation, "bounded_read", return_value=substituted_info), \
             patch.object(resources, "mounts", return_value=[f"143 {mount_path.as_posix()} /dev/loop0 ext4 /"]), \
             patch.object(resources, "cmd") as mocked_cmd:
            with self.assertRaisesRegex(Refusal, "mount-identity"):
                resources.release_mount(mount_path, "/dev/loop0", 142, 0)
            mocked_cmd.assert_not_called()

        # Matching mount_id (142) executes umount and verifies clear.
        matching_info = f"142 29 7:0 / {mount_path.as_posix()} ro,nosuid - ext4 /dev/loop0 ro\n".encode("ascii")
        mountinfo_seq = iter([matching_info, matching_info, b""])
        findmnt_seq = iter([[f"142 {mount_path.as_posix()} /dev/loop0 ext4 /"], []])
        with patch.object(isolation, "bounded_read", side_effect=lambda *a: next(mountinfo_seq)), \
             patch.object(resources, "mounts", side_effect=lambda *a: next(findmnt_seq)), \
             patch.object(resources, "cmd", return_value="") as mocked_cmd:
            resources.release_mount(mount_path, "/dev/loop0", 142, 0)
            mocked_cmd.assert_called_once_with("umount", 0, mount_path)

        # Substituted loop backing identity (different back-ino or back-maj:min or file inode) must refuse losetup -d.
        size = 256 * MIB
        image = resources.root / "a.ext4"
        orig_dev = (8 << 8) | 1  # 8:1
        orig_ino = 50001
        backing = (orig_dev, orig_ino)
        good_stat = SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_uid=0, st_nlink=1,
                                    st_size=size, st_blocks=size // 512, st_dev=orig_dev, st_ino=orig_ino)
        base_row = {"name": "/dev/loop4", "back-file": image.as_posix(), "back-ino": orig_ino,
                    "back-maj:min": isolation.dev_majmin(orig_dev), "ro": False, "offset": 0, "sizelimit": 0}
        for label, row_override, file_ino in (
            ("substituted-back-ino", {"back-ino": orig_ino + 1}, orig_ino),
            ("substituted-back-dev", {"back-maj:min": "8:2"}, orig_ino),
            ("substituted-file-inode", {}, orig_ino + 1),
            ("substituted-loop-dev", {"name": "/dev/loop5"}, orig_ino),
        ):
            with self.subTest(case=label):
                row = {**base_row, **row_override}
                cur_stat = SimpleNamespace(**{**vars(good_stat), "st_ino": file_ino})
                with patch.object(resources, "loop_rows", return_value=[row]), \
                     patch.object(Path, "lstat", return_value=cur_stat), \
                     patch.object(isolation, "bounded_read", return_value=f"{size // 512}\n".encode("ascii")), \
                     patch.object(resources, "cmd") as mocked_cmd:
                    with self.assertRaisesRegex(Refusal, "loop-identity"):
                        resources.release_loop(image, size, False, backing, "/dev/loop4", 0)
                    mocked_cmd.assert_not_called()

        # Matching backing identity detaches the loop cleanly.
        loop_seq = iter([[base_row], [base_row], []])
        with patch.object(resources, "loop_rows", side_effect=lambda *a: next(loop_seq)), \
             patch.object(Path, "lstat", return_value=good_stat), \
             patch.object(isolation, "bounded_read", return_value=f"{size // 512}\n".encode("ascii")), \
             patch.object(resources, "cmd", return_value="") as mocked_cmd:
            resources.release_loop(image, size, False, backing, "/dev/loop4", 0)
            mocked_cmd.assert_called_once_with("loop", 0, "-d", "/dev/loop4")

    def test_partial_network_creation_registers_and_cleans_up_created_resources(self):
        resources = self.make_resources()
        live_links = {}
        live_ns = {}
        next_ifindex = [100]
        deleted = []

        def fake_ip(_end, *args, **kwargs):
            if args[:2] == ("link", "add"):
                name = args[3]
                if name == resources.links["b"]:
                    raise Refusal("ip-command-status")
                live_links[name] = {"ifindex": next_ifindex[0], "ifalias": ""}
                next_ifindex[0] += 1
                return ""
            if args[:2] == ("link", "set") and "alias" in args:
                name = args[3]
                live_links[name]["ifalias"] = args[args.index("alias") + 1]
                return ""
            if args[:2] == ("link", "del"):
                name = args[3]
                deleted.append(("link", name))
                live_links.pop(name, None)
                return ""
            if args[:3] == ("-j", "-d", "link"):
                name = args[-1]
                if name not in live_links:
                    return f'Device "{name}" does not exist.\n'
                return json.dumps([live_links[name]])
            if args[:2] == ("netns", "add"):
                live_ns[args[2]] = (7, 900 + len(live_ns))
                return ""
            if args[:2] == ("netns", "pids"):
                return ""
            if args[:2] == ("netns", "del"):
                deleted.append(("netns", args[2]))
                live_ns.pop(args[2], None)
                return ""
            return ""

        def fake_stat(path_self):
            name = path_self.name
            dev, ino = live_ns[name]
            return SimpleNamespace(st_dev=dev, st_ino=ino)

        def fake_exists(path_self):
            return path_self.name in live_ns

        with patch.object(resources, "network_names", return_value=False), \
             patch.object(resources, "ip", side_effect=fake_ip), \
             patch.object(Path, "stat", fake_stat), \
             patch.object(Path, "exists", fake_exists):
            with self.assertRaisesRegex(Refusal, "ip-command-status"):
                resources.make_links(0)
            # Bridge, netns a, veth a, netns b, and failed veth b guard are all registered incrementally.
            self.assertEqual([label for label, _ in resources.cleanup_stack],
                             ["link", "namespace", "link", "namespace", "link"])
            errors = resources.cleanup(0)
            self.assertEqual(errors, [])
        self.assertEqual(deleted, [
            ("netns", resources.namespaces["b"]),
            ("link", resources.links["a"]),
            ("netns", resources.namespaces["a"]),
            ("link", resources.bridge),
        ])
