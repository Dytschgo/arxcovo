//! Offline Ubuntu web variant. The installer leaves nginx masked and the
//! interface disabled. A private first boot provisions the static origin.

use super::{AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe, media::Image};
use crate::vm::{Id, paths::ArtifactPin};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, io::Write};

const MAX_PACKAGES: usize = 32;
const MAX_PACKAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 64 * 1024 * 1024;

pub const RECIPE_ID: &str = "ubuntu-noble-amd64-web-v1";
pub const PROFILE_PIN: ArtifactPin = ArtifactPin {
    size: 3595,
    sha256: [
        0x05, 0xe1, 0x3e, 0x47, 0x80, 0xab, 0x93, 0xd3, 0x6f, 0x0b, 0x1e, 0xb2, 0x4c, 0xd2, 0xd5,
        0xc1, 0xa8, 0xec, 0x3b, 0x13, 0x92, 0xcc, 0xee, 0x6c, 0x3d, 0x57, 0xc2, 0x2b, 0xea, 0x25,
        0xa5, 0x71,
    ],
};

/// One exact amd64 or architecture-independent package from the ISO pool.
/// Dependencies already present at the required version in the base image
/// are accounted for during offline closure verification.
#[derive(Clone, Copy, Debug)]
pub struct PoolPackage<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub relative_path: &'a str,
    pub artifact: ArtifactPin,
}

pub struct IsoPoolManifest<'a> {
    pub iso: AssetPin,
    /// Upstream signed archive metadata and package index evidence.
    pub signed_metadata: ArtifactPin,
    /// Strictly ordered by package name. Include the `nginx` package.
    pub packages: &'a [PoolPackage<'a>],
}

impl IsoPoolManifest<'_> {
    /// Canonical receipt pin. The owner-reviewed expected pin comes from
    /// outside this receipt and outside any agent-controlled input.
    pub fn canonical_pin(&self) -> Result<ArtifactPin, RecipeError> {
        let metadata = self.signed_metadata;
        if self.iso.sha256 != UbuntuRecipe::ISO_SHA256
            || self.iso.length == 0
            || !(1..=MAX_METADATA_BYTES).contains(&metadata.size)
            || metadata.sha256 == [0; 32]
            || !(1..=MAX_PACKAGES).contains(&self.packages.len())
        {
            return Err(RecipeError::Unqualified);
        }
        let mut text = format!(
            "arxcovo-iso-pool-v1\niso {} {}\nmetadata {} {}\n",
            self.iso.length,
            hex::encode(self.iso.sha256),
            metadata.size,
            hex::encode(metadata.sha256),
        );
        let mut previous = "";
        let mut total = 0_u64;
        let mut has_nginx = false;
        for (i, package) in self.packages.iter().enumerate() {
            let PoolPackage {
                name,
                version,
                relative_path,
                artifact,
            } = *package;
            if name <= previous
                || !valid_token(
                    name,
                    64,
                    |b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'+' | b'.' | b'-'),
                )
                || !name.as_bytes()[0].is_ascii_alphanumeric()
                || !valid_token(version, 128, |b| {
                    b.is_ascii_alphanumeric() || b".+-:~".contains(&b)
                })
                || !version.as_bytes()[0].is_ascii_digit()
                || !relative_path.starts_with("pool/")
                || !relative_path.ends_with(".deb")
                || !valid_token(relative_path, 256, |b| {
                    b.is_ascii_alphanumeric() || b".+-_~/".contains(&b)
                })
                || relative_path
                    .split('/')
                    .any(|part| matches!(part, "" | "." | ".."))
                || self.packages[..i]
                    .iter()
                    .any(|other| other.relative_path == relative_path)
                || artifact.size == 0
                || artifact.sha256 == [0; 32]
            {
                return Err(RecipeError::Unqualified);
            }
            total = total
                .checked_add(artifact.size)
                .filter(|&size| size <= MAX_PACKAGE_BYTES)
                .ok_or(RecipeError::Unqualified)?;
            has_nginx |= name == "nginx";
            previous = name;
            writeln!(
                text,
                "{name} {version} {relative_path} {} {}",
                artifact.size,
                hex::encode(artifact.sha256)
            )
            .expect("string write");
        }
        if !has_nginx {
            return Err(RecipeError::Unqualified);
        }
        Ok(pin(text.as_bytes()))
    }
}

fn valid_token(value: &str, max: usize, allowed: impl Fn(u8) -> bool) -> bool {
    (1..=max).contains(&value.len()) && value.bytes().all(allowed)
}

/// Implemented by the host's offline media inspector. A successful witness
/// must establish upstream signatures, index-to-deb hashes, amd64/all package
/// identity and the complete exact dependency closure against this base
/// recipe, including installed base versions. Verify against the same pinned
/// ISO content later staged through protected handles. No guest or MCP input
/// supplies this witness.
pub trait IsoPoolVerifier {
    fn verify(&self, base: &UbuntuRecipe, manifest: &IsoPoolManifest<'_>) -> bool;
}

pub struct UbuntuWebRecipe<'a> {
    base: &'a UbuntuRecipe,
    manifest: &'a IsoPoolManifest<'a>,
    digest: [u8; 32],
}

impl<'a> UbuntuWebRecipe<'a> {
    pub fn qualify(
        base: &'a UbuntuRecipe,
        profile_pin: Option<ArtifactPin>,
        manifest: &'a IsoPoolManifest<'a>,
        manifest_pin: Option<ArtifactPin>,
        verifier: &impl IsoPoolVerifier,
    ) -> Result<Self, RecipeError> {
        let iso = base.iso.ok_or(RecipeError::Unqualified)?;
        let manifest_digest = manifest.canonical_pin()?;
        if profile_pin != Some(PROFILE_PIN)
            || profile_pin != Some(profile_pin_from_literals())
            || manifest_pin != Some(manifest_digest)
            || iso.length != manifest.iso.length
            || iso.sha256 != manifest.iso.sha256
        {
            return Err(RecipeError::Unqualified);
        }
        let mut digest = Sha256::new();
        digest.update(RECIPE_ID.as_bytes());
        digest.update(PROFILE_PIN.sha256);
        digest.update(manifest_digest.sha256);
        for asset in [
            base.iso,
            base.kernel,
            base.initrd,
            base.guest,
            base.service,
            base.finalize,
            base.nocloud_schema,
            base.firmware_code,
            base.firmware_vars,
            base.qemu_receipt,
        ] {
            let asset = asset
                .filter(|asset| asset.length != 0 && asset.sha256 != [0; 32])
                .ok_or(RecipeError::Unqualified)?;
            digest.update(asset.length.to_be_bytes());
            digest.update(asset.sha256);
        }
        if !verifier.verify(base, manifest) {
            return Err(RecipeError::Unqualified);
        }
        Ok(Self {
            base,
            manifest,
            digest: digest.finalize().into(),
        })
    }

    pub fn recipe_digest(&self) -> [u8; 32] {
        self.digest
    }

    /// For offline inspection. Staging requires `write_seed` and host custody.
    pub fn render_seed(&self, build_id: Id, guest: &[u8]) -> Result<Vec<u8>, RecipeError> {
        let mut out = Vec::new();
        self.extend_seed(&self.base.render_seed(build_id, guest)?, &mut out)?;
        Ok(out)
    }

    /// Verify the base Ubuntu assets before writing any web seed bytes.
    pub fn write_seed(
        &self,
        assets: UbuntuAssets<'_>,
        build_id: Id,
        guest: &[u8],
        out: impl Write,
    ) -> Result<(), RecipeError> {
        let mut base = Vec::new();
        self.base.write_seed(assets, build_id, guest, &mut base)?;
        self.extend_seed(&base, out)
    }

    fn extend_seed(&self, base: &[u8], out: impl Write) -> Result<(), RecipeError> {
        let mut files = cidata_files(base).ok_or(RecipeError::Unqualified)?;
        let mut user =
            String::from_utf8(files[6].to_vec()).map_err(|_| RecipeError::Unqualified)?;
        // Ubuntu's pinned profile ends in the autoinstall late-command list.
        // Keep its finalizer intact and append one separate late command.
        if !user.ends_with(
            "      curtin in-target --target=/target -- /bin/sh /usr/local/sbin/arx-finalize\n",
        ) {
            return Err(RecipeError::Unqualified);
        }
        let mut stage = String::new();
        for package in self.manifest.packages {
            stage.push_str(
                &STAGE_LINE
                    .replace("{path}", package.relative_path)
                    .replace("{name}", package.name)
                    .replace("{size}", &package.artifact.size.to_string())
                    .replace("{sha256}", &hex::encode(package.artifact.sha256)),
            );
        }
        let command = INSTALL_PROFILE
            .replace("{nginx}", NGINX_CONFIG)
            .replace("{network}", GUEST_NETWORK)
            .replace("{sysctl}", GUEST_SYSCTL)
            .replace("{stage}", stage.trim_end())
            .replace("{verify}", self.package_checks("$target").trim_end());
        user.push_str("    - |\n");
        for line in command.lines() {
            if !line.is_empty() {
                user.push_str("      ");
                user.push_str(line);
            }
            user.push('\n');
        }
        files[6] = user.as_bytes();
        Image::new(files)
            .ok_or(RecipeError::Unqualified)?
            .write(out)
            .map_err(|error| RecipeError::Write(error.kind()))
    }

    /// Run on a detached verification boot, in addition to the base health
    /// contract. This inspects the inert configuration without a private key.
    pub fn detached_health(&self) -> String {
        let mut hashes = String::new();
        for (path, bytes) in [
            ("nginx.conf", NGINX_CONFIG),
            ("static-network.yaml", GUEST_NETWORK),
            ("static-ipv6.conf", GUEST_SYSCTL),
        ] {
            writeln!(
                hashes,
                "{}  /etc/arxcovo/{path}",
                hex::encode(Sha256::digest(bytes))
            )
            .expect("string write");
        }
        DETACHED_PROFILE
            .replace("{pins}", hashes.trim_end())
            .replace("{verify}", self.package_checks("").trim_end())
    }

    fn package_checks(&self, root: &str) -> String {
        self.manifest
            .packages
            .iter()
            .map(|package| {
                VERIFY_LINE
                    .replace("{root}", root)
                    .replace("{name}", package.name)
                    .replace("{version}", package.version)
            })
            .collect()
    }
}

// This reads only CIDATA just emitted by UbuntuRecipe. Use its fixed primary
// root records and rebuild both namespaces with the existing bounded writer.
fn cidata_files(base: &[u8]) -> Option<[&[u8]; 7]> {
    const BLOCK: usize = 2048;
    const NAMES: [&[u8]; 7] = [
        b"ARXAUDIT.;1",
        b"ARXAUDIT.SVC;1",
        b"ARXGUEST.;1",
        b"ARXGUEST.SVC;1",
        b"FINALIZE.;1",
        b"METADATA.;1",
        b"USERDATA.;1",
    ];
    let mut records = base.get(23 * BLOCK + 68..24 * BLOCK)?;
    let mut files = [&[][..]; 7];
    for (file, name) in files.iter_mut().zip(NAMES) {
        let length = usize::from(*records.first()?);
        let record = records.get(..length)?;
        if record.get(33..33 + name.len())? != name {
            return None;
        }
        let start =
            (u32::from_le_bytes(record.get(2..6)?.try_into().ok()?) as usize).checked_mul(BLOCK)?;
        let length_bytes = u32::from_le_bytes(record.get(10..14)?.try_into().ok()?) as usize;
        *file = base.get(start..start.checked_add(length_bytes)?)?;
        records = &records[length..];
    }
    Some(files)
}

fn pin(bytes: &[u8]) -> ArtifactPin {
    ArtifactPin {
        size: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    }
}

fn profile_pin_from_literals() -> ArtifactPin {
    pin(&[
        RECIPE_ID,
        NGINX_CONFIG,
        GUEST_NETWORK,
        GUEST_SYSCTL,
        INSTALL_PROFILE,
        DETACHED_PROFILE,
        STAGE_LINE,
        VERIFY_LINE,
    ]
    .join("\0")
    .into_bytes())
}

const STAGE_LINE: &str = "stage_deb '{path}' '{name}' '{size}' '{sha256}'\n";
const VERIFY_LINE: &str = "verify_package \"{root}\" '{name}' '{version}'\n";

const INSTALL_PROFILE: &str = r#"set -eu
export LC_ALL=C
target=/target
dir=$target/var/lib/arxcovo/web-install
install -d -m 0755 "$target/var/lib/arxcovo"
mkdir -m 0755 "$dir"
mkdir -m 0755 "$dir/lists"
stage_deb() {
  src=/cdrom/$1
  dest=$dir/$2.deb
  test "$(stat -c %s "$src")" = "$3"
  install -m 0644 "$src" "$dest"
  printf '%s  %s\n' "$4" "$dest" | sha256sum --check --strict
}
{stage}
systemctl --root="$target" mask nginx.service apt-daily.timer apt-daily-upgrade.timer apt-daily.service apt-daily-upgrade.service
policy=$target/usr/sbin/policy-rc.d
if test -e "$policy" || test -L "$policy"; then mv -T "$policy" "$dir/policy-original"; fi
restore_policy() {
  rm -f "$policy"
  if test -e "$dir/policy-original" || test -L "$dir/policy-original"; then
    mv -T "$dir/policy-original" "$policy"
  fi
}
trap restore_policy EXIT
printf '#!/bin/sh\nexit 101\n' > "$policy"
chmod 0755 "$policy"
curtin in-target --target="$target" -- /bin/sh -ec '
  export DEBIAN_FRONTEND=noninteractive
  apt-get -y --no-download --no-remove --no-install-recommends \
    -o Dir::Etc::sourcelist=/dev/null -o Dir::Etc::sourceparts=- \
    -o Dir::State::lists=/var/lib/arxcovo/web-install/lists \
    install /var/lib/arxcovo/web-install/*.deb
  audit=$(dpkg --audit)
  test -z "$audit"
'
restore_policy
trap - EXIT
verify_package() {
  actual=$(dpkg-query --admindir="$1/var/lib/dpkg" -W -f='${Status}:${Version}' "$2")
  test "$actual" = "install ok installed:$3"
}
{verify}
rm -rf "$dir"
rm -f "$target"/etc/nginx/sites-enabled/default "$target"/etc/nginx/sites-available/default
rm -f "$target"/etc/apt/sources.list "$target"/etc/apt/sources.list.d/*.list "$target"/etc/apt/sources.list.d/*.sources
cat > "$target/etc/arxcovo/nginx.conf" <<'ARX_NGINX'
{nginx}ARX_NGINX
cat > "$target/etc/arxcovo/static-network.yaml" <<'ARX_NETWORK'
{network}ARX_NETWORK
cat > "$target/etc/arxcovo/static-ipv6.conf" <<'ARX_IPV6'
{sysctl}ARX_IPV6
printf '%s\n' ubuntu-noble-amd64-web-v1 > "$target/etc/arxcovo/web-finalized"
"#;

const DETACHED_PROFILE: &str = r#"set -eu
export LC_ALL=C
verify_package() {
  actual=$(dpkg-query --admindir="$1/var/lib/dpkg" -W -f='${Status}:${Version}' "$2")
  test "$actual" = "install ok installed:$3"
}
{verify}
test "$(systemctl is-enabled nginx.service 2>/dev/null || :)" = masked
absent() {
  test ! -e "$1"
  test ! -L "$1"
}
absent /etc/nginx/sites-enabled/default
absent /etc/arxcovo/origin.key
absent /etc/arxcovo/origin.crt
test ! -e /var/lib/arxcovo/web-install
test ! -e /dev/sr0
listeners=$(ss -H -ltn '( sport = :80 or sport = :443 )')
test -z "$listeners"
printf '%s\n' ubuntu-noble-amd64-web-v1 | cmp -s - /etc/arxcovo/web-finalized
sha256sum --check --strict <<'ARX_PINS'
{pins}
ARX_PINS
"#;

const NGINX_CONFIG: &str = r#"user www-data;
events {}
http {
  include /etc/nginx/mime.types;
  default_type application/octet-stream;
  access_log off;
  server {
    listen 10.0.2.15:443 ssl;
    server_name _;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_certificate /etc/arxcovo/origin.crt;
    ssl_certificate_key /etc/arxcovo/origin.key;
    root /srv/arxcovo/site;
    index index.html;
    location = /arx-health {
      default_type text/plain;
      return 200 "ok\n";
    }
    location / {
      try_files $uri $uri/ =404;
    }
  }
}
"#;

const GUEST_NETWORK: &str = r#"network:
  version: 2
  renderer: networkd
  ethernets:
    origin:
      match:
        macaddress: '52:54:00:12:34:56'
      addresses: [10.0.2.15/24]
      dhcp4: false
      dhcp6: false
      accept-ra: false
      link-local: []
"#;

const GUEST_SYSCTL: &str =
    "net.ipv6.conf.all.disable_ipv6=1\nnet.ipv6.conf.default.disable_ipv6=1\n";

/// Private first-boot delivery plan. The supervisor supplies fresh ECDSA
/// material for this DNS name and verifies its validity and key binding.
pub struct OriginProvisioning {
    server_name: String,
}

impl OriginProvisioning {
    pub const HEALTH_PATH: &str = "/arx-health";
    pub const MAX_CERT_LIFETIME_SECONDS: u64 = 30 * 24 * 60 * 60;
    pub const NIC_MAC: &str = "52:54:00:12:34:56";
    pub const CERT_PATH: &str = "/etc/arxcovo/origin.crt";
    pub const KEY_PATH: &str = "/etc/arxcovo/origin.key";
    pub const KEY_MODE: u32 = 0o600;

    pub fn new(generation: Id) -> Self {
        Self {
            server_name: format!("arx-{}.invalid", hex::encode(generation)),
        }
    }

    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Bind bounded, already validated PEM material to a private transfer. A2
    /// keeps the CA certificate on the host and delivers only the leaf/key.
    pub fn payload<'a>(
        &'a self,
        leaf_pem: &'a [u8],
        key_pem: &'a [u8],
    ) -> Option<OriginPayload<'a>> {
        if !(1..=8 * 1024).contains(&leaf_pem.len()) || !(1..=4 * 1024).contains(&key_pem.len()) {
            return None;
        }
        Some(OriginPayload {
            plan: self,
            leaf_pem,
            key_pem,
        })
    }
}

/// For an authenticated, bounded, unforwarded virtio-serial operation. No
/// Debug, serialization, command-line or guest-script interface is provided.
/// The receiver installs the fixed config and static NIC profile, validates
/// nginx and reports fixed status. The supervisor proves clean stop before
/// committing this generation.
pub struct OriginPayload<'a> {
    plan: &'a OriginProvisioning,
    leaf_pem: &'a [u8],
    key_pem: &'a [u8],
}

impl<'a> OriginPayload<'a> {
    pub fn server_name(&self) -> &str {
        self.plan.server_name()
    }

    /// Called only by the private transport encoder.
    pub fn into_private_parts(self) -> (&'a [u8], &'a [u8]) {
        (self.leaf_pem, self.key_pem)
    }
}
