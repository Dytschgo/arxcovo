//! Offline Compose recipe attestation. Qualification does not grant a launch.
use super::{
    RecipeError,
    media::Image,
    ubuntu_web::{UbuntuWebRecipe, cidata_files},
};
use crate::vm::{Id, paths::ArtifactPin};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Read};

pub const RECIPE_ID: &str = "ubuntu-noble-amd64-compose-v1";
pub const DEPENDENCY_LABEL: &str = "ARXCOMPOSE";
pub const MAX_MEDIUM: u64 = 512 * 1024 * 1024;
const REQUIRED: [&str; 4] = [
    "docker-ce",
    "docker-ce-cli",
    "containerd.io",
    "docker-compose-plugin",
];

#[derive(Clone, Copy)]
pub struct PackagePin<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub archive: ArtifactPin,
}

pub struct Closure<'a> {
    pub docker_fingerprint: [u8; 20],
    pub docker_signing_key: ArtifactPin,
    pub signed_docker_metadata: ArtifactPin,
    pub signed_ubuntu_metadata: ArtifactPin,
    pub base_versions: ArtifactPin,
    pub medium: ArtifactPin,
    pub packages: &'a [PackagePin<'a>],
}

/// Supplies the independent signature/index, dependency closure and medium
/// inventory witness for the exact pinned Ubuntu and Docker package bytes.
pub trait OfflineVerifier {
    fn verify(&self, web_digest: [u8; 32], closure: &Closure<'_>) -> bool;
}

pub struct UbuntuComposeRecipe<'a> {
    closure: &'a Closure<'a>,
    web: &'a UbuntuWebRecipe<'a>,
    digest: [u8; 32],
}

impl<'a> UbuntuComposeRecipe<'a> {
    pub fn qualify(
        web: &'a UbuntuWebRecipe<'a>,
        closure: &'a Closure<'a>,
        expected: ArtifactPin,
        verifier: &impl OfflineVerifier,
    ) -> Result<Self, RecipeError> {
        let web_digest = web.recipe_digest();
        if closure.receipt(web_digest)? != expected || !verifier.verify(web_digest, closure) {
            return Err(RecipeError::Unqualified);
        }
        Ok(Self {
            closure,
            web,
            digest: expected.sha256,
        })
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    /// Produce the seven-file seed for inspection. A future activation
    /// adapter must also verify custody of the ISO, seed and dependency medium.
    pub fn render_seed(&self, build: Id, guest: &[u8]) -> Result<Vec<u8>, RecipeError> {
        let base = self.web.render_seed(build, guest)?;
        let mut files = cidata_files(&base).ok_or(RecipeError::Unqualified)?;
        let mut user =
            String::from_utf8(files[6].to_vec()).map_err(|_| RecipeError::Unqualified)?;
        user.push_str("    - |\n");
        for line in self.closure.install_command().lines() {
            if !line.is_empty() {
                user.push_str("      ");
            }
            user.push_str(line);
            user.push('\n');
        }
        files[6] = user.as_bytes();
        let mut seed = Vec::new();
        Image::new(files)
            .ok_or(RecipeError::Unqualified)?
            .write(&mut seed)
            .map_err(|error| RecipeError::Write(error.kind()))?;
        Ok(seed)
    }

    pub fn detached_health(&self) -> String {
        let mut script = self.web.detached_health();
        script.push_str(DETACHED_COMPOSE);
        script.push_str(&self.closure.package_checks(""));
        script
    }

    /// Verify the bounded read-only medium supplied by the custodied reader
    /// before it is attached for install. Detached verification requires its
    /// removal and disk-only boot.
    pub fn verify_medium(&self, medium: impl Read) -> Result<(), RecipeError> {
        let pin = self.closure.medium;
        let mut limited = medium.take(pin.size + 1);
        let mut hash = Sha256::new();
        let size = std::io::copy(&mut limited, &mut hash).map_err(|_| RecipeError::Unqualified)?;
        if size != pin.size || hash.finalize()[..] != pin.sha256 {
            return Err(RecipeError::Unqualified);
        }
        Ok(())
    }
}

impl Closure<'_> {
    pub fn receipt(&self, web_digest: [u8; 32]) -> Result<ArtifactPin, RecipeError> {
        use std::fmt::Write as _;
        if !(4..=32).contains(&self.packages.len()) || self.docker_fingerprint == [0; 20] {
            return Err(RecipeError::Unqualified);
        }
        let mut text = format!(
            "{RECIPE_ID}\n{DEPENDENCY_LABEL}\n{}\n{}\n",
            hex::encode(web_digest),
            hex::encode(self.docker_fingerprint),
        );
        for pin in [
            self.docker_signing_key,
            self.signed_docker_metadata,
            self.signed_ubuntu_metadata,
            self.base_versions,
            self.medium,
        ] {
            append_pin(&mut text, pin)?;
        }
        let mut names = BTreeSet::new();
        let mut bytes = 0_u64;
        for pkg in self.packages {
            if !token(pkg.name, 64, b"+.-")
                || !token(pkg.version, 128, b"+.:-~")
                || !names.insert(pkg.name)
                || !pkg.name.as_bytes()[0].is_ascii_lowercase()
                || !pkg.version.as_bytes()[0].is_ascii_digit()
            {
                return Err(RecipeError::Unqualified);
            }
            writeln!(text, "{} {}", pkg.name, pkg.version).expect("write to string");
            append_pin(&mut text, pkg.archive)?;
            bytes = bytes
                .checked_add(pkg.archive.size)
                .ok_or(RecipeError::Unqualified)?;
        }
        if bytes > self.medium.size || !REQUIRED.iter().all(|name| names.contains(name)) {
            return Err(RecipeError::Unqualified);
        }
        writeln!(
            text,
            "{} {}",
            hex::encode(Sha256::digest(self.install_command())),
            hex::encode(Sha256::digest(DETACHED_COMPOSE))
        )
        .expect("write to string");
        Ok(ArtifactPin {
            size: text.len() as u64,
            sha256: Sha256::digest(text).into(),
        })
    }
}

fn append_pin(text: &mut String, pin: ArtifactPin) -> Result<(), RecipeError> {
    use std::fmt::Write as _;
    if !(1..=MAX_MEDIUM).contains(&pin.size) || pin.sha256 == [0; 32] {
        return Err(RecipeError::Unqualified);
    }
    writeln!(text, "{} {}", pin.size, hex::encode(pin.sha256)).expect("write to string");
    Ok(())
}

fn token(value: &str, max: usize, punctuation: &[u8]) -> bool {
    (1..=max).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || punctuation.contains(&b))
}

impl Closure<'_> {
    fn install_command(&self) -> String {
        use std::fmt::Write as _;
        let mut stage = String::new();
        let mut archives = String::new();
        for pkg in self.packages {
            writeln!(
                stage,
                "stage_deb {} {} {}",
                pkg.name,
                pkg.archive.size,
                hex::encode(pkg.archive.sha256)
            )
            .expect("write to string");
            write!(
                archives,
                " /var/lib/arxcovo/compose-install/{}.deb",
                pkg.name
            )
            .expect("write to string");
        }
        INSTALL_COMPOSE
            .replace("{stage}", stage.trim_end())
            .replace("{archives}", &archives)
            .replace("{verify}", &self.package_checks("/target"))
    }

    fn package_checks(&self, root: &str) -> String {
        use std::fmt::Write as _;
        let mut script = String::new();
        for pkg in self.packages {
            writeln!(
                script,
                "verify_package '{root}' '{}' '{}'",
                pkg.name, pkg.version
            )
            .expect("write to string");
        }
        script
    }
}

const INSTALL_COMPOSE: &str = r#"set -eu
export LC_ALL=C
target=/target
dir=$target/var/lib/arxcovo/compose-install
medium=/run/arx-compose-install
mkdir -m 0700 "$dir" "$medium"
mount -t iso9660 -o ro,nodev,nosuid,noexec /dev/disk/by-label/ARXCOMPOSE "$medium"
policy=$target/usr/sbin/policy-rc.d
had_policy=0
if test -e "$policy" || test -L "$policy"; then
  mv -T "$policy" "$dir/policy-original"
  had_policy=1
fi
cleanup() {
  rm -f "$policy"
  if test "$had_policy" = 1; then mv -T "$dir/policy-original" "$policy"; fi
  umount "$medium"
  rmdir "$medium"
}
trap cleanup EXIT
printf '#!/bin/sh\nexit 101\n' > "$policy"
chmod 0755 "$policy"
systemctl --root="$target" mask docker.service docker.socket containerd.service nginx.service
stage_deb() {
  src=$medium/$1.deb
  dest=$dir/$1.deb
  test -f "$src"
  test ! -L "$src"
  test "$(stat -c %s "$src")" = "$2"
  install -m 0600 "$src" "$dest"
  printf '%s  %s\n' "$3" "$dest" | sha256sum --check --strict
}
{stage}
curtin in-target --target="$target" -- /bin/sh -ec '
  export DEBIAN_FRONTEND=noninteractive
  for archive do dpkg --install "$archive"; done
  test -z "$(dpkg --audit)"
' arx-compose-install{archives}
verify_package() {
  actual=$(dpkg-query --admindir="$1/var/lib/dpkg" -W -f='${Status}:${Version}' "$2")
  test "$actual" = "install ok installed:$3"
}
{verify}
cleanup
trap - EXIT
rm -f "$dir"/*.deb
rmdir "$dir"
install -d -o root -g root -m 0700 "$target/var/lib/arx-compose" "$target/var/lib/arx-compose/empty" "$target/var/lib/arx-compose/docker-config" "$target/var/lib/arx-compose/backup-staging" "$target/var/lib/docker/volumes"
install -d -m 0755 "$target/etc/systemd/system/arx-guest.service.d"
cat > "$target/etc/systemd/system/arx-guest.service.d/compose.conf" <<'ARX_GUEST'
[Unit]
RequiresMountsFor=/run/arx-compose/secrets
ARX_GUEST
printf '%s\n' 'tmpfs /run/arx-compose/secrets tmpfs rw,nosuid,nodev,noexec,mode=0700,size=4M 0 0' >> "$target/etc/fstab"
systemctl --root="$target" mask sleep.target suspend.target hibernate.target hybrid-sleep.target
sed -i '/^[^#].*[[:space:]]swap[[:space:]]/s/^/#/' "$target/etc/fstab"
printf '%s\n' ubuntu-noble-amd64-compose-v1 > "$target/etc/arxcovo/compose-finalized"
chown root:root "$target/etc/arxcovo/compose-finalized"
chmod 0644 "$target/etc/arxcovo/compose-finalized"
"#;

const DETACHED_COMPOSE: &str = r#"
test ! -e /dev/disk/by-label/ARXCOMPOSE
test ! -e /var/lib/arxcovo/compose-install
for unit in docker.service docker.socket containerd.service nginx.service; do
  test "$(systemctl is-enabled "$unit" 2>/dev/null || :)" = masked
  ! systemctl is-active --quiet "$unit"
done
test ! -e /run/docker.sock
test -z "$(swapon --noheadings)"
test "$(findmnt -n -o FSTYPE --mountpoint /run/arx-compose/secrets)" = tmpfs
test "$(stat -c '%U:%G:%a' /run/arx-compose/secrets)" = root:root:700
test -z "$(find /run/arx-compose/secrets -mindepth 1 -maxdepth 1 -print -quit)"
test "$(stat -c '%U:%G:%a' /var/lib/arx-compose/backup-staging)" = root:root:700
test -z "$(find /var/lib/arx-compose/backup-staging -mindepth 1 -maxdepth 1 -print -quit)"
printf '%s\n' ubuntu-noble-amd64-compose-v1 | cmp -s - /etc/arxcovo/compose-finalized
"#;
