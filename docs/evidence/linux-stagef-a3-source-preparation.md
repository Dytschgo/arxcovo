# Stage F A3 source preparation

2026-11-14; OpenAI / gpt-6.1-sol / high; implementation sub-agent, no delegates.
Base: `6b462f3a3f60d4109ddb7229ebde80ea17e5ca7a`; clean starting worktree.
Source is the commit containing this note. Reviewed locally against AGENTS,
KICKOFF, engineering skill/profile/safeguards, brief 0020 privileged A3 and
ADR 0020 D8 R4. Independent provider review remains pending with coordinator.

The only source changes are tests in `exec/hierarchy/parse.rs` and dormant
assertions in `exec/disposable.rs`. Production R4 remains exact NNP=1 and
CapBnd=00000000000000c1, bounded to 8192 bytes. The pure fixture uses the real
production parser on synthetic status data. It covers profile-on, NNP disabled,
a broader bounding set, both mismatches, and a wrong bounding set despite
matching effective/permitted sets. The unchanged packaged profile directives
and Linux capability bit mapping are pinned. These are synthetic inputs,
not captured kernel output or proof of systemd's profile application.

Two new Linux-only ignored tests, `disposable_linux_a3_standard` and
`disposable_linux_a3_none`, call the existing `LinuxFs::system` production
reader and parser. Before reading status they require the existing four
disposable-host environment gates, real/effective root and procfs membership
accepted by the existing fixed `arxd.service/supervisor` location parser.
They assert procfs magic and the respective hardening result. No setup,
Runner, account, service modification or new harness is introduced.

Neither case is selected: the unchanged driver still selects exactly
`exec::disposable::disposable_linux_a1_a2`. A future separately reviewed
driver must establish unchanged-package profile presence/absence and runtime
attribution before either ignored assertion can provide A3 acceptance.
No authority to design or execute that driver follows from this slice.

Call graph inspected before test selection: the three exact tests below call
pure parsers and compile-time included source/package strings only. There are
no calls to LinuxFs, setup, Runner, subprocess or native discovery. The native
helper is reachable only from the two new ignored tests, in the existing
Linux/test-gated disposable module. No ignored test was selected.

Local verification on Windows, offline, on these source blobs:

- `exec/hierarchy/parse.rs`: `145a44d4912fc28c39b0832d7decb331b64b465e`.
- `exec/disposable.rs`: `d484890abfe271a9b05d164c9aecb1066308219a`.
- `cargo test -p arxd --lib --locked --offline exec::hierarchy::parse::tests::a3_packaged_profile_status_fixtures -- --exact`: one passed.
- `cargo test -p arxd --lib --locked --offline exec::hierarchy::parse::tests::p6_exact_hardening -- --exact`: one passed.
- `cargo test -p arxd --lib --locked --offline exec::hierarchy::parse::tests::bounded_valid_inputs_and_total_byte_data -- --exact`: one passed.
- `cargo clippy -p arxd --tests --locked --offline -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

No Linux target compilation, native query, service/process/account operation,
host/root/filesystem probe, network/GitHub/CI access, dispatch or Python tests
were performed. Windows compilation excludes the Linux-only assertions;
their compile and runtime behavior remain unproven. No mutation-test credit
or acceptance credit is claimed. The production parser function is unchanged.

Driver, manual workflow, packaged unit/profile, production adapter/setup,
Cargo, policy, ADR and PROGRESS remain unchanged. A1/A2 historical run
43627472312 and independent Muse acceptance 50fef47 are context only; that
consumed grant gives no future execution authority. Production execution
remains inactive. M2, remaining Stage F (including A3), Stage G and real-Runner
PA6 remain open; **unverified on real hardware**. Optional harness work stays
parked. No publication or merge was attempted.
