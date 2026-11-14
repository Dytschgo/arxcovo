# A3 Standard hosted execution record

2026-11-14. Coordinator capture; independent acceptance review pending.

PR91 merged as `beb7117b08aad68da790e05a85b335f9655f5236` after five green gates and Muse exact-head PASS 6883300536. Its tree equals reviewed head `e13f7a576beb1c235233672e1aea081581d7d3d8`. Candidate comparison against `fba8b3c` verified only the six D7c-allowed paths; the only runtime difference is the Standard TEST constant.

Run https://github.com/Dytschgo/arxcovo/actions/runs/43635131545, attempt 1, workflow_dispatch, actor and triggering actor Dytschgo, exact head above. Job 132570170629 succeeded from 2026-11-14T20:00:31Z to 20:02:03Z (92 seconds). Guard tests, dependency fetch and guard/stage/run/finally-clean succeeded. One accepted dispatch consumed Standard's grant. Completion deleted UNIX_EXECUTION_REVIEWED_SHA; a separate API read confirmed HTTP 404. No retry.

Only this fixed-schema feedback is retained from the guard/run step, using a 2 MiB log-read ceiling. No raw output payload or host identity is retained.

```json
{"schema":"unix-protection-f0","version":2,"provenance":{"source_sha":"beb7117b08aad68da790e05a85b335f9655f5236","workflow_sha":"beb7117b08aad68da790e05a85b335f9655f5236","run":"43635131545","attempt":"1"},"first":null,"final":null,"mutation_lower_bound":"cgroup","cleanup":"confirmed","internal":{"service":"absent","fixture-check":"absent","harness":"absent","carried":null,"refusal":null}}
```

A green workflow or internal absent statuses alone do not prove A3. Independent review must join this result to the exact selected ignored test, packaged profile, proc-status parser, runtime attribution and completion/cleanup checks. Credit may extend only to Standard on this disposable path. None is still ineligible until that review accepts successful cleanup. M2, full Stage F, Stage G, real-Runner PA6 and installed/dedicated acceptance remain open; **unverified on real hardware**. Historical failures are not reclassified. This record grants no additional native authority.
