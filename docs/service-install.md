# Installing and verifying the arxd service (owner guide)

Status: this is the **contract** for the owner-only install flow in ADR 0009.
The `arxd service …` and `arxd client-config …` subcommands are not implemented
yet. Do not run anything here until a merged release of `arxd` contains them.
Every step is **unverified on real hardware** until someone performs it and
records the result in docs/PROGRESS.md. Agents never perform these steps. Leave
the old Lab Manager installation, its state and the `agent-windows-test` VM
untouched.

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
   Administrators. Arxcovo does not need this password: it uses S4U, a logon
   that needs no password. Keep the password for your own records or disable
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
   yourself.

## macOS

1. **(admin)** Create a **standard** (not administrator) user named `arxcovo`
   in System Settings › Users & Groups, and create its workspace under its home.
2. **(admin)** Place the binary at
   `/Library/Application Support/Arxcovo/libexec/arxd`, owned by `root:wheel`
   with mode `0755`. Then run `sudo … service install`, run `client-config new`
   as yourself, and finish with `sudo … service configure …` and `sudo …
   service start`.
3. macOS limits: only one command runs at a time. After each command, every
   process owned by `arxcovo` is killed, so do not use that account
   interactively while the service is running.

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
