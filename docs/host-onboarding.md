# Onboarding the Mac mini and the Linux host (owner runbook)

Status: 2026-11-27, written against main `4eb24e29` plus the launchd parser,
updated for the Linux cluster owner commands (`service cluster-config` and the
`service configure` cluster options). It covers the two hosts arriving around 2026-12-01: an Apple
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
| Owner CLI that writes the cluster installation file and the service `cluster` setting | Implemented and unit tested (`service cluster-config`, `service configure --cluster-config --cluster-intent`); the installed start is expected to hit the drop-in capability blocker in Linux step 6d | Refused (`cluster-config-unsupported`, `service-config`) | `service/cluster_install.rs`, `cluster_runtime/install.rs`, `config.rs` |
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
   Do not open those ports to the internet. A cluster join needs both Linux
   hosts on one LAN subnet; Tailscale addresses cannot satisfy the join route
   check (Linux step 6c).

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
   sudo useradd --create-home --home-dir /var/lib/arxcovo-build --shell /bin/sh arxcovo
   sudo -u arxcovo mkdir /var/lib/arxcovo-build/work
   ```

   Do not use `--system` or a `nologin` shell. The native account lookup
   (`service/native_unix.rs` `account`) accepts only UIDs of 1000 or more with
   a login shell, so `service configure` would refuse such an account with
   `service-identity`. The password stays locked; never add a sudo rule.

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

6. **Cluster setup (Linux only).** The cluster runtime is Linux-only, so the
   first Linux host creates the cluster (the *founder*). A later Linux host
   joins it (a *joiner*). The Mac mini and Windows refuse every cluster
   command and setting. Four distinct local accounts are involved (ADR 0046):

   | Role | Account in this guide | Rule enforced in source |
   | --- | --- | --- |
   | Daemon | `arxd-cluster` (fixed name) | own primary group, not in `sudo`/`wheel` (`Child::spawn`) |
   | Operator | `arxd-operator` | member of the control group `arxd-control` |
   | Agent (local MCP) | `<your-login>` | not in `arxd-control` |
   | Build | `arxcovo` | not in `arxd-control`, not an administrator |

   Every account needs a UID of 1000 or more and a login shell. The native
   account lookup (`service/native_unix.rs` `account`) refuses system accounts
   and `nologin`/`false` shells. `useradd` leaves the password locked unless
   you set one.

   a. **(admin)** Create the control group and the two cluster accounts:

      ```sh
      sudo groupadd arxd-control
      sudo useradd --user-group --create-home --shell /bin/sh arxd-cluster
      sudo useradd --user-group --create-home --shell /bin/bash --groups arxd-control arxd-operator
      ```

   b. **(admin)** Create the private layout. Keep it outside `/home`, because
      the service drop-in hides `/home`. Keep it outside `/etc/arxcovo` and
      `/var/lib/arxcovo`, because the cluster paths must stay disjoint from the
      service roots and from the build workspace.

      ```sh
      sudo install -d -o root -g root -m 0755 /etc/arxcovo-cluster /var/lib/arxcovo-cluster
      sudo install -d -o arxd-cluster -g arxd-cluster -m 0700 \
        /var/lib/arxcovo-cluster/workspace /var/lib/arxcovo-cluster/state \
        /var/lib/arxcovo-cluster/credentials /var/lib/arxcovo-cluster/wrap \
        /var/lib/arxcovo-cluster/history /var/lib/arxcovo-cluster/pki
      sudo install -d -o arxd-cluster -g arxd-cluster -m 0700 /var/lib/arxcovo-cluster/pki/custody
      sudo install -d -o arxd-cluster -g arxd-control -m 2710 /var/lib/arxcovo-cluster/control
      stat -c '%a %U:%G' /var/lib/arxcovo-cluster/control
      ```

      The last command must print `2710 arxd-cluster:arxd-control`. If the
      setgid bit is missing, run
      `sudo chmod 2710 /var/lib/arxcovo-cluster/control` and check again.
      Do not add POSIX ACLs to these directories.

   c. **(admin)** Write the private installation file. Pick a LAN address of
      this host and two free TCP ports, for example `7400` (peer) and `7401`
      (enrollment). On the founder:

      ```sh
      sudo /usr/local/libexec/arxcovo/arxd service cluster-config \
        --out /etc/arxcovo-cluster/cluster.json --role founder \
        --workspace /var/lib/arxcovo-cluster/workspace --state /var/lib/arxcovo-cluster/state \
        --credentials /var/lib/arxcovo-cluster/credentials --wrap /var/lib/arxcovo-cluster/wrap \
        --custody /var/lib/arxcovo-cluster/pki/custody --history /var/lib/arxcovo-cluster/history \
        --owner-socket /var/lib/arxcovo-cluster/control/owner.sock \
        --peer <this-host-lan-ip>:7400 --enrollment <this-host-lan-ip>:7401 \
        --operator-account arxd-operator --control-group arxd-control \
        --agent-account <your-login> --build-account arxcovo
      ```

      On a joiner, use `--role joiner` and add
      `--issuer-peer <founder-lan-ip>:7400` (the founder's peer address).
      The command prints `daemon-uid=<UID>`, the `arxd-cluster` UID used by
      `cluster-control` below. It never replaces an existing file: delete the
      old file yourself first if you must start over.

      What the command checks before it writes (`service/cluster_install.rs`,
      `service/cluster_runtime/install.rs`):
      - it runs as root, and resolves the four accounts and the group. The
        operator must be in `arxd-control`; the agent and build accounts must
        not be;
      - every path is absolute and canonical, and the paths do not overlap.
        `--owner-socket` is at most 100 bytes, uses only
        `A-Z a-z 0-9 / _ - .` and is not directly under `/`;
      - `--peer` and `--enrollment` use the same address and different ports.
        The address is a private IPv4 address, `100.64.0.0/10` or an IPv6
        unique-local address, and it is on an up interface of this host;
      - a joiner's issuer lies inside the subnet of that interface. Joining
        therefore needs both hosts on one LAN subnet. A Tailscale address is a
        `/32` interface, which cannot contain the founder's address;
      - the complete layout passes the runtime's own loader
        (`Settings::vetted`) with the installed service roots as ordinary
        roots: directory owners and modes as in step b, no symlinks, disjoint
        objects. A founder file gets a fresh random cluster ID.

      It then publishes the file atomically as `arxd-cluster`, mode `0600`.
      Refusals print `arxd: <token>` with a fixed exit code:
      `cluster-config-argument`, `cluster-config-account`,
      `cluster-config-address` and `cluster-config-layout` exit 3;
      `cluster-config-owner-required` exits 4; `cluster-config-exists` and
      `cluster-config-io` exit 1; `cluster-config-unsupported` exits 7 on
      macOS and Windows. A malformed command line prints `arxd: usage`
      (exit 2).

   d. **(admin)** Add the cluster launch setting. Repeat your step 5
      `service configure` command with two more options, then start:

      ```sh
      sudo /usr/local/libexec/arxcovo/arxd service configure <your step 5 options> \
        --cluster-config /etc/arxcovo-cluster/cluster.json --cluster-intent create-founder
      sudo /usr/local/libexec/arxcovo/arxd service start
      ```

      On a joiner, use `--cluster-intent join`. `configure` resolves
      `arxd-cluster` itself. Before it changes anything, it repeats the checks
      the service runs at every start: account, file, layout and separation
      from the service workspace. A refusal prints `service-config`. A later
      `configure` without the two options removes the setting. After the
      first successful start (founder) or join (joiner), reconfigure with
      `--cluster-intent recover-founder` or `recover-joiner` for every later
      restart. The create and join intents are for the first start only
      (`m3_activation_linux.rs` restarts with the recover intents).

      **Expected blocker, found by source reading and not yet run:** the
      standard-rights drop-in limits the root service to
      `CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_CHOWN`. Without
      `CAP_DAC_READ_SEARCH` the root parent can neither read the `0600` file
      owned by `arxd-cluster` nor search the `0700` custody parent. `service
      start` would then exit with `service-config` from `Child::spawn`.
      `configure` runs under `sudo` with full capabilities, so it does not
      detect this. The CI acceptance never started the cluster through the
      installed service. It runs the foreground daemon directly as the daemon
      account. Until a reviewed fix lands, use that foreground form on the
      host, in a terminal or `tmux`:

      ```sh
      sudo -u arxd-cluster /usr/local/libexec/arxcovo/arxd \
        --cluster-config /etc/arxcovo-cluster/cluster.json --cluster-intent create-founder
      ```

      Use the same intent rules. Stop it with Ctrl-C (SIGINT); ADR 0047
      covers the graceful close. Leave the service `cluster` setting out
      while you run the foreground form.

   e. **Mint and join.** On the founder, log in as the operator (for example
      `sudo -iu arxd-operator`) and run:

      ```sh
      /usr/local/libexec/arxcovo/arxd service cluster-control mint --socket /var/lib/arxcovo-cluster/control/owner.sock --daemon-uid <founder-daemon-uid>
      ```

      The join envelope appears only on that controlling terminal. On the
      joiner, with its daemon started with intent `join`, the operator runs
      `arxd service cluster-control join --socket /var/lib/arxcovo-cluster/control/owner.sock --daemon-uid <joiner-daemon-uid>`.
      Paste the envelope at the hidden prompt, then press Enter. The envelope
      carries the cluster CA pin and checksum. The new node gets its
      mutual-TLS identity and enters Raft through admission (ADR 0040, 0042,
      0046). Never paste the envelope into chat, an issue or a log.

   Joining has been exercised only in the three-container CI acceptance.
   Installed service activation, real Linux host onboarding and installed
   multi-host join are **unverified on real hardware**.

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
   `observe`). The first is an opened file at
   `/Library/LaunchDaemons/com.arxcovo.arxd.plist` whose bytes differ from
   Arxcovo's plist, or that fails the file-protection check even when its
   bytes match. The opened descriptor must be a regular file on a local
   filesystem with one hard link, owner and group `root:wheel`, mode `0644`,
   no ACL entries, and readable within the size limit. An opened file with
   a wrong mode such as `0600`, the wrong owner or group, or an opened
   non-regular descriptor reports `service-foreign-registration`. A path that
   fails `open` itself, other than a missing file (`ENOENT`), reports
   `service-permissions`, including a symlink (`O_NOFOLLOW`) and a
   Unix-domain socket. The second source is a launchd job with this label
   whose recognized `launchctl print` output (recognized structure and
   state, `owner_query.rs:396`) has a plist path, program, or arguments
   that differ from Arxcovo's. Unrecognized structure or state reports
   `service-manager` once the query is reached. Only `status` is
   strictly read-only. The other verbs check first and change nothing when
   that first check reports it, but they check again later (`owner_plan.rs`),
   and nothing is rolled back: `install` can report it at its final check,
   after it has created the private directories and state, run `disable` and
   published the plist; `start` can report it before `bootstrap`, after
   `enable` has run; `stop` and `uninstall` can report it before `bootout`,
   after `disable` has run, or after `bootout` has already stopped the job,
   and they never remove the plist in that case. Inspect the conflicting
   file or job yourself before rerunning.
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
