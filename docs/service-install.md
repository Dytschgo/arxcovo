# Installing and verifying the arxd service (owner guide)

Current S3 checkpoint (2026-11-12): owner service install, configure, start,
stop, status and uninstall, and unelevated `client-config new`, are wired in
source under ADR 0018. Independent Opus review and all five hosted gates are
still required before publication. Native, installed and dedicated paths are
**unverified on real hardware**; M2 is incomplete. Agents never execute these
owner steps. The historical status below records earlier slices.

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

Historical status (2026-11-11):

- Main (`d7f3ebd`, #19) has inventory, host identity, the local IPC/stdio
  bridge and activity v2. The lifecycle plumbing in the PR20 candidate
  passed 75 runnable Windows tests after integration with main 8fafbe7 but is not installed anywhere.
- Owner commands and native application wiring do not exist yet. The
  configuration foundation (PR21) is separate and unmerged.
- Windows S4U is **not established**. Both disposable experiments failed (runs
  42995659949 and 43003724132), and no third is planned. The Windows recipe
  below is the intended contract, not a proven path.
- You have separately authorized a foreground, current-user Windows
  development mode (ADR 0010). It is not part of this guide, it is not a
  fallback for the service, and it does not meet the milestone 2 acceptance
  below.

Some steps need administrator or root rights. They are marked **(admin)**.
Everything else runs as your normal, unelevated login. Never paste a password or
token into chat, an issue, a command line or a log.

## What you end up with

- A service `arxd`. It runs as SYSTEM on Windows and as root on Linux and macOS,
  and it holds the private config and the activity database.
- A separate **standard** account (`Arxcovo` on Windows, `arxcovo` on
  Linux/macOS). Every agent command runs as that account. If that account is
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
   Install writes the fixed unit and the standard-rights drop-in. Choosing
   `--rights admin` later removes that drop-in. It also requires you to add the
   account to `sudo` or `wheel` and to provision a NOPASSWD sudoers rule
   yourself. In admin mode, root commands started with `sudo` stay in the run's
   cgroup and are killed with it. A root command can still deliberately move
   itself to another cgroup or start a separate systemd unit, and such a
   process can outlive the run. Builds can also read the service's private
   state, and `list_hosts` reports that. Do not enable lingering
   (`loginctl enable-linger`) for `arxcovo` in either mode.

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
4. macOS limits: only one command runs at a time. Before and after each
   command, every process whose real or effective user is `arxcovo` must be
   gone; after a command they are killed. A process left behind, or one that
   launchd, `cron` or `at` starts for that account later, blocks further
   commands until the service restarts. Do not use that account interactively
   or give it login items while the service is running.
5. **Endpoint (ADR 0015, not yet implemented).** The service creates
   `/Library/Application Support/Arxcovo/run` itself. Do not change stock
   folders such as `/private/var/run`. An earlier `/private/var/run/arxcovo` is
   ignored. You may remove it by hand: `sudo rm` a leftover socket, then
   `sudo rmdir` the folder. Rerun `client-config new`, because older client
   configs name the old path and are refused. After a crash or power loss,
   remove the stale `run/arxd.sock` by hand; it now survives a reboot.

## Start, stop and reboot (Linux and macOS, ADR 0014)

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
- On macOS, install currently always refuses with `service-manager`, until the
  launchd behaviour it relies on has been captured from a real system.
- You do not create `/run/arxcovo` or `/private/var/run/arxcovo` yourself.
  systemd creates the Linux directory at each start, and the service creates
  the macOS one. If either path already exists with the wrong owner or mode,
  start is refused. Nothing repairs it for you.
- macOS service startup is **not yet expected to work on a stock install**.
  ADR 0015 settles the pass-through policy and new runtime location, but
  implementation and the required hosted positive controls are still pending.
- Windows currently registers the service as automatic (delayed). It can
  therefore start at boot even after install alone, or after a stop. This will
  be aligned with the rules above in a later change.

## Verify

1. `arxd service status` prints `running`. On Windows, `sc.exe qc arxd` shows
   `LocalSystem` and the quoted fixed path followed by `service run`.
2. Check the file protections. As yourself, opening the config or state
   directory must fail with access denied. Then run any command through Claude
   Code, as in step 3: listing the config directory must also fail with access
   denied when the build account tries it.
3. Register the bridge for Claude Code (user scope), using your own paths:

   ```sh
   claude mcp add --scope user arxcovo -- "C:\Program Files\Arxcovo\arxd.exe" mcp-stdio --client-config "C:\Users\<you>\.arxcovo\client.json"
   ```

   In Claude Code, ask it to call `list_hosts`. The result must show
   `execution.available: true` and a `cargo` entry in `tools`. Then ask it to
   call `run_command` with `{"tool":"cargo","args":["--version"]}`.
4. The milestone 2 acceptance passes when all of the following hold:
   - the run result shows `observed_identity.kind = windows_sid`, the
     `Arxcovo` SID, Medium integrity, `rights: standard` and exit code 0;
   - `list_activity` returns the same `activity_id`, with outcome `succeeded`
     and `identity_matches_pin: true`;
   - the row is still there after `arxd service stop` and `arxd service start`.

   Record the SID, the version line and the time in docs/PROGRESS.md. Never
   record your own login name or the client token.
5. Negative checks, which are worth doing once:
   - Stop the service, rename the build account, and start the service again.
     `list_hosts` must report `account_missing`, and `run_command` must be
     refused, never run as another identity. If the configured name instead
     resolves to a different SID or UID, the reason is `account_identity_changed`.
   - Add the account to Administrators while `rights` is `standard`. The result
     must be `rights_mismatch`.

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
