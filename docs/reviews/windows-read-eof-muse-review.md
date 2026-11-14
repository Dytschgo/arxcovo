# Windows read-side broken pipe EOF — independent review (Muse fallback)

Reviewer: Muse Spark, high effort (Grok fallback exhausted; user-authorized Muse fallback).
Target: `083b0e6c78b131dda3780ee7e1e4b29c2a77ac21` on base `7afea2fdcb17268396fbdb17c36ee8cf991e2ce4`.
Scope: `crates/arxd/src/development/windows.rs` + `docs/evidence/windows-read-eof-main43625345526.md` (2 files only).
Originating signal: ordinary-main CI run `43625345526`, Windows job `132539815010`, 2026-11-14 18:17:03Z:

```text
c=overflow r=execution_failed s=final_out_read i=0 e=109 v=1 w=- ms=- o=12288 d=0 p=0 h=160 cw=258 cc=1 ja=0 jt=2 jk=0 jv=0
```

Localizes error 109 to final stdout ReadFile with prior total 12288. No cause credit taken for historical main71; this is a first focused fix for a newly localized occurrence.
D7b/PR86 authority and scope unchanged: PR86 must be consumed before this source changes main; this review grants no run, publish, or dispatch authority.

## Standards

- One-concern change: production `drain` factored into pure `drain_io` plus three focused regression tests. No harness/CI/workflow/Cargo/policy/PROGRESS edits.
- `drain_io` carries no `unsafe`; all native calls stay in `drain` closures. `GetLastError` captured immediately on failed `PeekNamedPipe`/`ReadFile` returns.
- Bounded read preserved: `available.min(4096)` into a 4096 buffer; append capped by `(limit + REDACTION_MARGIN).saturating_sub(bytes.len())`; verdict stays `total > limit`.
- Smell baseline: no new duplication (shared drain path extracted, not copied), no speculative generality (three closures are the minimal seam), no middle-man (production `drain` is a thin native adapter over `drain_io`, justified by the pure seam). Pre-existing `n as usize` slice pattern unchanged, not introduced here.

## Spec

- Required behavior delivered: `Err(ERROR_BROKEN_PIPE)` from read is EOF returning `Ok(total > limit)`, matching peek behavior; prior bytes/total and existing overflow verdict preserved; no failure capture on 109.
- Non-109 errors unchanged: still one `failure(error)` call then `Err(Reason::ExecutionFailed)` — covered for 5/6/55 at both peek and read, totals 0/8192/12288.
- Peek semantics unchanged: `Ok(0)` skips read; 109 is EOF; other errors fatal. Overflow bounds unchanged: success path retains `total 16384 / bytes 12288 / dropped 8192` at limit 8192; empty/zero-byte reads covered.
- Test-only `capture_injected` and the diag stage table now agree with real-read handling (read-stage 109 no longer captured). Cleanup, handles, timeout/cancel/shutdown, quarantine, and native assertions untouched per diff.
- Regression meaningful: pre-fix red `left: Err(ExecutionFailed)` vs `right: Ok(true)` on peek 4096 + read 109 at total 12288/limit 8192, with the failure buffer deliberately poisoned to catch appends; post-fix green. Fixture callgraph verified pure by reading: tests call only `drain_io`, mock closures, `Captured` arithmetic and assertions — never `drain`, `run_inner`, `diagnostic_snapshot`, or native replay. Independent re-run on this head: exact filter `development::windows::tests::drain_read_broken_pipe_preserves_overflow` → 1 passed, 386 filtered out, 0.00s.

## Security

- No secrets, owner data, paths, reparse points, network, or process operations in the change. Mock closures are test-only; production error values come from `GetLastError`, not injected metadata.
- Failure capture fires exactly once and only for non-EOF errors; EOF paths emit no diagnostic, so no error-code oracle or log-spam change.
- Residual note (non-blocking): `b[..(n as usize).min(keep)]` trusts `n <= 4096` from the OS contract, as before; a hostile in-process mock returning `n > 4096` would panic rather than over-read. Production-safe; no change requested.

## Verdict

Accept for the stated scope. No blockers. Native behavior remains **unverified on real hardware**; required gates and D7b/PR86 consumption still pending before main.
