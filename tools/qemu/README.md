# Arxcovo QEMU public egress

The Windows x64 build for ADR 0064 takes a single policy path in the
`arx-public-egress` property of a `-netdev user` backend. E1b owns policy
custody and boot admission. Pass the custodied path as a QEMU keyval string
(double any comma in the path). The static recipe uses IPv4 only, the
10.0.2.0/24 synthetic subnet and its 10.0.2.2 gateway and 10.0.2.3 DNS alias.
It supplies one TCP forward from 127.0.0.1 in the lease range 49152–49215 to
10.0.2.15:443.

## Policy v1

The policy is ASCII with LF line endings and a final LF. The file is at most
32 KiB. It has the following fields in order, with no spaces or blank lines.
Angle-bracketed values are placeholders.

```text
version=1
interface_index=<nonzero canonical Windows IPv4 interface index in decimal>
interface_luid=<nonzero canonical Windows interface LUID in decimal>
source=<canonical IPv4 address>
gateway=<canonical IPv4 address>
deny=<canonical IPv4 CIDR>
fingerprint=<64 lowercase hex digits>
```

Repeat `deny=` zero to 512 times before the fingerprint, with each prefix
appearing once. Prefixes are in the range 1–32 with host bits clear. Include
the enrolled host, LAN, VPN, router and public hairpin inventory. The compiled
special-purpose table, the synthetic subnet, and the captured Windows interface
addresses and on-link prefixes also apply. The fingerprint is SHA-256 over all
bytes preceding the fingerprint line, including the LF of the preceding line.
The supervisor is responsible for authenticating this digest against its held
enrollment and verifying the file's path and custody. A digest inside the file
checks format and integrity by itself.

An enrollment must cover the entire site's reachable host, LAN and hairpin
inventory for the boot, including dormant adapters and public-numbered LANs.
Windows route selection and the socket operation are separate calls.


## Packet and socket enforcement

- IPv4 input admits the enrolled guest address, ordinary TCP and UDP, and
  packets without IP options or fragments. ARP for the gateway still works.
  IPv6, ICMP input, NCSI, guest broadcasts and unsupported protocols are
  dropped before they can take a host socket path.
- Before translation, the whole synthetic subnet is denied. That includes
  the DNS alias at 10.0.2.3 and the gateway or loopback alias at 10.0.2.2.
  The backend also disables the libslirp DNS relay, host-loopback translation,
  DHCP and TCP protocol helpers. The policy profile refuses optional guest
  forwarding, SMB and TFTP. Public DNS uses literal public IPv4 resolvers
  through the same TCP and UDP gates.
- TCP checks the translated peer immediately before the outbound connect.
  The asynchronous zero-byte connect probe, writes and urgent data all
  recheck the peer. UDP looks up sockets by both endpoints. It checks the
  translated peer before connecting
  and before every datagram, and sends through that connected socket. Windows
  filters received UDP datagrams to the socket's connected peer. A VM can
  have one policy backend with at most 256 live UDP host sockets. At capacity,
  new peers are dropped without opening a socket. A failed first send or
  policy refusal releases the new socket immediately. UDP sockets expire
  after 30 seconds without a successful guest send, measured on the host's
  monotonic clock. The frozen table in the patch includes 15 prefixes. Its
  conservative entries cover all of the IETF protocol-assignment block and
  the expired 6to4 relay block.
  [IANA IPv4 special-purpose registry](https://www.iana.org/assignments/iana-ipv4-special-registry/)
- Each outbound socket sets Windows IP_UNICAST_IF and binds the enrolled
  source. Route lookup must return the enrolled interface index and LUID,
  default prefix and next hop. Startup verifies the address and uplink,
  captures current IPv4 interface addresses and on-link prefixes, and
  subscribes to IPv4 route, address and interface changes. A notification,
  failed check or poll error latches the backend closed. The main loop
  rechecks uplink health and closes outbound flows and requests a VM pause
  on a fault. [Windows route lookup](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getbestroute) ·
  [Windows interface binding](https://learn.microsoft.com/en-us/windows/win32/winsock/ipproto-ip-socket-options)
- A reply to a host-initiated forward must match the existing TCP socket
  marked incoming by the accept path. The guest may see 10.0.2.2 as the
  peer for that socket. The guest cannot use the exception to create a new
  outbound socket. The forward uses an exclusive IPv4 loopback listener.

## Build and evidence

Start with the QEMU and libslirp sources pinned in
[public-egress-receipt.json](public-egress-receipt.json). Verify the QEMU
signature and both archive hashes. Extract QEMU and place the contents of
the libslirp archive in `subprojects/slirp` before applying the patch with
`git -c core.autocrlf=false apply arxcovo-public-egress.patch`. The patch includes the baseline's
Windows build-tree symlink workaround. The build statically links the
patched libslirp into the system emulator. The stock system libslirp DLL
cannot supply the policy implementation.

Use the pinned UCRT64 toolchain and an out-of-tree build under D: with the
repository's scratch and host reserves. Configure from the build directory.

```sh
../source/qemu-11.1.3/configure \
  --target-list=x86_64-softmmu --enable-whpx --enable-tools \
  --enable-slirp --enable-zstd \
  --disable-gtk --disable-sdl --disable-opengl --disable-vnc --disable-spice \
  --disable-guest-agent --disable-docs --disable-werror --disable-download \
  --enable-strip --prefix=D:/ArxcovoData/qemu/11.1.3-arx-egress-2 \
  --bindir=. --datadir=. --with-suffix=share
meson test --no-rebuild arx-egress
```

Build the portable system emulator, image tool, firmware and test executable
before running the test. The build receipt binds all installed payload bytes
through the full manifest. Use the receipt JSON to identify the precise patch,
QEMU executable and package. The unselected backend follows the stock network
paths. The paused WHPX checks cover startup and loopback listen, with guest
boot, socket binding under traffic, route transitions and actual egress
**unverified on real hardware**.

## M4 qualification witness distribution

The Windows x64 candidate ID is `qemu-11.1.3-arx-witness-1-windows-x64`.
The [build receipt](witness-receipt.json) binds the signed QEMU source,
patch, toolchain inputs, wire version 1, open table version 1, and the
[complete payload inventory](witness-payload.sha256). The inventory
includes the byte count and SHA-256 of each file, including firmware
names containing commas. Its installed name is `MANIFEST.sha256`.
Install the byte-identical receipt beside it as `RECEIPT.txt`.
Independent distribution review is pending. QMP retains upstream version
11.1.3.

The system executable accepts the paused WHPX recipe from 7b2 with its
sealed BIOS and bundle data locations. The image tool accepts the sealed
qcow2 JSON info and qcow2 create recipes, including create with one
approved backing golden. These are the only entry points in this
candidate. Guest install, installed boot and detached verification
recipes require their own reviewed coverage. The stock qualification
runner still refuses the open contract. The public-egress distribution
has a separate pin and build.

The binaries constrain dynamic loads to System32, including the
explicit WHPX and kernel32 requests. TCG is compiled out. Initial
imports, activation context, actual System32 resolution, late events,
and the Job and dynamic-code policies require the native launcher and
debugger proof specified by ADR 0054. At build time the packager checks
the x64 PE format and resolves the static import closure against pinned
UCRT64 payload files, Windows System32 names and API sets. That inventory
does not establish which objects actually map.

### Consumer channel

Use two private byte pipes with child endpoints in the launch handle
allowlist. The launcher sets `ARX_WITNESS_RX` and `ARX_WITNESS_TX` to the
respective positive child handle values in lowercase hexadecimal without
prefix or leading zero. The child checks pipe type, excludes the stdio
handles, clears inheritance and removes both environment entries.
The launcher's deadline bounds channel waits. Incomplete or mismatched
frames and refused operations cause process exit with status 125.
Callers of the qualification runner cannot select the endpoints or
construct channel messages.

Records are exactly 48 bytes. Multibyte fields are little-endian.

| Offset | Width | Field |
| --- | ---: | --- |
| 0 | 4 | ASCII magic `ARXW` |
| 4 | 1 | Wire version 1 |
| 5 | 1 | Open-table version 1 |
| 6 | 1 | Kind |
| 7 | 1 | Stage (1 paused, 2 info, 3 create) |
| 8 | 16 | Nonzero launcher-generated launch nonce |
| 24 | 4 | Sequence |
| 28 | 1 | Role |
| 29 | 1 | Site |
| 30 | 1 | Write bit (0 or 1) |
| 31 | 1 | Zero |
| 32 | 8 | Child handle value, or zero when absent |
| 40 | 8 | Zero |

The supervisor sends INIT (1) with sequence zero to bind the selected
stage and the nonce. After init, the child increments the sequence once
for each open or barrier, up to 64. For an open the child sends REQUEST
(2) and the supervisor sends GRANT (3) with the child-local handle. The
child responds with ADOPT (4) carrying the same live handle. The
supervisor duplicates it back from the held child and checks its
identity, rights and role against the grant before sending ACK (5).
Each exchange repeats the nonce, stage, sequence, role, site and write
bit. ACK has a zero handle field. A pathname never travels on the wire.
The supervisor selects the handle from its sealed plan and role
bindings.

The child sends STARTUP (6) once, after responding to `query-status`
with QMP `prelaunch`. It gates `cont` on the matching ACK. The child
sends TERMINAL (7) following successful cleanup or helper return and
waits for ACK before exit. INIT, STARTUP, TERMINAL and their applicable
ACK records have zero role, site, write and handle fields. A refused
launch can exit without a terminal record.

| Stage | Role and site values | File consumption |
| --- | --- | --- |
| Paused | BIOS (1), ROM size (1) | Firmware size from the granted `share/bios-256k.bin` handle |
| Paused | BIOS (1), ROM read (2) | ROM bytes from the granted `share/bios-256k.bin` handle |
| Paused | VAPIC (2), ROM read (2) | ROM bytes from the granted `share/kvmvapic.bin` handle |
| Info | Image (3), block open (3) | One read handle to the enrolled golden or offline qcow2. The sealed info command leaves backing traversal off. |
| Create | Golden (4), block open (3) | One read handle when the sealed command names the approved backing golden |
| Create | Output (5), block open (3) | The single write-capable grant for the already reserved, empty output |

Firmware resolution in the paused recipe uses exact selectors. The
ROM reader consumes the granted handle through the CRT descriptor.
The helper's Windows file protocol uses its granted handle for
metadata, data I/O, and the output flush. The qcow2 create path skips
its ordinary by-name file creation step and reuses one protocol node
until its output handle closes. This build refuses the Windows
host-device driver and generic file create and block reopen paths.

### Rebuild and evidence

Verify the upstream detached signature with the fingerprint in the
receipt. Verify the MSYS2 base signature and all archive and detached
signature digests in [the input lock](witness-toolchain.sha256).
Use the pinned base and the package set from the lock. Extract the
upstream archive in D: scratch. The source archive's unavailable
symlinks in unused firmware source and test trees are unnecessary
for these build targets. Apply the patch from the source root with
`git -c core.autocrlf=false apply arxcovo-witness.patch` after checking
its digest. Use an out-of-tree UCRT64 build. Invoke configure with
the ordered receipt arguments, then build `qemu-system-x86_64.exe`,
`qemu-img.exe`, `trace/trace-events-all`, and the EDK2 targets named in
the Meson install inventory. Use `SOURCE_DATE_EPOCH=0` for compilation
and set the repository's D: scratch and resource limits.

Run `package-witness.ps1 -Build <build> -Source <source> -UcrtBin <bin>
-Stage <new-directory>` with the pinned toolchain on PATH. Compare the
complete staged manifest and both executables to the receipt, then add
the receipt. Package scripts use new output directories.

On Windows, `python -I -B tools/qemu/test-witness.py <staged-directory>`
exercises synthetic, unbacked and backed create and short-lived info
reads. It checks the retained output object and rejected writer opens,
stale sequences, wrong role and nonce, and missing grants. It exercises
system executable refusal before WHPX initialization. The fixture
demonstrates child framing and handle use. It uses neither a qualifying
debugger nor VM dispatch. The real witness, the closed native resource
inventory required by ADR 0054, and full recipe coverage are
**unverified on real hardware**.
