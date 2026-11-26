# M3 three-container hosted pins

C16 runs on the existing Linux Rust worker after C14 using `python3 -I -B tools/ci/m3-containers/run.py --mode live`. The harness performs the R2 startup gates once and uses the same attributed bridge, units, sealed ext4 rootfs and per-node data images for the live campaign. The separate `--mode dry` entry retains the explicit zero-daemon qualification. It seals the daemon at mode 0400 and executes only the C bootstrap and compiler version commands.

## Host and inputs

| Input | Pin |
| --- | --- |
| Worker | GitHub-hosted x64 `ubuntu-24.04` runner, CI workflow, `rust (ubuntu-latest)` job |
| AWS profile | Ubuntu 24.04.5 LTS, `ubuntu24`, `20261116.133.1`, variant `7.0.0-1014-aws` / systemd `255.4-1ubuntu8.18` |
| Azure profile | Ubuntu 24.04.5 LTS, `ubuntu24`, `20261116.376.1`, variant `7.0.0-1017-azure` / systemd `255.4-1ubuntu8.18` (all 13 pinned tools verified true) |
| 20261122.23.1 profile | Ubuntu 24.04.5 LTS, `ubuntu24`, `20261122.23.1`, variants `7.0.0-1018-aws` / `255.4-1ubuntu8.19` (AWS) and `7.0.0-1020-azure` / `255.4-1ubuntu8.19` (Azure) |
| systemd package pin | `255.4-1ubuntu8.18` on the 20261116 profiles and `255.4-1ubuntu8.19` on `20261122.23.1` (both AWS and Azure variants), each verified by a direct hosted Ubuntu 24.04 receipt |
| Host arena | Attributed directory under /var/lib with fixed sudo -n entry |
| Tools | gcc, nft, ip, mkfs.ext4, losetup, systemd-run, systemctl, mount, umount, findmnt, ldd, dpkg-query and git at pinned system paths (raw symlink hops vetted before any root host command; canonical executables invoked) |
| Rust | The workflow-installed 1.95.0 x86_64-unknown-linux-gnu toolchain, using real Cargo and rustc binaries |
| Source receipt | Checked checkout SHA and workflow SHA/ref, repository, run and attempt; source bytes matched to git blobs |
| Daemon artifact receipt | Effective `CARGO_TARGET_DIR` (default `workspace/target`), SQLite-loader-checked `build-messages.jsonl` digest, Cargo link `arxd-build-messages.jsonl` (`compiler-artifact` `executable`) digest, content-bound `arxd-receipt.json` matching HEAD/run/attempt and ELF SHA-256, and loaded image readback |
| Reconnect helper receipt | Linux `arxd` library test obtained from one exact Cargo compiler-artifact under `debug/deps`. The receipt binds the ignored selector, HEAD, run, attempt, Cargo JSONL digest, ELF SHA-256 and size |

The profiles come from direct hosted Ubuntu 24.04 receipts (`20261116.133.1 / 7.0.0-1014-aws / 255.4-1ubuntu8.18` on AWS, `20261116.376.1 / 7.0.0-1017-azure / 255.4-1ubuntu8.18` on Azure with all 13 pinned tools verified true, `20261122.23.1 / 7.0.0-1018-aws / 255.4-1ubuntu8.19` on AWS, and `20261122.23.1 / 7.0.0-1020-azure / 255.4-1ubuntu8.19` on Azure). Each profile in `tools/ci/m3-containers/pins.json` is keyed by unique `(image_os, image_version)` and carries a closed `variants` list of exact `(kernel, systemd_package)` pairs with no regex, prefix or wildcard matching. The launcher vets all 13 tool paths before running `git`, `dpkg-query` or any other root command, selects by exact image identity (`runner-image` on unknown or duplicate images), selects by exact kernel match within that image's `variants` (`runner-kernel-os` on an unpinned kernel or duplicate variant), requires that matched variant's exact systemd package (`runner-systemd`), and records actual kernel, systemd and tool versions from the vetted executables. [AWS receipt](https://github.com/Dytschgo/arxcovo/actions/runs/44974576640/job/137061567775) · [Azure receipt](https://github.com/Dytschgo/arxcovo/actions/runs/44978108723/job/137072425820) · [20261122.23.1 AWS receipt](https://github.com/Dytschgo/arxcovo/actions/runs/45702491236/job/139515925302) · [20261122.23.1 Azure observation](https://github.com/Dytschgo/arxcovo/actions/runs/45797918583)

GitHub serves image version `20261122.23.1` ([ubuntu24/20261122.23 release](https://github.com/actions/runner-images/releases/tag/ubuntu24%2F20261122.23)) on both AWS (`7.0.0-1018-aws`) and Azure (`7.0.0-1020-azure`) hosts. Both exact variants are pinned under the single `20261122.23.1` profile; any other kernel for that image still fails closed with `runner-kernel-os`. The live C16 run on the `7.0.0-1020-azure` variant is **unverified on hosted runner** until CI exercises it.

The sealed fixture uses the pinned host's Ubuntu 24.04 userland. The entry refuses a host/userland mismatch with `offline-userland-input`. The fixture's allowlist copies shell, loader closure, compiler and linker inputs, Rust libraries and target stdlib before sealing.

## Local fixture

| Resource | Pin |
| --- | --- |
| Root image | 1 GiB preallocated ext4, mounted read-only, manifest and image SHA-256 |
| Data image | One preallocated 256 MiB ext4 loop per node, 4096-byte blocks, 16,384 inodes |
| Volatile filesystems | /run 4 MiB, /tmp 8 MiB, /dev 1 MiB, private devpts |
| Outer unit | 100% CPU quota, 512 MiB memory, zero swap, 128 PIDs and open files, zero core size |
| B broker | Bounding mask 0xc1, no-new-privileges, umask 0077; inner limit 128 MiB, 32 PIDs, zero swap |
| Role IDs | Owner 21070, daemon 21071, BUILD 21072, agent 21073 |
| Network | A 192.168.72.11, B 192.168.72.12, C 192.168.72.13 and probe-local 192.168.72.14 on /24 with exact pinned MACs. Preflight checks explicit IPv4 host routes for overlap. The bridge has no default route or DNS. |

The protected local account files use files-only NSS, expired logins and a locked password. The workload subjects in B run under their daemon and BUILD identities. The C14 ordinary slot lives on B's data filesystem with an immutable parent and private writable output. The two namespace generations exercise an exclusive inert Unix bind with fresh volatile /run and the same persistent marker inode. The network probes use only the three-endpoint fabric and temporary synthetic receivers. The 192.0.2.72 and 169.254.254.72 targets are fixture-owned numeric endpoints. IPv6 and TCP and UDP DNS controls are also synthetic.

## Dry evidence

| Phase | Ceiling |
| --- | ---: |
| Preflight | 8 s |
| Stage | 24 s |
| Containment | 16 s |
| Network | 12 s |
| Cleanup reserve | 25 s |

The single bounded JSON receipt reports phase durations, runner and tool provenance, source, artifact, manifest and root image digests, containment readbacks, allow and deny counters and cleanup confirmation. Cleanup failure fails qualification and preserves the earlier phase/category if present. The step and its time ceilings are **unverified on hosted runner**. Installed service, physical suspend and multi-host paths are **unverified on real hardware**.

## Live evidence

| Phase | Ceiling |
| --- | ---: |
| Provenance, image staging and the R2 startup gates | 60 s |
| Activation, consumed-code refusal and CA custody | 60 s |
| One B build and the pre-loss ledger checkpoint | 30 s |
| Attributed A leader loss, survivor writes and same-data recovery | 55 s |
| Redacted evidence and operation accounting | 10 s |
| Confirmed normal closes and scoped cleanup | 25 s |

B's grant pins A's public member digest. B's build uses the sealed wrapper and the C14 Cargo fixture on B's ext4 data image. The wrapper and host readbacks verify the BUILD role, cgroup, private-path refusals and artifact. Durable activity must correlate both sides to the one operation, with no local A execution.

The receipt contains the matched consumed-code refusal and membership position, the three independently verified R7 checkpoints, one intentional kill and three confirmed normal closes. Every phase has a separate time bound and the harness has a 240-second ceiling. It latches the first failure and emits a single bounded ASCII JSON receipt after cleanup. The live campaign and timings are **unverified on hosted runner**. Installed and physical paths are **unverified on real hardware**.
