# Controlled policy mapping: coordinator resolution of S1

2026-11-15, Sol coordinator. Source review `bce660f` reported a potential
fail-closed mismatch: Python unit config omits a suffix, while the Rust
systemctl-property predicate requires `:rbind`. No source change is made.

Primary systemd v254 implementation distinguishes these boundaries:

- [config_parse_bind_paths](https://github.com/systemd/systemd/blob/v254/src/core/load-fragment.c#L4783)
  initializes recursive binding true and optional/missing-source false. With
  the two explicit paths and no options, these defaults remain.
- [systemctl property printer](https://github.com/systemd/systemd/blob/v254/src/systemctl/systemctl-show.c#L1411)
  serializes BindReadOnlyPaths entries as space-separated source:destination
  tokens, appending :rbind when recursive. An optional-source flag is rendered
  with a leading minus. The controller rejects that optional form.

Thus the existing Python source:destination config and Rust exact
source:destination:rbind property check agree with this upstream implementation.
The two functions compare different interfaces, not a shared wire format.
No optional suffix acceptance, unknown syntax, mask, or ownership fence is added.
S1's asserted source incompatibility is resolved on this evidence; its important
runtime caveat remains: no actual systemctl property or mount view has run.
Different installed behavior must still fail closed and cannot earn acceptance.
The fresh-host grant and source/run/oracle assessment remain required.

S2 remains an explicit conservative refusal: unrelated /etc mounts are rejected;
S3's possible short-read refusal is not widened. Profile=None, host=False, closed
TEST and no run_body edge remain. Builder/reviewer checks are reused only for
identical source bytes. No new test, build, host read, native execution, helper,
account or grant. A7/A11/M2/full F/G/PA6 remain **unverified on real hardware**.
