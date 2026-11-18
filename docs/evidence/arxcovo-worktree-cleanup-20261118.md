# Arxcovo old-worktree cleanup, 2026-11-18

Owner authorization: clean old Arxcovo worktrees; other projects have a separate
cleanup thread. Coordinator: Sol 6.1, high effort. No application behavior changed.

| Verification | Observed result |
| --- | --- |
| Initial C: Arxcovo worktrees | 810 |
| Old clean/generated-only trees removed | 780 |
| Dirty/untracked trees preserved | 6 |
| Trees created today preserved | 24 |
| Final directory and Git registration counts | 30 each |
| Recovery refs | 810; each matches its original HEAD |
| Current pending CI checkpoint hashes | All three unchanged during cleanup |
| Application source | Equals merged main `08c4b999c23f9b516b3a942039ebc3c3f1a6dd04` |

Recovery refs are local under
`refs/arxcovo-cleanup/2026-11-18/<original-worktree-name>`. Existing branch refs
were retained. The original worktree-to-commit mapping is in the D: inventory;
no branch deletion, global prune or forced Git removal was used.

Normal `git worktree remove` completed for 776 roots. Four residual roots had
already lost their Git registrations and `.git` files. Their surviving source
files (108 total) matched preserved Git blobs, allowing text CRLF normalization.
Remaining content was generated output or empty dependency junctions. A D: test
confirmed junction unlinking preserved its target; the four known junctions
were removed without recursion before recursive removal of the plain residual
directories. All four removals completed.

| C: free-space observation | Bytes |
| --- | ---: |
| Before worktree batches | 35,407,282,176 |
| After first batch, 712 complete removals | 524,247,322,624 |
| After second batch, 64 complete removals | 577,711,296,512 |
| After four residual removals | 580,643,573,760 |
| Final verification at 17:59:32 UTC | 689,380,073,472 |

Other cleanup was concurrent. These are drive-wide observations, not an exact
measurement of bytes reclaimed by Arxcovo alone. Earlier attributable cleanup
of three Cargo targets observed 17,622,765,568 to 27,456,466,944 bytes free.

Local evidence is retained under `D:/Temp/arx-worktree-cleanup/`:
`inventory.json`, `recovery-refs.json`, both `removals*.jsonl`, both batch
`cleanup-summary*.json`, `residuals-verified.json`,
`residual-cleanup-summary.json`, and `final-verification.json`.
The final check independently matched all recovery HEADs, all protected roots,
the directory/registration counts and the pending workspace hashes.

No tests or builds were needed for this filesystem cleanup. No runtime database,
LabManager installation/state, VM, backup, owner credential or other-project data
was changed. Runtrace's production database was already on D:; T3 Code state and
Codex history were left in place. Future Arxcovo scratch and tool caches stay on
D:, with C: free space checked before builds or delegation.
