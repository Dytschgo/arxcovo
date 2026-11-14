# Installing and verifying the arxd service (owner guide)

Source baseline: actual main **`aa49ce1e99475d07d63c6b0d23fa94bdb30bab74`**
(2026-11-14 audit), not the local branch named main or unpublished diagnostics.
This is executable acceptance preparation, **not an acceptance pass**. M2 is
incomplete; real-Runner PA6 is OPEN. Installed and dedicated paths remain
**unverified on real hardware**. No owner steps are requested now.

**Stop before installation or acceptance at this baseline on every OS.**
`Execution::configured` selects the inert `Native` executor: successful config
preflight still returns `MechanismUnavailable`, `run` never launches, and
`capability` always reports `available: false` with null hardening. Linux's
standard bridge is test-constructible only. Windows S4U/shim/Job and macOS
UID-sweep production execution remain absent. macOS additionally has a launchd
parser stub returning `Query::Unknown`; owner lifecycle refuses `service-manager`.
Accounts, tools or owner provisioning cannot repair these engineering gaps.

Claim sources at that main pin (repository links locate the files; read their
Git objects at the pin, not candidate worktree source):

- [executor](../crates/arxd/src/service/executor.rs): `Native`,
  `Execution::configured`, `unix_bridge_for_test`, `capability`, `schema`, `assess`.
- [owner query](../crates/arxd/src/service/owner_query.rs): `launchd`;
  [owner CLI](../crates/arxd/src/service/owner_cli.rs): `parse`, `dispatch`;
  [SCM](../crates/arxd/src/service/owner_scm.rs): `register`, `start`, `stop`.
- [layout](../crates/arxd/src/service/layout.rs) and
  [native Unix](../crates/arxd/src/service/native_unix.rs): macOS endpoint,
  ancestor policy and endpoint creation;
  [MCP](../crates/arxd/src/mcp.rs), [stdio entry](../crates/arxd/src/main.rs)
  and [store](../crates/arxd/src/store.rs): requests, inventory and metadata.
- Contracts: [ADR 0009](adr/0009-service-lifecycle-and-dedicated-execution.md),
  [0015](adr/0015-macos-stock-ancestors-and-runtime-placement.md),
  [0016](adr/0016-windows-install-provisioning-and-owner-entry-points.md),
  [0019](adr/0019-unix-native-execution-bridge.md) and
  [0020](adr/0020-linux-execution-hierarchy-and-janitor.md).

The recipes below describe future owner operations after implementation is ready.
They do not authorize agents to perform owner steps or change existing authority.

Configure fully revalidates the existing config before replacement. A missing
old workspace refuses with `service-workspace` (exit 3), even if the new
workspace is valid. Restore the old workspace, or uninstall, delete the config
by hand, install and configure. Changing the pinned build account also requires
uninstall, manual config deletion, install and configure (ADR 0009/0018).

A missing account in the existing config refuses with `service-identity`
(exit 3), before checking its old workspace. Restore the old account facts,
or uninstall, delete the config by hand, install and configure. The separate
`service-account-missing` token can come from a new-account lookup on Windows;
full validation of the previous config retains the library's Identity mapping.

On `client-config-partial`, no registration hash was supplied. The private file
and any newly created private parent remain; the file may contain credentials.
Delete that private file by hand and rerun. No automatic rollback or deletion
occurs. A success prints only `principal=sha256`, never the token.

Every step is **unverified on real hardware** until someone performs it and
records the result in docs/PROGRESS.md. Agents never perform these steps. Leave
the old Lab Manager installation, its state and the `agent-windows-test` VM
untouched.

Historical evidence only: S3/candidate wiring notes and ADR proposal statuses
are not current production availability. The earlier Windows development
acceptance used the owner's unelevated identity, explicitly excluded installed
service/dedicated acceptance, and does not satisfy M2. See
[development evidence](windows-development-execution.md). It supplies no service
fallback. Neither the optional unpublished Windows replay harness nor its
extensions is an M2 prerequisite. Windows main71 `ExecutionFailed` is known
intermittent, cause unknown; no retry or fix is claimed. Existing no-retry,
review and required-check gates remain in force.

Some steps need administrator or root rights. They are marked **(admin)**.
Everything else runs as your normal, unelevated login. Never paste a password or
token into chat, an issue, a command line or a log.

## What you end up with

- A service `arxd`. It runs as SYSTEM on Windows and as root on Linux and macOS,
  and it holds the private config and the activity database.
- A separate **standard** account (`Arxcovo` on Windows, `arxcovo` on
  Linux/macOS). The required production contract runs every agent command as that account;
  this execution path is currently unavailable on every OS. If that account is
  missing or misconfigured, commands are refused. They never run as you or as
  the service.
- A private client file in your profile. Claude Code uses it to reach the
  service through `arxd mcp-stdio`.

## Windows

1. **(admin)** Create the build account as a standard user, and type the
   password at the hidden prompt: `net user Arxcovo * /add`. Leave it out of
   Administrators. The intended design does not need this password: it uses
   S4U, a logon that needs no password (not yet established; see Current
   status). Keep the password for your own records or disable
   interactive use if you like. Optional hardening: in `secpol.msc`, add the
   account to "Deny log on locally" and "Deny log on through Remote Desktop
   Services". Do not add it to "Deny log on as a batch job".
2. **(admin)** Create a workspace the account can modify, outside your profile
   and outside `C:\ProgramData\Arxcovo`, for example `D:\ArxcovoData\work`. Do
   not use a junction or symlink. Grant `Arxcovo` modify rights on that folder
   only.
3. **(admin)** Provide tools for the account. Suggested Rust recipe
   (**unverified**):
   - Install rustup with `RUSTUP_HOME=C:\ArxcovoTools\rustup` and
     `CARGO_HOME=C:\ArxcovoTools\cargo` and `--no-modify-path`, so that only
     administrators can write the toolchain.
   - Give `Arxcovo` read and execute access on `C:\ArxcovoTools`.
   - Builds that link need the MSVC build tools, which `cargo --version` does
     not.
4. **(owner only, admin; DOCUMENT ONLY, unverified on real hardware)** Create
   `C:\Program Files\Arxcovo` and replace its inherited directory ACL before
   copying the binary. The required protected ACL grants inheritable full
   control to SYSTEM and Administrators, and read/execute to Users. Use fixed
   SIDs so the recipe does not depend on localized group names:

   ```powershell
   New-Item -ItemType Directory -Path 'C:\Program Files\Arxcovo'
   icacls 'C:\Program Files\Arxcovo' /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-32-545:(OI)(CI)RX'
   ```

   This recipe assumes a new directory; `/grant:r` does not remove unrelated
   explicit ACEs on an existing directory. The final directory policy must
   reject those grants, including inherit-only CREATOR OWNER full control.
   The owner must inspect any existing directory rather than treating this as
   an ACL repair command. Agents must not execute these commands.
   **Copy, do not move**, the release binary to exactly
   `C:\Program Files\Arxcovo\arxd.exe` so it inherits the destination ACL;
   moving can retain the source descriptor. The install library checks the
   pinned binary directory before restore or creation and refuses unsafe ACLs.
   Owner CLI wiring is present in source, unverified on real hardware. The owner invocation is:

   ```powershell
   & "C:\Program Files\Arxcovo\arxd.exe" service install
   ```

   Install refuses if `C:\ProgramData\Arxcovo` already exists with an unexpected
   owner or permissions. In that case, inspect that folder and remove it
   yourself. Install never takes it over.
5. As yourself, **unelevated**, create the client file:

   ```powershell
   & "C:\Program Files\Arxcovo\arxd.exe" client-config new --principal claude-code --out "$env:USERPROFILE\.arxcovo\client.json"
   ```

   It prints the principal and a SHA-256 hash. The hash is not secret, but the
   file is. Do not run this or Claude Code from an elevated terminal. Elevated
   processes create files owned by Administrators, and the strict ownership
   check refuses them.
6. **(admin)** Configure and start the service:

   ```powershell
   & "C:\Program Files\Arxcovo\arxd.exe" service configure --build-account Arxcovo --client-account <your-login> --rights standard --workspace D:\ArxcovoData\work --env "PATH=C:\ArxcovoTools\cargo\bin;C:\Windows\System32;C:\Windows" --env RUSTUP_HOME=C:\ArxcovoTools\rustup --env CARGO_HOME=D:\ArxcovoData\work\.cargo --env SystemRoot=C:\Windows --agent claude-code=<hash>
   & "C:\Program Files\Arxcovo\arxd.exe" service start
   ```

   If the `Arxcovo` account does not already have **Log on as a batch job**,
   `configure` grants it to that account only, because S4U needs that right.
   It records the grant in a private file, `owner-record.json`, next to the
   config (ADR 0012). It never takes ownership of a right that was already
   there. To change any setting later, rerun `configure`. If a configure was
   interrupted, the next `configure` or `uninstall` resolves it automatically
   and does not remove a right whose origin it cannot prove. The one exception
   follows a power failure or a Windows crash, and it is described under
   Uninstall. Do not edit
   `arxd.json` or `owner-record.json` by hand. Startup checks the config's
   owner and refuses unsafe config protection (exit code 4). The owner record
   is checked by owner operations; service startup does not read that record.

## Linux (systemd 254 or newer)

1. **(admin)** Create the account with no extra groups and a home outside
   `/home`, because the service hides `/home` from builds:

   ```sh
   sudo useradd --system --create-home --home-dir /var/lib/arxcovo-build --shell /usr/sbin/nologin arxcovo
   ```

   Then create the workspace as that user:
   `sudo -u arxcovo mkdir /var/lib/arxcovo-build/work`.
2. **(admin)** Install the tools for `arxcovo`, for example rustup into its home.
3. **(admin)** Install the binary and the service, then configure it:

   ```sh
   sudo install -o root -g root -m 0755 -D arxd /usr/local/libexec/arxcovo/arxd
   sudo /usr/local/libexec/arxcovo/arxd service install
   ```

   Then run `client-config new` as yourself, and
   `sudo …/arxd service configure … --client-account <your-login>` with the
   matching `--env PATH=…`, `CARGO_HOME`, `RUSTUP_HOME` and
   `HOME=/var/lib/arxcovo-build`. Finally run `sudo …/arxd service start`.
   Install writes the fixed unit and standard-rights drop-in. The owner parser
   accepts `--rights standard|admin`; Linux admin plans remove the drop-in,
   but no production executor exists for either mode. This guide's acceptance
   uses standard rights only. Do not enable lingering for the build account.

## macOS

1. **(admin)** Create a **standard** (not administrator) user named `arxcovo`
   in System Settings › Users & Groups, and create its workspace under its home.
2. **(admin)** Create `/Library/Application Support/Arxcovo` and its
   `libexec` folder with
   `sudo install -d -o root -g wheel -m 0755 "/Library/Application Support/Arxcovo/libexec"`.
   `Arxcovo` must stay `0755`, so that your user can reach the endpoint
   folder `Arxcovo/run`. Place the binary at
   `/Library/Application Support/Arxcovo/libexec/arxd`, owned by `root:wheel`
   with mode `0755`. Then run `sudo … service install`, run `client-config new`
   as yourself, and finish with `sudo … service configure …` and `sudo …
   service start`.
3. macOS supports **standard rights only**. `configure --rights admin` is
   refused, and the service refuses to execute with an admin config. Do not
   add `arxcovo` to `admin` or give it a sudoers rule: in standard mode a
   working `sudo -n true` makes every command be refused with
   `rights_mismatch`.
   Admin mode stays unavailable until a separately reviewed design can contain
   and clean up root processes started through `sudo`.
4. Planned macOS containment contract (not a working executor): only one
   command runs at a time. Before and after each
   command, every process whose real or effective user is `arxcovo` must be
   gone; after a command they are killed. A process left behind, or one that
   launchd, `cron` or `at` starts for that account later, blocks further
   commands until the service restarts. Do not use that account interactively
   or give it login items while the service is running.
5. **Endpoint (ADR 0015, implemented foundation).** The service creates
   `/Library/Application Support/Arxcovo/run` itself. Do not change stock
   folders such as `/private/var/run`. An earlier `/private/var/run/arxcovo` is
   ignored. You may remove it by hand: `sudo rm` a leftover socket, then
   `sudo rmdir` the folder. Rerun `client-config new`, because older client
   configs name the old path and are refused. After a crash or power loss,
   remove the stale `run/arxd.sock` by hand; it now survives a reboot.

## Start, stop and reboot (source contract; installed behavior unverified)

The following lifecycle plans describe Linux/macOS; Windows SCM details follow.

- `service install` registers the service but does not start it. It also does
  not start at boot until you run `service start` once.
- `service start` starts the service now and keeps it starting at every boot.
- `service stop` stops it now, and it stays stopped after a reboot until you
  run `service start` again.
- If `service start` fails part way, the service can be left stopped but set to
  start at the next boot. `service status` then shows `stopped` or `failed`.
  Run `service start` or `service stop` again to settle it.
- Running `service install` again is safe while the service is not running:
  - it changes no file;
  - it keeps your config, activity history and drop-in choice;
  - it leaves the service stopped and off at boot.

  If an earlier install was interrupted, running it again finishes the job.
  While the service is running, install refuses with `service-active` and
  changes nothing. Run `service stop` first, or `service uninstall` if stop
  reports `service-not-installed`.
- After an upgrade whose service files differ, install refuses with
  `service-foreign-registration`. Before replacing the installed binary, use
  that old binary to run `service uninstall`. Then use the new binary to run
  `service install`, then `service configure` again if you use admin rights on
  Linux, then `service start`. Your config and activity history are kept.
- On macOS, install currently always refuses with `service-manager`, because
  `owner_query::launchd` still returns `Query::Unknown`. Historical observations
  have not become an accepted production parser.
- You do not create `/run/arxcovo` or `/private/var/run/arxcovo` yourself.
  systemd creates the Linux directory at each start, and the service creates
  macOS `.../Arxcovo/run`. If either directory has the wrong owner or mode,
  start is refused. Nothing repairs it for you.
- ADR 0015's macOS ancestor policy, runtime placement and new-directory
  ownership handling exist in source. Historical hosted observations do not
  establish an installed service or implement the launchd parser. Its stub
  still blocks owner lifecycle; provisioning is not the missing parser.
- Windows implements the same boot intent: register/install leaves demand
  start; start selects automatic delayed start; stop waits for stopped before
  selecting demand start. A partial start can leave automatic intent, and a
  failed stop can leave the previous intent. Installed boot behavior remains
  **unverified on real hardware**.

## Manual M2 acceptance (future, OS-aware)

1. Pin independently reviewed source and binary, target OS and time. Check
   readiness **before install**: at the baseline above, stop on every OS for
   missing dedicated execution; macOS also stops for missing launchd parsing.
   Linux still needs ADR 0019/0020 Stage F evidence and Stage G production
   wiring, including open real-Runner PA6 and panic/release-profile gates.
   Preserve existing Stage F scope and no-retry restrictions. Windows and
   macOS need their own reviewed execution implementations. Do not extend
   diagnostics or enable development mode to get past a refusal.
2. Once ready, the owner uses the platform recipe above for the protected fixed
   binary/layout, dedicated **standard build account**, accessible tools and
   workspace, private client config and registered principal. The service is
   LocalSystem on Windows or root on Unix; it is not the build identity.
   Owner CLI grammar is identical on each OS (substitute private local values):

   ```text
   arxd service install
   arxd client-config new --principal claude-code --out CLIENT_FILE
   arxd service configure --build-account BUILD_ACCOUNT --client-account CLIENT_ACCOUNT --rights standard --workspace WORKSPACE --env PATH=TOOL_PATH --agent claude-code=HASH
   arxd service start
   arxd service status
   ```

   Run `client-config new` unelevated as the Claude Code client account; all
   service verbs require the owner context. Use the fixed absolute binary
   path from the OS recipe, never a PATH-selected binary. `configure` also
   accepts repeated `--env KEY=VALUE` and `--agent PRINCIPAL=HASH` pairs; supply
   the account's needed HOME/CARGO_HOME/RUSTUP_HOME/SystemRoot as appropriate.
   Output is `claude-code=HASH`; the hash registers the private file's token,
   not the token itself. Require status `running`. Do not publish raw output
   or loosen file protections to make the client connect.
3. Register the stdio bridge in unelevated Claude Code, substituting the fixed
   OS binary and private client file (quote paths with spaces):

   ```text
   claude mcp add --scope user arxcovo -- "ABSOLUTE_ARXD_BINARY" mcp-stdio --client-config "PRIVATE_CLIENT_FILE"
   ```

   The arxd entry accepts exactly `mcp-stdio --client-config FILE`. The Claude
   registration line is the guide's client invocation; the source pins verify
   arxd's bridge arguments and MCP schemas, not an installed Claude CLI version.
   Invoke these MCP tool envelopes in order; they are not shell commands:

   ```json
   {"name":"list_hosts","arguments":{}}
   ```

   Require `hosts[0]` OS, arch, logical CPUs, RAM and disk inventory, and
   `execution.available: true`. Stop on unavailable and retain only its fixed
   reason. Require genuine account-scoped cargo discovery, not a stale/empty
   cache. `discover_tools` accepts `{}` if discovery needs refreshing; then
   reread `list_hosts` and require a found cargo entry with observation time.

   ```json
   {"name":"run_command","arguments":{"tool":"cargo","args":["--version"]}}
   ```

   ```json
   {"name":"list_activity","arguments":{"limit":20}}
   ```

   Current schemas reject unknown fields. `list_hosts` and `discover_tools`
   take empty objects. Dedicated `run_command` requires exactly one of `tool`
   (fixed tool enum including cargo) or `executable`; optional `args` has at
   most 128 strings of at most 8192 characters, `cwd` at most 4096 characters,
   `timeout_s` 1–3600 (default 60), `output_bytes` 2–8192 (default 4096).
   Absolute executable length is at most 4096; runtime validation also applies.
   `list_activity` accepts `limit` 1–50 (default 20), positive `before_id` and
   boolean `include_reads` (default false). Use the last row's ID as `before_id`
   to page backwards when needed; activity visibility is caller-scoped.
4. Require cargo exit 0, `outcome: succeeded`, `rights: standard`,
   `cleanup_confirmed: true` and `metadata.identity_mode: dedicated_account`.
   Verify `metadata.identity_matches_pin: true` against real backend evidence:

   | OS | Required observation and boundary |
   | --- | --- |
   | Windows | `observed_identity.kind: windows_sid`, pinned build SID, Medium integrity (8192), non-elevated; `observation_point: post_switch`; `tree_boundary: windows_job` |
   | Linux | `observed_identity.kind: unix_uid`, pinned UID/GID; `observation_point: pre_exec`; `tree_boundary: linux_cgroup` |
   | macOS | `observed_identity.kind: unix_uid`, pinned UID/GID; `observation_point: pre_exec`; `tree_boundary: mac_uid_sweep`, once implemented |

   Compare sensitive identity values locally. Synthetic reports and development
   identity observations cannot satisfy these assertions. Require confirmed
   tree cleanup, not just the cargo process's exit.
5. Match the returned `activity_id` to the caller's succeeded `run_command`
   activity row and its dedicated identity-match/rights/cleanup metadata.
   Have the owner use the fixed binary's `service stop`, `service start` and
   `service status`; reconnect Claude Code freshly and read the same row with
   `list_activity`. Require the same activity ID and persisted terminal facts.
   Record start/stop/restart results per OS; one OS cannot cover another's
   service lifecycle. Boot-intent behavior also needs real per-OS evidence.
6. Retain only source/binary pin, OS, time, sanitized cargo version,
   identity-match/rights/boundary/cleanup booleans or fixed labels, activity ID
   correlation and restart-persistence result. Never record raw SID/UID/GID,
   owner login, private paths, credentials, environment or raw command output.
   A failure stays a failure. No acceptance was executed for this guide update;
   all installed/dedicated paths are **unverified on real hardware**.

Fresh Grok/Muse independent review is required before normal publication of
this correction. Existing cross-provider review, required checks and coordinator
merge authority remain unchanged. This recipe adds no product approval flow.

## Uninstall

**(admin)** Run `arxd service uninstall`. It works whether the service is
running, stopped or was never started. It turns off start at boot, stops the
service and confirms it stopped, then removes the unit or plist. If the manager reports an
unexpected error, a timeout, or a service with the same name that Arxcovo did
not install, uninstall stops and leaves the files in place. It deliberately leaves the config, the activity state, the workspace
and the build account. A later `service install` keeps and reuses them. Delete
those yourself if you no longer want them.

On Windows, it also prints one line about the batch-logon right (ADR 0012):

- `batch-right-removed`: `configure` granted the right, and uninstall removed
  it. This happens even if you also added the right yourself after
  `configure`. If you want to keep it, add it again after uninstall.
- `batch-right-retained-unproven`: a `configure` was interrupted while
  granting, so Arxcovo cannot prove it added the right. It leaves the right in
  place. If you don't want it, remove it in `secpol.msc`.
- `batch-right-retained-not-arxcovo`: the right was already there before
  Arxcovo, so it stays.
- `batch-right-already-absent` or `batch-right-not-recorded`: there was
  nothing to remove.

If `arxd` itself crashes or is closed partway through, the next run handles
it as described above. A power failure, a Windows crash (blue screen), a hard
reset or reverting a VM or disk snapshot is different: Windows may lose
Arxcovo's last change to `owner-record.json` even after the command reported
success. Suppose that happens soon after an uninstall, or soon after a
`configure` that failed or was interrupted while granting the right again.
If you later grant **Log on as a batch job** to the build account yourself, a
future uninstall can remove your grant and print `batch-right-removed`. Restoring an older
`owner-record.json` from a backup has the same effect. If you see that token
and want to keep the right, grant it again in `secpol.msc`. No other right,
account or file is affected. This case is not tested on real hardware (ADR
0012, decision 6).

Only if every `configure` or `uninstall` keeps reporting
`service-owner-recovery-required` is the record damaged. In that case, delete
`C:\ProgramData\Arxcovo\config\owner-record.json` as an administrator. Arxcovo
then treats the right as not its own, and you remove it yourself if you don't
want it.
