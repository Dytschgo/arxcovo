# Service manager artifacts

These constant files implement the manager settings in ADR 0009. They contain
no placeholders, environment expansion or shell commands. The future owner-only
installer embeds their bytes and refuses conflicting existing artifacts.

They are not installed by checking out or building this repository. No service
manager or account operation has been performed for this change. Installation
and native manager validation remain **unverified on real hardware**.

- Linux requires systemd 254 or newer for `DelegateSubgroup=supervisor`. The
  standard-rights drop-in intentionally restricts build children too. Admin mode
  requires the owner's explicit service reconfiguration and account setup.
- The macOS plist runs as root and receives SIGTERM from launchd. The future
  installer must create the private log directory before registration.
- Windows uses native SCM registration with a fixed quoted executable path and
  argument vector; it has no interpolated manager file.

See [owner installation contract](../docs/service-install.md). The install and
configure subcommands remain unfinished; do not manually apply these artifacts
as a substitute for the required installer checks.
