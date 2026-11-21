# ADR 0044: Sealed issuer custody for promoted voters

The owner adopted D11 for M3 step 12 on 2026-11-21. The original issuer
can hand off its CA to both newly promoted voters after the exact uniform
membership is applied and agrees with the committed and effective views.
The initial, learner-only and joint configurations supply no recipient.
Both peers check the actual leader epoch, the full membership log ID,
bound member keys and exact peer addresses. The recipient also checks
its original retained publication and the post-E strict gate.

A distinct version 1 peer method uses the existing mutually authenticated
TLS 1.3 connection, its disabled resumption and early data, and the existing
transport task and deadlines. The exchange is Hello, Challenge, Sealed,
then Acknowledgement. A fixed direction-specific label and the full
context derive the per-transfer key from the actual TLS exporter. The
context contains the cluster, original root fingerprint, both node/SPKI
pairs, leader epoch, full promotion position and a random 32-byte receiver
challenge. One package uses that key with ChaCha20-Poly1305. The package
is capped at 16 KiB before allocation and the sealed and acknowledgement
frames require authenticated end-of-stream. The CA plaintext stays in
custody. Raft and the public identity publication receive no CA key.

Voter custody uses a fixed private voter-custody directory beside the
learner staging directory. The learner member inventory and original
installation locks stay in place. The receiver validates the original
DER and key with the existing CA loader at checked time, then seals the
CA under its own ADR 0037 context. Create-new directory and reservation,
checked handles, file and directory sync, and authenticated readback
precede the retained job's completion latch. The latch and an actual
authorized load gate in-process use. A missing or partial reservation
gets no repair, overwrite, regeneration or cleanup. A lost acknowledgement
makes the sender's result Unknown and spends its transfer attempt.

A promoted voter needs the original loaded CA and the eligible local
leadership view to activate the retained enrollment endpoint. It uses
the existing clock, AnchorQuery and Redeem path. Under the local trust
model, revocation refuses future issuer admission. The original issuer
extends CA trust to the two promoted daemons, and revocation cannot
erase CA material already disclosed to a compromised voter.

Step 13 owns authenticated completion history and cross-process activation.
C12 observes in-process custody and issuance after orderly founder close.
The unchanged ADR 0037 distinct-UID probe covers the original layouts.
The new voter path still needs qualification in the isolated deployment
layout before production activation. Service and multi-host paths are
**unverified on real hardware**.
