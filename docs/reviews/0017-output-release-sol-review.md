# ADR0017 output-release independent review

Verdict: **PASS**, no blocking findings. OpenAI / gpt-6.1-sol / medium, coordinator, single-agent cross-provider review of the Opus architecture amendment. This accepts the local contract for Sol high implementation; it does not approve the existing validator implementation or any merge.

Pinned base: `2f3b4863dd431794a86114db2d4933d7a282c1ee`. Reviewed head: `0132c3e1f866be9a9f2a2924a5a09193fe2f6520`, three docs-only commits. Read the full amendment, brief and disposition, the retained output probe, current `complete`, `assess`, `Pin`, output mapping and MCP forwarding code at the base. Checked tracked output-count references: development and native capture structures keep numeric counts; this amendment changes only the service result JSON built in `Completed`.

## Standards

No findings. The proposal records the explicit ADR0009 response-domain amendment, source-derived reasoning, no native validation and a bounded implementation/test scope. No new abstraction, configurable policy or string filter is required. The inherited brief's earlier base/scope describes the completed validator; its separate follow-up section names the actual output amendment base and is authoritative for this slice. Implementation must update the ADR/brief acceptance status after this review, retaining all prior evidence.

## Spec

No findings. The full match is computed from admissible unredacted observations before identifier withholding. It includes Unix GID and Windows token class, so `observed_id` alone would be insufficient. `assess` computes it before cleanup precedence; therefore matching-child output remains useful on cleanup failures, timeout, nonzero exit and other partial outcomes. Missing identity or mismatched token facts cannot release either stream, even when cleanup precedence hides the mismatch reason.

The constant withheld form makes no false zero-count claim and leaks no captured length. MCP currently forwards the JSON value and has no separate typed output schema requiring a numeric count. The change is nevertheless an explicit response compatibility amendment, not a claim of identical value types. Output is not stored in activity rows. The nonempty dispatch matrix and real in-memory MCP test requirements cover both disclosure and over-withholding, including deliberately returning synthetic SID-B text from a matching child's output to reject string scrubbing.

The source-of-pipes obligation remains a native-adapter prerequisite: the gate checks adapter evidence consistency, not continuous identity or the truth of pipe provenance. Native adapters still require their own review and verification before availability. This amendment grants no activation or privileged experiment authority.

## Evidence and reversibility

Ran UTF-8/LF and relative file-link checks for the three reviewed docs. Read the actual three-file diff and relevant base code. No Rust suite, native backend, process/pipe, owner/admin, SCM/LSA, account, privilege, VM or Lab Manager operation ran. No external factual lookup was needed for this source/contract consistency review. Before: failed identity can still return captured bytes. After the specified implementation: only full-match captures are released; metadata, row and quarantine remain unchanged. A revert requires no migration but would reopen disclosure once a non-refusing backend exists, so it is not a safe operational rollback at that point.

Standards: 0 findings. Spec: 0 findings. Full independent Opus review of the complete validator plus this implementation, and green hosted gates, remain required. Installed/native/dedicated paths are **unverified on real hardware**; M2 remains incomplete.
