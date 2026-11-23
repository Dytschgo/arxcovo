use super::{
    paths::{Artifact, LocalPath},
    qemu_args::{self, CommandPlan},
    runtime::OfflineLease,
};
use serde::Deserialize;

pub(super) const CLUSTER: usize = 65_536;
const MAX_INFO: usize = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidImage;

/// `QCowHeader` sizes in QEMU v11.1.3 `block/qcow2.h`: the version-3 fields
/// end at byte 104 (`header_length` is at 100), and the 11.1.3 writer adds
/// `compression_type` at 104 plus 7 padding bytes, for 112 in total.
const V3_BASE: usize = 104;
const V3_PINNED: usize = 112;
/// `QCOW2_INCOMPAT_COMPRESSION` (bit 3). Every other incompatible bit is
/// refused: dirty (0), corrupt (1), external data file (2), extended L2 (4)
/// and anything unknown.
const INCOMPAT_COMPRESSION: u64 = 1 << 3;
/// `QCOW2_COMPRESSION_TYPE_ZLIB` and `QCOW2_COMPRESSION_TYPE_ZSTD`.
const COMPRESSION_ZLIB: u8 = 0;
const COMPRESSION_ZSTD: u8 = 1;
/// QEMU limits a backing file name to 1023 bytes.
const MAX_BACKING_NAME: usize = 1023;

/// Only the runtime recipe's qcow2 subset, parsed against the pinned header
/// layout. Inspect the first cluster before allowing qemu-img or QEMU to
/// resolve any embedded reference. `expected` is the one approved backing
/// file; `None` requires an unbacked image.
pub fn inspect_header(cluster: &[u8], expected: Option<&LocalPath>) -> Result<u64, InvalidImage> {
    if cluster.len() != CLUSTER || &cluster[..4] != b"QFI\xfb" {
        return Err(InvalidImage);
    }
    let u32_at = |i| u32::from_be_bytes(cluster[i..i + 4].try_into().unwrap());
    let u64_at = |i| u64::from_be_bytes(cluster[i..i + 8].try_into().unwrap());
    let header_length = u32_at(100) as usize;
    let compression = match header_length {
        V3_BASE => COMPRESSION_ZLIB,
        V3_PINNED if cluster[105..V3_PINNED].iter().all(|&byte| byte == 0) => cluster[104],
        _ => return Err(InvalidImage),
    };
    let incompatible = match compression {
        COMPRESSION_ZLIB => 0,
        COMPRESSION_ZSTD => INCOMPAT_COMPRESSION,
        _ => return Err(InvalidImage),
    };
    let virtual_size = u64_at(24);
    if u32_at(4) != 3
        || u32_at(20) != 16
        || virtual_size == 0
        || u32_at(32) != 0
        || u32_at(60) != 0
        || u64_at(72) != incompatible
        || u64_at(80) != 0
        || u64_at(88) != 0
        || u32_at(96) != 4
    {
        return Err(InvalidImage);
    }
    let offset = usize::try_from(u64_at(8)).map_err(|_| InvalidImage)?;
    let len = u32_at(16) as usize;
    let end = match expected {
        None if offset == 0 && len == 0 => CLUSTER,
        Some(backing)
            if len <= MAX_BACKING_NAME
                && offset >= header_length + 8
                && offset.checked_add(len).and_then(|e| cluster.get(offset..e))
                    == Some(backing.as_str().as_bytes()) =>
        {
            offset
        }
        _ => return Err(InvalidImage),
    };
    // Header extensions are bounded by the backing name or the first cluster.
    let mut entries = &cluster[header_length..end];
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
            // External data file, encryption, bitmaps and anything unknown.
            _ => return Err(InvalidImage),
        }
        entries = &entries[next..];
    }
    Err(InvalidImage)
}

/// Read-only inspection of the immutable golden image. Validation happens in
/// the sealed constructor.
pub fn golden_info<'a>(
    image: &'a Artifact,
    golden: &Artifact,
    header: &[u8],
) -> Result<CommandPlan<'a>, InvalidImage> {
    qemu_args::golden_info(image, golden, header)
}

/// Inspection of a writable guest disk under the offline lease.
pub fn offline_info<'a>(
    lease: &'a OfflineLease,
    image: &'a Artifact,
    disk: &Artifact,
    header: &[u8],
    backing: Option<&Artifact>,
) -> Result<CommandPlan<'a>, InvalidImage> {
    qemu_args::offline_info(lease, image, disk, header, backing)
}

/// Exclusive qcow2 creation of a staging image under the offline lease. A
/// backing golden comes with its first cluster, which must be unbacked.
pub fn create<'a>(
    lease: &'a OfflineLease,
    image: &'a Artifact,
    staging: &Artifact,
    backing: Option<(&Artifact, &[u8])>,
    size: u64,
    ceiling: u64,
) -> Result<CommandPlan<'a>, InvalidImage> {
    qemu_args::create(lease, image, staging, backing, size, ceiling)
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
