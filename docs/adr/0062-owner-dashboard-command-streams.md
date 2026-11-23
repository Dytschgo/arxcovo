# ADR 0062 · Owner dashboard command streams

## Decision

The foreground dashboard may execute a bounded, owner-initiated command through
the configured unelevated Windows `current_user` executor. This is a narrow
exception to ADR 0029's ban on HTTP execution. The authority is granted at
daemon composition to the dashboard listener and requires a live owner session,
same-origin CSRF-protected ticket issuance, and single-use WebSocket
authentication. MCP over HTTP continues to provide inventory only.

The dashboard accepts a discovered tool key and a bounded argument vector.
The executor supplies the pinned program, workspace and environment. It
shares admission, the busy fence and Job cleanup with local MCP. Valid command
attempts produce owner activity rows that contain typed metadata. Raw command
arguments and output never enter the activity store.

The graphical console uses a read-only QMP capture lease pinned to a VM
generation. A production listener returns 503 for a known VM until the local
runtime can supply verified display and scratch custody. Native dashboard
execution and graphical capture are **unverified on real hardware**.
