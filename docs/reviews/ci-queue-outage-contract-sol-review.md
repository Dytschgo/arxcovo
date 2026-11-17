# Idle queue credit contract review

Root Sol medium,2026-11-16. Opus ADR389e6c8 against mainc612152.
Standards PASS, Spec PASS with the clarifications applied here, Security PASS.
Static review only; no implementation or test result is claimed.

The sweep over validated created/start/end boundaries credits waiting only when
no required CI job executes. Execution, pre-created idle and final overhead stay
charged; overlapping queue is not summed. A slower actual execution cannot be
hidden by subtracting another job's queue. Invalid or failed-run data withholds
credit and falls back to the existing wall rule, while transport limits still
fail closed. The same three OSes, Apple Silicon,900s limit, attempt1/head/repo
selection and live labels remain binding. No permissions or protection change.

Clarification1: ci-speed requires BOTH baseline and candidate to be green and
creditable, consistent with required test5. The draft gate paragraph mentioned
only candidate greenery while its tests denied uncreditable baselines. A failed
baseline's truncated execution is not a valid speed comparison. A fixture must
show that even a faster green candidate cannot use a red baseline for ci-speed.

Clarification2: longest-job reduction rejects a queue-only change with fixed
execution durations, not all possible runtime noise. The actual execution-time
change still needs independent review and measured evidence; no source speedup
is inferred from a noisy metric. ADR wording now states that limitation.

The amendment remains Proposed, not active. User authorized preparation only.
The concrete implementation must have an independent Opus review and platform
results before a separately scoped bootstrap route is presented. ADR0022's
historical exception grants no authority. While main43880904887 is latest and
red, this policy grants it no queue credit and does not unblock PR162 or PR159.
No rerun, manual success status, label or protection modification is authorized.
M2 open,M3 unchecked; hardware paths **unverified on real hardware**.
