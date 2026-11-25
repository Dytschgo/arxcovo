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
        let expected = Self::NOCLOUD_PROFILE_PIN;
        if schema.length != expected.length || schema.sha256 != expected.sha256 {
            return Err(RecipeError::Unqualified);
        }
        let separator: &[u8] = b"\0";
        schema.verify(
            Self::NOCLOUD_SCHEMA
                .chain(separator)
                .chain(USER_DATA_TEMPLATE.as_bytes())
                .chain(separator)
                .chain(Self::SERVICE)
                .chain(separator)
                .chain(Self::FINALIZE)
                .chain(separator)
                .chain(Self::ACCOUNT_AUDIT_SERVICE)
                .chain(separator)
                .chain(Self::ACCOUNT_AUDIT_SCRIPT),
        )?;
        let meta = format!(
            "instance-id: arx-install-{}\nlocal-hostname: arxcovo\n",
            hex::encode(build_id),
        );
        Ok((meta, user_data(guest_pin, service, finalize)))
    }
}

/// Probe-only NoCloud seed. The live guest reports the launch nonce, NIC
/// setup observations and socket outcomes from a fixed program. The seed has no
/// autoinstall stanza, guest service, bootloader action or credential.
#[cfg(any(test, all(windows, target_arch = "x86_64")))]
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
            if (
                len(reply) >= len(payload)
                and reply[:2] == ident
                and (reply[2] & 0x80) != 0
                and ((reply[2] >> 3) & 0x0f) == 0
                and (reply[2] & 0x02) == 0
                and (reply[3] & 0x0f) in (0, 3)
                and reply[4:6] == b'\x00\x01'
                and reply[12:len(payload)] == question
            ):
                return 'R'
            return 'M'
    except OSError:
        return 'E'

def run_probes(devices):
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
    if setup == '1111':
        outcomes = ''.join(probe_tcp(t, guest_v4) for t in TCP)
        outcomes += probe_dns(DNS, guest_v4) + probe_dns(('10.0.2.3', 53), guest_v4)
    else:
        outcomes = 'E' * 8

    return setup, outcomes

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

pub(super) const DETACHED_HEALTH: &str = r#"set -eu
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
fn network_probe_python_fixture() {
    // The interpreter is required only for the explicitly enabled guest-script
    // check. Ordinary Rust test runs have no host Python dependency.
    let Some(python) = std::env::var_os("ARX_TEST_NETWORK_PROBE_PYTHON") else {
        return;
    };
    // Run the functions embedded in the seed with a fake socket. No packets
    // are sent and the fixture does not inspect the host's routes.
    let fixture = include_str!("network_probe_fixture.py");
    // fcntl is Linux-only. These mocked fixture calls never use its ioctl.
    let program = format!(
        "__name__ = 'fixture'\nimport sys, types\nsys.modules['fcntl'] = types.ModuleType('fcntl')\n{PROBE_START}{PROBE_BODY}\n{PROBE_FIXTURE_BEFORE}\n{fixture}\n{PROBE_FIXTURE_AFTER}"
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
"#;
