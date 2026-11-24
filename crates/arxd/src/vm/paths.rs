use sha2::{Digest, Sha256};
use std::io::{self, Read};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPath;

/// Validates raw spelling before any host Path normalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalPath(String);

impl LocalPath {
    pub fn parse(raw: &str) -> Result<Self, InvalidPath> {
        let bytes = raw.as_bytes();
        if bytes.len() > 240
            || !matches!(bytes, [drive, b':', b'\\' | b'/', ..] if drive.is_ascii_alphabetic())
        {
            return Err(InvalidPath);
        }
        let normalized = raw.replace('/', "\\");
        if normalized.len() > 3 && !normalized[3..].split('\\').all(safe_component) {
            return Err(InvalidPath);
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_drive_root(&self) -> bool {
        self.0.len() == 3
    }

    pub fn contains(&self, child: &Self) -> bool {
        let root = self.0.to_ascii_lowercase();
        let child = child.0.to_ascii_lowercase();
        child == root
            || child
                .strip_prefix(&root)
                .is_some_and(|tail| self.is_drive_root() || tail.starts_with('\\'))
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.contains(other) || other.contains(self)
    }

    pub fn join(&self, name: &str) -> Result<Self, InvalidPath> {
        if !safe_component(name) {
            return Err(InvalidPath);
        }
        let separator = if self.is_drive_root() { "" } else { "\\" };
        Self::parse(&format!("{}{separator}{name}", self.0))
    }
}

fn safe_component(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end_matches(' ');
    let stem = stem.to_ascii_uppercase();
    !name.is_empty()
        && !name.ends_with(['.', ' '])
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'.' | b'-' | b'_'))
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit())
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactRole {
    Qemu,
    /// The separately pinned patched QEMU/libslirp build for a public-egress
    /// static boot (ADR 0064). Never interchangeable with the stock `Qemu`.
    PublicEgressQemu,
    /// The read-only custodied public-egress policy artifact (ADR 0064).
    PublicEgressPolicy,
    QemuImg,
    FirmwareCode,
    VarsSeed,
    Vars,
    GuestDisk,
    StaticDisk,
    StaticVars,
    SnapshotDisk,
    SnapshotVars,
    SnapshotStagingDisk,
    SnapshotStagingVars,
    Golden,
    TestInstallDisk,
    WindowsSetupVars,
    UbuntuInstallIso,
    UbuntuCidata,
    UbuntuKernel,
    UbuntuInitrd,
    WindowsInstallIso,
    WindowsAnswerSeed,
    WindowsDriverMedium,
    Staging,
    Scratch,
    Dll,
    Data,
}

impl ArtifactRole {
    pub fn writable(self) -> bool {
        matches!(
            self,
            Self::Vars
                | Self::WindowsSetupVars
                | Self::GuestDisk
                | Self::StaticDisk
                | Self::StaticVars
                | Self::SnapshotStagingDisk
                | Self::SnapshotStagingVars
                | Self::TestInstallDisk
                | Self::Staging
                | Self::Scratch
        )
    }
}

#[derive(Clone, Debug)]
pub struct Artifact {
    role: ArtifactRole,
    path: LocalPath,
    identity: Option<FileIdentity>,
    pin: Option<ArtifactPin>,
}

impl Artifact {
    /// A recipe input only. Native consumption also requires held custody.
    pub fn new(role: ArtifactRole, path: LocalPath) -> Result<Self, InvalidPath> {
        if path.is_drive_root() {
            return Err(InvalidPath);
        }
        Ok(Self {
            role,
            path,
            identity: None,
            pin: None,
        })
    }

    pub fn path_for(&self, role: ArtifactRole) -> Result<&LocalPath, InvalidPath> {
        (self.role == role).then_some(&self.path).ok_or(InvalidPath)
    }

    /// Expected object from staging or a recipe receipt. Only the native
    /// launcher can compare it with a retained filesystem object.
    pub fn with_identity(mut self, identity: FileIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    pub fn with_pin(mut self, pin: ArtifactPin) -> Self {
        self.pin = Some(pin);
        self
    }

    pub(super) fn identity(&self) -> Option<FileIdentity> {
        self.identity
    }

    pub(super) fn pin(&self) -> Option<ArtifactPin> {
        self.pin
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileIdentity {
    pub volume: u64,
    pub file: [u8; 16],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactPin {
    pub size: u64,
    pub sha256: [u8; 32],
}

impl ArtifactPin {
    pub fn verify(self, reader: impl Read, limit: u64) -> io::Result<bool> {
        if self.size == 0 || self.size > limit {
            return Ok(false);
        }
        let mut reader = reader.take(self.size.saturating_add(1));
        let mut digest = Sha256::new();
        let size = io::copy(&mut reader, &mut digest)?;
        Ok(size == self.size && digest.finalize()[..] == self.sha256)
    }
}
