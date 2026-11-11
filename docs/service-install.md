# Installing and verifying the arxd service (owner guide)

Status: this is the **contract** for the owner-only install flow in ADR 0009.
Only the exact `arxd service run` entry and lifecycle plumbing are implemented
in the slices 1–2 candidate. Outside Windows SCM it exits 10; Unix exits 7
because the real fixed-layout application integration is unavailable. It cannot
report ready or serve inventory yet. Owner commands and `arxd client-config …`
are not implemented. Do not run anything here until a merged release contains
the complete installation and application integration.
The config/owner-plan foundation adds library-only validated types and recording
fake tests. It does not implement owner commands or an installer. See
[foundation API and limits](service-foundation.md) before wiring native adapters.

Every step is **unverified on real hardware** until someone performs it and
records the result in docs/PROGRESS.md. Agents never perform these steps. Leave
the old Lab Manager installation, its state and the `agent-windows-test` VM
untouched.

Current status (2026-11-11):

- Main (`d7f3ebd`, #19) has inventory, host identity, the local IPC/stdio
  bridge and activity v2. The lifecycle plumbing in the PR20 candidate
  passed 50 tests but is not installed anywhere.
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
4. **(admin)** Copy the release binary to exactly
   `C:\Program Files\Arxcovo\arxd.exe`. Then, from an elevated terminal, run:

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

   `configure` grants **Log on as a batch job** to the `Arxcovo` account only,
   because S4U needs that right, and records that it did so. `uninstall`
   removes the right again. To change any setting later, rerun `configure`.
   Do not edit `arxd.json` by hand: a changed owner stops the service from
   starting (exit code 4).

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
2. **(admin)** Place the binary at
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
     `list_hosts` must report `account_identity_changed`, and `run_command` must
     be refused, never run as another identity.
   - Add the account to Administrators while `rights` is `standard`. The result
     must be `rights_mismatch`.

## Uninstall

**(admin)** Run `arxd service uninstall`. It stops and removes the service,
removes the unit or plist, and removes the batch-logon right if `configure`
granted it. It deliberately leaves the config, the activity state, the workspace
and the build account. Delete those yourself if you no longer want them.
