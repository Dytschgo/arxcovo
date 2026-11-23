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
  Writes to that socket, including urgent data, recheck the peer. UDP looks
  up sockets by both endpoints, checks the translated peer before connecting
  and before every datagram, and sends through that connected socket. Windows
  filters received UDP datagrams to the socket's connected peer. The frozen
  table in the patch includes 15 prefixes. Its conservative entries cover
  all of the IETF protocol-assignment block and the expired 6to4 relay block.
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
  --enable-strip --prefix=D:/ArxcovoData/qemu/11.1.3-arx-egress-1 \
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
