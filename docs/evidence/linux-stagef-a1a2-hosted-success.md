# A1/A2 first successful hosted execution

2026-11-14. Coordinator evidence capture; independent acceptance review pending.

PR86 merged as `1e9d8818de1e337b1402ca7e2b71ac02456b82f3` at 18:35:23Z after all five required gates and Muse exact-head review comment 6882239709. The squash tree equals reviewed head `4a025ed6510de866c8db4f0e932e34d520605fb2`. Only ADR0019, its review, and PROGRESS differ from corrected-source baseline `7afea2fdcb17268396fbdb17c36ee8cf991e2ce4`.

D7b's single accepted dispatch is https://github.com/Dytschgo/arxcovo/actions/runs/43627472312, attempt 1, event workflow_dispatch, actor and triggering actor Dytschgo. Exact source and workflow SHA are the main squash above. Job 132546488843 ran 18:35:47–18:37:01Z (74 seconds) and succeeded. Guard tests, locked dependency fetch, and the guard/stage/run/finally-clean step succeeded. No dispatch was repeated.

The only retained runtime output is the fixed-schema [feedback record](linux-stagef-a1a2-run-43627472312.json), extracted from that step with a 2 MiB log-read ceiling. It has first=null, final=null, mutation_lower_bound=cgroup, cleanup=confirmed, three absent internal statuses and no carried refusal. Internal absent alone is not a success assertion; the completed workflow and inspected driver/fixture must support any A1/A2 credit. No raw logs, paths, IDs of OS principals or output payloads are retained.

The completion monitor deleted UNIX_EXECUTION_REVIEWED_SHA; a separate API read confirmed HTTP 404. The grant is consumed, and no further native execution is authorized. The successful runner is not available for reuse.

Independent review must verify the run/step/source binding and the production setup/fixture assertions before A1/A2 is marked accepted. This note does not close M2, full Stage F, Stage G, real-Runner PA6, or the historical failed runs. Installed/dedicated paths are **unverified on real hardware**. Optional diagnostics remain parked.
