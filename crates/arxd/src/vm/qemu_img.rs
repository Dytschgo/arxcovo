use super::{
    paths::{Artifact, ArtifactRole, InvalidPath, LocalPath},
    qemu_args::{self, CommandPlan},
    runtime::OfflineLease,
};
use serde::Deserialize;

const CLUSTER: usize = 65_536;
const MAX_INFO: usize = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidImage;

/// Only the runtime recipe's qcow2 subset. Inspect the first cluster before
/// allowing qemu-img to resolve any embedded reference.
pub fn inspect_header(cluster: &[u8], expected: Option<&LocalPath>) -> Result<u64, InvalidImage> {
    if cluster.len() != CLUSTER || &cluster[..4] != b"QFI\xfb" {
        return Err(InvalidImage);
    }
    let u32_at = |i| u32::from_be_bytes(cluster[i..i + 4].try_into().unwrap());
    let u64_at = |i| u64::from_be_bytes(cluster[i..i + 8].try_into().unwrap());
    let virtual_size = u64_at(24);
    let offset = usize::try_from(u64_at(8)).map_err(|_| InvalidImage)?;
    let len = u32_at(16) as usize;
    if u32_at(4) != 3
        || u32_at(20) != 16
        || virtual_size == 0
        || u32_at(32) != 0
        || u32_at(60) != 0
        || u64_at(72) != 0
        || u64_at(80) != 0
        || u64_at(88) != 0
        || u32_at(96) != 4
        || u32_at(100) != 104
    {
        return Err(InvalidImage);
    }
    let end = match expected {
        None if offset == 0 && len == 0 => CLUSTER,
        Some(backing)
            if offset >= 112
                && offset.checked_add(len).and_then(|e| cluster.get(offset..e))
                    == Some(backing.as_str().as_bytes()) =>
        {
            offset
        }
        _ => return Err(InvalidImage),
    };
    let mut entries = &cluster[104..end];
    let mut backing_format = false;
    let mut feature_table = false;
    while entries.len() >= 8 {
        let kind = u32::from_be_bytes(entries[..4].try_into().unwrap());
        let len = u32::from_be_bytes(entries[4..8].try_into().unwrap()) as usize;
        if kind == 0 && len == 0 && backing_format == expected.is_some() {
            return Ok(virtual_size);
        }
        let padded = len.checked_add(7).ok_or(InvalidImage)? & !7;
        let next = padded.checked_add(8).ok_or(InvalidImage)?;
        let extension = entries.get(8..next).ok_or(InvalidImage)?;
        match kind {
            0xe279_2aca if !backing_format && &extension[..len] == b"qcow2" => {
                backing_format = true
            }
            0x6803_f857 if !feature_table && len.is_multiple_of(48) => feature_table = true,
            _ => return Err(InvalidImage),
        }
        entries = &entries[next..];
    }
    Err(InvalidImage)
}

pub fn golden_info<'a>(
    image: &'a Artifact,
    golden: &Artifact,
    header: &[u8],
) -> Result<CommandPlan<'a>, InvalidImage> {
    inspect_header(header, None)?;
    info(image, golden, ArtifactRole::Golden)
}

pub fn offline_info<'a>(
    _lease: &OfflineLease,
    image: &'a Artifact,
    disk: &Artifact,
    header: &[u8],
    backing: Option<&Artifact>,
) -> Result<CommandPlan<'a>, InvalidImage> {
    let backing = backing
        .map(|artifact| role(artifact, ArtifactRole::Golden))
        .transpose()?;
    inspect_header(header, backing)?;
    info(image, disk, ArtifactRole::GuestDisk)
}

fn info<'a>(
    image: &'a Artifact,
    disk: &Artifact,
    disk_role: ArtifactRole,
) -> Result<CommandPlan<'a>, InvalidImage> {
    Ok(qemu_args::qemu_img_info(
        role(image, ArtifactRole::QemuImg)?,
        role(disk, disk_role)?,
    ))
}

pub fn create<'a>(
    lease: &'a OfflineLease,
    image: &'a Artifact,
    staging: &Artifact,
    backing: Option<&Artifact>,
    size: u64,
    ceiling: u64,
) -> Result<CommandPlan<'a>, InvalidImage> {
    let clusters = size.div_ceil(CLUSTER as u64);
    let peak = clusters
        .checked_mul(CLUSTER as u64 + 32)
        .and_then(|n| n.checked_add(4 * CLUSTER as u64));
    if size == 0 || peak.is_none_or(|peak| peak > ceiling) {
        return Err(InvalidImage);
    }
    let backing = backing
        .map(|artifact| role(artifact, ArtifactRole::Golden))
        .transpose()?;
    Ok(qemu_args::qemu_img_create(
        lease,
        role(image, ArtifactRole::QemuImg)?,
        role(staging, ArtifactRole::Staging)?,
        backing,
        size,
    ))
}

fn role(artifact: &Artifact, role: ArtifactRole) -> Result<&LocalPath, InvalidImage> {
    artifact.path_for(role).map_err(|InvalidPath| InvalidImage)
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Info {
    filename: String,
    format: String,
    virtual_size: u64,
    actual_size: u64,
    backing_filename: Option<String>,
    full_backing_filename: Option<String>,
    backing_filename_format: Option<String>,
    dirty_flag: bool,
    encrypted: Option<bool>,
}

pub fn parse_info(
    bytes: &[u8],
    disk: &LocalPath,
    expected_size: u64,
    ceiling: u64,
    backing: Option<&LocalPath>,
) -> Result<u64, InvalidImage> {
    if bytes.len() > MAX_INFO {
        return Err(InvalidImage);
    }
    let info: Info = serde_json::from_slice(bytes).map_err(|_| InvalidImage)?;
    let backing = backing.map(LocalPath::as_str);
    if info.filename != disk.as_str()
        || info.format != "qcow2"
        || info.virtual_size == 0
        || info.virtual_size != expected_size
        || info.actual_size > ceiling
        || info.dirty_flag
        || info.encrypted.unwrap_or(false)
        || info.backing_filename.as_deref() != backing
        || info.full_backing_filename.as_deref() != backing
        || info.backing_filename_format.as_deref() != backing.map(|_| "qcow2")
    {
        return Err(InvalidImage);
    }
    Ok(info.actual_size)
}
