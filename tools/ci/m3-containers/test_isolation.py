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


def mounts(*, broker=True, cgroup_root="/"):
    delegate_root = "/system.slice/arxd.service" if cgroup_root == "/" else f"{cgroup_root}/system.slice/arxd.service"
    data = [
        ("/", "/", "ro", "ext4"), ("/", "/data", "rw", "ext4"),
        ("/", "/proc", "rw", "proc"), ("/", "/run", "rw", "tmpfs"),
        ("/", "/tmp", "rw", "tmpfs"), ("/", "/dev", "rw", "tmpfs"),
        ("/", "/dev/pts", "rw", "devpts"), (cgroup_root, "/sys/fs/cgroup", "ro", "cgroup2"),
    ]
    if broker:
        data.append((delegate_root, "/sys/fs/cgroup/system.slice/arxd.service", "rw", "cgroup2"))
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

    def test_mount_inventory_host_unit_cgroup_root_and_escape_refusals(self):
        run_id = "1" * 16
        b0_root = f"/system.slice/arx-r2-{run_id}-b0.service"
        b1_root = f"/system.slice/arx-r2-{run_id}-b1.service"
        a0_root = f"/system.slice/arx-r2-{run_id}-a0.service"
        c0_root = f"/system.slice/arx-r2-{run_id}-c0.service"

        b0_good = mounts(broker=True, cgroup_root=b0_root)
        a0_good = mounts(broker=False, cgroup_root=a0_root)
        isolation.check_mount_inventory(b0_good, True, cgroup_root=b0_root)
        isolation.check_mount_inventory(a0_good, False, cgroup_root=a0_root)

        refusal_cases = (
            ("broker-host-root-escape", mounts(broker=True, cgroup_root="/"), True, b0_root),
            ("broker-cgroup-root-only-escape", b0_good.replace(f" {b0_root} /sys/fs/cgroup ", " / /sys/fs/cgroup "), True, b0_root),
            ("broker-wrong-unit", mounts(broker=True, cgroup_root=b1_root), True, b0_root),
            ("broker-unprefixed-delegate",
             b0_good.replace(f" {b0_root}/system.slice/arxd.service ", " /system.slice/arxd.service "), True, b0_root),
            ("broker-wrong-unit-delegate",
             b0_good.replace(f" {b0_root}/system.slice/arxd.service ", f" {b1_root}/system.slice/arxd.service "), True, b0_root),
            ("nonbroker-host-root-escape", mounts(broker=False, cgroup_root="/"), False, a0_root),
            ("nonbroker-wrong-unit", mounts(broker=False, cgroup_root=c0_root), False, a0_root),
            ("nonbroker-unexpected-delegate", mounts(broker=True, cgroup_root=a0_root), False, a0_root),
        )
        for label, candidate, broker, expected_root in refusal_cases:
            with self.subTest(case=label):
                with self.assertRaisesRegex(Refusal, "^mount-inventory$"):
                    isolation.check_mount_inventory(candidate, broker, cgroup_root=expected_root)

        for bad_root in ("", "system.slice/unit.service", f"{b0_root}/", f"{b0_root}/../other", "/system.slice//unit"):
            with self.subTest(bad_root=bad_root):
                with self.assertRaisesRegex(Refusal, "^mount-cgroup-root$"):
                    isolation.check_mount_inventory(b0_good, True, cgroup_root=bad_root)

    def test_check_namespace_verifies_host_unit_cgroup_root_and_broker_unit_membership(self):
        resources = Cleanup.make_resources()
        resources.facts = {"a": {}, "b": {}, "c": {}}
        b0_unit = f"arx-r2-{resources.id}-b0.service"
        b1_unit = f"arx-r2-{resources.id}-b1.service"
        a0_unit = f"arx-r2-{resources.id}-a0.service"
        b0_root = f"/system.slice/{b0_unit}"
        a0_root = f"/system.slice/{a0_unit}"

        def run_check(node, unit, mountinfo_text, outer_cgroup_text):
            outer_pid = 4100
            init_pid = 4101
            reads = {
                Path(f"/proc/{outer_pid}/task/{outer_pid}/children"): f"{init_pid}\n".encode("ascii"),
                Path(f"/proc/{init_pid}/status"): (
                    f"NSpid:\t{init_pid}\t1\nSeccomp:\t0\n" + status(0, 0)
                ).encode("ascii"),
                Path(f"/proc/{init_pid}/mountinfo"): mountinfo_text.encode("ascii"),
                Path(f"/proc/{init_pid}/attr/current"): b"unconfined\n",
                Path(f"/proc/{outer_pid}/cgroup"): outer_cgroup_text.encode("ascii"),
            }
            net_path = Path("/run/netns") / resources.namespaces[node]
            ns_inos = {
                Path("/proc/self/ns/mnt"): 101, Path("/proc/self/ns/uts"): 102,
                Path("/proc/self/ns/ipc"): 103, Path("/proc/self/ns/pid"): 104,
                Path("/proc/self/ns/net"): 105, Path("/proc/self/ns/cgroup"): 106,
                Path(f"/proc/{init_pid}/ns/mnt"): 201, Path(f"/proc/{init_pid}/ns/uts"): 202,
                Path(f"/proc/{init_pid}/ns/ipc"): 203, Path(f"/proc/{init_pid}/ns/pid"): 204,
                Path(f"/proc/{init_pid}/ns/net"): 205, Path(f"/proc/{init_pid}/ns/cgroup"): 206,
                net_path: 205,
            }
            delegate_calls = []

            def fake_delegate(delegate_path, init_path):
                delegate_calls.append((delegate_path, init_path))
                return {"caps": 193}

            with patch.object(isolation, "remaining", return_value=10.0), \
                 patch.object(isolation, "bounded_read", side_effect=lambda path, _limit: reads[path]), \
                 patch.object(Path, "stat", lambda p: SimpleNamespace(st_ino=ns_inos[p])), \
                 patch.object(resources, "check_delegate", side_effect=fake_delegate):
                resources.check_namespace(node, unit, outer_pid, 100.0)
            return delegate_calls

        # Matching host-perspective cgroup_root passes for both broker (b0) and non-broker (a0).
        calls = run_check("b", b0_unit, mounts(broker=True, cgroup_root=b0_root), f"0::{b0_root}/init\n")
        self.assertEqual(calls, [(Path("/sys/fs/cgroup") / f"system.slice/{b0_unit}/system.slice/arxd.service", Path("/proc/4101"))])
        self.assertEqual(run_check("a", a0_unit, mounts(broker=False, cgroup_root=a0_root), f"0::{a0_root}/init\n"), [])

        # Host root "/" mountinfo is refused when unit-scoped host cgroup_root is expected.
        with self.assertRaisesRegex(Refusal, "^mount-inventory$"):
            run_check("b", b0_unit, mounts(broker=True, cgroup_root="/"), f"0::{b0_root}/init\n")
        # Mismatched broker unit (b1 cgroup while checking b0 unit) is refused.
        with self.assertRaisesRegex(Refusal, "^broker-host-membership$"):
            run_check("b", b0_unit, mounts(broker=True, cgroup_root=b0_root), f"0::/system.slice/{b1_unit}/init\n")
        # Wrong node unit attribution is refused before reading /proc.
        with self.assertRaisesRegex(Refusal, "^unit-attribution$"):
            run_check("a", b0_unit, mounts(broker=False, cgroup_root=a0_root), f"0::{a0_root}/init\n")

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
                m_pay("ether", "daddr", a_mac), m_pay("ether", "saddr", b_mac),
                m_pay("ether", "type", "ip"),
                m_pay("ip", "saddr", b_ip), m_pay("ip", "daddr", a_ip),
                m_pay("ip", "protocol", "tcp"),
                m_pay("tcp", "dport", 22171),
            ], "accept"),
            rule_row("bridge", "r2_forward", 12, "peer-return", [
                m_meta("iifname", PORTS["a"]), m_meta("oifname", PORTS["b"]),
                m_pay("ether", "daddr", b_mac), m_pay("ether", "saddr", a_mac),
                m_pay("ether", "type", "ip"),
                m_pay("ip", "saddr", a_ip), m_pay("ip", "daddr", b_ip),
                m_meta("l4proto", "tcp"),
                m_pay("tcp", "sport", 22171),
                {"match": {"op": "in", "left": {"ct": {"key": "state"}}, "right": "established"}},
            ], "accept"),
            rule_row("bridge", "r2_forward", 13, "ipv6", [
                m_meta("iifname", PORTS["b"]), m_meta("oifname", PORTS["c"]),
                m_pay("ether", "daddr", c_mac), m_pay("ether", "saddr", b_mac),
                m_pay("ether", "type", "ip6"),
                m_pay("ip6", "saddr", "fd42:72::12"), m_pay("ip6", "daddr", NETWORK["ipv6"]),
                m_pay("ip6", "nexthdr", "tcp"),
                m_pay("tcp", "dport", 22174),
            ], "drop"),
            rule_row("bridge", "r2_forward", 14, "dns-udp", [
                m_meta("iifname", PORTS["b"]), m_meta("oifname", PORTS["c"]),
                m_pay("ether", "daddr", c_mac), m_pay("ether", "saddr", b_mac),
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

        # Wrong-protocol, contradictory ARP EtherType, wrong-family, duplicate, or catch-all extra deps must be refused.
        arp_source = next(src for name, src, _ in bridge_subset["r2_forward"][1] if name == "arp-a-b")
        self.assertEqual(isolation.source_protocols(arp_source), {"ether", "arp"})
        for label, family, subset, base_rows, idx, bad_expr in (
            ("wrong-l4", "bridge", bridge_subset, bridge_rows, 3, m_meta("l4proto", "udp")),
            ("wrong-family-dep", "bridge", bridge_subset, bridge_rows, 3, m_meta("nfproto", "ipv4")),
            ("duplicate-dep", "bridge", bridge_subset, bridge_rows, 3, m_pay("ether", "type", "ip")),
            ("arp-contradictory-ethertype", "bridge", bridge_subset, bridge_rows, 7, m_pay("ether", "type", "ip")),
            ("extra-on-catchall", "bridge", bridge_subset, bridge_rows, 8, m_pay("ether", "type", "ip")),
            ("ether-in-inet", "inet", inet_subset, inet_rows, 2, m_pay("ether", "type", "ip")),
        ):
            with self.subTest(case=label):
                tampered = deepcopy(base_rows)
                tampered[idx]["rule"]["expr"].insert(0, bad_expr)
                with self.assertRaisesRegex(Refusal, "nft-"):
                    isolation.read_counters(tampered, subset, family, TABLE)

    def test_policy_dual_mac_bridge_rules_emit_daddr_before_saddr_and_reject_reversed_readback(self):
        graph = isolation.policy(BRIDGE, PORTS)
        dual_mac_labels = set()
        arp_labels = set()
        for _chain, (_hook, rules) in graph["bridge"].items():
            for label, predicate, _verdict in rules:
                has_daddr = "ether daddr " in predicate
                has_saddr = "ether saddr " in predicate
                if has_daddr and has_saddr:
                    dual_mac_labels.add(label)
                    self.assertLess(
                        predicate.index("ether daddr "),
                        predicate.index("ether saddr "),
                        f"{label} must emit ether daddr before ether saddr",
                    )
                elif label.startswith("arp-"):
                    arp_labels.add(label)
                    self.assertTrue(has_saddr and not has_daddr, f"{label} must retain single ether saddr")
        self.assertEqual(
            dual_mac_labels,
            {
                "peer", "peer-return", "enrollment", "enrollment-return", "build", "build-return",
                "forbidden", "stray-outside", "link-local", "ipv6", "dns-tcp", "dns-udp",
                "synthetic-local", "outside-to-host",
            },
        )
        self.assertEqual(len(arp_labels), 6)

        # Reversed ether saddr before ether daddr in nft JSON readback must fail with nft-match-drift.
        b_ip, b_mac = NETWORK["nodes"]["b"]
        a_ip, a_mac = NETWORK["nodes"]["a"]
        peer_source = next(src for label, src, _ in graph["bridge"]["r2_forward"][1] if label == "peer")
        reversed_exprs = [
            {"match": {"op": "==", "left": {"meta": {"key": "iifname"}}, "right": PORTS["b"]}},
            {"match": {"op": "==", "left": {"meta": {"key": "oifname"}}, "right": PORTS["a"]}},
            {"match": {"op": "==", "left": {"payload": {"protocol": "ether", "field": "saddr"}}, "right": b_mac}},
            {"match": {"op": "==", "left": {"payload": {"protocol": "ether", "field": "daddr"}}, "right": a_mac}},
            {"match": {"op": "==", "left": {"payload": {"protocol": "ether", "field": "type"}}, "right": "ip"}},
            {"match": {"op": "==", "left": {"payload": {"protocol": "ip", "field": "saddr"}}, "right": b_ip}},
            {"match": {"op": "==", "left": {"payload": {"protocol": "ip", "field": "daddr"}}, "right": a_ip}},
            {"match": {"op": "==", "left": {"payload": {"protocol": "ip", "field": "protocol"}}, "right": "tcp"}},
            {"match": {"op": "==", "left": {"payload": {"protocol": "tcp", "field": "dport"}}, "right": 22171}},
        ]
        with self.assertRaisesRegex(Refusal, "^nft-match-drift$"):
            isolation.rules_readback(reversed_exprs, peer_source, "bridge")

    def test_bridge_meta_protocol_dependencies_accepted_for_matching_ip_and_rejected_otherwise(self):
        graph = isolation.policy(BRIDGE, PORTS)
        b_ip, b_mac = NETWORK["nodes"]["b"]
        a_ip, a_mac = NETWORK["nodes"]["a"]
        c_ip, c_mac = NETWORK["nodes"]["c"]
        local_ip, _ = NETWORK["local"]

        def m_meta(key, value):
            return {"match": {"op": "==", "left": {"meta": {"key": key}}, "right": value}}

        def m_pay(proto, field, value):
            return {"match": {"op": "==", "left": {"payload": {"protocol": proto, "field": field}}, "right": value}}

        forward_map = {label: src for label, src, _ in graph["bridge"]["r2_forward"][1]}
        inet_local_source = next(src for label, src, _ in graph["inet"]["r2_input"][1] if label == "local")

        peer_with_meta_proto = [
            m_meta("iifname", PORTS["b"]), m_meta("oifname", PORTS["a"]),
            m_pay("ether", "daddr", a_mac), m_pay("ether", "saddr", b_mac),
            m_meta("protocol", "ip"),
            m_pay("ip", "saddr", b_ip), m_pay("ip", "daddr", a_ip),
            m_pay("ip", "protocol", "tcp"),
            m_pay("tcp", "dport", 22171),
        ]
        ipv6_with_meta_proto = [
            m_meta("iifname", PORTS["b"]), m_meta("oifname", PORTS["c"]),
            m_pay("ether", "daddr", c_mac), m_pay("ether", "saddr", b_mac),
            m_meta("protocol", "ip6"),
            m_pay("ip6", "saddr", "fd42:72::12"), m_pay("ip6", "daddr", NETWORK["ipv6"]),
            m_pay("ip6", "nexthdr", "tcp"),
            m_pay("tcp", "dport", 22174),
        ]
        arp_base = [
            m_meta("iifname", PORTS["a"]), m_meta("oifname", PORTS["b"]),
            m_pay("ether", "saddr", a_mac),
            m_pay("ether", "type", "arp"),
            m_pay("arp", "saddr ether", a_mac), m_pay("arp", "saddr ip", a_ip), m_pay("arp", "daddr ip", b_ip),
        ]
        catchall_base = [m_meta("iifname", PORTS["b"])]
        inet_local_base = [
            m_meta("iifname", BRIDGE),
            m_meta("nfproto", "ipv4"),
            m_pay("ip", "saddr", b_ip), m_pay("ip", "daddr", local_ip),
            m_meta("l4proto", "tcp"),
            m_pay("tcp", "dport", 22174),
        ]

        # Accepted for matching bridge IP and IPv6 rules.
        isolation.rules_readback(peer_with_meta_proto, forward_map["peer"], "bridge")
        isolation.rules_readback(ipv6_with_meta_proto, forward_map["ipv6"], "bridge")

        # Rejected on wrong-protocol, wrong-family, duplicate, or catch-all rules.
        for label, exprs, source, family in (
            ("ip6-meta-on-ip", [m_meta("protocol", "ip6"), *peer_with_meta_proto], forward_map["peer"], "bridge"),
            ("ip-meta-on-ip6", [m_meta("protocol", "ip"), *ipv6_with_meta_proto], forward_map["ipv6"], "bridge"),
            ("ip-meta-on-arp", [m_meta("protocol", "ip"), *arp_base], forward_map["arp-a-b"], "bridge"),
            ("ip6-meta-on-arp", [m_meta("protocol", "ip6"), *arp_base], forward_map["arp-a-b"], "bridge"),
            ("ip-meta-on-inet", [m_meta("protocol", "ip"), *inet_local_base], inet_local_source, "inet"),
            ("ip6-meta-on-inet", [m_meta("protocol", "ip6"), *inet_local_base], inet_local_source, "inet"),
            ("duplicate-ip-meta", [m_meta("protocol", "ip"), *peer_with_meta_proto], forward_map["peer"], "bridge"),
            ("duplicate-ip6-meta", [m_meta("protocol", "ip6"), *ipv6_with_meta_proto], forward_map["ipv6"], "bridge"),
            ("ip-meta-on-catchall", [m_meta("protocol", "ip"), *catchall_base], forward_map["forward-in-b"], "bridge"),
            ("ip6-meta-on-catchall", [m_meta("protocol", "ip6"), *catchall_base], forward_map["forward-in-b"], "bridge"),
        ):
            with self.subTest(case=label):
                with self.assertRaisesRegex(Refusal, "nft-"):
                    isolation.rules_readback(exprs, source, family)

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

    @staticmethod
    def build_policy_nft_rows(family, table_name, table_handle_id, chains):
        rows = [
            {"metainfo": {"version": "1.0.9", "json_schema_version": 1}},
            {"table": {"family": family, "name": table_name, "handle": table_handle_id}},
        ]
        rule_handle = table_handle_id * 100
        for chain_name, (hook, body) in chains.items():
            rows.append({
                "chain": {
                    "family": family, "table": table_name, "name": chain_name,
                    "type": "filter", "hook": hook, "prio": -310, "policy": "accept",
                }
            })
            for label, source, verdict in body:
                exprs = []
                tokens = source.split()
                idx = 0
                emitted_l3 = False
                emitted_l4 = False
                while idx < len(tokens):
                    head = tokens[idx]
                    if head in ("iifname", "oifname"):
                        exprs.append({"match": {"op": "==", "left": {"meta": {"key": head}},
                                                "right": tokens[idx + 1].strip('"')}})
                        idx += 2
                    elif head == "ct":
                        exprs.append({"match": {"op": "in", "left": {"ct": {"key": tokens[idx + 1]}},
                                                "right": tokens[idx + 2]}})
                        idx += 3
                    elif head == "arp":
                        if family == "bridge" and not emitted_l3:
                            exprs.append({"match": {"op": "==", "left": {"payload": {"protocol": "ether", "field": "type"}},
                                                    "right": "arp"}})
                            emitted_l3 = True
                        exprs.append({"match": {"op": "==", "left": {"payload": {"protocol": "arp",
                                                "field": f"{tokens[idx + 1]} {tokens[idx + 2]}"}},
                                                "right": tokens[idx + 3]}})
                        idx += 4
                    elif head in ("ip", "ip6"):
                        if not emitted_l3:
                            if family == "bridge":
                                exprs.append({"match": {"op": "==", "left": {"payload": {"protocol": "ether", "field": "type"}},
                                                        "right": head}})
                            else:
                                exprs.append({"match": {"op": "==", "left": {"meta": {"key": "nfproto"}},
                                                        "right": "ipv4" if head == "ip" else "ipv6"}})
                            emitted_l3 = True
                        exprs.append({"match": {"op": "==", "left": {"payload": {"protocol": head, "field": tokens[idx + 1]}},
                                                "right": tokens[idx + 2]}})
                        idx += 3
                    elif head in ("tcp", "udp"):
                        if not emitted_l4:
                            exprs.append({"match": {"op": "==", "left": {"meta": {"key": "l4proto"}}, "right": head}})
                            emitted_l4 = True
                        exprs.append({"match": {"op": "==", "left": {"payload": {"protocol": head, "field": tokens[idx + 1]}},
                                                "right": int(tokens[idx + 2])}})
                        idx += 3
                    else:
                        exprs.append({"match": {"op": "==", "left": {"payload": {"protocol": head, "field": tokens[idx + 1]}},
                                                "right": tokens[idx + 2]}})
                        idx += 3
                rule_handle += 1
                rows.append({
                    "rule": {
                        "family": family, "table": table_name, "chain": chain_name,
                        "handle": rule_handle, "comment": f"r2-{label}",
                        "expr": [*exprs, {"counter": {"packets": 0, "bytes": 0}}, {verdict: None}],
                    }
                })
        return rows

    def test_committed_unobserved_link_netns_and_nft_cleanup_with_cleanup_reserve(self):
        # 1a. Committed-but-unobserved bridge: ip link add commits, first readback fails, cleanup recovers with reserve.
        resources = self.make_resources()
        live_links = {}
        deleted_links = []
        fail_first_readback = [True]

        def fake_ip_link(end, *args, **kwargs):
            if args[:2] == ("link", "add"):
                name = args[3]
                live_links[name] = {
                    "ifname": name, "ifindex": 201, "ifalias": "",
                    "address": NETWORK["local"][1], "linkinfo": {"info_kind": "bridge"},
                }
                return ""
            if args[:3] == ("-j", "-d", "link"):
                name = args[-1]
                if fail_first_readback[0]:
                    fail_first_readback[0] = False
                    raise Refusal("ip-phase-deadline")
                if name not in live_links:
                    return f'Device "{name}" does not exist.\n'
                return json.dumps([live_links[name]])
            if args[:2] == ("link", "del"):
                name = args[3]
                deleted_links.append(name)
                live_links.pop(name, None)
                return ""
            return ""

        with patch.object(resources, "ip", side_effect=fake_ip_link):
            with self.assertRaisesRegex(Refusal, "ip-phase-deadline"):
                resources.add_link(resources.bridge, 0, "type", "bridge", kind="bridge", mac=NETWORK["local"][1])
            self.assertEqual(resources.cleanup(10), [])
            self.assertEqual(deleted_links, [resources.bridge])
            self.assertEqual(live_links, {})

        # Unobserved bridge with unprovable attribution (wrong kind or foreign alias) must be refused.
        for bad_row in (
            {"ifname": resources.bridge, "ifindex": 202, "address": NETWORK["local"][1],
             "linkinfo": {"info_kind": "dummy"}},
            {"ifname": resources.bridge, "ifindex": 202, "address": NETWORK["local"][1],
             "linkinfo": {"info_kind": "bridge"}, "ifalias": "foreign_table"},
        ):
            with self.subTest(bad_link=bad_row):
                del_calls = []
                with patch.object(resources, "links_at", return_value=[bad_row]), \
                     patch.object(resources, "ip", side_effect=lambda *a, **kw: del_calls.append(a)):
                    with self.assertRaisesRegex(Refusal, "link-attribution"):
                        resources.remove_link(resources.bridge, None, 10, tagged=False,
                                              expected_kind="bridge", expected_mac=NETWORK["local"][1])
                    self.assertEqual(del_calls, [])

        # 1b. Committed-but-unobserved veth: positive recovery requires host nsid + reciprocal peer attribution in attributed netns.
        resources = self.make_resources()
        veth_name = resources.links["a"]
        ns_name = resources.namespaces["a"]
        ns_path = Path("/run/netns") / ns_name
        ns_id = (7, 901)
        veth_live = {}
        peer_live = {}
        veth_deleted = []
        fail_veth_readback = [True]

        def fake_ip_veth(_end, *args, **kwargs):
            if args[:2] == ("link", "add"):
                veth_live[veth_name] = {
                    "ifname": veth_name, "ifindex": 301, "link_index": 2, "link_netnsid": 7,
                    "ifalias": "", "linkinfo": {"info_kind": "veth"},
                }
                peer_live["eth0"] = {
                    "ifname": "eth0", "ifindex": 2, "link_index": 301, "link_netnsid": 0,
                    "linkinfo": {"info_kind": "veth"},
                }
                return ""
            if args == ("-j", "netns", "list-id"):
                return json.dumps([{"nsid": 7, "name": ns_name}])
            if args[:3] == ("-j", "-d", "link"):
                if fail_veth_readback[0]:
                    fail_veth_readback[0] = False
                    raise Refusal("ip-phase-deadline")
                name = args[-1]
                if name not in veth_live:
                    return f'Device "{name}" does not exist.\n'
                return json.dumps([veth_live[name]])
            if args[:2] == ("-n", ns_name) and args[2:5] == ("-j", "-d", "link"):
                name = args[-1]
                if name not in peer_live:
                    return f'Device "{name}" does not exist.\n'
                return json.dumps([peer_live[name]])
            if args[:2] == ("link", "del"):
                veth_deleted.append(args[3])
                veth_live.pop(args[3], None)
                peer_live.pop("eth0", None)
                return ""
            return ""

        with patch.object(resources, "ip", side_effect=fake_ip_veth), \
             patch.object(Path, "exists", lambda p: p == ns_path), \
             patch.object(Path, "stat", lambda p: SimpleNamespace(st_dev=ns_id[0], st_ino=ns_id[1])):
            with self.assertRaisesRegex(Refusal, "ip-phase-deadline"):
                resources.add_link(veth_name, 0, "type", "veth", "peer", "name", "eth0", "netns", ns_name,
                                   kind="veth", peer_node="a", peer_ns_identity=ns_id)
            self.assertEqual(resources.cleanup(10), [])
            self.assertEqual(veth_deleted, [veth_name])
            self.assertEqual(veth_live, {})

        # Unobserved unrelated same-name veth with wrong peer namespace, missing/ambiguous nsid, or mismatched peer
        # attribution must NOT be deleted.
        base_host_veth = {
            "ifname": veth_name, "ifindex": 301, "link_index": 2, "link_netnsid": 7,
            "ifalias": "", "linkinfo": {"info_kind": "veth"},
        }
        matching_peer = [{
            "ifname": "eth0", "ifindex": 2, "link_index": 301, "link_netnsid": 0,
            "linkinfo": {"info_kind": "veth"},
        }]
        good_nsid_rows = [{"nsid": 7, "name": ns_name}]
        for label, host_row, peer_rows, ns_stat_pair, nsid_rows in (
            # Exact counterexample: reciprocal indices match (301 <-> 2), but host veth points to nsid 9 (netns B)
            # while attributed netns A has nsid 7.
            ("wrong-peer-netnsid", {**base_host_veth, "link_netnsid": 9}, matching_peer, ns_id,
             [{"nsid": 7, "name": ns_name}, {"nsid": 9, "name": "other-b"}]),
            ("missing-nsid-empty", base_host_veth, matching_peer, ns_id, []),
            ("missing-nsid-other", base_host_veth, matching_peer, ns_id, [{"nsid": 9, "name": "other-b"}]),
            ("ambiguous-duplicate-nsid", base_host_veth, matching_peer, ns_id,
             [{"nsid": 7, "name": ns_name}, {"nsid": 7, "name": "other-b"}]),
            ("ambiguous-duplicate-name", base_host_veth, matching_peer, ns_id,
             [{"nsid": 7, "name": ns_name}, {"nsid": 8, "name": ns_name}]),
            ("missing-peer", base_host_veth, [], ns_id, good_nsid_rows),
            ("mismatched-peer-link-index", base_host_veth, [{
                "ifname": "eth0", "ifindex": 2, "link_index": 999, "link_netnsid": 0,
                "linkinfo": {"info_kind": "veth"},
            }], ns_id, good_nsid_rows),
            ("mismatched-peer-ifindex", base_host_veth, [{
                "ifname": "eth0", "ifindex": 5, "link_index": 301, "link_netnsid": 0,
                "linkinfo": {"info_kind": "veth"},
            }], ns_id, good_nsid_rows),
            ("same-netns-no-link-netnsid", {
                "ifname": veth_name, "ifindex": 301, "link_index": 2, "ifalias": "",
                "linkinfo": {"info_kind": "veth"},
            }, matching_peer, ns_id, good_nsid_rows),
            ("substituted-peer-netns", base_host_veth, matching_peer, (7, 999), good_nsid_rows),
        ):
            with self.subTest(bad_veth=label):
                del_calls = []

                def fake_bad_ip(_end, *args, rows=nsid_rows, **kwargs):
                    if args == ("-j", "netns", "list-id"):
                        return json.dumps(rows)
                    if args[:2] == ("link", "del"):
                        del_calls.append(args)
                        return ""
                    raise AssertionError(f"unexpected ip call: {args}")

                with patch.object(resources, "links_at", return_value=[host_row]), \
                     patch.object(resources, "ns_links_at", return_value=peer_rows), \
                     patch.object(Path, "exists", lambda p: p == ns_path), \
                     patch.object(Path, "stat", lambda p: SimpleNamespace(st_dev=ns_stat_pair[0], st_ino=ns_stat_pair[1])), \
                     patch.object(resources, "ip", side_effect=fake_bad_ip):
                    with self.assertRaisesRegex(Refusal, "link-attribution"):
                        resources.remove_link(veth_name, None, 10, tagged=False,
                                              expected_kind="veth", peer_node="a", peer_ns_identity=ns_id)
                    self.assertEqual(del_calls, [])

        # 2. Committed-but-unobserved namespace: netns add commits then times out before stat(); cleanup recovers.
        resources = self.make_resources()
        ns_name = resources.namespaces["a"]
        ns_path = Path("/run/netns") / ns_name
        ns_live = [False]
        ns_deleted = []

        def fake_ip_ns(_end, *args, **kwargs):
            if args[:2] == ("netns", "add"):
                ns_live[0] = True
                raise Refusal("ip-command-deadline")
            if args[:2] == ("netns", "list"):
                return f"{ns_name}\n" if ns_live[0] else ""
            if args[:2] == ("netns", "pids"):
                return ""
            if args[:2] == ("netns", "del"):
                ns_deleted.append(args[2])
                ns_live[0] = False
                return ""
            return ""

        nsfs_line = f"55 21 0:4 / {ns_path.as_posix()} rw - nsfs nsfs rw\n".encode("ascii")
        with patch.object(resources, "ip", side_effect=fake_ip_ns), \
             patch.object(Path, "exists", lambda p: ns_live[0] if p == ns_path else False), \
             patch.object(Path, "lstat", lambda p: SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o444)), \
             patch.object(isolation, "bounded_read", return_value=nsfs_line):
            with self.assertRaisesRegex(Refusal, "ip-command-deadline"):
                resources.add_namespace("a", 0)
            self.assertEqual(resources.cleanup(10), [])
            self.assertEqual(ns_deleted, [ns_name])

        # Unobserved namespace that is a symlink or non-nsfs mount must refuse deletion.
        with patch.object(Path, "exists", return_value=True), \
             patch.object(Path, "lstat", return_value=SimpleNamespace(st_uid=0, st_mode=stat.S_IFLNK | 0o777)), \
             patch.object(resources, "ip") as mocked_ip:
            with self.assertRaisesRegex(Refusal, "netns-attribution"):
                resources.remove_namespace("a", None, 10, allow_unobserved=True)
            mocked_ip.assert_not_called()

        # 3. Committed-but-unobserved nft tables: nft -f commits both complete two-chain tables, handle readback hits
        # expired setup deadline; cleanup uses fresh cleanup deadline, verifies exact complete policy, and deletes both.
        resources = self.make_resources()
        resources.remember("link", lambda _end: None)
        expected_graph = isolation.policy(resources.bridge, resources.links)
        live_tables = {}
        deleted_tables = []
        setup_end = 100
        cleanup_end = 200

        def fake_nft(end, *args):
            if args[0] == "-f":
                for fam, h in (("bridge", 31), ("inet", 32)):
                    live_tables[fam] = self.build_policy_nft_rows(fam, resources.table, h, expected_graph[fam])
                return ""
            if args[:2] == ("-j", "-a"):
                if end == setup_end:
                    raise Refusal("nft-phase-deadline")
                return json.dumps({"nftables": live_tables[args[4]]})
            if args == ("list", "tables"):
                return "\n".join(f"table {fam} {resources.table}" for fam in live_tables)
            if args[:2] == ("delete", "table"):
                deleted_tables.append(args[2])
                live_tables.pop(args[2], None)
                return ""
            return ""

        with patch.object(Path, "write_text", return_value=0), \
             patch.object(resources, "nft", side_effect=fake_nft), \
             patch.object(resources, "links_at", return_value=[]):
            with self.assertRaisesRegex(Refusal, "nft-phase-deadline"):
                resources.apply_policy(setup_end)
            self.assertEqual(resources.cleanup(cleanup_end), [])
            self.assertEqual(set(deleted_tables), {"bridge", "inet"})
            self.assertEqual(live_tables, {})

        # Table-only, incomplete-chain, arbitrary-empty-expr, and alien-chain unobserved tables must NOT be deleted.
        full_bridge = self.build_policy_nft_rows("bridge", resources.table, 41, expected_graph["bridge"])
        only_forward = [row for row in full_bridge
                        if not ("chain" in row and row["chain"]["name"] == "r2_input")
                        and not ("rule" in row and row["rule"]["chain"] == "r2_input")]
        empty_expr_table = [
            {"table": {"family": "bridge", "name": resources.table, "handle": 41}},
            {"chain": {"family": "bridge", "table": resources.table, "name": "r2_forward",
                       "type": "filter", "hook": "forward", "prio": -310, "policy": "accept"}},
            {"rule": {"family": "bridge", "table": resources.table, "chain": "r2_forward",
                      "handle": 4101, "comment": "r2-peer", "expr": []}},
        ]
        for label, bad_rows in (
            ("table-only", [{"table": {"family": "bridge", "name": resources.table, "handle": 41}}]),
            ("incomplete-one-chain", only_forward),
            ("missing-last-rule", full_bridge[:-1]),
            ("empty-expr-r2-comment", empty_expr_table),
            ("alien-chain", [
                {"table": {"family": "bridge", "name": resources.table, "handle": 41}},
                {"chain": {"family": "bridge", "table": resources.table, "name": "alien_chain",
                           "type": "filter", "hook": "forward", "prio": -310, "policy": "accept"}},
            ]),
        ):
            with self.subTest(bad_nft=label):
                nft_calls = []

                def fake_bad_nft(_end, *args, rows=bad_rows):
                    nft_calls.append(args)
                    if args == ("list", "tables"):
                        return f"table bridge {resources.table}\n"
                    if args[:2] == ("-j", "-a"):
                        return json.dumps({"nftables": rows})
                    raise AssertionError(f"unexpected nft delete: {args}")

                with patch.object(resources, "nft", side_effect=fake_bad_nft), \
                     patch.object(resources, "links_at", return_value=[]):
                    with self.assertRaisesRegex(Refusal, "nft-"):
                        resources.remove_table("bridge", None, cleanup_end, expected_policy=expected_graph["bridge"])
                    self.assertFalse(any(call[:2] == ("delete", "table") for call in nft_calls))

    def test_committed_unobserved_mount_recovery_and_backing_mismatch_refusal(self):
        import fixture
        size = 256 * MIB
        orig_dev = (8 << 8) | 1
        orig_ino = 71001
        setup_end = 100
        cleanup_end = 200

        def run_case(*, tamper_backing_before_cleanup):
            resources = self.make_resources()
            image = resources.root / "a.ext4"
            mount = resources.root / "a"
            img_stat = SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_uid=0, st_nlink=1,
                                       st_size=size, st_blocks=size // 512, st_dev=orig_dev, st_ino=orig_ino)
            mounted = [False]
            attached = [False]
            current_back_ino = [orig_ino]
            umount_calls = []
            detach_calls = []

            def fake_capture(argv, end, **kwargs):
                exe = argv[0]
                if exe == TOOLS["mkfs"]:
                    return b""
                if exe == TOOLS["loop"] and "-J" in argv:
                    rows = []
                    if attached[0]:
                        rows.append({
                            "name": "/dev/loop2", "back-file": image.as_posix(),
                            "back-ino": current_back_ino[0], "back-maj:min": isolation.dev_majmin(orig_dev),
                            "ro": False, "offset": 0, "sizelimit": 0,
                        })
                    return json.dumps({"loopdevices": rows}).encode("ascii")
                if exe == TOOLS["loop"] and "--find" in argv:
                    attached[0] = True
                    return b"/dev/loop2\n"
                if exe == TOOLS["loop"] and "-d" in argv:
                    detach_calls.append((end, argv))
                    attached[0] = False
                    return b""
                if exe == TOOLS["mount"]:
                    mounted[0] = True
                    return b""
                if exe == TOOLS["findmnt"]:
                    if mounted[0] and end == setup_end:
                        raise Refusal("phase-deadline")
                    if mounted[0]:
                        return f"188 {mount.as_posix()} /dev/loop2 ext4 /\n".encode("ascii")
                    return b""
                if exe == TOOLS["umount"]:
                    umount_calls.append((end, argv))
                    mounted[0] = False
                    return b""
                return b""

            def fake_bounded_read(path, _limit):
                if path.name == "mountinfo":
                    if mounted[0]:
                        return f"188 29 7:2 / {mount.as_posix()} rw,nosuid - ext4 /dev/loop2 rw\n".encode("ascii")
                    return b""
                if path.name == "size":
                    return f"{size // 512}\n".encode("ascii")
                raise AssertionError(path)

            with patch.object(fixture, "capture", side_effect=fake_capture), \
                 patch.object(isolation, "allocate"), \
                 patch.object(isolation, "private_directory"), \
                 patch.object(isolation, "bounded_read", side_effect=fake_bounded_read), \
                 patch.object(Path, "lstat", return_value=img_stat):
                with self.assertRaisesRegex(Refusal, "findmnt-phase-deadline"):
                    resources.volume("a", resources.root / "data-a", setup_end)
                self.assertTrue(mounted[0])
                self.assertEqual([label for label, _ in resources.cleanup_stack], ["loop-a", "mount-a"])
                if tamper_backing_before_cleanup:
                    current_back_ino[0] = orig_ino + 1
                errors = resources.cleanup(cleanup_end)
            return errors, umount_calls, detach_calls, mounted[0], attached[0]

        # Positive recovery: pre-registered mount callback verifies backing loop/image dev+ino under cleanup_end and umounts.
        errors, umounts, detaches, still_mounted, still_attached = run_case(tamper_backing_before_cleanup=False)
        self.assertEqual(errors, [])
        self.assertEqual(len(umounts), 1)
        self.assertEqual(umounts[0][0], cleanup_end)
        self.assertEqual(len(detaches), 1)
        self.assertEqual(detaches[0][0], cleanup_end)
        self.assertFalse(still_mounted)
        self.assertFalse(still_attached)

        # Negative refusal: backing mismatch before cleanup refuses umount and losetup -d through pre-registered callbacks.
        errors, umounts, detaches, still_mounted, still_attached = run_case(tamper_backing_before_cleanup=True)
        self.assertEqual(errors, ["mount-a", "loop-a"])
        self.assertEqual(umounts, [])
        self.assertEqual(detaches, [])
        self.assertTrue(still_mounted)
        self.assertTrue(still_attached)

    def test_mkfs_alias_to_mke2fs_passes_explicit_ext4_type(self):
        import fixture
        dir_ok = SimpleNamespace(st_uid=0, st_mode=stat.S_IFDIR | 0o755)
        reg_ok = SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o755)
        lnk_ok = SimpleNamespace(st_uid=0, st_mode=stat.S_IFLNK | 0o777)
        stats = {"/": dir_ok, "/usr": dir_ok, "/usr/sbin": dir_ok, "/usr/sbin/mkfs.ext4": lnk_ok, "/usr/sbin/mke2fs": reg_ok}
        canonical_mkfs = fixture.vet_tool("/usr/sbin/mkfs.ext4", lstat=stats.__getitem__,
                                          readlink=lambda p: "mke2fs")
        self.assertEqual(canonical_mkfs, "/usr/sbin/mke2fs")

        for vol_name, size in (("root", 1024 * MIB), ("a", 256 * MIB)):
            with self.subTest(volume=vol_name):
                resources = isolation.Resources(
                    {"repo": "org/repo", "head": "a"*40, "run": "1", "attempt": "1"},
                    load_pins(),
                    {**TOOLS, "mkfs": canonical_mkfs},
                )
                captured_argvs = []
                orig_dev = (8 << 8) | 1
                orig_ino = 70001
                image = resources.root / f"{vol_name}.ext4"
                mount = resources.root / vol_name
                img_stat = SimpleNamespace(st_mode=stat.S_IFREG | (0o400 if vol_name == "root" else 0o600),
                                           st_uid=0, st_nlink=1, st_size=size, st_blocks=size // 512,
                                           st_dev=orig_dev, st_ino=orig_ino)
                mounted = [False]
                attached = [False]

                def fake_capture(argv, _end, **kwargs):
                    captured_argvs.append(argv)
                    exe = argv[0]
                    if exe == "/usr/sbin/mke2fs":
                        return b""
                    if exe == TOOLS["loop"] and "-J" in argv:
                        rows = []
                        if attached[0]:
                            rows.append({
                                "name": "/dev/loop2", "back-file": image.as_posix(),
                                "back-ino": orig_ino, "back-maj:min": isolation.dev_majmin(orig_dev),
                                "ro": vol_name == "root", "offset": 0, "sizelimit": 0,
                            })
                        return json.dumps({"loopdevices": rows}).encode("ascii")
                    if exe == TOOLS["loop"] and "--find" in argv:
                        attached[0] = True
                        return b"/dev/loop2\n"
                    if exe == TOOLS["findmnt"]:
                        if mounted[0]:
                            return f"188 {mount.as_posix()} /dev/loop2 ext4 /\n".encode("ascii")
                        return b""
                    if exe == TOOLS["mount"]:
                        mounted[0] = True
                        return b""
                    return b""

                def fake_bounded_read(path, _limit):
                    if path.name == "mountinfo":
                        if mounted[0]:
                            return f"188 29 7:2 / {mount.as_posix()} ro,nosuid - ext4 /dev/loop2 ro\n".encode("ascii")
                        return b""
                    if path.name == "size":
                        return f"{size // 512}\n".encode("ascii")
                    raise AssertionError(path)

                fake_vfs = SimpleNamespace(
                    f_flag=(getattr(isolation.os, "ST_RDONLY", 1) if vol_name == "root" else 0),
                    f_frsize=4096, f_bavail=60000, f_blocks=63000, f_favail=16380, f_files=16384,
                )
                with patch.object(fixture, "capture", side_effect=fake_capture), \
                     patch.object(isolation, "allocate"), \
                     patch.object(isolation, "private_directory"), \
                     patch.object(isolation, "bounded_read", side_effect=fake_bounded_read), \
                     patch.object(Path, "chmod"), \
                     patch.object(Path, "lstat", return_value=img_stat), \
                     patch.object(isolation.os, "ST_RDONLY", 1, create=True), \
                     patch.object(isolation.os, "statvfs", return_value=fake_vfs, create=True):
                    resources.volume(vol_name, resources.root / "tree", 10)

                mkfs_calls = [argv for argv in captured_argvs if argv[0] == "/usr/sbin/mke2fs"]
                self.assertEqual(len(mkfs_calls), 1)
                self.assertEqual(mkfs_calls[0][1:3], ["-t", "ext4"])
