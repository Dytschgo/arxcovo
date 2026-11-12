# Arxcovo

AGPL-3.0 home-lab manager. Milestone 1 contains workspace scaffolds only; host management begins in milestone 2.

Install Rust via rustup (pinned toolchain), Node 24.18.0, npm 11.16.0, and cargo-nextest 0.9.151.

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

The binaries print their scaffold identity and exit. Electron loads built UI in a sandboxed renderer. No host administration or VM operations exist yet. See [progress](docs/PROGRESS.md) and [CI setup](docs/ci-administration.md). Hardware paths are **unverified on real hardware**.

Owner service commands and `client-config new` are wired in source (ADR 0018).
The latter creates private credentials and prints only a principal/hash line.
Independent review and hosted gates remain required; installation and dedicated
execution are **unverified on real hardware**. See the [owner guide](docs/service-install.md).
