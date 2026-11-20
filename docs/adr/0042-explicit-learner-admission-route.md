# ADR0042: Explicit issuer peer routing for learner self-admission

Accepted by the owner on 2026-11-21 (D9).

The retained learner takes a separate numeric issuer peer endpoint and advertises the
canonical address of its attached listener. The endpoint routes the connection to the
original Redeem issuer. The learner authenticates that issuer using the exact original
cluster, node and SPKI binding together with TLS 1.3 proof of possession. Before applying
the original Redeem position E, this exception serves a single self-admission exchange.
The learner uses the applied strict registry after crossing E. The issuer requires strict
mutual TLS for every admission request and rechecks the initiating leaf against its
applied registry at dispatch and during the guarded membership observation.

The advertised IP must match the TCP source IP and be eligible for direct LAN or Tailscale
routing. The port is the authenticated learner's self-advertisement. Reachability and
port ownership remain unproven by that field. The admission request carries only the
canonical address, and the listener derives the actor from the actual connection.

Fixtures provide the explicit peer endpoint. The one-paste arxje1 envelope remains
defined by ADR0040 and carries the enrollment endpoint. Peer and enrollment ports may
differ. Step 13 settles the private startup input for deployment. Service and multi-host
paths are **unverified on real hardware**.
