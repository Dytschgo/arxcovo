# Onboarding the Mac mini and the Linux host (owner runbook)

Status: 2026-11-27, written against main `4eb24e29` plus the launchd parser in
this change. It covers the two hosts arriving around 2026-12-01: an Apple
Silicon **Mac mini (macOS)** and an **x86_64 Linux host (Ubuntu or Debian with
systemd 254 or newer)**. Every command was checked against the source named
next to it. None of them has been run on either machine yet: real Mac mini and
Linux host onboarding is **unverified on real hardware**. Record each real
result in docs/PROGRESS.md.

Steps marked **(admin)** need `sudo`. Everything else runs as your normal
login. Never paste a password, token, join envelope or client file into chat,
an issue, a command line or a log. Replace `<...>` placeholders with your own
local values; do not commit them.

## What works today and what does not

| Capability | Linux host | Mac mini | Source |
| --- | --- | --- | --- |
| Binary layout and `service install/start/stop/status/uninstall` | Implemented and unit tested | Implemented and unit tested; launchd absence pinned from hosted macOS evidence, loaded-job grammar synthetic | `service/owner_plan.rs`, `owner_unix.rs`, `owner_query.rs` |
| `service configure` and `client-config new` | Implemented | Implemented; standard rights only | `service/owner_cli.rs`, `config.rs` |
| Local MCP (`arxd mcp-stdio`) to the installed service | Implemented | Implemented | `main.rs` |
| Agent commands as the dedicated build account (installed service) | Not available: the installed executor is inert and reports `execution.available: false` | Not available: no macOS UID-sweep executor; post-v1 | `service/executor.rs` (`Execution::configured`) |
| Cluster join (join code, cluster CA, mutual TLS, Raft) | Implemented in source for Linux; foreground and supervised worker; container-tested only | **Not available**: the cluster runtime is Linux-only, and macOS refuses a `cluster` service setting | `service/cluster_runtime.rs`, `cluster/consensus/bound_start.rs`, `service/config.rs` |
| Owner CLI that writes the cluster installation file or the service `cluster` setting | **Missing**; see Linux step 6 | n/a | `service/config.rs` (`from_request` writes `cluster: None`) |
| Test VMs (QEMU/KVM on Linux, HVF on Mac) | M4 is Windows/WHPX first; Linux KVM path unverified | QEMU HVF and Apple Virtualization.framework guests: post-v1 | KICKOFF, ROADMAP |
| Installed boot behaviour, real launchd/systemd output on these machines | **unverified on real hardware** | **unverified on real hardware** | ADR 0014 |

So the first useful milestone on each machine is: the service installs, starts,
reports `running`, and Claude Code reaches it through `mcp-stdio` with
`list_hosts`. Agent command execution and multi-host joining follow later.

## Before you start (both machines)

1. Download or build the `arxd` binary for the target from a reviewed main
   commit. Note the commit hash; you will record it with the result.
2. Decide the build account name. This guide uses `arxcovo`, matching
   [service-install.md](service-install.md). It must be a standard account,
   never an administrator, and it must have no sudo rule.
3. Check reachability between the new host and an existing host. Over Tailscale
   (if both machines are in your tailnet):

   ```sh
   tailscale status
   tailscale ping <other-host-name>
   ```

   Over the LAN, use `ping <other-host-lan-address>`. Cluster peer and
   enrollment ports come from the private cluster file (Linux step 6); once
   they are chosen, check them with `nc -vz <other-host-address> <port>`.
   Do not open those ports to the internet.

## Linux host (x86_64, systemd 254 or newer)

Fixed paths (`service/layout.rs`, `owner_query.rs`):

| Item | Path |
| --- | --- |
| Binary | `/usr/local/libexec/arxcovo/arxd`, `root:root 0755` |
| Unit | `/etc/systemd/system/arxd.service` |
| Standard-rights drop-in | `/etc/systemd/system/arxd.service.d/50-arxcovo-standard-rights.conf` |
| Config | `/etc/arxcovo/arxd.json` (private, root) |
| State | `/var/lib/arxcovo` |
| Local endpoint | `/run/arxcovo/arxd.sock` (systemd creates `/run/arxcovo` at each start) |

1. Confirm systemd is new enough: `systemctl --version` must show 254 or later.
2. **(admin)** Create the build account and its workspace (home outside
   `/home`, because the drop-in hides `/home` from builds):

   ```sh
   sudo useradd --system --create-home --home-dir /var/lib/arxcovo-build --shell /usr/sbin/nologin arxcovo
   sudo -u arxcovo mkdir /var/lib/arxcovo-build/work
   ```

3. **(admin)** Install the binary and register the service. Install never
   starts the daemon and leaves it off at boot.

   ```sh
   sudo install -o root -g root -m 0755 -D arxd /usr/local/libexec/arxcovo/arxd
   sudo /usr/local/libexec/arxcovo/arxd service install
   ```

4. As yourself, **without sudo**, create the private client file. It prints
   `claude-code=<hash>`; the hash is not secret, the file is.

   ```sh
   /usr/local/libexec/arxcovo/arxd client-config new --principal claude-code --out "$HOME/.arxcovo/client.json"
   ```

5. **(admin)** Configure, start and check:

   ```sh
   sudo /usr/local/libexec/arxcovo/arxd service configure --build-account arxcovo --client-account <your-login> --rights standard --workspace /var/lib/arxcovo-build/work --env PATH=/usr/local/bin:/usr/bin:/bin --env HOME=/var/lib/arxcovo-build --agent claude-code=<hash>
   sudo /usr/local/libexec/arxcovo/arxd service start
   sudo /usr/local/libexec/arxcovo/arxd service status
   ```

   `status` prints one token: `running`, `starting`, `stopping`, `stopped`,
   `failed` or `not-installed`. Errors print `arxd: <token>` with a fixed exit
   code; see [service-install.md](service-install.md). Add `--env
   CARGO_HOME=...` and `--env RUSTUP_HOME=...` once tools are installed for
   `arxcovo`.

6. **Cluster join (not owner-ready yet).** The Linux cluster runtime exists in
   source with these exact entry points:

   - Foreground daemon: `arxd --cluster-config <FILE> --cluster-intent <INTENT>`
     with `INTENT` one of `create-founder`, `join`, `recover-founder`,
     `recover-joiner`, plus optional `--inherit-listeners`
     (`service/cluster_runtime.rs` `entry`; ADR 0047).
   - Join code: on an existing member, the operator account runs
     `arxd service cluster-control mint --socket <SOCKET> --daemon-uid <UID>`.
     The join envelope appears only on that controlling terminal.
   - Paste: on the new host, with its daemon started with intent `join`, the
     operator runs `arxd service cluster-control join --socket <SOCKET> --daemon-uid <UID>`
     and pastes the envelope at the hidden prompt, then presses Enter. The
     envelope carries the cluster CA pin and checksum; the new node gets its
     mutual-TLS identity and enters Raft through admission (ADR 0040, 0042,
     0046).

   `<SOCKET>` is the `owner_socket` path in the cluster file: absolute, at
   most 100 bytes, only `A-Z a-z 0-9 / _ - .`, and not directly under `/`.
   `<UID>` is the numeric UID of the local `arxd-cluster` daemon account.

   What is missing before you can do this on the new host: there is no owner
   command that writes the private cluster installation file (`workspace`,
   `state`, `credentials`, `wrap`, `custody`, `history`, `owner_socket`,
   `peer`, `enrollment` and the owner, daemon, agent and build UIDs/GIDs in
   `service/cluster_runtime.rs` `Settings`), creates the `arxd-cluster` and
   operator accounts and control group, or adds the `cluster` launch setting
   to the service config. `service configure` writes the config without that
   setting. Do not hand-edit `/etc/arxcovo/arxd.json`. Joining has only been
   exercised in the three-container CI acceptance; installed multi-host join is
   **unverified on real hardware**.

## Mac mini (Apple Silicon, macOS)

Fixed paths (`service/layout.rs`, `owner_query.rs`, ADR 0015):

| Item | Path |
| --- | --- |
| Binary | `/Library/Application Support/Arxcovo/libexec/arxd`, `root:wheel 0755` |
| Plist | `/Library/LaunchDaemons/com.arxcovo.arxd.plist` (label `com.arxcovo.arxd`) |
| Config | `/Library/Application Support/Arxcovo/config/arxd.json` |
| State | `/Library/Application Support/Arxcovo/state` |
| Log | `/Library/Logs/Arxcovo/arxd.log` |
| Local endpoint | `/Library/Application Support/Arxcovo/run/arxd.sock` (the service creates `run`) |

1. **(admin)** Create a **standard** user `arxcovo` in System Settings › Users
   & Groups and a workspace folder under its home. Do not add it to `admin`
   or give it a sudoers rule; macOS supports standard rights only.
2. **(admin)** Install the binary:

   ```sh
   sudo install -d -o root -g wheel -m 0755 "/Library/Application Support/Arxcovo/libexec"
   sudo install -o root -g wheel -m 0755 arxd "/Library/Application Support/Arxcovo/libexec/arxd"
   ```

   `/Library/Application Support/Arxcovo` must stay `0755` so your login can
   reach the endpoint folder. If Gatekeeper quarantines a downloaded binary,
   inspect it before clearing the attribute yourself.
3. **(admin)** Register the daemon. Install first checks that the job is not
   loaded, runs `launchctl disable system/com.arxcovo.arxd`, then writes the
   plist, and finally confirms that `launchctl print` reports the job absent
   (`owner_plan.rs`). It does not load or start it.

   ```sh
   sudo "/Library/Application Support/Arxcovo/libexec/arxd" service install
   ```

4. As yourself, **without sudo**:

   ```sh
   "/Library/Application Support/Arxcovo/libexec/arxd" client-config new --principal claude-code --out "$HOME/.arxcovo/client.json"
   ```

5. **(admin)** Configure, start and check:

   ```sh
   sudo "/Library/Application Support/Arxcovo/libexec/arxd" service configure --build-account arxcovo --client-account <your-login> --rights standard --workspace <arxcovo-workspace> --env PATH=/usr/bin:/bin:/usr/sbin:/sbin --env HOME=<arxcovo-home> --agent claude-code=<hash>
   sudo "/Library/Application Support/Arxcovo/libexec/arxd" service start
   sudo "/Library/Application Support/Arxcovo/libexec/arxd" service status
   ```

   `start` runs `launchctl enable system/com.arxcovo.arxd` and then
   `launchctl bootstrap system /Library/LaunchDaemons/com.arxcovo.arxd.plist`
   when the job is not loaded. macOS status tokens: `not-installed`,
   `stopped`, `starting` (`spawn scheduled` or `starting`), `running`.
   `stopped` is ambiguous on purpose: it covers both a job that is not loaded
   while the matching plist is installed, and a loaded job whose launchd state
   is `not running` or `waiting`. The parser does not read launchd's last exit
   code, so `stopped` says nothing about why the daemon is not running.

   If `status` keeps printing `stopped` after `start`, the job is probably
   loaded but not running: `start` skips `bootstrap` for a loaded job. Run
   `service stop` and then `service start`, and check
   `/Library/Logs/Arxcovo/arxd.log` for why it exited.

   If `status` prints `arxd: service-manager` while the job is loaded, the real
   `launchctl print` output did not match the synthetic loaded grammar; status
   changes nothing. `stop` and `uninstall` re-query launchd at several points.
   A failure at the first query changes nothing, but a later failure can leave
   a partial stop: `disable` may already have run (the job will not start at
   boot), or `bootout` may already have stopped the job while the plist stays
   in place. Rerunning the same command is safe once the cause is fixed.
   Report the macOS version; the fix is a reviewed fixture, not a manual
   workaround.

   `arxd: service-foreign-registration` has two sources (`owner_unix.rs`
   `observe`): a file at `/Library/LaunchDaemons/com.arxcovo.arxd.plist` whose
   bytes differ from Arxcovo's plist, or a launchd job with this label whose
   plist path, program or arguments differ from Arxcovo's. `status` and
   `install` change nothing when they report it. `stop` and `uninstall` check
   again before `bootout` and after it (`owner_plan.rs`), so a foreign result
   there can come after `disable` has already run, or after `bootout` has
   already stopped the job; the plist is never removed in that case. Inspect
   the conflicting file or job yourself before rerunning.
6. Cluster join: not available on macOS in this version. The cluster runtime is
   Linux-only (`cluster/consensus/bound_start.rs`), `--cluster-config` exits
   with the config error on macOS, and the service refuses a `cluster`
   setting on macOS (`service/config.rs`). The Mac mini can run as a
   standalone host reached through its own `mcp-stdio` client.

## Connect Claude Code (both machines)

Register the stdio bridge in an unelevated Claude Code on that machine, using
the fixed binary path for the OS and quoting paths with spaces:

```text
claude mcp add --scope user arxcovo -- "<ABSOLUTE_ARXD_BINARY>" mcp-stdio --client-config "<PRIVATE_CLIENT_FILE>"
```

Then call `list_hosts` with `{}`. Expect host inventory (OS, architecture, CPUs,
RAM, disk) and `execution.available: false` with a fixed reason: dedicated
execution is not implemented for the installed service yet. Do not loosen file
modes to make the client connect.

## Stop, uninstall and upgrade

- `sudo <binary> service stop` stops the daemon and keeps it off at boot.
- `sudo <binary> service uninstall` disables, stops, proves the stop and then
  removes the unit or plist. It keeps the config, state, workspace and build
  account.
- Before replacing the binary with one whose unit or plist differs, run
  `service uninstall` with the **old** binary, then `service install` and
  `service start` with the new one; otherwise install refuses with
  `service-foreign-registration`.

## Evidence to record per machine

Record the binary commit, OS version, date, and for each of `install`,
`configure`, `start`, `status`, `stop`, `start` again and `uninstall`: the
exit code and the single status or error token. On the Mac mini, also record
whether `status` printed `running` (this is the first real check of the
loaded-job grammar). Never record raw `launchctl print` output, UIDs, account
names, IP addresses, client files or join envelopes.
