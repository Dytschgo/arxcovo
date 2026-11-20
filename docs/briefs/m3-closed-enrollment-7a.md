# M3 step 7a closed enrollment protocol

A private enrollment connection owns one TCP stream and a full TLS 1.3
handshake. It uses one server leaf, with client authentication, resumption,
tickets and early data disabled. The client checks the original code's root
fingerprint before dialing and verifies the chain, cluster and CertificateVerify
before Query. It checks the canonical voter-list digest and the handshake leaf's
node and SPKI before it constructs Redeem.

Frames start with `ARXE`, version 1, a kind byte and a big-endian u16 body
length. Integers in the body are big-endian.

| Kind | Body |
| --- | --- |
| 1 Query | 16-byte cluster, 16-byte code ID |
| 2 Anchor | Status 0, 16 random challenge bytes, canonical voter list |
| 3 Redeem | 16-byte nonzero request ID, 16-byte cluster, 16-byte code ID, 32-byte anchor digest, 16-byte challenge, 32-byte proof, u16 length, CSR DER |
| 4 Result | Status 0, u64 learner node, original u64 term, leader and index, u16 length, leaf DER |

Anchor and Result may instead contain a single status 1 byte. DER fields hold
1–4096 bytes. The learner node is at least 2 and the index fits the signed
storage bound. All fields must exhaust their enclosing frame. The client
retains the complete decoded canonical list from this exchange. The bounded
Result carries protocol data only.

Redeem must match Query, the anchor digest and the fresh challenge from the same
connection. The listener consumes the continuation and requires authenticated
TLS write-half EOF with zero additional plaintext before dispatch. The client
requires the corresponding authenticated end after Result. Invalid or incomplete
traffic terminates the exchange. Once the client starts sending Redeem, any
failure is possible-effect uncertainty. The operation never automatically retries.

The listener permits eight sessions and two concurrent handshakes, with no
admission queue. Both directions have a separate 128 KiB ciphertext cap for the
whole TLS connection. The socket has one absolute 12-second budget. The
registration gate checks close and remaining time before admitting either
backend operation with a five-second contract. On socket expiry, the supervisor
retains the connection task while it joins its admitted work. Close latches
admission, drops the listener and joins the connection tasks. An observer may
time out with Unconfirmed and observe the same retained join again.

The test backend uses a disposable CA, real TLS possession, signed synthetic
CSRs and opaque fixture results. The source compiles without a live backend
caller. This is protocol-level evidence. Live issuance, durable join and
native or multi-host acceptance are **unverified on real hardware**.
