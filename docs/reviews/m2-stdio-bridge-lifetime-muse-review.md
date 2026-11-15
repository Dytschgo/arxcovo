# Independent review: M2 real-stdio bridge lifetime fixture fix (Muse)

Reviewer: Muse Spark via Muse Code harness (different-provider independent review of Sol work).
Date: 2026-11-15. Scope is read-only review plus this review file; no source or PROGRESS edits.

## Pins

- Base: `1869175ba31e5c81386f45d293ed50365081fb1f`
- Fix commit retained: `943373fffc1fc66f99f281cc197cf7cf46840b75` (test-only `InventoryPeer` ownership fix)
- Reviewed checkout HEAD at review time: `3b7d72b8c2ad4e7e13e65fc309505bed03f712e4` (fix plus PROGRESS/evidence-only commits `9fefaad`, `943373f`, `3b7d72b`; no production delta)
- Diff scope (`1869175..HEAD`): exactly 4 files, `crates/arxd/tests/inventory.rs` (+48/-6 approx), `crates/arxd/tests/inventory/activity_v2.rs` (+18/-8 approx), `docs/evidence/m2-activity-bridge-lifetime.md` (new, 89 lines), `docs/PROGRESS.md` (+6). No production, Cargo, ADR, workflow, driver, policy, or native-backend change.
- Blob pins re-verified with `git hash-object`: `inventory.rs` = `e92ef2669e6777cede83859d383190c59aef421d`, `activity_v2.rs` = `40a3c40fd8879a415b0aade734cb7f2ca8312bca`. Both match the evidence file.
- Reported historical event: PR109 Windows run `43662502017`, job `132654674576`, reported head `a746ab4`, symptom `bridge completion status` timeout / `activity unavailable` response after `fail_finish`. No external logs were fetched and none are claimed here.

## Standards

Rule baseline: [AGENTS.md](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-68200548-d79b71da05ab3cbe/AGENTS.md) (small conventional commits, one-concern PRs, every behavior has a meaningful test, no unverified claims, hardware paths labelled unverified; technical fences; CI under 15 minutes). No repo-wide style doc beyond that was needed for this test-only diff; `cargo fmt` / `cargo clippy -D warnings` are the enforced gates and both pass (see Checks).

Findings (2 minor, no blockers):

1. `InventoryPeer` thin wrapper (`inventory.rs:66-93`): at first glance a possible Middle Man (struct plus `Deref` that mostly forwards to `RunningService`). Judgement: not a smell to fix. It owns the joint SDK-service plus optional `Child` lifetime, which is exactly the behavior under test, and `Deref` keeps all existing `peer.call_tool` / `peer.list_tools` call sites unchanged across 10+ callers. Removing it would scatter ownership logic instead.
2. `cancel(self)` consumes the peer (`inventory.rs:78`): callers must call `peer.cancel().await` rather than dropping. All in-scope call sites do (verified: `inventory.rs` lines 163, 216, 249, 340, 568, 681, 799, 975, 1035-1036, 1295, 1493; `activity_v2.rs` lines 468, 580-581). A dropped peer still kills the child via retained `kill_on_drop(true)`, so the failure mode is a lost diagnostic, not an orphan. No change requested; noting it so a future refactor does not assume `Drop` reaps gracefully with diagnostics.

No Duplicated Code, Feature Envy, Data Clumps, Primitive Obsession, Repeated Switches, Shotgun Surgery, Divergent Change, Speculative Generality, Message Chains, Refused Bequest, or Mysterious Name beyond the documented wrapper. `child_diagnostic` is byte-identical (bounded 512-byte, category-only mapping); the 15-second shutdown bound and 2-second diagnostic bound are unchanged values, only moved to after SDK cancellation.

## Spec

Spec sources: [docs/evidence/m2-activity-bridge-lifetime.md](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-68200548-d79b71da05ab3cbe/docs/evidence/m2-activity-bridge-lifetime.md) and the historical CI symptom it cites. Requirements checked against the diff and observed runs:

- Proven mechanism matches the code: the old `stdio_peer` detached a `tokio::spawn` whose 15-second `child.wait()` started at initialization while the SDK session was still live; its diagnostic reader then blocked on the live child's stderr, and dropping the kill-on-drop child killed the bridge independently of SDK cancellation. The new `InventoryPeer` starts the same 15-second bound only after `service.cancel()` completes, then kills/reaps before reading the bounded closed diagnostics, and panics on failed shutdown. HTTP peers use the same wrapper with `child: None`. Verified in `inventory.rs:77-106` and `inventory.rs:910-950`.
- No orphan / early-kill / unbounded-shutdown regression: orphan impossible in the normal path (child owned until `cancel`; `kill_on_drop(true)` retained for panic/drop paths; `stdio_peer` init-failure path already kills and reaps before panicking). Early kill removed, not added (shutdown wait begins after SDK cancel). No new unbounded wait: 15s child wait, 2s diagnostic read, 10s SDK initialize, and the new 5s finish-response bound are all `tokio::time::timeout`-bounded; `service.cancel()` itself is pre-existing behavior, unchanged.
- Privacy / error / storage assertions unchanged: `activity_v2.rs` keeps `activity unavailable` with no `activities` page, no `SECRET-MARKER` leak, `started` outcome row, persisted-marker absence (`windows(13)` scan), pagination, maximum-field response bounds, validation, and fail-start branches. The only behavioral deltas are the intentional 18-second idle on stdio after `fail_finish` and the 5-second bounded-response wrapper with message `finish failure must produce a bounded SDK response`. No timeout raised, relaxed, retried, or load-flooded.
- CI cost is meaningful and disclosed: ~19s for this one exact test (observed 19.69s here; evidence reports red 19.23s / green 19.21s). Within the 12/15-minute budget; no whole-suite timing claim is made.

Spec verdict: PASS. One explicit non-claim is handled correctly: the controlled 18s idle reproduces the matching diagnostic and symptom but does not establish what caused the historical CI scheduling duration; the evidence states the historical cause remains unknown.

## Security

Test-transport boundary: stdio subprocess is `arxd mcp-stdio --client-config` against a private synthetic fixture and local listener only; no execution configuration, no actual command execution reached (`list_activity` plus synthetic rejected args). No production or native-execution-backend change; no new network, admin, service, workflow, or policy surface.

Findings (none blocking):

- No secrets introduced: `child_diagnostic` still reads at most 512 bytes, only after exit/kill, maps to a fixed category list, and never prints arbitrary stderr. Token/SECRET-MARKER non-exposure assertions are retained on both transports.
- Argument hygiene unchanged: requests use fixed `list_activity` shapes; `ClientConfig` files remain private (`0o600` / `protect`). No unsafe paths, reparse points, or external fetches in the diff.

Security verdict: PASS, no findings.

## Checks observed (this review session)

- Call-graph before test: all `stdio_peer` / `client` / `InventoryPeer` / `.cancel()` call sites enumerated above; production `mcp.rs:18` (`activity unavailable` mapping) and `store.rs` error mappings confirmed untouched via 4-file-only diff.
- `cargo test --locked --offline -p arxd --test inventory activity_v2::sdk_http_and_real_stdio_activity_visibility_storage_failures_and_bounds -- --exact --nocapture`: PASS, 19.69s (1 passed, 32 filtered).
- `cargo test --locked --offline -p arxd --test inventory activity_v2::read_flood_preserves_actions_and_lifecycle -- --exact`: PASS, 5.08s.
- `cargo test --locked --offline -p arxd --test inventory activity_v2::exactly_once_completion_rolls_back_duplicate_missing_and_repeated -- --exact`: PASS, 0.06s.
- `cargo fmt --all -- --check`: PASS. `git diff --check`: PASS.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`: PASS, 30.88s.
- No retries, no broadened suites, no loop-until-green. The historical red (19.23s, `Err(Elapsed(()))` / `diagnostic-timeout`) was not re-run here; the evidence's red/green pair is accepted as builder-reported, with the green side independently re-observed.

## Close

- Standards: 2 minor non-blocking notes; worst is the `Deref` wrapper shape, judged justified.
- Spec: 0 findings; fix implements the specified ownership change with assertions preserved.
- Security: 0 findings.

Not accepted or verified here: historical CI scheduling cause (unknown), M2/F/G/PA6 acceptance, real-hardware behavior (**unverified on real hardware**), hosted required checks, PR109 native-dispatch unblock, and merge approval. Those remain with the coordinator.
