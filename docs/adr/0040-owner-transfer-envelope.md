# ADR0040 One-paste enrollment transfer

Date: 2026-11-20. Step 8a codec slice. The owner chose one LAN or Tailscale
address per code in D3.

The private `arxje1-` envelope uses lowercase hex over a family byte, one
raw IPv4 or IPv6 address, a big-endian port, a big-endian CA length, the
original public CA DER, the canonical 270-byte `arxj2` and a four-byte
checksum. The checksum is the first four bytes of SHA-256 over
`arxcovo/join/transfer/v1` and the preceding binary fields. It detects
copy errors. The inner fingerprint pins the original CA bytes. TLS and the
canonical voter anchor still establish issuer identity before proof release.

The raw paste cap is 8,800 bytes including optional outer ASCII whitespace.
The CA cap is 4,096 bytes. Paste requires a matching explicit local prefix
with a usable unicast destination. The joiner must derive those prefixes
from its locally qualified LAN or Tailscale interfaces. The codec performs
no discovery or dial. Step 8b will compose it with the closed TLS client
and a single bounded connection attempt.

The service CLI acquires Windows owner authority and uses SCM or local
configuration. On Unix it checks the current root identity and invokes
the service manager or local configuration operations. Neither path sends
an owner-authenticated request into the running daemon. The local listener
admits a configured SID or UID, then turns a verified agent token into an
MCP agent principal. Even a principal named `owner` remains an agent. The
retained founder job and its 7b listener have no owner channel or protected
display. The ready issuer's endpoint and signer checks are private inside
the frozen ingress module.

Step 8a therefore adds the codec and refusal coverage. The owner Mint
caller and live CLI route move to step 13, where the authenticated owner
channel and issuer eligibility can be composed. There is no new hosted P1
test. Service and multi-host paths are **unverified on real hardware**.
