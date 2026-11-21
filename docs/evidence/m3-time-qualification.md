# M3 elapsed-time qualification

Status: source prepared for independent review. The workflow has not been
dispatched. D5 remains open pending the approved first hosted attempt.

## Authorized run

The dispatch-only M3 time qualification workflow uses `ubuntu-24.04` and a
25-minute job cap outside required CI. The coordinator dispatches once on
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

The fixture runs the Linux binary with distinct disposable owner, daemon,
agent and build UIDs on the qualified default-route address. The owner's
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
The grant authorizes a single dispatch.

## UTC witness and restart

The coordinator approved W1 for this run. The harness uses HTTPS HEAD on
`https://www.google.com/generate_204` and
`https://www.cloudflare.com/cdn-cgi/trace`, with verified TLS and hostnames,
a generic User-Agent and no cookies, query parameters, redirects or retries.
The origins receive ordinary HTTPS connection metadata. Requests carry no
repository, owner or personal data and the harness reads no response body.
A missing, cached, ambiguous, failed or over-budget sample ends the
qualification. Each independent Date sample must fit the ±30 s premise
within the local checked bracket after ±1 s for resolution and ±1 s
assumed source accuracy. Artifacts record source, bracket duration, offset
bounds and the first result. There were no reference requests during local
validation.

Existing C13 restart evidence at the persisted committed floor and C15
controlled clock tests support the conditional hosted interpretation.
This is one elapsed qualification on one continuously retained issuer.
Native suspend, physical multi-host behavior and other OSes remain
**unverified on real hardware**. D5 and full M3 acceptance remain open if
the hosted reference, provenance or elapsed observation is unavailable.
