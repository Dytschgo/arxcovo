# M3 first enrollment slice: independent Sol review

Target: Opus17cd7d8 retained860cd50. Source: docs/briefs/m3-enrollment-first-slice.md and ADR0005. No implementation/test/native action ran.

Standards PASS: narrow deterministic core, exact files and existing dependencies, no transport/signing/host effects. Security/Spec conditional, implementation blocked on T1.

T1: proposal-time expiry does not prove real-time grant or certificate expiry. Counterexample: mint at0, propose redeem at1000ms, delay that sole command until3600000ms without any intervening enrollment command. proposed+SKEW is not behind committed, so apply grants at one hour. The 2*SKEW margin does not address this delay. Keep deterministic proposal-time semantics explicitly limited; settle a fresh trusted post-commit issuance check (including the grant fields and trust assumptions it needs), or another concrete safe boundary before claiming real15-minute acceptance. No CA/signing source is authorized by this correction. Require a delayed sole-command counterexample test that cannot silently be called physical-expiry proof.

T2: MAX_PENDING only bounds usable unconsumed codes. Repeated mint/redeem can retain unlimited consumed records for24hours; add a finite total-record cap and deterministic full-state refusal. Do not prune live dedup records early just to admit another mint. Use checked timestamp arithmetic and specify state effects of overflow/node-exhaustion consistently with time ratcheting.

The proof in a future redeem log is a credential and remains outside the pure-state secrecy claim; future log/transport/signing trust is gated. No new security or hardware acceptance is granted. ADR0005 proposed architecture/CA key/storage decisions remain gated. User steering permits independent M3 work while M2 hardware gaps stay open; the coordinator reconciles the stale milestone-order checklist explicitly.

After Opus correction, Sol will recheck the precise delta before implementation. No broad architecture redesign or additional planning framework is needed.
