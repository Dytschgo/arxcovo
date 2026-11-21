# M3 three-container hosted pins

R2 qualifies Proposal A's containment on the existing Linux Rust worker before any C16 campaign. The fixed invocation is `python3 -I -B tools/ci/m3-containers/run.py --mode dry`. It follows C14 and shares the ordinary loader-checked `target/debug/arxd` artifact. The dry entry inspects and seals the inert bytes at mode 0400. Only the compiled C bootstrap and the staged compiler version commands execute in the fixture.

## Host and inputs

| Input | Pin |
| --- | --- |
| Worker | GitHub-hosted x64 Linux, CI workflow, rust (ubuntu-latest) |
| Observed image | Ubuntu 26.04.1 LTS, ImageOS ubuntu26, ImageVersion 20261116.202.1 |
| Kernel and systemd | 7.0.0-1020-azure, systemd 259.9-0ubuntu0.1 |
| Host arena | Attributed directory under /var/lib with fixed sudo -n entry |
| Tools | gcc, nft, ip, mkfs.ext4, losetup, systemd-run, systemctl, mount, umount, findmnt, ldd, dpkg-query and git at pinned system paths |
| Rust | The workflow-installed 1.95.0 x86_64-unknown-linux-gnu toolchain, using real Cargo and rustc binaries |
| Source receipt | Checked checkout SHA and workflow SHA/ref, repository, run and attempt; source bytes matched to git blobs |
| Artifact receipt | Exact effective target path, ELF interpreter and SHA-256, and loaded image readback |

The [runner image release](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20261116.202) supplies historical host provenance. The launcher records the actual tool versions after validating the paths and the runner profile.

The approved design pins Ubuntu 24.04 inside the sealed fixture. The observed runner has Ubuntu 26.04 host-local tools and ELF libraries. The checked-in workflow supplies no digest-pinned local Ubuntu 24.04 userland. The entry refuses a host/userland mismatch with `offline-userland-input`. Qualifying the observed 26.04 userland or supplying an offline 24.04 input requires a pin decision. The fixture's allowlist copies shell, loader closure, compiler and linker inputs, Rust libraries and target stdlib before sealing.

## Local fixture

| Resource | Pin |
| --- | --- |
| Root image | 1 GiB preallocated ext4, mounted read-only, manifest and image SHA-256 |
| Data image | One preallocated 256 MiB ext4 loop per node, 4096-byte blocks, 16,384 inodes |
| Volatile filesystems | /run 4 MiB, /tmp 8 MiB, /dev 1 MiB, private devpts |
| Outer unit | 100% CPU quota, 512 MiB memory, zero swap, 128 PIDs and open files, zero core size |
| B broker | Bounding mask 0xc1, no-new-privileges, umask 0077; inner limit 128 MiB, 32 PIDs, zero swap |
| Role IDs | Owner 21070, daemon 21071, BUILD 21072, agent 21073 |
| Network | A 198.18.72.11, B 198.18.72.12, C 198.18.72.13 on /24 with exact pinned MACs. No default route or DNS. |

The protected local account files use files-only NSS, expired logins and a locked password. The workload subjects in B run under their daemon and BUILD identities. The C14 ordinary slot lives on B's data filesystem with an immutable parent and private writable output. The two namespace generations exercise an exclusive inert Unix bind with fresh volatile /run and the same persistent marker inode. The network probes use only the three-endpoint fabric and temporary synthetic receivers. The 192.0.2.72 and 169.254.254.72 targets are fixture-owned numeric endpoints. IPv6 and TCP and UDP DNS controls are also synthetic.

## Evidence

| Phase | Ceiling |
| --- | ---: |
| Preflight | 8 s |
| Stage | 24 s |
| Containment | 16 s |
| Network | 12 s |
| Cleanup reserve | 25 s |

The single bounded JSON receipt reports phase durations, runner and tool provenance, source, artifact, manifest and root image digests, containment readbacks, allow and deny counters and cleanup confirmation. Cleanup failure fails qualification and preserves the earlier phase/category if present. The step and its time ceilings are **unverified on hosted runner**. Installed service, physical suspend and multi-host paths are **unverified on real hardware**.
