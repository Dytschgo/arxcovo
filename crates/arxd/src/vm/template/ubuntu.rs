//! The autoinstall identity uses Subiquity's locked password marker. On
//! 24.04.3 account creation can be deferred to target cloud-init, so late
//! commands neither inspect nor change accounts. Removing and disabling the
//! target datasource also disables that deferred creation path. Detached
//! health asserts there is no login-capable account before qualification.
//! A fixed root oneshot checks protected files and publishes a boot-bound
//! verdict readable by the unprivileged health script. A guest with
//! compromised root can forge the verdict. Readiness proves liveness,
//! not guest integrity.
//!
//! Subiquity can copy installer logs after late commands. The seed supplies
//! no reusable credentials. Acceptance on a real install must confirm that
//! this remaining installer residue contains no secrets.

use super::media::{Image, MAX_GUEST};
use crate::vm::Id;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{self, Read, Write},
};

#[derive(Clone, Copy)]
pub struct AssetPin {
    pub length: u64,
    pub sha256: [u8; 32],
}

impl AssetPin {
    /// Read at most the pinned length plus one byte. The EOF probe detects an
    /// overlong asset without reading the rest, even on an unending stream.
    fn verify(self, mut input: impl Read) -> Result<(), RecipeError> {
        let mut digest = Sha256::new();
        let mut buffer = [0; 16 * 1024];
        let mut remaining = self.length;
        while remaining > 0 {
            let used = remaining.min(buffer.len() as u64) as usize;
            input
                .read_exact(&mut buffer[..used])
                .map_err(|_| RecipeError::Unqualified)?;
            digest.update(&buffer[..used]);
            remaining -= used as u64;
        }
        loop {
            match input.read(&mut buffer[..1]) {
                Ok(0) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                _ => return Err(RecipeError::Unqualified),
            }
        }
        if digest.finalize()[..] != self.sha256 {
            return Err(RecipeError::Unqualified);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum RecipeError {
    Unqualified,
    MediaLimit,
    Write(io::ErrorKind),
}

impl fmt::Display for RecipeError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            Self::Unqualified => "recipe_unqualified",
            Self::MediaLimit => "seed_media_limit",
            Self::Write(_) => "seed_media_write",
        })
    }
}

impl std::error::Error for RecipeError {}

/// Pins supplied by owner-private configuration. A missing pin stays
/// unverified. Provenance and custody are the responsibility of the caller
/// supplying verified pins and already-open asset handles.
#[derive(Default)]
pub struct UbuntuRecipe {
    pub iso: Option<AssetPin>,
    pub kernel: Option<AssetPin>,
    pub initrd: Option<AssetPin>,
    pub guest: Option<AssetPin>,
    pub service: Option<AssetPin>,
    pub finalize: Option<AssetPin>,
    /// Pin the versioned NoCloud profile and every fixed seed literal.
    pub nocloud_schema: Option<AssetPin>,
    pub firmware_code: Option<AssetPin>,
    pub firmware_vars: Option<AssetPin>,
    pub qemu_receipt: Option<AssetPin>,
}

/// Host-owned readers positioned at the start of each asset. The receipt is
/// the reviewed QEMU, qemu-img and distribution receipt as one pinned asset.
#[derive(Default)]
pub struct UbuntuAssets<'a> {
    pub iso: Option<&'a mut dyn Read>,
    pub kernel: Option<&'a mut dyn Read>,
    pub initrd: Option<&'a mut dyn Read>,
    pub firmware_code: Option<&'a mut dyn Read>,
    pub firmware_vars: Option<&'a mut dyn Read>,
    pub qemu_receipt: Option<&'a mut dyn Read>,
}

impl UbuntuRecipe {
    pub const ID: &str = "ubuntu-noble-amd64-v1";
    pub const ISO_NAME: &str = "ubuntu-24.04.3-live-server-amd64.iso";
    pub const ISO_SHA256: [u8; 32] = [
        0xc3, 0x51, 0x4b, 0xf0, 0x05, 0x61, 0x80, 0xd0, 0x93, 0x76, 0x46, 0x2a, 0x7a, 0x1b, 0x4f,
        0x21, 0x3c, 0x1d, 0x6e, 0x8e, 0xa6, 0x7f, 0xae, 0x5c, 0x25, 0x09, 0x9c, 0x6f, 0xd3, 0xd8,
        0x27, 0x4b,
    ];
    // Arxcovo's NoCloud seed schema identifier, including the newline.
    const NOCLOUD_SCHEMA: &[u8] = b"arxcovo-nocloud-v2\n";

    /// SHA256 and length of the schema identifier, user-data template, guest
    /// unit, finalizer, audit unit and audit script, separated by NUL bytes.
    pub const NOCLOUD_PROFILE_PIN: AssetPin = AssetPin {
        length: 4757,
        sha256: [
            0x37, 0xaa, 0xba, 0x54, 0x48, 0x07, 0x89, 0x14, 0xff, 0x93, 0xf6, 0x46, 0xcc, 0xe7,
            0x8d, 0x26, 0x55, 0x88, 0xf1, 0xa1, 0x4d, 0x5d, 0x34, 0xe6, 0x62, 0x55, 0x22, 0x95,
            0xae, 0xe0, 0x57, 0x2c,
        ],
    };

    pub const SERVICE: &[u8] = br"[Unit]
Description=Arxcovo guest supervisor
Wants=arx-account-audit.service
After=systemd-udevd.service arx-account-audit.service

[Service]
Type=simple
User=root
ExecStartPre=/usr/bin/udevadm wait --timeout=10 /dev/virtio-ports/org.arxcovo.agent.0
ExecStart=/usr/local/bin/arx-guest serve-virtio
Delegate=pids
DelegateSubgroup=arx-supervisor

[Install]
WantedBy=multi-user.target
";

    const ACCOUNT_AUDIT_SERVICE: &[u8] = br"[Unit]
Description=Arxcovo boot account audit
Before=arx-guest.service

[Service]
Type=oneshot
User=root
Group=root
RemainAfterExit=yes
RuntimeDirectory=arx-account-audit
RuntimeDirectoryMode=0755
ExecStart=/usr/local/sbin/arx-account-audit

[Install]
WantedBy=multi-user.target
";

    const ACCOUNT_AUDIT_SCRIPT: &[u8] = br#"#!/bin/sh
set -eu
export LC_ALL=C
result=/run/arx-account-audit/result
rm -f -- "$result"
boot_id=$(cat /proc/sys/kernel/random/boot_id)

audit_accounts() {
    test ! -e /root/.ssh && test ! -L /root/.ssh || return 1
    for key in /etc/ssh/ssh_host_*; do
        test ! -e "$key" && test ! -L "$key" || return 1
    done
    awk -F: '
        FILENAME == ARGV[1] {
            if (NF != 9 || $1 == "" || locked[$1]++ || $2 !~ /^[!*]/) bad = 1
            next
        }
        {
            if (NF != 7 || !($1 in locked) || seen[$1]++ || ($2 != "x" && $2 != "*")) bad = 1
            if ($1 == "root") {
                if ($3 != "0") bad = 1
                root = 1
            } else if ($3 !~ /^[0-9]+$/ || $3 + 0 == 0 ||
                       $7 !~ /^\/(usr\/)?(sbin\/nologin|bin\/false|bin\/sync|sbin\/halt|sbin\/shutdown)$/) {
                bad = 1
            }
        }
        END { if (bad || !root) exit 1 }
    ' /etc/shadow /etc/passwd
}

verdict=account-check-failed
if audit_accounts; then verdict=ok; fi
entry=$(mktemp "$result.XXXXXX")
trap 'rm -f -- "$entry"' EXIT
printf '%s %s\n' "$verdict" "$boot_id" > "$entry"
chmod 0644 "$entry"
mv -T -- "$entry" "$result"
"#;

    pub const FINALIZE: &[u8] = br#"#!/bin/sh
set -eu

rm -f /etc/arxcovo/template-finalized
install -m 0644 /dev/null /etc/cloud/cloud-init.disabled
rm -f /etc/cloud/cloud.cfg.d/99-installer.cfg
rm -rf /var/lib/cloud /var/log/installer /var/lib/subiquity /var/lib/arx-guest
rm -f /var/log/cloud-init.log /var/log/cloud-init-output.log
rm -rf /root/.ssh
rm -f /etc/ssh/ssh_host_* /var/lib/systemd/random-seed

# An empty regular machine-id permits a transient ID on the first boot.
# Drop the persisted D-Bus fallback and point subsequent lookups to systemd.
rm -f /etc/machine-id /var/lib/dbus/machine-id
install -m 0444 /dev/null /etc/machine-id
install -d -m 0755 /var/lib/dbus
ln -s /etc/machine-id /var/lib/dbus/machine-id

systemctl --root=/ enable arx-account-audit.service
rm -f /usr/local/sbin/arx-finalize
install -d -m 0755 /etc/arxcovo
printf '%s\n' ubuntu-noble-amd64-v1 > /etc/arxcovo/template-finalized
"#;

    /// Verify all pins before writing any seed bytes. The build ID is a
    /// host-generated opaque ID, and the binary is borrowed from a bounded
    /// in-memory snapshot so the bytes written are the bytes checked.
    pub fn write_seed(
        &self,
        assets: UbuntuAssets<'_>,
        build_id: Id,
        guest: &[u8],
        out: impl Write,
    ) -> Result<(), RecipeError> {
        let iso = required(self.iso)?;
        if iso.sha256 != Self::ISO_SHA256 {
            return Err(RecipeError::Unqualified);
        }
        let external = [
            (self.kernel, assets.kernel),
            (self.initrd, assets.initrd),
            (self.firmware_code, assets.firmware_code),
            (self.firmware_vars, assets.firmware_vars),
            (self.qemu_receipt, assets.qemu_receipt),
            (self.iso, assets.iso),
        ];
        for (pin, input) in &external {
            required(*pin)?;
            if input.is_none() {
                return Err(RecipeError::Unqualified);
            }
        }
        let (meta, user) = self.seed_config(build_id, guest)?;
        let image = image(guest, &meta, &user)?;
        for (pin, input) in external {
            required(pin)?.verify(input.ok_or(RecipeError::Unqualified)?)?;
        }
        image
            .write(out)
            .map_err(|error| RecipeError::Write(error.kind()))
    }

    /// Installer-side late command appended only by TEST Ubuntu seeds. It
    /// leaves `/etc/netplan/00-installer-config.yaml` intact so the offline
    /// installer network stays off, and adds the installed VirtIO TEST route.
    pub const TEST_ROUTE_COMMAND: &str = r#"set -eu
umask 077
file=/target/etc/netplan/60-arxcovo-test.yaml
test ! -e "$file"
test ! -L "$file"
cat > "$file" <<'ARX_NETPLAN'
network:
  version: 2
  renderer: networkd
  ethernets:
    arxcovo-test:
      match:
        driver: virtio_net
      dhcp4: true
      dhcp6: false
      accept-ra: false
      link-local: []
      routes:
        - to: default
          via: 10.0.2.2
ARX_NETPLAN
chmod 0600 "$file"
"#;

    /// Encode the seed for offline inspection in memory. This verifies the
    /// embedded assets only. Staging media for a build requires `write_seed`,
    /// which also verifies the pinned host-side assets and specific ISO.
    pub fn render_seed(&self, build_id: Id, guest: &[u8]) -> Result<Vec<u8>, RecipeError> {
        let (meta, user) = self.seed_config(build_id, guest)?;
        let mut bytes = Vec::new();
        image(guest, &meta, &user)?
            .write(&mut bytes)
            .map_err(|error| RecipeError::Write(error.kind()))?;
        Ok(bytes)
    }

    /// Encode the Ubuntu TEST seed for offline inspection in memory. Appends
    /// the TEST-only VirtIO netplan late command without modifying the shared
    /// offline finalizer or NoCloud profile pin.
    pub fn render_test_seed(&self, build_id: Id, guest: &[u8]) -> Result<Vec<u8>, RecipeError> {
        let mut out = Vec::new();
        append_test_route_seed(&self.render_seed(build_id, guest)?, &mut out)?;
        Ok(out)
    }

    /// Verify all pinned host-side and embedded assets via `write_seed` before
    /// writing the Ubuntu TEST seed with the TEST-only netplan late command.
    pub fn write_test_seed(
        &self,
        assets: UbuntuAssets<'_>,
        build_id: Id,
        guest: &[u8],
        out: impl Write,
    ) -> Result<(), RecipeError> {
        let mut base = Vec::new();
        self.write_seed(assets, build_id, guest, &mut base)?;
        append_test_route_seed(&base, out)
    }

    /// Compute the sealed Ubuntu TEST recipe digest over the recipe ID,
    /// seven external asset pins, and the embedded TEST recipe content
    /// (service unit, shared finalizer, NoCloud profile pin, TEST route late
    /// command, detached health script, and canonical NoCloud documents).
    #[cfg(any(test, all(windows, target_arch = "x86_64")))]
    pub(in crate::vm) fn test_recipe_digest(&self) -> Result<[u8; 32], RecipeError> {
        let schema = required(self.nocloud_schema)?;
        verify_nocloud_profile(schema)?;
        let user = user_data(
            required(self.guest)?,
            required(self.service)?,
            required(self.finalize)?,
        );
        let meta = nocloud_meta(TEST_RECIPE_DIGEST_BUILD_ID);
        let test_user = append_test_route_user_data(&user)?;
        self.test_recipe_digest_with(
            Self::SERVICE,
            Self::FINALIZE,
            Self::NOCLOUD_PROFILE_PIN,
            Self::TEST_ROUTE_COMMAND,
            DETACHED_HEALTH,
            (&meta, &test_user),
        )
    }

    #[cfg(any(test, all(windows, target_arch = "x86_64")))]
    fn test_recipe_digest_with(
        &self,
        service_bytes: &[u8],
        finalize_bytes: &[u8],
        nocloud_profile: AssetPin,
        test_route_command: &str,
        detached_health: &str,
        (meta, user): (&str, &str),
    ) -> Result<[u8; 32], RecipeError> {
        let service = required(self.service)?;
        let finalize = required(self.finalize)?;
        let schema = required(self.nocloud_schema)?;
        service.verify(service_bytes)?;
        finalize.verify(finalize_bytes)?;
        if schema.length != nocloud_profile.length || schema.sha256 != nocloud_profile.sha256 {
            return Err(RecipeError::Unqualified);
        }
        let mut hash = Sha256::new()
            .chain_update(b"arxcovo.recipe.ubuntu-test.v2\0")
            .chain_update(Self::ID);
        for pin in [
            self.iso,
            self.kernel,
            self.initrd,
            self.guest,
            self.firmware_code,
            self.firmware_vars,
            self.qemu_receipt,
        ] {
            let pin = required(pin)?;
            hash.update(pin.length.to_be_bytes());
            hash.update(pin.sha256);
        }
        for bytes in [
            service_bytes,
            finalize_bytes,
            test_route_command.as_bytes(),
            detached_health.as_bytes(),
            meta.as_bytes(),
            user.as_bytes(),
        ] {
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(Sha256::digest(bytes));
        }
        hash.update(nocloud_profile.length.to_be_bytes());
        hash.update(nocloud_profile.sha256);
        Ok(hash.finalize().into())
    }

    fn seed_config(&self, build_id: Id, guest: &[u8]) -> Result<(String, String), RecipeError> {
        let guest_pin = required(self.guest)?;
        let service = required(self.service)?;
        let finalize = required(self.finalize)?;
        let schema = required(self.nocloud_schema)?;
        if guest.len() > MAX_GUEST {
            return Err(RecipeError::MediaLimit);
        }
        guest_pin.verify(guest)?;
        service.verify(Self::SERVICE)?;
        finalize.verify(Self::FINALIZE)?;
        verify_nocloud_profile(schema)?;
        Ok((
            nocloud_meta(build_id),
            user_data(guest_pin, service, finalize),
        ))
    }
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
const TEST_RECIPE_DIGEST_BUILD_ID: Id = [0; 16];

fn nocloud_meta(build_id: Id) -> String {
    format!(
        "instance-id: arx-install-{}\nlocal-hostname: arxcovo\n",
        hex::encode(build_id),
    )
}

fn verify_nocloud_profile(schema: AssetPin) -> Result<(), RecipeError> {
    let expected = UbuntuRecipe::NOCLOUD_PROFILE_PIN;
    if schema.length != expected.length || schema.sha256 != expected.sha256 {
        return Err(RecipeError::Unqualified);
    }
    let separator: &[u8] = b"\0";
    schema.verify(
        UbuntuRecipe::NOCLOUD_SCHEMA
            .chain(separator)
            .chain(USER_DATA_TEMPLATE.as_bytes())
            .chain(separator)
            .chain(UbuntuRecipe::SERVICE)
            .chain(separator)
            .chain(UbuntuRecipe::FINALIZE)
            .chain(separator)
            .chain(UbuntuRecipe::ACCOUNT_AUDIT_SERVICE)
            .chain(separator)
            .chain(UbuntuRecipe::ACCOUNT_AUDIT_SCRIPT),
    )
}

/// Probe-only NoCloud seed. The live guest reports the launch nonce, NIC
/// setup observations and socket outcomes from a fixed program. The seed has no
/// autoinstall stanza, guest service, bootloader action or credential.
#[cfg(test)]
pub(in crate::vm) fn network_probe_seed(
    nonce: &[u8; 16],
    targets: &crate::vm::guest::network_probe::Targets,
) -> Result<Vec<u8>, RecipeError> {
    use std::fmt::Write;
    if !targets.valid() {
        return Err(RecipeError::Unqualified);
    }
    let encoded_nonce = hex::encode(nonce);
    let endpoints = targets.guest_tcp();
    let mut script = String::from(PROBE_START);
    writeln!(script, "NONCE = bytes.fromhex({encoded_nonce:?})").expect("String write");
    writeln!(script, "TCP = [").expect("String write");
    for address in endpoints {
        writeln!(
            script,
            "    ({:?}, {}),",
            address.ip().to_string(),
            address.port()
        )
        .expect("String write");
    }
    writeln!(script, "]").expect("String write");
    writeln!(
        script,
        "DNS = ({:?}, {})",
        targets.dns.ip().to_string(),
        targets.dns.port()
    )
    .expect("String write");
    script.push_str(PROBE_BODY);
    script.push_str(PROBE_ENTRY);
    write_probe_seed(&encoded_nonce, &script)
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub(in crate::vm) fn network_witness_seed(
    nonce: &[u8; 16],
    targets: &crate::vm::windows::qualification::WitnessTargets,
) -> Result<Vec<u8>, RecipeError> {
    network_witness_seed_for(nonce, targets.tcp, targets.udp, targets.dns)
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
fn network_witness_seed_for(
    nonce: &[u8; 16],
    tcp: [std::net::SocketAddr; 3],
    udp: [std::net::SocketAddr; 3],
    dns: std::net::SocketAddr,
) -> Result<Vec<u8>, RecipeError> {
    use std::fmt::Write;
    let encoded_nonce = hex::encode(nonce);
    let mut script = String::from(PROBE_START);
    writeln!(script, "NONCE = bytes.fromhex({encoded_nonce:?})").expect("String write");
    for (kind, addresses) in [("TCP", tcp), ("UDP", udp)] {
        writeln!(script, "{kind} = [").expect("String write");
        for address in addresses {
            writeln!(
                script,
                "    ({:?}, {}),",
                address.ip().to_string(),
                address.port()
            )
            .expect("String write");
        }
        writeln!(script, "]").expect("String write");
    }
    writeln!(script, "DNS = ({:?}, {})", dns.ip().to_string(), dns.port()).expect("String write");
    script.push_str(PROBE_BODY);
    script.push_str(WITNESS_ENTRY);
    write_probe_seed(&encoded_nonce, &script)
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
fn write_probe_seed(encoded_nonce: &str, script: &str) -> Result<Vec<u8>, RecipeError> {
    use std::fmt::Write;
    let mut cloud = String::from(
        "#cloud-config\nwrite_files:\n  - path: /run/arxcovo-network-probe.py\n    permissions: '0600'\n    content: |\n",
    );
    for line in script.lines() {
        writeln!(cloud, "      {line}").expect("String write");
    }
    cloud.push_str("runcmd:\n  - [python3, /run/arxcovo-network-probe.py]\n");
    let metadata = format!("instance-id: arxcovo-net-{encoded_nonce}\n");
    let image =
        Image::probe([metadata.as_bytes(), cloud.as_bytes()]).ok_or(RecipeError::MediaLimit)?;
    let mut output = Vec::new();
    image
        .write(&mut output)
        .map_err(|e| RecipeError::Write(e.kind()))?;
    Ok(output)
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
const PROBE_START: &str = "import fcntl, ipaddress, pathlib, socket, struct, subprocess, time\n";

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
const PROBE_BODY: &str = r#"
def nic_ipv4(name):
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            packed = fcntl.ioctl(s.fileno(), 0x8915, struct.pack('256s', name.encode('ascii')[:15]))
            return socket.inet_ntoa(packed[20:24])
    except OSError:
        return None

def has_slirp_default_route(name):
    try:
        for line in pathlib.Path('/proc/net/route').read_text().splitlines()[1:]:
            cols = line.split()
            if len(cols) >= 4 and cols[0] == name and cols[1] == '00000000' and cols[2] == '0202000A':
                flags = int(cols[3], 16)
                if flags & 0x3 == 0x3:
                    return True
    except OSError:
        pass
    return False

def ipv6_is_disabled(name):
    try:
        for line in pathlib.Path('/proc/net/if_inet6').read_text().splitlines():
            cols = line.split()
            if len(cols) >= 6 and cols[5] == name:
                if len(cols[0]) != 32 or not ipaddress.IPv6Address(int(cols[0], 16)).is_link_local:
                    return False
        for line in pathlib.Path('/proc/net/ipv6_route').read_text().splitlines():
            cols = line.split()
            if len(cols) >= 10 and cols[9] == name and cols[0] == '0' * 32 and cols[1] == '00':
                return False
    except (OSError, ValueError):
        return False
    return True

def probe_tcp(target, bind_v4):
    is_v6 = ':' in target[0]
    family = socket.AF_INET6 if is_v6 else socket.AF_INET
    try:
        with socket.socket(family, socket.SOCK_STREAM) as conn:
            conn.settimeout(1.0)
            if not is_v6:
                conn.bind((bind_v4, 0))
            conn.connect(target)
            return 'R'
    except OSError:
        return 'E'

def probe_dns(target, bind_v4):
    label = b'arx-' + NONCE.hex().encode('ascii')
    ident = NONCE[:2]
    question = bytes([len(label)]) + label + b'\x07invalid\x00\x00\x01\x00\x01'
    payload = ident + bytes.fromhex('01000001000000000000') + question
    is_v6 = ':' in target[0]
    family = socket.AF_INET6 if is_v6 else socket.AF_INET
    try:
        with socket.socket(family, socket.SOCK_DGRAM) as conn:
            conn.settimeout(1.0)
            if not is_v6:
                conn.bind((bind_v4, 0))
            conn.connect(target)
            conn.send(payload)
            reply = conn.recv(512)
            return dns_reply(payload, reply)
    except OSError:
        return 'E'

def dns_reply(query, reply):
    if (
        len(reply) >= len(query)
        and reply[:2] == query[:2]
        and (reply[2] & 0x80) != 0
        and ((reply[2] >> 3) & 0x0f) == 0
        and (reply[2] & 0x02) == 0
        and (reply[3] & 0x0f) in (0, 3)
        and reply[4:6] == b'\x00\x01'
        and reply[12:len(query)] == query[12:]
    ):
        return 'R'
    return 'M'

def nic_setup(devices):
    ports = [p for p in devices if p.name != 'lo']
    nic_ready = (
        len(ports) == 1
        and (ports[0] / 'device/driver').resolve().name == 'virtio_net'
        and (ports[0] / 'operstate').read_text().strip() == 'up'
    )
    guest_v4 = None
    default_route = False
    no_v6 = False
    if nic_ready:
        name = ports[0].name
        for _ in range(40):
            guest_v4 = nic_ipv4(name)
            # The sealed TEST netdev uses the default slirp subnet. Restricted
            # DHCP supplies the address but may omit the router option.
            if guest_v4 == '10.0.2.15' and not has_slirp_default_route(name):
                try:
                    subprocess.run(['ip', '-4', 'route', 'add', 'default', 'via', '10.0.2.2',
                                    'dev', name], check=False, timeout=1)
                except (OSError, subprocess.TimeoutExpired):
                    pass
            default_route = has_slirp_default_route(name)
            if guest_v4 == '10.0.2.15' and default_route:
                break
            time.sleep(0.25)
        no_v6 = ipv6_is_disabled(name)

    ipv4_lease = guest_v4 == '10.0.2.15'
    setup = ''.join('1' if flag else '0' for flag in (nic_ready, ipv4_lease, default_route, no_v6))
    return setup, guest_v4, ports[0].name if nic_ready else None

def run_probes(devices):
    setup, guest_v4, _ = nic_setup(devices)
    if setup == '1111':
        outcomes = ''.join(probe_tcp(t, guest_v4) for t in TCP)
        outcomes += probe_dns(DNS, guest_v4) + probe_dns(('10.0.2.3', 53), guest_v4)
    else:
        outcomes = 'E' * 8

    return setup, outcomes
"#;

#[cfg(test)]
const PROBE_ENTRY: &str = r#"
if __name__ == '__main__':
    port = pathlib.Path('/dev/virtio-ports/org.arxcovo.agent.0')
    for _ in range(40):
        if port.exists():
            break
        time.sleep(0.25)

    setup, outcomes = run_probes(pathlib.Path('/sys/class/net').iterdir())
    if port.exists():
        with port.open('wb', buffering=0) as wire:
            wire.write(b'ARXNET2 ' + NONCE.hex().encode('ascii') + b' ' + setup.encode('ascii') + b' ' + outcomes.encode('ascii') + b'\n')
"#;

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
const WITNESS_ENTRY: &str = r#"
import errno

def witness_attempt(target, source, nic, tcp, payload=b''):
    v6 = ':' in target[0]
    family = socket.AF_INET6 if v6 else socket.AF_INET
    mode = socket.SOCK_STREAM if tcp else socket.SOCK_DGRAM
    try:
        with socket.socket(family, mode) as conn:
            conn.settimeout(1.0)
            conn.setsockopt(socket.SOL_SOCKET, socket.SO_BINDTODEVICE, nic.encode('ascii') + b'\0')
            if not v6:
                conn.bind((source, 0))
            try:
                if tcp:
                    conn.connect(target)
                    return 'R'
                return 'S' if conn.sendto(payload, target) == len(payload) else 'E'
            except TimeoutError:
                return 'T'
            except OSError as error:
                if v6 and error.errno in (errno.ENETUNREACH, errno.EADDRNOTAVAIL):
                    return 'L'
                if not v6 and tcp and error.errno in (errno.ECONNREFUSED, errno.ECONNRESET):
                    return 'F'
                return 'E'
    except OSError:
        return 'E'

def gateway_reachable(nic):
    try:
        reply = subprocess.run(['ip', '-4', 'neigh', 'get', '10.0.2.2', 'dev', nic],
                               timeout=1, check=True, capture_output=True).stdout.split()
    except (OSError, subprocess.SubprocessError):
        return False
    return bool(reply and reply[0] == b'10.0.2.2' and b'lladdr' in reply and b'REACHABLE' in reply)

def witness_dns(target, source, nic, query):
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as conn:
            conn.settimeout(1.0)
            conn.setsockopt(socket.SOL_SOCKET, socket.SO_BINDTODEVICE, nic.encode('ascii') + b'\0')
            conn.bind((source, 0))
            conn.connect(target)
            if conn.send(query) != len(query):
                return 'E'
            try:
                return dns_reply(query, conn.recv(512))
            except TimeoutError:
                # A sent query only. The independent witness determines
                # whether the datagram crossed the restricted backend.
                return 'S'
    except OSError:
        return 'E'

def witness_results(devices):
    setup, source, nic = nic_setup(devices)
    if setup != '1111':
        return setup, '0', 'EEEEEEEE'
    # First issue the host-alias UDP probe. Resolving the gateway neighbor
    # also yields the positive on-link slirp control, separate from IP egress.
    alias = witness_attempt(UDP[2], source, nic, False, NONCE)
    for _ in range(10):
        if gateway_reachable(nic):
            control = '1'
            break
        time.sleep(0.1)
    else:
        control = '0'
    tcp = ''.join(witness_attempt(target, source, nic, True) for target in TCP)
    udp = ''.join(witness_attempt(target, source, nic, False, NONCE) for target in UDP[:2]) + alias
    label = b'arx-' + NONCE.hex().encode('ascii')
    query = NONCE[:2] + bytes.fromhex('01000001000000000000') + bytes([len(label)]) + label
    query += b'\x07invalid\x00\x00\x01\x00\x01'
    dns = ''.join(witness_dns(target, source, nic, query) for target in (DNS, ('10.0.2.3', 53)))
    return setup, control, tcp + udp + dns

if __name__ == '__main__':
    port = pathlib.Path('/dev/virtio-ports/org.arxcovo.agent.0')
    for _ in range(40):
        if port.exists():
            break
        time.sleep(0.25)
    setup, control, outcomes = witness_results(pathlib.Path('/sys/class/net').iterdir())
    if port.exists():
        with port.open('wb', buffering=0) as wire:
            wire.write(('ARXNET3 ' + NONCE.hex() + ' ' + setup + ' ' + control + ' ' + outcomes + '\n').encode('ascii'))
"#;

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
const INSTALLED_ROUTE_ASSERTION: &str =
    "assert ip('-4','route','show','default','dev',n)[:3]==[b'default',b'via',b'10.0.2.2']\n";

#[cfg(all(windows, target_arch = "x86_64"))]
pub(in crate::vm) fn installed_witness_script(
    nonce: [u8; 16],
    targets: &crate::vm::windows::qualification::WitnessTargets,
) -> Result<String, RecipeError> {
    installed_witness_script_for(nonce, targets.tcp, targets.udp, targets.dns)
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
fn installed_witness_script_for(
    nonce: [u8; 16],
    tcp: [std::net::SocketAddr; 3],
    udp: [std::net::SocketAddr; 3],
    dns: std::net::SocketAddr,
) -> Result<String, RecipeError> {
    use std::fmt::Write;
    let mut script = String::from(
        "set -eu\n/usr/bin/python3 - <<'ARX_NETWORK'\nimport pathlib, socket, subprocess, time\n",
    );
    writeln!(script, "NONCE=bytes.fromhex({:?})", hex::encode(nonce)).expect("String write");
    for (name, addresses) in [
        ("TCP", tcp.to_vec()),
        ("UDP", udp.to_vec()),
        ("DNS", vec![dns, "10.0.2.3:53".parse().expect("fixed")]),
    ] {
        writeln!(script, "{name}=[").expect("String write");
        for target in addresses {
            writeln!(script, "({:?},{}),", target.ip().to_string(), target.port())
                .expect("String write");
        }
        script.push_str("]\n");
    }
    let functions = WITNESS_ENTRY
        .split("def witness_results(")
        .next()
        .expect("fixed script");
    script.push_str(functions);
    let dns = PROBE_BODY
        .split("def dns_reply(")
        .nth(1)
        .expect("fixed script");
    script.push_str("def dns_reply(");
    script.push_str(dns.split("def nic_setup(").next().expect("fixed script"));
    script.push_str(r#"
ports=[p for p in pathlib.Path('/sys/class/net').iterdir() if p.name!='lo']
assert len(ports)==1 and (ports[0]/'device/driver').resolve().name=='virtio_net'
assert (ports[0]/'operstate').read_text().strip()=='up'
n=ports[0].name
def ip(*args):
    return subprocess.run(['/usr/sbin/ip', *args],check=True,timeout=1,capture_output=True).stdout.split()
assert b'10.0.2.15/24' in ip('-4','addr','show','dev',n)
"#);
    script.push_str(INSTALLED_ROUTE_ASSERTION);
    script.push_str(
        r#"
assert b'inet6' not in ip('-6','addr','show','dev',n,'scope','global')
assert not ip('-6','route','show','default','dev',n)
a=witness_attempt(UDP[2],'10.0.2.15',n,False,NONCE)
for _ in range(10):
    if gateway_reachable(n): break
    time.sleep(.1)
else: raise SystemExit(1)
t=[witness_attempt(d,'10.0.2.15',n,True) for d in TCP]
u=[witness_attempt(d,'10.0.2.15',n,False,NONCE) for d in UDP[:2]]+[a]
x=b'arx-'+NONCE.hex().encode()
q=NONCE[:2]+bytes.fromhex('01000001000000000000')+bytes([len(x)])+x+b'\x07invalid\0\0\1\0\1'
assert t[0] in 'FT' and t[1]=='L' and t[2] in 'FT' and u==list('SLS')
assert [witness_dns(d,'10.0.2.15',n,q) for d in DNS]==['S','S']
ARX_NETWORK
"#,
    );
    let script = script
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    if script.len() > arx_guest::protocol::MAX_SCRIPT {
        return Err(RecipeError::MediaLimit);
    }
    Ok(script)
}

pub(in crate::vm) const DETACHED_HEALTH: &str = r#"set -eu
export LC_ALL=C
verify_account_result() {
    boot_id=$(cat "$1")
    result=$2
    test ! -L "$result"
    test -f "$result"
    test "$(stat -c '%u:%g:%a:%s' -- "$result")" = 0:0:644:40
    printf 'ok %s\n' "$boot_id" | cmp -s - "$result"
}
verify_account_result /proc/sys/kernel/random/boot_id /run/arx-account-audit/result
printf '%s\n' ubuntu-noble-amd64-v1 | cmp -s - /etc/arxcovo/template-finalized
test -x /usr/local/bin/arx-guest
test -f /etc/systemd/system/arx-guest.service
systemctl is-active --quiet arx-guest.service
test -f /etc/cloud/cloud-init.disabled
test ! -e /var/lib/cloud/seed
test ! -e /etc/cloud/cloud.cfg.d/99-installer.cfg
test ! -e /usr/sbin/sshd
test "$(wc -c </etc/machine-id)" -eq 33
grep -Eq '^[0-9a-f]{32}$' /etc/machine-id
test "$(cat /etc/machine-id)" != 00000000000000000000000000000000
test "$(readlink /var/lib/dbus/machine-id)" = /etc/machine-id
"#;

#[cfg(test)]
pub(super) fn health_uses_public_paths(script: &str) -> bool {
    script
        .split(|c: char| c.is_ascii_whitespace() || r#"<>\"'();="#.contains(c))
        .filter(|word| word.starts_with('/'))
        .all(|path| {
            matches!(
                path,
                "/proc/sys/kernel/random/boot_id"
                    | "/run/arx-account-audit/result"
                    | "/etc/arxcovo/template-finalized"
                    | "/usr/local/bin/arx-guest"
                    | "/etc/systemd/system/arx-guest.service"
                    | "/etc/cloud/cloud-init.disabled"
                    | "/var/lib/cloud/seed"
                    | "/etc/cloud/cloud.cfg.d/99-installer.cfg"
                    | "/usr/sbin/sshd"
                    | "/etc/machine-id"
                    | "/var/lib/dbus/machine-id"
            )
        })
}

#[cfg(all(test, unix))]
#[test]
fn unprivileged_receipt_check_requires_current_root_owned_success() {
    use std::{fs, process::Command};

    let (predicate, _) = DETACHED_HEALTH
        .split_once("\nverify_account_result /proc/")
        .unwrap();
    // Account metadata is synthetic, while sh executes the actual decision
    // and compares the verdict and boot ID in a fixture's public files.
    let script = format!(
        "{predicate}\n{}",
        r#"
stat() {
    test "$1:$2:$3:$4" = "-c:%u:%g:%a:%s:--:$RESULT_PATH" || return 1
    printf '%s\n' "$RESULT_METADATA"
}
verify_account_result "$1" "$2"
"#
    );
    let root = tempfile::tempdir().unwrap();
    let receipt = root.path().join("result");
    let boot = root.path().join("boot_id");
    let current = "00000000-0000-0000-0000-000000000001";
    let previous = "00000000-0000-0000-0000-000000000002";
    fs::write(&boot, format!("{current}\n")).unwrap();
    for (category, id, meta, expected) in [
        ("ok", current, "0:0:644:40", true),
        ("ok", previous, "0:0:644:40", false),
        ("account-check-failed", current, "0:0:644:58", false),
        ("ok", current, "65534:65534:644:40", false),
        ("ok", current, "0:0:666:40", false),
        ("no", current, "0:0:644:40", false),
    ] {
        fs::write(&receipt, format!("{category} {id}\n")).unwrap();
        let status = Command::new("sh")
            .args(["-c", &script, "account-result-fixture"])
            .arg(&boot)
            .arg(&receipt)
            .env("RESULT_PATH", &receipt)
            .env("RESULT_METADATA", meta)
            .status()
            .unwrap();
        assert_eq!(status.success(), expected, "{category} {id} {meta}");
    }
}

fn required(pin: Option<AssetPin>) -> Result<AssetPin, RecipeError> {
    pin.filter(|pin| pin.length != 0)
        .ok_or(RecipeError::Unqualified)
}

fn append_test_route_seed(base: &[u8], out: impl Write) -> Result<(), RecipeError> {
    let mut files = super::ubuntu_web::cidata_files(base).ok_or(RecipeError::Unqualified)?;
    let user = std::str::from_utf8(files[6]).map_err(|_| RecipeError::Unqualified)?;
    let user = append_test_route_user_data(user)?;
    files[6] = user.as_bytes();
    Image::new(files)
        .ok_or(RecipeError::Unqualified)?
        .write(out)
        .map_err(|error| RecipeError::Write(error.kind()))
}

fn append_test_route_user_data(user: &str) -> Result<String, RecipeError> {
    if !user.ends_with(
        "      curtin in-target --target=/target -- /bin/sh /usr/local/sbin/arx-finalize\n",
    ) {
        return Err(RecipeError::Unqualified);
    }
    let mut user = user.to_owned();
    user.push_str("    - |\n");
    for line in UbuntuRecipe::TEST_ROUTE_COMMAND.lines() {
        if !line.is_empty() {
            user.push_str("      ");
            user.push_str(line);
        }
        user.push('\n');
    }
    Ok(user)
}

fn image<'a>(guest: &'a [u8], meta: &'a str, user: &'a str) -> Result<Image<'a>, RecipeError> {
    Image::new([
        UbuntuRecipe::ACCOUNT_AUDIT_SCRIPT,
        UbuntuRecipe::ACCOUNT_AUDIT_SERVICE,
        guest,
        UbuntuRecipe::SERVICE,
        UbuntuRecipe::FINALIZE,
        meta.as_bytes(),
        user.as_bytes(),
    ])
    .ok_or(RecipeError::MediaLimit)
}

fn user_data(guest: AssetPin, service: AssetPin, finalize: AssetPin) -> String {
    USER_DATA_TEMPLATE
        .replace("{guest_hash}", &hex::encode(guest.sha256))
        .replace("{service_hash}", &hex::encode(service.sha256))
        .replace("{finalize_hash}", &hex::encode(finalize.sha256))
        .replace(
            "{audit_unit_hash}",
            &hex::encode(Sha256::digest(UbuntuRecipe::ACCOUNT_AUDIT_SERVICE)),
        )
        .replace(
            "{audit_script_hash}",
            &hex::encode(Sha256::digest(UbuntuRecipe::ACCOUNT_AUDIT_SCRIPT)),
        )
}

const USER_DATA_TEMPLATE: &str = r#"#cloud-config
autoinstall:
  version: 1
  interactive-sections: []
  source:
    id: ubuntu-server
    search_drivers: false
  refresh-installer:
    update: false
  drivers:
    install: false
  ssh:
    install-server: false
    allow-pw: false
    authorized-keys: []
  identity:
    hostname: arxcovo
    username: arx
    password: '!'
  storage:
    layout:
      name: direct
      ptable: gpt
      match:
        path: /dev/vda
  network:
    version: 2
    renderer: networkd
    ethernets:
      disabled:
        match:
          name: '*'
        dhcp4: false
        dhcp6: false
        accept-ra: false
        link-local: []
        activation-mode: 'off'
  apt:
    geoip: false
    fallback: offline-install
    mirror-selection:
      primary:
        - uri: file:///cdrom
          arches: [amd64]
  updates: security
  shutdown: poweroff
  late-commands:
    - |
      set -eu
      seed=/run/arx-cidata
      mkdir "$seed"
      mount -t iso9660 -o ro,nosuid,nodev,noexec -L CIDATA "$seed"
      trap 'umount "$seed"; rmdir "$seed"' EXIT
      (
        cd "$seed"
        printf '%s  %s\n' '{guest_hash}' arx-guest '{service_hash}' arx-guest.service '{finalize_hash}' arx-finalize |
          sha256sum --check --strict
        printf '%s  %s\n' '{audit_unit_hash}' arx-account-audit.service '{audit_script_hash}' arx-account-audit |
          sha256sum --check --strict
      )
      install -m 0755 "$seed/arx-guest" /target/usr/local/bin/arx-guest
      install -m 0644 "$seed/arx-guest.service" /target/etc/systemd/system/arx-guest.service
      install -m 0700 "$seed/arx-finalize" /target/usr/local/sbin/arx-finalize
      install -o 0 -g 0 -m 0644 "$seed/arx-account-audit.service" /target/etc/systemd/system/arx-account-audit.service
      install -o 0 -g 0 -m 0700 "$seed/arx-account-audit" /target/usr/local/sbin/arx-account-audit
      systemctl --root=/target enable arx-guest.service
      curtin in-target --target=/target -- /bin/sh /usr/local/sbin/arx-finalize
"#;

#[cfg(test)]
#[test]
fn network_probe_seed_has_only_live_nocloud_configuration() {
    let nonce = [42; 16];
    let targets = crate::vm::guest::network_probe::fixture_targets();
    let seed = network_probe_seed(&nonce, &targets).unwrap();
    assert!(seed.windows(6).any(|bytes| bytes == b"CIDATA"));
    assert!(
        seed.windows(32)
            .any(|bytes| bytes == hex::encode(nonce).as_bytes())
    );
    assert!(!seed.windows(11).any(|bytes| bytes == b"autoinstall"));
    assert!(seed.len() <= 64 * 1024);
}

#[cfg(test)]
#[test]
fn test_seed_appends_route_without_changing_shared_finalizer() {
    let pin = |bytes: &[u8]| AssetPin {
        length: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    };
    let guest = b"offline-test-guest\x00\xff";
    let host = Some(pin(b"synthetic host asset"));
    let recipe = UbuntuRecipe {
        iso: Some(AssetPin {
            length: 20,
            sha256: UbuntuRecipe::ISO_SHA256,
        }),
        kernel: host,
        initrd: host,
        guest: Some(pin(guest)),
        service: Some(pin(UbuntuRecipe::SERVICE)),
        finalize: Some(pin(UbuntuRecipe::FINALIZE)),
        nocloud_schema: Some(UbuntuRecipe::NOCLOUD_PROFILE_PIN),
        firmware_code: host,
        firmware_vars: host,
        qemu_receipt: host,
    };
    let build = [0x5a; 16];
    let base_iso = recipe.render_seed(build, guest).unwrap();
    let test_iso = recipe.render_test_seed(build, guest).unwrap();
    assert_eq!(test_iso, recipe.render_test_seed(build, guest).unwrap());
    assert_ne!(test_iso, base_iso);
    assert!(
        !base_iso
            .windows(b"arxcovo-test".len())
            .any(|window| window == b"arxcovo-test")
    );
    assert!(
        !base_iso
            .windows(b"60-arxcovo-test.yaml".len())
            .any(|window| window == b"60-arxcovo-test.yaml")
    );

    let base_files = super::ubuntu_web::cidata_files(&base_iso).unwrap();
    let test_files = super::ubuntu_web::cidata_files(&test_iso).unwrap();
    for index in 0..6 {
        assert_eq!(test_files[index], base_files[index], "cidata file {index}");
    }
    assert_eq!(test_files[4], UbuntuRecipe::FINALIZE);
    assert!(
        !UbuntuRecipe::FINALIZE
            .windows(b"arxcovo-test".len())
            .any(|window| window == b"arxcovo-test")
    );

    let base_user = std::str::from_utf8(base_files[6]).unwrap();
    let test_user = std::str::from_utf8(test_files[6]).unwrap();
    let appended = test_user
        .strip_prefix(base_user)
        .unwrap()
        .strip_prefix("    - |\n")
        .unwrap();
    let script = appended
        .lines()
        .map(|line| line.strip_prefix("      ").unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(format!("{script}\n"), UbuntuRecipe::TEST_ROUTE_COMMAND);
    assert_eq!(UbuntuRecipe::TEST_ROUTE_COMMAND.len(), 416);
    assert_eq!(
        hex::encode(Sha256::digest(UbuntuRecipe::TEST_ROUTE_COMMAND.as_bytes())),
        "82d4979ece16cef92ade02e11ee459d7cc29e0c1006e7cd33cf6524ae6bff8ea"
    );
    assert!(test_user.contains("60-arxcovo-test.yaml"));
    assert!(test_user.contains("via: 10.0.2.2"));
    assert!(!test_user.contains("00-installer-config.yaml"));

    let mut bad_files = base_files;
    bad_files[6] = b"#cloud-config\nautoinstall:\n  late-commands: []\n";
    let mut bad_iso = Vec::new();
    Image::new(bad_files).unwrap().write(&mut bad_iso).unwrap();
    let mut out = Vec::new();
    assert!(matches!(
        append_test_route_seed(&bad_iso, &mut out),
        Err(RecipeError::Unqualified)
    ));
    assert!(out.is_empty());
}

#[cfg(test)]
#[test]
fn network_probe_python_fixture() {
    // The interpreter is required only for the explicitly enabled guest-script
    // check. Ordinary local Rust test runs have no host Python dependency.
    // Hosted CI sets CI=true and must name the interpreter; otherwise this
    // check would pass without running.
    let Some(python) = std::env::var_os("ARX_TEST_NETWORK_PROBE_PYTHON") else {
        if std::env::var("CI").ok().as_deref() == Some("true") {
            panic!("ARX_TEST_NETWORK_PROBE_PYTHON must name a Python 3 interpreter when CI=true");
        }
        return;
    };
    // Run the functions embedded in the seed with a fake socket. No packets
    // are sent and the fixture does not inspect the host's routes.
    let fixture = include_str!("network_probe_fixture.py");
    let route = serde_json::to_string(INSTALLED_ROUTE_ASSERTION).unwrap();
    // fcntl is Linux-only. These mocked fixture calls never use its ioctl.
    let program = format!(
        "__name__ = 'fixture'\nimport sys, types\nsys.modules['fcntl'] = types.ModuleType('fcntl')\n{PROBE_START}{PROBE_BODY}\n{PROBE_FIXTURE_BEFORE}\n{fixture}\nROUTE_SOURCE = {route}\n{PROBE_FIXTURE_AFTER}"
    );
    let result = std::process::Command::new(python)
        .args(["-I", "-B", "-c", &program])
        .status()
        .unwrap();
    assert!(result.success());
}

#[cfg(test)]
const PROBE_FIXTURE_BEFORE: &str = r#"
from unittest.mock import MagicMock, patch

sock = MagicMock()
sock.recv.side_effect = socket.timeout('fixture')
NONCE = bytes(16)
with patch.object(socket, 'socket') as constructor:
    constructor.return_value.__enter__.return_value = sock
    assert probe_dns(('192.0.2.1', 53), '10.0.2.15') == 'E'
    sock.send.assert_called_once()
    sock.recv.assert_called_once()

label = b'arx-' + NONCE.hex().encode('ascii')
query = NONCE[:2] + bytes.fromhex('01000001000000000000') + bytes([len(label)]) + label
query += b'\x07invalid\x00\x00\x01\x00\x01'
good = query[:2] + b'\x81\x83' + query[4:]
for reply, expected in [(good, 'R'), (good[:-1], 'M'), (b'\x13' + good[1:], 'M')]:
    sock = MagicMock()
    sock.recv.return_value = reply
    with patch.object(socket, 'socket') as constructor:
        constructor.return_value.__enter__.return_value = sock
        assert probe_dns(('192.0.2.1', 53), '10.0.2.15') == expected
        sock.send.assert_called_once_with(query)

def check_ipv6(addresses, routes, expected):
    def read(path):
        data = {'if_inet6': addresses, 'ipv6_route': routes}[path.name]
        if data is None:
            raise OSError('fixture')
        return data
    with patch.object(pathlib.Path, 'read_text', read):
        assert ipv6_is_disabled('eth0') is expected

local = 'febf0000000000000000000000000001 02 40 20 80 eth0'
global_addr = '20010db8000000000000000000000001 02 40 00 80 eth0'
default_route = ('0' * 32 + ' 00 ' + '0' * 32 + ' 00 ' + '0' * 32 +
                 ' 00000400 00000000 00000000 00000001 eth0')
check_ipv6(local, '', True)
check_ipv6(global_addr, '', False)
check_ipv6(local, default_route, False)
check_ipv6(None, '', False)
check_ipv6(local, None, False)
"#;

#[cfg(test)]
const PROBE_FIXTURE_AFTER: &str = r#"
nic_ipv4 = lambda name: '10.0.2.15'
has_slirp_default_route = lambda name: True
ipv6_is_disabled = lambda name: False
probe_tcp, probe_dns = MagicMock(return_value='R'), MagicMock(return_value='R')
assert run_probes([nic]) == ('1110', 'EEEEEEEE')
probe_tcp.assert_not_called()
probe_dns.assert_not_called()

for tokens, expected in [
    (b'default via 10.0.2.2 dev eth0'.split(), True),
    (b'default via 10.0.2.1 dev eth0'.split(), False),
    ([], False),
]:
    try:
        exec(ROUTE_SOURCE, {'ip': lambda *args: tokens, 'n': 'eth0'})
        seen = True
    except AssertionError:
        seen = False
    assert seen is expected
"#;

#[cfg(test)]
fn valid_python(path: &std::path::Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    let metadata = std::fs::symlink_metadata(path);
    #[cfg(not(windows))]
    let metadata = std::fs::metadata(path);
    let Ok(metadata) = metadata else { return false };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return false;
        }
    }
    metadata.is_file() && metadata.len() > 0
}

#[cfg(test)]
fn resolve_python(override_path: Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    if let Some(path) = override_path {
        return valid_python(&path).then_some(path);
    }
    #[cfg(unix)]
    let names = {
        let system = std::path::PathBuf::from("/usr/bin/python3");
        if valid_python(&system) {
            return Some(system);
        }
        ["python3", "python"]
    };
    #[cfg(windows)]
    let names = ["python.exe", "python3.exe"];
    let paths = std::env::var_os("PATH")?;
    names
        .into_iter()
        .flat_map(|name| {
            std::env::split_paths(&paths)
                .filter(|dir| dir.is_absolute())
                .map(move |dir| dir.join(name))
        })
        .find(|path| valid_python(path))
}

#[cfg(test)]
fn python_executable() -> &'static std::path::Path {
    static PYTHON: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    PYTHON.get_or_init(|| {
        resolve_python(std::env::var_os("ARX_TEST_NETWORK_PROBE_PYTHON").map(Into::into))
            .expect("Python 3 is required for witness script behavioral tests")
    })
}

#[cfg(test)]
#[test]
fn witness_python_rejects_relative_override() {
    assert!(resolve_python(Some("python".into())).is_none());
}

#[cfg(test)]
#[test]
fn witness_python_rejects_missing_override() {
    let dir = tempfile::tempdir().unwrap();
    assert!(resolve_python(Some(dir.path().join("missing-python"))).is_none());
}

#[cfg(test)]
async fn run_witness_interpreter(
    program: &std::path::Path,
    args: &[&str],
    input: &str,
    seconds: u64,
) -> Vec<u8> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    assert!(program.is_absolute());
    let output = tokio::time::timeout(std::time::Duration::from_secs(seconds), async {
        let mut child = tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap_or_else(|e| panic!("could not start {}: {e}", program.display()));
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .await
            .unwrap();
        child.wait_with_output().await.unwrap()
    })
    .await
    .unwrap_or_else(|_| panic!("{} exceeded {seconds}s", program.display()));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[cfg(test)]
async fn run_witness_python(mode: &str, meta: &str, input: &str) -> Vec<u8> {
    run_witness_interpreter(
        python_executable(),
        &[
            "-I",
            "-B",
            "-c",
            include_str!("witness_script_fixture.py"),
            mode,
            meta,
        ],
        input,
        60,
    )
    .await
}

#[cfg(test)]
fn extract_probe_script_from_cidata(iso: &[u8]) -> String {
    const BLOCK: usize = 2048;
    let primary = &iso[16 * BLOCK..17 * BLOCK];
    assert_eq!(&primary[..7], b"\x01CD001\x01");
    let root_lba = u32::from_le_bytes(primary[158..162].try_into().unwrap()) as usize;
    let root_len = u32::from_le_bytes(primary[166..170].try_into().unwrap()) as usize;
    let mut dir = &iso[root_lba * BLOCK..root_lba * BLOCK + root_len];
    let mut userdata = None;
    while let Some(&len) = dir.first().filter(|&&len| len != 0) {
        let (rec, rest) = dir.split_at(usize::from(len));
        dir = rest;
        let name = &rec[33..33 + usize::from(rec[32])];
        if name == b"USERDATA.;1" {
            let start = u32::from_le_bytes(rec[2..6].try_into().unwrap()) as usize * BLOCK;
            let size = u32::from_le_bytes(rec[10..14].try_into().unwrap()) as usize;
            userdata = Some(std::str::from_utf8(&iso[start..start + size]).unwrap());
            break;
        }
    }
    let cloud = userdata.expect("USERDATA.;1 in probe ISO");
    let body = cloud
        .strip_prefix("#cloud-config\nwrite_files:\n  - path: /run/arxcovo-network-probe.py\n    permissions: '0600'\n    content: |\n")
        .expect("probe cloud-config prefix")
        .strip_suffix("runcmd:\n  - [python3, /run/arxcovo-network-probe.py]\n")
        .expect("probe cloud-config suffix");
    let mut out = String::new();
    for line in body.lines() {
        out.push_str(line.strip_prefix("      ").expect("6-space yaml indent"));
        out.push('\n');
    }
    out
}

#[cfg(test)]
#[test]
fn ubuntu_test_recipe_identity_binds_embedded_route_and_preserves_shared_goldens() {
    use super::ubuntu_web::{
        IsoPoolManifest, IsoPoolVerifier, PROFILE_PIN, PoolPackage, UbuntuWebRecipe,
    };
    use crate::vm::paths::ArtifactPin;

    struct OkVerifier;
    impl IsoPoolVerifier for OkVerifier {
        fn verify(&self, _: &UbuntuRecipe, _: &IsoPoolManifest<'_>) -> bool {
            true
        }
    }

    let pin = |bytes: &[u8]| AssetPin {
        length: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    };
    let guest = b"offline-test-guest\x00\xff";
    let host = Some(pin(b"synthetic host asset"));
    let iso_pin = AssetPin {
        length: 19,
        sha256: UbuntuRecipe::ISO_SHA256,
    };
    let recipe = UbuntuRecipe {
        iso: Some(iso_pin),
        kernel: host,
        initrd: host,
        guest: Some(pin(guest)),
        service: Some(pin(UbuntuRecipe::SERVICE)),
        finalize: Some(pin(UbuntuRecipe::FINALIZE)),
        nocloud_schema: Some(UbuntuRecipe::NOCLOUD_PROFILE_PIN),
        firmware_code: host,
        firmware_vars: host,
        qemu_receipt: host,
    };

    let original = recipe.test_recipe_digest().unwrap();
    let canonical = recipe
        .render_test_seed(TEST_RECIPE_DIGEST_BUILD_ID, guest)
        .unwrap();
    let files = super::ubuntu_web::cidata_files(&canonical).unwrap();
    let meta = std::str::from_utf8(files[5]).unwrap();
    let user = std::str::from_utf8(files[6]).unwrap();
    assert_eq!(meta, nocloud_meta(TEST_RECIPE_DIGEST_BUILD_ID));
    let (_, base_user) = recipe
        .seed_config(TEST_RECIPE_DIGEST_BUILD_ID, guest)
        .unwrap();
    assert_eq!(user, append_test_route_user_data(&base_user).unwrap());
    let digest = |meta: &str, user: &str| {
        recipe
            .test_recipe_digest_with(
                UbuntuRecipe::SERVICE,
                UbuntuRecipe::FINALIZE,
                UbuntuRecipe::NOCLOUD_PROFILE_PIN,
                UbuntuRecipe::TEST_ROUTE_COMMAND,
                DETACHED_HEALTH,
                (meta, user),
            )
            .unwrap()
    };
    assert_eq!(digest(meta, user), original);
    let route_block = user.strip_prefix(&base_user).unwrap();
    let change_route_block =
        |old: &str, new: &str| format!("{base_user}{}", route_block.replacen(old, new, 1));
    let changed_header = change_route_block("    - |\n", "    - |-\n");
    let changed_indent = change_route_block("      set -eu\n", "     set -eu\n");
    let changed_meta = meta.replacen(
        "local-hostname: arxcovo\n",
        "local-hostname: arxcovo-alt\n",
        1,
    );
    assert_ne!(digest(meta, &changed_header), original);
    assert_ne!(digest(meta, &changed_indent), original);
    assert_ne!(digest(&changed_meta, user), original);
    let first = recipe.render_test_seed([0x11; 16], guest).unwrap();
    let second = recipe.render_test_seed([0x22; 16], guest).unwrap();
    assert_ne!(first, second);
    let first = super::ubuntu_web::cidata_files(&first).unwrap();
    let second = super::ubuntu_web::cidata_files(&second).unwrap();
    assert_ne!(first[5], second[5]);
    for (files, build_id) in [(first, [0x11; 16]), (second, [0x22; 16])] {
        assert_eq!(files[5], nocloud_meta(build_id).as_bytes());
        assert_eq!(files[6], user.as_bytes());
        assert_eq!(
            digest(meta, std::str::from_utf8(files[6]).unwrap()),
            original
        );
    }

    // Changing one byte of the TEST route late command changes the TEST recipe
    // digest and 16-byte ID prefix.
    let modified_route = UbuntuRecipe::TEST_ROUTE_COMMAND.replace("10.0.2.2", "10.0.2.9");
    assert_ne!(modified_route, UbuntuRecipe::TEST_ROUTE_COMMAND);
    let with_changed_route = recipe
        .test_recipe_digest_with(
            UbuntuRecipe::SERVICE,
            UbuntuRecipe::FINALIZE,
            UbuntuRecipe::NOCLOUD_PROFILE_PIN,
            &modified_route,
            DETACHED_HEALTH,
            (meta, user),
        )
        .unwrap();
    assert_ne!(with_changed_route, original);
    assert_ne!(with_changed_route[..16], original[..16]);

    // Changing FINALIZE, SERVICE, NOCLOUD_PROFILE_PIN or DETACHED_HEALTH also
    // alters the TEST recipe identity.
    let mut alt_finalize = UbuntuRecipe::FINALIZE.to_vec();
    alt_finalize.push(b'\n');
    let mut recipe_alt_finalize = UbuntuRecipe {
        finalize: Some(pin(&alt_finalize)),
        ..UbuntuRecipe {
            iso: recipe.iso,
            kernel: recipe.kernel,
            initrd: recipe.initrd,
            guest: recipe.guest,
            service: recipe.service,
            finalize: recipe.finalize,
            nocloud_schema: recipe.nocloud_schema,
            firmware_code: recipe.firmware_code,
            firmware_vars: recipe.firmware_vars,
            qemu_receipt: recipe.qemu_receipt,
        }
    };
    assert!(recipe_alt_finalize.test_recipe_digest().is_err());
    assert_ne!(
        recipe_alt_finalize
            .test_recipe_digest_with(
                UbuntuRecipe::SERVICE,
                &alt_finalize,
                UbuntuRecipe::NOCLOUD_PROFILE_PIN,
                UbuntuRecipe::TEST_ROUTE_COMMAND,
                DETACHED_HEALTH,
                (meta, user),
            )
            .unwrap(),
        original
    );
    let mut alt_profile = UbuntuRecipe::NOCLOUD_PROFILE_PIN;
    alt_profile.sha256[0] ^= 1;
    recipe_alt_finalize.finalize = recipe.finalize;
    recipe_alt_finalize.nocloud_schema = Some(alt_profile);
    assert!(recipe_alt_finalize.test_recipe_digest().is_err());
    assert_ne!(
        recipe_alt_finalize
            .test_recipe_digest_with(
                UbuntuRecipe::SERVICE,
                UbuntuRecipe::FINALIZE,
                alt_profile,
                UbuntuRecipe::TEST_ROUTE_COMMAND,
                DETACHED_HEALTH,
                (meta, user),
            )
            .unwrap(),
        original
    );
    let alt_health = format!("{DETACHED_HEALTH}\n");
    assert_ne!(
        recipe
            .test_recipe_digest_with(
                UbuntuRecipe::SERVICE,
                UbuntuRecipe::FINALIZE,
                UbuntuRecipe::NOCLOUD_PROFILE_PIN,
                UbuntuRecipe::TEST_ROUTE_COMMAND,
                &alt_health,
                (meta, user),
            )
            .unwrap(),
        original
    );

    // Shared base Ubuntu seed and static-web seed/recipe goldens stay unchanged.
    let build = [0x5a; 16];
    let base_seed = recipe.render_seed(build, guest).unwrap();
    assert_eq!(
        hex::encode(Sha256::digest(&base_seed)),
        "08fd082e494459e6a3ed24857ee59b4039feaf312b32e74ebc1d740a6aa66755"
    );
    let packages = [
        PoolPackage {
            name: "nginx-common",
            version: "1.24.0-2ubuntu7.1",
            relative_path: "pool/main/n/nginx/nginx-common_all.deb",
            artifact: ArtifactPin {
                size: 24,
                sha256: [
                    0x2b, 0xec, 0x25, 0x98, 0x51, 0x17, 0xff, 0x20, 0xab, 0xeb, 0xef, 0x50, 0xc1,
                    0x16, 0xa5, 0x65, 0xde, 0x44, 0xb7, 0xd2, 0x38, 0xbd, 0xcb, 0x25, 0x9a, 0xbd,
                    0x93, 0xbd, 0xd2, 0x92, 0xf7, 0xa5,
                ],
            },
        },
        PoolPackage {
            name: "nginx",
            version: "1.24.0-2ubuntu7.1",
            relative_path: "pool/main/n/nginx/nginx_amd64.deb",
            artifact: ArtifactPin {
                size: 17,
                sha256: [
                    0x17, 0x37, 0x8f, 0xea, 0x18, 0x6a, 0xbd, 0x99, 0x24, 0xc3, 0xb3, 0x32, 0x72,
                    0x85, 0xd2, 0x27, 0xe6, 0x29, 0xa2, 0x5e, 0x1d, 0xb3, 0xc1, 0x23, 0x85, 0xe0,
                    0x4c, 0x5b, 0x1f, 0xa3, 0x62, 0x85,
                ],
            },
        },
    ];
    let manifest = IsoPoolManifest {
        iso: iso_pin,
        signed_metadata: ArtifactPin {
            size: 17,
            sha256: [0x83; 32],
        },
        packages: &packages,
    };
    let web = UbuntuWebRecipe::qualify(
        &recipe,
        Some(PROFILE_PIN),
        &manifest,
        Some(manifest.canonical_pin().unwrap()),
        &OkVerifier,
    )
    .unwrap();
    let web_seed = web.render_seed(build, guest).unwrap();
    assert_eq!(
        hex::encode(Sha256::digest(&web_seed)),
        "5cfdb57ec055450f90a6349d8ed5cc4f2fcee7dbb829f9a370872ad9476adfc0"
    );
    assert_eq!(
        hex::encode(web.recipe_digest()),
        "807f245f5b0d20fa2c44054e70f108a56b1ddc2924f9186979ad54d96285e00f"
    );
}

#[cfg(test)]
#[tokio::test]
async fn arxnet3_witness_seed_script_behavior_with_fake_sockets_and_tools() {
    use crate::vm::qualification_record::ProbeOutcome;
    use crate::vm::{guest::network_probe::witness_response, qmp::ChannelError};
    use std::collections::BTreeMap;
    use std::net::SocketAddr;

    let nonce = [0xab; 16];
    let tcp: [SocketAddr; 3] = [
        "192.0.2.10:40001".parse().unwrap(),
        "[2001:db8::10]:40002".parse().unwrap(),
        "10.0.2.2:40003".parse().unwrap(),
    ];
    let udp: [SocketAddr; 3] = [
        "192.0.2.10:40004".parse().unwrap(),
        "[2001:db8::10]:40005".parse().unwrap(),
        "10.0.2.2:40006".parse().unwrap(),
    ];
    let dns: SocketAddr = "192.0.2.53:53".parse().unwrap();

    let iso = network_witness_seed_for(&nonce, tcp, udp, dns).unwrap();
    let emitted = extract_probe_script_from_cidata(&iso);
    let meta = serde_json::json!({
        "nonce": hex::encode(nonce),
        "tcp": tcp.map(|a| (a.ip().to_string(), a.port())),
        "udp": udp.map(|a| (a.ip().to_string(), a.port())),
        "dns": (dns.ip().to_string(), dns.port()),
    })
    .to_string();

    let stdout = run_witness_python("arxnet3", &meta, &emitted).await;
    let frames: BTreeMap<String, String> = serde_json::from_slice(&stdout).unwrap();

    // Feed the exact emitted ARXNET3 frames into Rust's witness_response parser.
    let reachable = witness_response(frames["reachable"].as_bytes(), &nonce)
        .await
        .unwrap();
    assert!(reachable.gateway_reachable);
    assert_eq!(reachable.validated_attempts, 8);
    assert_eq!(
        reachable.diagnostic.outcomes,
        [
            ProbeOutcome::TimedOut,
            ProbeOutcome::Ipv6PathAbsent,
            ProbeOutcome::RefusedOrReset,
            ProbeOutcome::DatagramSent,
            ProbeOutcome::Ipv6PathAbsent,
            ProbeOutcome::DatagramSent,
            ProbeOutcome::DatagramSent,
            ProbeOutcome::DatagramSent,
        ]
    );

    for key in [
        "gateway_stale",
        "gateway_wrong_ip",
        "gateway_no_lladdr",
        "gateway_cmd_fail",
    ] {
        let parsed = witness_response(frames[key].as_bytes(), &nonce)
            .await
            .unwrap();
        assert!(!parsed.gateway_reachable, "{key}");
        assert_eq!(parsed.validated_attempts, 8, "{key}");
    }

    for (key, idx) in [
        ("ipv4_tcp_local_err", 0),
        ("ipv4_udp_local_err", 3),
        ("ipv4_udp_short_send", 5),
        ("ipv4_dns_local_err", 6),
    ] {
        let parsed = witness_response(frames[key].as_bytes(), &nonce)
            .await
            .unwrap();
        assert_eq!(
            parsed.diagnostic.outcomes[idx],
            ProbeOutcome::SocketError,
            "{key}"
        );
        assert_eq!(parsed.validated_attempts, 7, "{key}");
    }

    for key in [
        "absent_no_lease",
        "absent_no_route",
        "absent_no_nic",
        "absent_global_v6",
    ] {
        let parsed = witness_response(frames[key].as_bytes(), &nonce)
            .await
            .unwrap();
        assert!(!parsed.gateway_reachable, "{key}");
        assert_eq!(parsed.validated_attempts, 0, "{key}");
        assert!(
            parsed
                .diagnostic
                .outcomes
                .iter()
                .all(|o| *o == ProbeOutcome::NotAttempted),
            "{key}"
        );
    }

    for (key, idx, expected) in [
        ("malformed_dns_short", 6, ProbeOutcome::MalformedReply),
        ("malformed_dns_wrong_id", 6, ProbeOutcome::MalformedReply),
        ("malformed_dns_tc", 7, ProbeOutcome::MalformedReply),
        ("valid_dns_reply", 6, ProbeOutcome::Reachable),
        ("unexpected_tcp_connect", 0, ProbeOutcome::Reachable),
    ] {
        let parsed = witness_response(frames[key].as_bytes(), &nonce)
            .await
            .unwrap();
        assert_eq!(parsed.diagnostic.outcomes[idx], expected, "{key}");
        assert_eq!(parsed.validated_attempts, 7, "{key}");
    }

    assert!(matches!(
        witness_response(frames["absent_port"].as_bytes(), &nonce).await,
        Err(ChannelError::Eof)
    ));
}

#[cfg(test)]
#[tokio::test]
async fn installed_ubuntu_witness_script_behavior_with_fake_sockets_and_tools() {
    use std::net::SocketAddr;

    let nonce = [0xcd; 16];
    let tcp: [SocketAddr; 3] = [
        "192.0.2.10:41001".parse().unwrap(),
        "[2001:db8::10]:41002".parse().unwrap(),
        "10.0.2.2:41003".parse().unwrap(),
    ];
    let udp: [SocketAddr; 3] = [
        "192.0.2.10:41004".parse().unwrap(),
        "[2001:db8::10]:41005".parse().unwrap(),
        "10.0.2.2:41006".parse().unwrap(),
    ];
    let dns: SocketAddr = "192.0.2.53:53".parse().unwrap();

    let shell_script = installed_witness_script_for(nonce, tcp, udp, dns).unwrap();
    #[cfg(unix)]
    {
        run_witness_interpreter(std::path::Path::new("/bin/sh"), &["-n"], &shell_script, 15).await;
    }

    let python_body = shell_script
        .strip_prefix("set -eu\n/usr/bin/python3 - <<'ARX_NETWORK'\n")
        .expect("installed script shell wrapper prefix")
        .strip_suffix("\nARX_NETWORK")
        .expect("installed script heredoc suffix");
    let meta = serde_json::json!({
        "nonce": hex::encode(nonce),
        "tcp": tcp.map(|a| (a.ip().to_string(), a.port())),
        "udp": udp.map(|a| (a.ip().to_string(), a.port())),
        "dns": (dns.ip().to_string(), dns.port()),
    })
    .to_string();

    run_witness_python("installed", &meta, python_body).await;
}
