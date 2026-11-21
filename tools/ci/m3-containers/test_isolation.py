"""Pure R2 mount, process, policy, route and teardown controls."""

from copy import deepcopy
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from fixture import INNER, NETWORK, OUTER, Refusal, load_pins, filesystem_facts
import isolation


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
    def test_cleanup_keeps_trying_and_refuses_ambiguous_mount(self):
        resources = isolation.Resources({"repo": "org/repo", "head": "a"*40, "run": "1", "attempt": "1"}, load_pins())
        calls = []
        def fail(_end):
            calls.append("mount")
            raise Refusal("ambiguous")
        resources.remember("arena", lambda _: calls.append("arena"))
        resources.remember("mount", fail)
        resources.remember("unit", lambda _: calls.append("unit"))
        self.assertEqual(resources.cleanup(0), ["mount"])
        self.assertEqual(calls, ["unit", "mount", "arena"])
        with patch.object(resources, "mounts", return_value=["one", "two"]):
            with self.assertRaisesRegex(Refusal, "mount-identity"):
                resources.release_mount(Path("root"), "/dev/loop0", 0)
