//! The autoinstall identity uses Subiquity's locked password marker. On
//! 24.04.3 account creation can be deferred to target cloud-init, so late
//! commands neither inspect nor change accounts. Removing and disabling the
//! target datasource also disables that deferred creation path. The 4b2
//! detached health check must establish the expected account presence and
//! lock state before qualification.
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
    /// Pin the versioned NoCloud profile, including all three fixed literals.
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
    const NOCLOUD_SCHEMA: &[u8] = b"arxcovo-nocloud-v1\n";

    /// SHA256 and length of the schema identifier, user-data template, unit
    /// and finalizer, in that order, separated by NUL bytes.
    pub const NOCLOUD_PROFILE_PIN: AssetPin = AssetPin {
        length: 2807,
        sha256: [
            0xe2, 0xf3, 0xe3, 0x2a, 0xbf, 0x8b, 0xe9, 0xd6, 0xf8, 0xa8, 0x56, 0xab, 0x9d, 0xf9,
            0x75, 0x17, 0x4f, 0x5e, 0x89, 0x62, 0x6a, 0xf9, 0x29, 0x53, 0x4a, 0x10, 0xa3, 0x88,
            0x9f, 0x36, 0x89, 0x8a,
        ],
    };

    pub const SERVICE: &[u8] = br"[Unit]
Description=Arxcovo guest supervisor
After=systemd-udevd.service

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
                .chain(Self::FINALIZE),
        )?;
        let meta = format!(
            "instance-id: arx-install-{}\nlocal-hostname: arxcovo\n",
            hex::encode(build_id),
        );
        Ok((meta, user_data(guest_pin, service, finalize)))
    }
}

fn required(pin: Option<AssetPin>) -> Result<AssetPin, RecipeError> {
    pin.filter(|pin| pin.length != 0)
        .ok_or(RecipeError::Unqualified)
}

fn image<'a>(guest: &'a [u8], meta: &'a str, user: &'a str) -> Result<Image<'a>, RecipeError> {
    Image::new([
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
      )
      install -m 0755 "$seed/arx-guest" /target/usr/local/bin/arx-guest
      install -m 0644 "$seed/arx-guest.service" /target/etc/systemd/system/arx-guest.service
      install -m 0700 "$seed/arx-finalize" /target/usr/local/sbin/arx-finalize
      systemctl --root=/target enable arx-guest.service
      curtin in-target --target=/target -- /bin/sh /usr/local/sbin/arx-finalize
"#;
