# ADR0017 full-review blocker corrections

OpenAI / gpt-6.1-sol / medium coordinator, small follow-up under the instruction to fix mechanical review findings directly. Actual effort is recorded, not relabelled as the original Sol high MCP implementation route. Base source `07ae3c89e1d6f845a460bc13c5d112508ff7060c`; full review/report `48dd535dab98ddc673a116d08be7becf70283b00`. No publication or activation.

## B1: normative guard coverage

Added eight explicit cases to the real durable dispatch matrix (now 81): standard Windows integrity4096 and12288, admin integrity8192, admin not elevated at12288, Linux/macOS post_switch observations, Linux mac_uid_sweep boundary and macOS linux_cgroup boundary. The expected row still comes from explicit fixture names/contract, never assess. Four token-class mismatches produce I with withheld output, both wrong-point cases produce J with withheld output, both cross-platform boundaries produce H with full identity match and released output. All retain quarantine, metadata, durable/history/database/journal and nonempty-stream assertions. The original 50-case and 23-alias inventories remain intact.

Production assessment and the output release gate are unchanged. The review's five surviving guard mutations now fail assertions; the reverse macOS boundary is pinned too.

## B2: cached configured rights

Execution now keeps configured_rights independently of the validated execution Pin. Configured caches the existing Standard/Admin conversion before the config moves into the preflight closure; unavailable caches None. Capability reads the cached value, so an inventory-valid Unix admin config with no build GID reports admin/false protection rather than null/true. The immutable Pin remains the sole source of assessment rights. No availability, preflight order, store schema, metadata or native mapping change.

The pure configured/capability test checks both standard/admin and present/missing GID, including exact JSON. It calls only parse_inventory with FakeResolver and the cached constructor/status path; it never invokes the NativeResolver/preflight closure. A real dispatch test combines an invalid pin with a counted fake backend refusal, asserts the backend refusal wins, one preflight/zero run calls, a durable refused row and no quarantine. No native backend runs.

## Actual checks

- Initial test edit had an incorrect Activity::list signature and failed to compile; corrected to an owned-temp read-only SQL assertion. No failure/kill credit.
- Before production correction, focused run `414a2a1c-e2d2-4907-904a-c5c8908e2895`: matrix and precedence test passed; capability test failed on rights:null. The first scaffolding compile failure above is separate.
- After correction: all three focused tests passed, run `50f635a5-fb2d-4dc0-9768-952501e868a4`.
- Every mutation below changed actual production source, compiled and failed a named assertion. Source restored in finally before final verification. No build/setup failures counted.

| Mutation | Nextest run ID | Result |
| --- | --- | --- |
| standard-integrity | `c865cdd3-138a-4d65-ba70-73437966d970` | compiled assertion failure |
| admin-integrity | `54472ddc-e285-4188-88ba-82ee2fd8e1f3` | compiled assertion failure |
| admin-elevated | `88beb346-2db8-438a-b72a-814fe2921a08` | compiled assertion failure |
| unix-post-switch | `4abd7014-b656-4f36-a689-b46401d24e52` | compiled assertion failure |
| linux-mac-boundary | `0ab06d89-b3f1-41ae-a25e-d2b57357b4cd` | compiled assertion failure |
| mac-linux-boundary | `356771c9-8291-4a67-a391-fb904141c108` | compiled assertion failure |
| capability-from-pin | `166278bd-04a6-40bb-93ce-51dc0adcae3f` | compiled assertion failure |
| pin-before-backend | `1ba99a0d-06b7-4e0f-ba87-11cbbdb8d73e` | compiled assertion failure |

- Final formatting and locked workspace/all-target Clippy with warnings denied passed.
- Final normal workspace nextest **230 passed, 2 skipped**, run `c368f8ad-d53f-4e96-b863-d3174e76121d`, 10.713 seconds. Subprocess fixtures were exercised only through parents; none invoked directly. Known ExecutionFailed did not recur and is not claimed fixed.
- Whitespace checks passed. The brief's erroneous capability-from-pin instruction is corrected to preserve the already-required byte-identical capability output. The ADR's stale acceptance sentence now separates accepted contract from pending implementation/hosted acceptance.

Other review advisories remain open: discovery tool-return coverage, cancellation assertion strength and indirect pin-refusal call counting in the old case. The new precedence case has direct counters. No new native facts are claimed. No actual process backend, pipe/connect, guard/session/install, SCM/LSA/privilege/account/service, owner administration, VM, Lab Manager or S4U operation. No push/new PR, merge, gate waiver or experiment. CI cancellation/re-run attempts are separately documented in hosted-actions-stall.md; neither succeeded. Fresh independent Opus recheck remains required. Installed/native/dedicated M2 remains **unverified on real hardware** and incomplete.
