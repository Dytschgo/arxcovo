# S2b local unwind and admission evidence

Builder: OpenAI / gpt-6.1-sol / medium. Date: 2026-11-14.
Review handoff: fresh Anthropic Opus 5.5 high review required; not performed here.

## Pins and scope

Initial cwd and Git top level both resolved to the supplied isolated worktree; initial status clean. Initial HEAD `7899caa32b0fb3da8849c20d43016629a28336a8` is the prepared docs checkpoint on merged S2a `47ff894f27f9a9275b320ad2da519e70e6117349`.
Source/test commit: `efe8e55a360575e0da22202c24bbc7049cce5c60`.
Read AGENTS, KICKOFF, ADR0020 D6, accepted proposal U-a/U-b/U-c and final correction review `29e2d03943c921b8312b175bf9ce96e5443c9914`.
No production behavior changes after cfg(test) stripping. No second admission hook: the sole existing hook is reused; the added accessor only loads the real active counter. No workflow, harness, Cargo, policy, proposal, ADR, review or PROGRESS changes.

## Proof boundary

U-a Linux-only constructor follows the existing Group literal, creates only owned ordinary tempfile contents, and lives in the existing test module to respect the payload inventory. A test-only opaque owning wrapper keeps Group visibility unchanged. Mutex<Option<Group>> transfers the real Group into the fake RunnerPort run frame before a fixed literal panic. Real Drop attempts cleanup; the ordinary leaf remains nonempty, so cleanup cannot be confirmed. Direct Executor::run on the real bridge asserts Failed Unknown, residue already true, and readiness latched before returning. Nonroot-only test explicitly reports a skip for real/effective root. No real run_core, account resolution, cgroupfs, /sys, /proc or root operation occurs.

U-b has separate portable limit-one tests for latched readiness and independent Ready-plus-residue. A wrapper counts entry into actual bridge run; fake port counts Runner calls. Requests wait after preflight/quarantine and before slot acquisition at the sole admission hook. Panic case waits for the panic dispatch response, checks ordinary early refusal without bridge entry, and observes the real active count zero before releasing the held request. Independent residue case has no earlier panic/dispatch/quarantine: residue is published monotonically while held and preflight remains Ready. Both require one bridge entry, zero additional Runner calls, and containment refusal/latch. No state is reset. Token matching alone is not gate evidence.

U-c is only a bounded partial lexical guard, not semantic ownership or program-order proof. It checks the two current Group spellings, selected forget/leak/ManuallyDrop/Arc/thread patterns and backend.run before decrement in the same extracted closure. In-memory negative controls insert forget and an early decrement. Aliases, macros and arbitrary rewrites are not exhaustively understood.

## Runs

Windows local validation only; these run IDs are local labels, not hosted job IDs. Logs remain in ignored target/s2b-evidence and their hashes are recorded below. No hosted execution was requested or performed.

- `s2b-final2-fmt`: `cargo fmt --all -- --check`, exit 0.
- `s2b-final2-clippy`: `cargo clippy --locked --workspace --all-targets -- -D warnings`, exit 0.
- `s2b-final2-focus`: `cargo test --locked -p arxd s2b`, exit 0.
- `s2b-final2-inventory`: `cargo test --locked -p arxd inventory`, exit 0.
- `s2b-final2-nextest`: `cargo nextest run --locked --workspace --all-targets --no-fail-fast`, exit 0.

Final focused suite: 3 portable tests passed. Final full nextest: 392 passed, 3 skipped, 12.530 seconds test time. Linux U-a was cfg-excluded, not executed. Required Linux-hosted evidence must be obtained separately without workflow changes in this assignment.

## Mutations

Each executed mutant compiled, then failed a test assertion (exit 101). Files were written/restored as exact LF bytes and SHA-256 equality checked in finally. Final source is restored.

- `s2b-final-in_slot_readiness_refusal_omitted`: removes only the actual in-slot readiness refusal. `s2b_latched_readiness_gate` fails `S2b gate refuses zero additional Runner calls` (2 versus 1).
- `s2b-final-in_slot_residue_check_omitted`: removes only the initial residue observation/latch. `s2b_independent_residue_gate` fails the same assertion (1 versus 0).
- `s2b-final-catch_outside_slot-moved`: moves the slot decrement before the backend call (removes its original location). `s2b_partial_lexical_inventory_and_controls` fails `S2b bounded lexical ownership/order guard`. Both behavioral gate tests still pass, which is consistent with the lexical-only credit.
- `drop_skips_store`: UNRUN on Windows; native Linux compiled assertion-failure requirement preserved.
- `group_forgotten`: UNRUN as a real Linux Group mutant; native Linux compiled assertion-failure requirement preserved. The portable in-memory lexical control earns only lexical credit.

Restored SHA-256 for bridge (both gate mutants): `323fa965a041c4ad53c99d149f810e031d886cc09e83089d26c82d27b49026d1`.
Restored SHA-256 for executor (slot mutant): `45da7396dc926ad40b18a5fab1ff2fd898c3d0577b8cd13d91eba1a7205cf5ed`.

## Failed attempts and corrections

1. Initial fmt failed because a visibility replacement also touched helper parameters. Corrected parameter syntax; no behavior changed.
2. Two overlapping initial focus invocations failed the same incorrect test assertion that a caught panic must produce a transport error. Existing row-G reporting returns a successful transport response with failed activity. Corrected this single test assumption; subsequent focus passed. No third failed fix attempt.
3. Preliminary gate mutants compiled and failed generic refusal before the Runner-count assertion. Moved count assertions before refusal and split the two gate cases into separate tests; final runs fail the intended zero-Runner assertion.
4. Preliminary slot mutant inserted an extra decrement and failed lexical and slot-count assertions. Discarded that credit and reran the proper moved-decrement mutant described above.
5. First full nextest stopped at existing production payload inventory (`fallible payload`), 89 passed / 1 failed; constructor unwraps were visible outside its excluded test module. Moved constructor into existing test module without changing its cfg gate. The temporary log reporter also hit Windows cp1252 when printing Unicode failure output; original binary log was preserved. Final fmt/Clippy/focus/inventory and full nextest all pass after this correction. No check or scanner was weakened.

## Remaining gates

Linux U-a compilation/execution and the two Linux Group mutants remain unrun. Hosted Linux evidence and independent fresh Opus high review remain required. Real-Runner PA6 remains OPEN; M2 incomplete; hardware paths **unverified on real hardware**. A13 is not accepted. Production stays inactive. No activation, experiment, privileged fence bypass, ignored/native fixture, install, owner/admin step, private state, subagent, external request, push or PR occurred.

## Exact file and log SHA-256

- `crates/arxd/src/exec/cgroup.rs`: `64d4cb8c48e516d5fdcd92f85fe2c9c9abb028ee55251929e95824c16211d121`
- `crates/arxd/src/exec/mod.rs`: `f8d425f7a57a309b7e10c29b4609f2c0c75002a33bc1eb0c00e04cd47dd3f9b4`
- `crates/arxd/src/service/executor.rs`: `45da7396dc926ad40b18a5fab1ff2fd898c3d0577b8cd13d91eba1a7205cf5ed`
- `crates/arxd/src/service/unix_bridge/tests.rs`: `b2e0c0efc24feb46555fee12413edf79c8afe22d0221908fde59238a597a780d`
- `crates/arxd/src/service/unix_bridge/tests/e6.rs`: `f26d1ed0778c5b24495f46bde26b8248f046aedfbcae07440857453bf17dabb1`
- `crates/arxd/src/service/unix_bridge/tests/s2b.rs`: `6eb93098dfda74ae52e751a07f69fd9252417184d65d6064cb6a26270643e089`
- `target/s2b-evidence/catch_outside_slot.log`: `188a2763ed3ae6e83c169257b017e8f1dd86ab1c5ceaa1f2de75656950035959`
- `target/s2b-evidence/catch_outside_slot_moved.log`: `1d548cc3a0289ca6a3e2a27182198470cdad28d51d72c1a49727327a96407af8`
- `target/s2b-evidence/clippy.log`: `6116f808ff7e17c2f938c8c1da5fd5b0353b628733a5b8eafeef47dcd1fc19f0`
- `target/s2b-evidence/final-clippy.log`: `fffd86124fd103830bdafbd06f4b12f7b53584277f6e0ebcf4b295a960df770a`
- `target/s2b-evidence/final-fmt.log`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- `target/s2b-evidence/final-focus.log`: `3f8c0bbfb0c4e972a20d75596a9344e034f363d0d28ef5fc4dbb0734cf60eb16`
- `target/s2b-evidence/final-inventory.log`: `4e45d6b7c9ddbf7e4f6b14b95e4e678470459a7eecb91d0fa90225846d16ae0f`
- `target/s2b-evidence/final-nextest.log`: `a139850ea1aaced344a1a16fdcdcc35ec9c5ff2e1d3b2dd42ee18252943a638b`
- `target/s2b-evidence/fmt.log`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- `target/s2b-evidence/focus.log`: `d2f8b82f978c3f570a6ca5e95eda43e5f3efc71ba1efc22bd266410e8eb1848f`
- `target/s2b-evidence/in_slot_readiness_refusal_omitted.log`: `724205a954cb44b2c0c2694effdcf21f849559d2fe904feca569436dfb86b0bb`
- `target/s2b-evidence/in_slot_residue_check_omitted.log`: `433387ad96075aedc6d72a4bfe525d98a8bbf2f8207f47be7cca114189b6ebfa`
- `target/s2b-evidence/inventory.log`: `08db07a9a9fa1257054f450a657a87ad3c5a5204562b30d92980d159e0fccb12`
- `target/s2b-evidence/nextest.log`: `dafd4e6c4ecea2a4f60190ae44723911c1f72d6c00884cda8759b39a0bcc52b7`
