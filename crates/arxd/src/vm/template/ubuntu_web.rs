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
    size: 3487,
    sha256: [
        0x80, 0x2b, 0x3a, 0x19, 0xb2, 0x0c, 0x88, 0x93, 0x06, 0x97, 0x47, 0xaf, 0xeb, 0xe5, 0xea,
        0x3d, 0x5d, 0x3a, 0xc5, 0xd7, 0xf7, 0xeb, 0x2a, 0xc5, 0x4c, 0xd2, 0xd6, 0x4c, 0xb1, 0x8a,
        0xc0, 0xb2,
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
    /// Exact dependency order for one-at-a-time install, including `nginx`.
    pub packages: &'a [PoolPackage<'a>],
}

/// Only tests may enroll a small synthetic installer.
#[derive(Clone, Copy)]
pub(crate) enum ExpectedIso {
    Official,
    #[cfg(all(test, windows, target_arch = "x86_64"))]
    Fixture([u8; 32]),
}

impl ExpectedIso {
    fn digest(self) -> [u8; 32] {
        match self {
            Self::Official => UbuntuRecipe::ISO_SHA256,
            #[cfg(all(test, windows, target_arch = "x86_64"))]
            Self::Fixture(digest) => digest,
        }
    }
}

impl IsoPoolManifest<'_> {
    /// Canonical receipt pin. The owner-reviewed expected pin comes from
    /// outside this receipt and outside any agent-controlled input.
    pub fn canonical_pin(&self) -> Result<ArtifactPin, RecipeError> {
        self.canonical_pin_for(ExpectedIso::Official)
    }

    pub(crate) fn canonical_pin_for(&self, iso: ExpectedIso) -> Result<ArtifactPin, RecipeError> {
        let metadata = self.signed_metadata;
        if self.iso.sha256 != iso.digest()
            || self.iso.length == 0
            || !(1..=MAX_METADATA_BYTES).contains(&metadata.size)
            || metadata.sha256 == [0; 32]
            || !(1..=MAX_PACKAGES).contains(&self.packages.len())
        {
            return Err(RecipeError::Unqualified);
        }
        let mut text = format!(
            "arxcovo-iso-pool-v2\niso {} {}\nmetadata {} {}\n",
            self.iso.length,
            hex::encode(self.iso.sha256),
            metadata.size,
            hex::encode(metadata.sha256),
        );
        let mut total = 0_u64;
        let mut has_nginx = false;
        for (i, package) in self.packages.iter().enumerate() {
            let PoolPackage {
                name,
                version,
                relative_path,
                artifact,
            } = *package;
            if !valid_token(
                name,
                64,
                |b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'+' | b'.' | b'-'),
            ) || !name.as_bytes()[0].is_ascii_alphanumeric()
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
                    .any(|other| other.name == name || other.relative_path == relative_path)
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

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(crate) fn rebind_seed(seed: &[u8], generation: Id) -> Result<Vec<u8>, RecipeError> {
    if generation == [0; 16] {
        return Err(RecipeError::Unqualified);
    }
    let mut files = cidata_files(seed).ok_or(RecipeError::Unqualified)?;
    let meta = super::ubuntu::nocloud_meta(generation);
    files[5] = meta.as_bytes();
    let mut output = Vec::new();
    Image::new(files)
        .ok_or(RecipeError::Unqualified)?
        .write(&mut output)
        .map_err(|error| RecipeError::Write(error.kind()))?;
    Ok(output)
}

fn valid_token(value: &str, max: usize, allowed: impl Fn(u8) -> bool) -> bool {
    (1..=max).contains(&value.len()) && value.bytes().all(allowed)
}

/// Implemented by the host's offline media inspector. A successful witness
/// must establish upstream signatures, index-to-deb hashes, amd64/all package
/// identity and the complete exact dependency closure against this base
/// recipe, including installed base versions. Verify that the manifest order
/// permits installing and configuring one package at a time, including all
/// Depends and Pre-Depends. Verify against the same pinned
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
        Self::qualify_for(
            base,
            profile_pin,
            manifest,
            manifest_pin,
            verifier,
            ExpectedIso::Official,
        )
    }

    pub(crate) fn qualify_for(
        base: &'a UbuntuRecipe,
        profile_pin: Option<ArtifactPin>,
        manifest: &'a IsoPoolManifest<'a>,
        manifest_pin: Option<ArtifactPin>,
        verifier: &impl IsoPoolVerifier,
        expected_iso: ExpectedIso,
    ) -> Result<Self, RecipeError> {
        let iso = base.iso.ok_or(RecipeError::Unqualified)?;
        let manifest_digest = manifest.canonical_pin_for(expected_iso)?;
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
        let mut archives = String::new();
        for package in self.manifest.packages {
            stage.push_str(
                &STAGE_LINE
                    .replace("{path}", package.relative_path)
                    .replace("{name}", package.name)
                    .replace("{size}", &package.artifact.size.to_string())
                    .replace("{sha256}", &hex::encode(package.artifact.sha256)),
            );
            archives.push_str(&ARCHIVE_ARG.replace("{name}", package.name));
        }
        let command = INSTALL_PROFILE
            .replace("{nginx}", NGINX_CONFIG)
            .replace("{network}", GUEST_NETWORK)
            .replace("{sysctl}", GUEST_SYSCTL)
            .replace("{stage}", stage.trim_end())
            .replace("{archives}", archives.trim_end())
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
        self.detached_health_with_packages(self.package_checks("").trim_end())
    }

    fn detached_health_with_packages(&self, verify: &str) -> String {
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
            .replace("{verify}", verify)
    }

    #[cfg(any(test, all(windows, target_arch = "x86_64")))]
    pub(crate) fn detached_checks(&self) -> Vec<String> {
        let mut checks = vec![super::ubuntu::DETACHED_HEALTH.to_owned()];
        checks.extend(self.manifest.packages.chunks(8).map(|packages| {
            let verify = packages
                .iter()
                .map(|package| Self::package_check(package, ""))
                .collect::<String>();
            self.detached_health_with_packages(verify.trim_end())
        }));
        checks
    }

    fn package_checks(&self, root: &str) -> String {
        self.manifest
            .packages
            .iter()
            .map(|package| Self::package_check(package, root))
            .collect()
    }

    fn package_check(package: &PoolPackage<'_>, root: &str) -> String {
        VERIFY_LINE
            .replace("{root}", root)
            .replace("{name}", package.name)
            .replace("{version}", package.version)
    }
}

// Read only CIDATA emitted by UbuntuRecipe. File IDs follow the existing
// bootstrap contract. Their extents and the directory layout come from ISO9660.
pub(super) fn cidata_files(base: &[u8]) -> Option<[&[u8]; 7]> {
    const DESCRIPTOR_SECTOR: usize = 2048;
    const NAMES: [&[u8]; 7] = [
        b"ARXAUDIT.;1",
        b"ARXAUDIT.SVC;1",
        b"ARXGUEST.;1",
        b"ARXGUEST.SVC;1",
        b"FINALIZE.;1",
        b"METADATA.;1",
        b"USERDATA.;1",
    ];
    let primary = base
        .chunks_exact(DESCRIPTOR_SECTOR)
        .skip(16)
        .take_while(|descriptor| descriptor[0] != 255 && &descriptor[1..7] == b"CD001\x01")
        .find(|descriptor| descriptor[0] == 1)?;
    let block = usize::from(u16::from_le_bytes(primary[128..130].try_into().ok()?));
    if !(512..=2048).contains(&block)
        || !block.is_power_of_two()
        || block != usize::from(u16::from_be_bytes(primary[130..132].try_into().ok()?))
    {
        return None;
    }
    let root = primary.get(156..156 + usize::from(primary[156]))?;
    if *root.get(25)? != 2 {
        return None;
    }
    let directory = iso_extent(base, root, block)?;
    let mut files = [None; 7];
    for mut records in directory.chunks(block) {
        while let Some(&length) = records.first().filter(|&&length| length != 0) {
            let (record, remaining) = records.split_at_checked(usize::from(length))?;
            records = remaining;
            let name = record.get(33..33 + usize::from(*record.get(32)?))?;
            if record[25] == 2 && matches!(name, b"\0" | b"\x01") {
                continue;
            }
            let file = &mut files[NAMES.iter().position(|&expected| expected == name)?];
            if record[25] != 0 || file.is_some() {
                return None;
            }
            *file = Some(iso_extent(base, record, block)?);
        }
    }
    let [audit, unit, guest, service, finalize, meta, user] = files;
    Some([audit?, unit?, guest?, service?, finalize?, meta?, user?])
}

fn iso_extent<'a>(base: &'a [u8], record: &[u8], block: usize) -> Option<&'a [u8]> {
    let number = |offset| {
        let (little, big) = record.get(offset..offset + 8)?.split_at(4);
        let value = u32::from_le_bytes(little.try_into().ok()?);
        (value == u32::from_be_bytes(big.try_into().ok()?)).then_some(value as usize)
    };
    // The base writer emits neither extended attribute records nor interleave.
    if *record.get(1)? != 0 || record.get(26..28)? != [0, 0] {
        return None;
    }
    let start = number(2)?.checked_mul(block)?;
    let length = number(10)?;
    if length == 0 {
        return None;
    }
    base.get(start..start.checked_add(length)?)
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
        ARCHIVE_ARG,
        VERIFY_LINE,
    ]
    .join("\0")
    .into_bytes())
}

const STAGE_LINE: &str = "stage_deb '{path}' '{name}' '{size}' '{sha256}'\n";
const ARCHIVE_ARG: &str = "'/var/lib/arxcovo/web-install/{name}.deb' ";
const VERIFY_LINE: &str = "verify_package \"{root}\" '{name}' '{version}'\n";

const INSTALL_PROFILE: &str = r#"set -eu
export LC_ALL=C
target=/target
dir=$target/var/lib/arxcovo/web-install
install -d -m 0755 "$target/var/lib/arxcovo"
mkdir -m 0755 "$dir"
stage_deb() {
  src=/cdrom/$1
  dest=$dir/$2.deb
  test -f "$src"
  test ! -L "$src"
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
  for archive do
    dpkg --install "$archive"
  done
  audit=$(dpkg --audit)
  test -z "$audit"
' arx-web-install {archives}
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

pub use origin::{OriginPayload, OriginProvisioning};

mod origin {
    use super::Id;

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
            if !(1..=8 * 1024).contains(&leaf_pem.len()) || !(1..=4 * 1024).contains(&key_pem.len())
            {
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
    ///
    /// Raw bytes are visible only within the sealed origin module. A2 should
    /// implement the authenticated serial encoder as a private child here.
    ///
    /// ```compile_fail,E0624
    /// use arxd::vm::ubuntu_web::OriginProvisioning;
    /// let plan = OriginProvisioning::new([1; 16]);
    /// let payload = plan.payload(b"leaf fixture", b"key fixture").unwrap();
    /// let _ = payload.into_private_parts();
    /// ```
    pub struct OriginPayload<'a> {
        plan: &'a OriginProvisioning,
        leaf_pem: &'a [u8],
        key_pem: &'a [u8],
    }

    impl<'a> OriginPayload<'a> {
        pub fn server_name(&self) -> &str {
            self.plan.server_name()
        }

        #[allow(
            dead_code,
            reason = "A2 supplies the private authenticated serial encoder"
        )]
        fn into_private_parts(self) -> (&'a [u8], &'a [u8]) {
            (self.leaf_pem, self.key_pem)
        }
    }

    #[cfg(test)]
    #[test]
    fn sealed_payload_preserves_exact_transport_bytes() {
        let plan = OriginProvisioning::new([1; 16]);
        let payload = plan.payload(b"leaf fixture", b"key fixture").unwrap();
        assert_eq!(
            payload.into_private_parts(),
            (&b"leaf fixture"[..], &b"key fixture"[..])
        );
    }
}

#[cfg(test)]
#[test]
fn cidata_reader_follows_relocated_root_and_rejects_differing_extent_copies() {
    const BLOCK: usize = 2048;
    let files = [b"a", b"b", b"c", b"d", b"e", b"f", b"g"].map(<[u8; 1]>::as_slice);
    let mut seed = Vec::new();
    Image::new(files).unwrap().write(&mut seed).unwrap();
    let root_record = 16 * BLOCK + 156;
    let mut old_root = iso_extent(&seed, &seed[root_record..], BLOCK)
        .unwrap()
        .to_vec();
    // Relocate the primary directory, leaving the old extent unusable.
    let old_offset = u32::from_le_bytes(seed[root_record + 2..root_record + 6].try_into().unwrap())
        as usize
        * BLOCK;
    seed[old_offset..old_offset + old_root.len()].fill(0);
    let new_lba = (seed.len() / BLOCK) as u32;
    let parent_record = usize::from(old_root[0]);
    for entry in [0, parent_record] {
        old_root[entry + 2..entry + 6].copy_from_slice(&new_lba.to_le_bytes());
        old_root[entry + 6..entry + 10].copy_from_slice(&new_lba.to_be_bytes());
    }
    seed.extend_from_slice(&old_root);
    let pvd = 16 * BLOCK;
    let little_table =
        u32::from_le_bytes(seed[pvd + 140..pvd + 144].try_into().unwrap()) as usize * BLOCK;
    let big_table =
        u32::from_be_bytes(seed[pvd + 148..pvd + 152].try_into().unwrap()) as usize * BLOCK;
    seed[little_table + 2..little_table + 6].copy_from_slice(&new_lba.to_le_bytes());
    seed[big_table + 2..big_table + 6].copy_from_slice(&new_lba.to_be_bytes());
    for descriptor in [16 * BLOCK, 17 * BLOCK] {
        seed[descriptor + 80..descriptor + 84].copy_from_slice(&(new_lba + 1).to_le_bytes());
        seed[descriptor + 84..descriptor + 88].copy_from_slice(&(new_lba + 1).to_be_bytes());
    }
    seed[root_record + 2..root_record + 6].copy_from_slice(&new_lba.to_le_bytes());
    seed[root_record + 6..root_record + 10].copy_from_slice(&new_lba.to_be_bytes());
    assert_eq!(cidata_files(&seed), Some(files));
    seed[root_record + 6] ^= 1;
    assert_eq!(cidata_files(&seed), None);
}

#[cfg(test)]
mod static_runtime_tests {
    use super::*;

    #[test]
    fn static_seed_rebinds_metadata_and_preserves_other_installer_bytes() {
        let files = [b"a", b"b", b"c", b"d", b"e", b"f", b"g"].map(<[u8; 1]>::as_slice);
        let mut seed = Vec::new();
        Image::new(files).unwrap().write(&mut seed).unwrap();
        assert!(rebind_seed(&seed, [0; 16]).is_err());
        assert!(rebind_seed(b"invalid media", [23; 16]).is_err());

        let rebound = rebind_seed(&seed, [23; 16]).unwrap();
        let metadata = super::super::ubuntu::nocloud_meta([23; 16]);
        let mut expected = files;
        expected[5] = metadata.as_bytes();
        assert_eq!(cidata_files(&rebound), Some(expected));
        assert_eq!(cidata_files(&seed), Some(files));
    }

    #[test]
    fn static_web_health_keeps_every_package_check_within_the_guest_script_bound() {
        let base = UbuntuRecipe::default();
        let names = (0..MAX_PACKAGES)
            .map(|n| format!("package-{n:02}-{}", "x".repeat(45)))
            .collect::<Vec<_>>();
        let version = format!("1.0+{}", "a".repeat(120));
        let artifact = ArtifactPin {
            size: 1,
            sha256: [1; 32],
        };
        let packages = names
            .iter()
            .map(|name| PoolPackage {
                name,
                version: &version,
                relative_path: "pool/package.deb",
                artifact,
            })
            .collect::<Vec<_>>();
        let manifest = IsoPoolManifest {
            iso: AssetPin {
                length: 1,
                sha256: UbuntuRecipe::ISO_SHA256,
            },
            signed_metadata: artifact,
            packages: &packages,
        };
        let recipe = UbuntuWebRecipe {
            base: &base,
            manifest: &manifest,
            digest: [1; 32],
        };
        let checks = recipe.detached_checks();
        assert_eq!(checks[0], super::super::ubuntu::DETACHED_HEALTH);
        assert!(checks.len() > 2);
        let scripts = &checks[1..];
        assert!(scripts.iter().all(|script| {
            script.len() <= arx_guest::MAX_SCRIPT
                && script.contains("verify_package()")
                && script.contains("sha256sum --check --strict")
        }));
        for name in &names {
            let check = format!("verify_package \"\" '{name}' '{version}'");
            assert_eq!(
                scripts
                    .iter()
                    .filter(|script| script.contains(&check))
                    .count(),
                1,
                "{name}"
            );
        }
        assert_eq!(
            scripts
                .iter()
                .map(|script| script.matches("verify_package \"\" '").count())
                .sum::<usize>(),
            MAX_PACKAGES
        );
    }
}
