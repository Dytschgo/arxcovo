# Arxcovo

AGPL-3.0 home-lab manager, under development. The first complete v1 is not ready
for owner acceptance. The Windows backend preview and the Electron UI have
separate readiness: the backend implements authenticated local MCP and bounded
current-user commands; the UI still displays a workspace scaffold.

## What can be tested

The Windows foreground daemon supports host inventory, tool discovery, bounded
commands and persisted activity through authenticated local MCP. With an execution
configuration, Windows defaults to the unelevated account running the daemon.
That shares the account's file and credential access. The optional dedicated
account choice currently refuses execution with setup-required; it does not fall
back to the current account.

The daemon entry point is `arxd --config <private-daemon-config>`; its stdio bridge
is `arxd mcp-stdio --client-config <private-client-config>`. These require prepared
private configuration and matching credentials. See [Windows execution accounts](docs/windows-execution-accounts.md)
for the account and identity contract. The prepared owner package has not completed
its launcher or actual Claude Code acceptance journey. This is a backend preview,
not an installer or completed single-host acceptance.

The [installed-service guide](docs/service-install.md) covers a different,
privileged deployment route with its own unavailable executor and owner setup.
Its LocalSystem client identity must not be used for the per-user foreground route.

Multi-host enrollment/startup and three-node acceptance remain incomplete. VM
creation, snapshot/restore, script-vault activation and dashboard management are
later v1 work.
See [v1 readiness](docs/v1-readiness.md) for the acceptance path and blockers.
Hardware paths are **unverified on real hardware**.

## Development

Install Rust via rustup (pinned toolchain), Node 24.18.0, npm 11.16.0, and
cargo-nextest 0.9.151. Before running commands on the owner's Windows PC, follow
[D: scratch setup](docs/dev-scratch.md): set TMP, TEMP, CARGO_TARGET_DIR,
CARGO_HOME and any npm/Playwright cache paths explicitly on D:. Check C: free
space before builds; pause large builds below 20 GB. Existing owner caches stay
untouched.

```sh
npm ci
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
npm run lint
npm test
npm run build
npx playwright install chromium
npm run smoke
npm run test:desktop
npm start -w desktop
```

These are development commands, not permission to repeat held native tests or CI
attempts. The Electron shell loads the built UI in a sandboxed renderer; launching
it currently shows the scaffold, not the backend management interface.
See [progress](docs/PROGRESS.md) and [CI setup](docs/ci-administration.md).
