# Stage F fixed staging parent source correction

2026-11-14 · Sol / high · implementation only, pending independent review.
Started in the clean isolated checkout at `2bf8fdc8f9a14b480750975f10722c8307a721bb`,
directly above actual main `f01da6f425cc8d9d3ee8c304ab8112017618da44`.
Read AGENTS, KICKOFF, Debug/Engineering guidance, driver and A1/A2 contract.

Closed native run `43622157531`, attempt 1, refused before staging:
first = final = `E_PROTECTION`, `run_parent`, `initial`, ordinal 1,
root-owned directory with write_bits 18 (`0o022`); predecessor root directory
was root-owned with write_bits 0. The then-fixed plan placed staging under
`/opt`. Mutation was `none`, cleanup `not_reached`. D7a is consumed; the
reviewed-SHA variable was cleared (verified 404 in the closed disposition).
That disposition is carried forward, not rechecked through network access here.
Run `43091339273` remains unknown-cause and receives no diagnostic credit.

The existing parser replays the unchanged native JSON as
`{'first': (1, 'write'), 'final': (1, 'write')}`. Its SHA-256 remains
`eda437f17e697d5afaa2cec7a424f88b56472e91b9f18f53e57cd9d16c27d8c9`.

Before editing driver source, ran:

```powershell
python -B -c "import sys,unittest; sys.path.insert(0,'tools/unix-exec-validation'); import test_driver; r=unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromName('A1A2.test_native_writable_parent_regression',test_driver)); sys.exit(not r.wasSuccessful())"
```

Result: exit 1, one assertion failure: `planned staging parent reproduces the
observed writable /opt refusal`. The regression uses real-record replay plus
mocked lstat metadata; it reproduces the observed predicate, not a native pass.
The directly observed refusal makes speculative diagnosis unnecessary.

Driver changes only the fixed plan to `/var/lib/arx-ci-<run>-<attempt>` and the
F0b descriptor walk to `/`, `var`, `lib`, per-run name. Existing checks still
verify every ancestor, reject links/non-root ownership/group or other writes,
and exclusively create the private per-run directory. No parent override,
fallback, chmod, retry, or guard relaxation was added. Tests exercise parent
verification before reservation/mutation, staging collision, and every ancestor's
link/owner/write refusal. The old writable `/opt` still refuses exactly as recorded.

Inspected packaged `arxd.service`, standard profile, installer and A1/A2 override:
`ProtectSystem=full`, `ProtectHome=yes`, `PrivateTmp=yes`, `NoNewPrivileges=yes`
and the capability bound remain intact. Neither packaged file declares
ReadWritePaths/ReadOnlyPaths or execution path restrictions. The fixed `/var/lib`
location is compatible with these declared path restrictions and avoids home/tmp
isolation. Native ownership, permissions and mount execution behavior are **unrun**,
**unverified on real hardware**. No production/Rust path constant required editing.

Final finite verification (all runtime effects mocked):

```powershell
python -B -c "import sys; sys.path.insert(0,'tools/unix-exec-validation'); import mutation_check,test_f0_blockers,test_f0b_transport; mutation_check.main(); test_f0_blockers.mutations(); test_f0b_transport.mutations()"
```

Exit 0: 62 safe-suite tests pass; 14 driver mutants killed (including restoring
`/opt`, skipping parent protection, and staging reuse); 23 F0 mutants killed;
36 F0b mutants killed, 4 explicitly equivalent edits receive no kill credit.
Driver SHA-256 unchanged by mutations:
`8b0e1efeada395f8dd6b03e18c23b0648e99f9c29e8349ad2c3926735d6a9d59`.
`git diff --check` passes. Workflow, production packaging, Rust and native JSON
are byte-unchanged from the starting commit. No native CLI, host probe, service,
account, VM, network, CI dispatch, or delegated work was run.

This source correction grants no execution permission or acceptance credit.
Future execution needs source-fix review, required gates and a separate execution
disposition; there is no automatic retry. M2, full Stage F, Stage G and PA6 remain
OPEN; hardware remains unverified; optional diagnostic tracks remain parked.
