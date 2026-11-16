# Sol contract review: Linux S8 failure attribution

Reviewed Opus disposition2a8f92b on main e601caa. Root Sol coordinator,
medium effort,2026-11-16. Standards/Spec/Security PASS with the implementation
clarifications below, which are binding alongside the original proposal.
This authorizes only ordinary test observability, not a cause or behavior fix.

1. The assertion rule means exact evaluation and expectations are preserved.
   S8/S9c must evaluate their original call/run exactly once, capture the result,
   emit the bounded line, then assert that same result against the same expected
   value/message. This necessarily changes the assertion's actual-expression
   spelling; no expectation, hook, assertion, ordering or branch is removed.
   Do not run the operation twice just to preserve a textual assertion line.
2. A valid deadline result can occur before the first waitpid poll if the
   parent is preempted. The sleeping-mode control must not assert polls>=1.
   It must assert unknown/site=deadline/wait_ms>=deadline_ms, with polls allowed
   to be zero. Keep both existing deadlines, no timing increase or new retry.
3. Include read=eof as a closed enum; wait results may distinguish na, pending,
   reaped, interrupted, echild and other without releasing raw return values.
   Include a closed case label s8|s9c_file|s9c_directory. Render outcome only
   from exit_zero|exit_nonzero|unknown|absent|untrusted, never free text.
4. Timing/poll counters saturate; formatter output is bounded to512ASCII bytes
   and uses exact static keys/closed enum spellings. Do not expose any field
   of Observation, errno, IDs, paths, argv, environment or absolute timestamps.
   Missing-anchor control must prove reset of a previously populated record.
5. Every production-source insertion remains cfg(test), including any imports
   and timing guards; add no call or attribution write in the fork child.
   The pending production Child drop/reap behavior stays byte-identical.

Root inspected the collapsed branches and control setup only for contract
feasibility; no local Linux execution or causal hypothesis is credited. Rustup
reports x86_64-unknown-linux-gnu installed alongside Windows/macOS targets,
so one bounded compile-only cargo check is permissible. It may still fail for
missing native cross-compilation tooling; that is not Linux execution or proof
of Rust source validity. No target install or compiler setup is authorized.

One Sol high builder, source two-file scope plus evidence, no publishing. All
three existing pushes/stop rules apply after independent Opus source review.
No nextest/workflow/deadline/assertion weakening/quarantine change. S8 fix0/3,
observability0/3 before publication. M2 open, M3 unchecked, hardware unverified.
