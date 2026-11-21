# M3 elapsed-time qualification

**Status — first-attempt PASS.** The single authorized `workflow_dispatch`
ran on reviewed `main` at `44dd7a021a875ec4b49d7fc41f7df092fd260e0b`
(PR #237), on `ubuntu-24.04` with `ImageVersion=20261116.376.1`.
Independent Sol 6.2/high review passed and closed **M3 step 15b / D5 for
conditional hosted Linux elapsed-time qualification**. Review
`D:/Temp/arx-d5-44945822915/review.md` has SHA256 `408f285d…13ee`.
[GitHub Actions run 44945822915, job 136972784258](https://github.com/Dytschgo/arxcovo/actions/runs/44945822915/job/136972784258)

## Authorized run

The dispatch-only M3 time qualification workflow uses `ubuntu-24.04` and a
30-minute job cap outside required CI. The coordinator dispatches once on
the reviewed main SHA. `M3_TIME_REVIEWED_SHA` gates both the source and the
workflow SHA. `M3_TIME_REVIEWED_IMAGE` must match the runner's reported
`ImageVersion` before build. The workflow pins action revisions and the Rust
toolchain. The harness verifies the checkout and the digest of the same
non-test `arxd` executable at staging and privileged launch.

The build reserve is 385 seconds. The launcher reserves up to 1,030 seconds
for staging, readiness, real time, the remaining exchanges and teardown.
It requires room for the full dwell and reserves 55 seconds for the late
and positive phases after it. The retained issuer gets a separate 25-second
cleanup reserve. Only the one attributed foreground process gets SIGINT.
Unknown close or ownership preserves the fixture for runner teardown. The
first failure remains primary if cleanup fails.

The privileged fixture is staged in a verified root-owned directory
under `/var/lib`. Its ancestors allow no group or other writes. The fixture
runs the Linux binary with distinct disposable owner, daemon, agent and
build UIDs on the qualified default-route address. The owner's
private socket supplies the two Mints. The fixture client opens real TLS
sessions against the original CA and exact retained founder leaf. The
initial Query verifies its anchor and drops the continuation. The late
Query must return the exact uniform refusal before proof on a healthy
connection. The fresh Query and Redeem use a one-use CSR, and the issued
leaf must match its public key and the sole applied allocation. The
fixture starts no destination Engine.

| Recorded effect | Required value |
| --- | ---: |
| Engine starts / owned closes | 1 / 1 |
| Found | 1 |
| Acknowledged owner Mints | 2 |
| Server AnchorQuery / Redeem dispatches | 3 / 1 |
| Enrollment proposals and matched retained commands | 4 |
| New allocations | 1 |
| Membership changes after initialization | 0 |
| Monotonic elapsed from observed original Mint commit to late Query | At least 900 s |

## First-attempt artifacts

The workflow uploads `m3-time-first-attempt` with the reviewed head, image,
run and attempt, executable digest, bounded phase result, private observer
disposition, full effect positions, counters and cleanup state. The
elapsed refusal includes the applied and membership positions, enrollment
floor relative to the original Mint, the 900,000 ms TTL, checked-sample
margin and monotonic elapsed. The final owner status and bounded retained
inventory support partial-effect accounting on failure. The artifact
contains no paste, code identifiers, proof, CSR, credentials, addresses or
raw output from the founder. An incomplete witness earns no qualification.
Early sudo and privileged-entry failures record a bounded, redacted
failure category. The grant authorizes a single dispatch.

The independent review recorded these SHA256 digests. The JSONL and job
log were checked against their local copies in `D:/Temp/arx-d5-44945822915/`.
The archive digest comes from verified upload evidence. The build and
privileged launch reported the same executable digest.

| Evidence | SHA256 |
| --- | --- |
| `m3-time-first.jsonl` | `308eb19e8a6d4d67cb2f4712d310e44898dcdacecf9a03c282f6d6dfa8d6c211` |
| `job-136972784258.log` | `c200e556d3b3055542df4751c82cc05f72aea7c9e26a05e7fbe1bfa0240d08f5` |
| Uploaded archive `14723843666` | `a3516d13aba8ba31aade52c93ee4ebff7e1943914e647b1a0a71181f574a9609` |
| Built and privileged-launch `arxd` | `f64a54c8e20032dfd09ee8967595dfda37b2137ae12c86c609387c050627d061` |

The issuer remained up for `901.675592285 s` of monotonic time
(`901675592285 ns`) from the observed original Mint commit to the pre-Query
mark. At the 900,000 ms TTL, the late Query had `expiry_margin_ms=-1737`.
It returned uniform `policy-refused` before proof
(`proof_sent=false`, `allocations=0`, `redeems=0`, `pending=true`).
The same issuer completed a fresh Mint and Redeem with `issued` and
`allocations=1`.

The exact totals were `engine_starts=1`, `owned_closes=1`, `found=1`,
`committed_mints=2`, `anchor_dispatches=3`, `redeem_dispatches=1`,
`enrollment_proposals=4` and `allocations=1`. Membership stayed at
`[0,1,0]` after initialization. Retained positions were Found `[1,1,2]`,
Mints `[1,1,3]` and `[1,1,4]`, and Redeem `[1,1,5]`. Teardown recorded
`child_reaped=true`, `close_signalled=true`, `owned_close=true` and
`fixture_retained=false`.

## UTC witness and restart

The coordinator approved W1b for this run. The harness sends one HTTPS HEAD
to `https://www.google.com/generate_204` and one HTTPS GET to
`https://1.1.1.1/cdn-cgi/trace`. TLS validates the host name or IP address
against the certificate. Each request uses a generic User-Agent and carries
no cookies, credentials, query parameters, repository data, or owner
identifiers. There are no redirects or retries. The origins receive routine
HTTPS connection metadata, including the runner's public egress IP. The
Cloudflare body is bounded to 1024 bytes. The harness reads only `ts=` from
that body and requires it to agree with the Date header within 2 s.
Google's Date and Cloudflare's exact trace timestamp must agree within
5 s. Each response must have the exact HTTP/1.1 status line and one Date.
The trace requires one Content-Length and the client refuses transfer
encoding and bytes after the declared body. A missing, cached, ambiguous,
failed or over-budget sample ends the qualification. Both reference
values must fit the ±30 s premise within the local checked bracket, using
±1 s for resolution and ±1 s for assumed source accuracy. Bounds are
enforced at full precision before the artifact rounds offsets outward.

Both sources and their agreement must pass a shared three-second preflight
before Cargo builds. Read-only `chronyc tracking` or `adjtimex --print`
status is recorded as telemetry only. Artifacts record source, bracket
duration, offset bounds and the first result. There were no reference
requests during local validation.

W1b preflight passed with 0 ms reported disagreement. All five UTC
checkpoints passed within the ±30 s premise. `chronyc tracking` reported
`Not synchronised`, which ADR 0051 W1b treats as informational. UTC health
rests on the sampled HTTPS reference comparisons and brackets.

Existing C13 restart evidence at the persisted committed floor and C15
controlled clock tests support the conditional hosted interpretation.
This is one elapsed qualification on one continuously retained issuer.
Native suspend/resume, physical multi-host operation and Windows/macOS
issuance remain **unverified on real hardware**. Full M3 acceptance
remains pending steps 14b/C14 and 16a/C16.
