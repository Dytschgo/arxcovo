//! Disjoint TEST scope: sacrificial pressure, disposable recipes and the
//! independently read-only production store.

use super::*;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom},
    net::IpAddr,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StorageEnrollment {
    recipe: ScopedVolume,
    production: ScopedVolume,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopedVolume {
    root: String,
    root_identity: VolumePin,
    volume_identity: VolumePin,
    volume_guid: String,
    backing: String,
    backing_identity: VolumePin,
}

pub(super) struct Storage {
    pub(super) volume: capacity::AnchoredVolume,
    pub(super) root: PathPin,
    backing: PathPin,
    _vhd: OwnedHandle,
    _device: capacity::HeldDevice,
    pub(super) limits: Limits,
}

impl Storage {
    fn hold(
        names: &impl capacity::VolumeNames,
        enrolled: &ScopedVolume,
        outer: &capacity::AnchoredVolume,
        owner: PSID,
    ) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        let root = local(&enrolled.root)?;
        let path = local(&enrolled.backing)?;
        if root.is_drive_root()
            || !path.as_str()[..3].eq_ignore_ascii_case(&outer.drive)
            || path.is_drive_root()
        {
            return Err(reason);
        }
        let volume = capacity::anchor_volume(names, &root.as_str()[..3])?;
        let root = capacity::walk(names, &volume, &root, true, false)?;
        let backing = capacity::walk(names, outer, &path, false, true)?;
        if !enrolled.root_identity.matches(root.identity)
            || !enrolled.volume_identity.matches(volume.identity)
            || !volume.guid.eq_ignore_ascii_case(&enrolled.volume_guid)
            || volume.guid_bytes == outer.guid_bytes
            || volume.identity.volume == outer.identity.volume
            || !enrolled.backing_identity.matches(backing.identity)
            || !sink::held_private(&root.leaf, owner)
        {
            return Err(reason);
        }
        let (vhd, facts) = capacity::fixed_vhd(names, outer, &backing)?;
        let device = capacity::volume_extent(&volume, facts.disk)?;
        let limits = capacity::check_capacity(&capacity::CapacitySample {
            provider_subtype: facts.provider_subtype,
            vhd_virtual: facts.virtual_size,
            vhd_physical: facts.physical_size,
            sector: facts.sector,
            extent_start: device.extent_start,
            extent_length: device.extent_length,
            inner: capacity::held_free(&root.leaf)?,
            outer: capacity::held_free(&outer.root)?,
            backing_length: backing.leaf.metadata().map_err(|_| reason)?.len(),
            backing_allocated: backing.allocated,
        })?;
        if !capacity::still_bound(names, &volume) || !capacity::still_bound(names, outer) {
            return Err(reason);
        }
        Ok(Self {
            volume,
            root,
            backing,
            _vhd: vhd,
            _device: device,
            limits,
        })
    }

    fn identity(&self) -> impl Serialize {
        (
            self.volume.guid_bytes,
            self.volume.identity.volume,
            self.volume.identity.file,
            self.root.identity.file,
            self.backing.identity.file,
            self.limits.vhd_virtual_bytes,
            self.limits.inner_extent_bytes,
        )
    }
}

pub(super) struct Scope {
    pub(super) recipe: Storage,
    production: Storage,
    _database: PathPin,
    _lock: PathPin,
    pub(super) key: [u8; 32],
}

impl Scope {
    pub(super) fn validate_live(&self) -> Result<(), NativeRefusal> {
        for scope in [&self.recipe, &self.production] {
            if !capacity::still_bound(&capacity::MountManager, &scope.volume) {
                return Err(NativeRefusal::PathCustodyUnverified);
            }
            inventory::binding(&scope.root)?;
            inventory::binding(&scope.backing)?;
        }
        inventory::binding(&self._database)?;
        inventory::binding(&self._lock)
    }

    pub(super) fn hold(
        names: &impl capacity::VolumeNames,
        enrollment: &StorageEnrollment,
        trial: &PathPin,
        backing: &PathPin,
        volumes: [&capacity::AnchoredVolume; 2],
        evidence: &LocalPath,
        owner: PSID,
    ) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        let [trial_volume, outer] = volumes;
        let recipe = Storage::hold(names, &enrollment.recipe, outer, owner)?;
        let production = Storage::hold(names, &enrollment.production, outer, owner)?;
        let guids = [
            trial_volume.guid_bytes,
            recipe.volume.guid_bytes,
            production.volume.guid_bytes,
        ];
        let serials = [
            trial_volume.identity.volume,
            recipe.volume.identity.volume,
            production.volume.identity.volume,
        ];
        let backing_files = [
            backing.identity.file,
            recipe.backing.identity.file,
            production.backing.identity.file,
        ];
        if !distinct(&guids)
            || !distinct(&serials)
            || !distinct(&backing_files)
            || [backing, &recipe.backing, &production.backing]
                .into_iter()
                .any(|pin| pin.path.overlaps(evidence))
            || trial.path.overlaps(&recipe.root.path)
            || trial.path.overlaps(&production.root.path)
            || recipe.root.path.overlaps(&production.root.path)
        {
            return Err(reason);
        }
        let database = production
            .root
            .path
            .join("activity.sqlite3")
            .map_err(|_| reason)?;
        let lock = production
            .root
            .path
            .join("activity.lock")
            .map_err(|_| reason)?;
        let database = capacity::walk(names, &production.volume, &database, false, false)?;
        let lock = capacity::walk(names, &production.volume, &lock, false, false)?;
        if !sink::held_private(&database.leaf, owner)
            || !sink::held_private(&lock.leaf, owner)
            || [database.identity, lock.identity]
                .into_iter()
                .any(|id| id.volume != production.root.identity.volume)
            || database.identity == lock.identity
        {
            return Err(reason);
        }
        let host = read_store_host(&production.root, &database)?;
        let key = crate::vm::qualification_record::digest(
            &serde_json::to_vec(&(
                "arx-vm-qualification-storage-v1",
                recipe.identity(),
                production.identity(),
                database.identity.file,
                lock.identity.file,
                host,
            ))
            .map_err(|_| reason)?,
        );
        Ok(Self {
            recipe,
            production,
            _database: database,
            _lock: lock,
            key,
        })
    }

    pub(super) fn within_shared_cap(&self, trial: &Limits) -> bool {
        aggregate_within_cap(
            trial.backing_allocated_bytes,
            self.recipe.limits.backing_allocated_bytes,
            self.production.limits.backing_allocated_bytes,
            0,
        )
    }

    pub(super) fn aggregate_reservation_within_cap(
        &self,
        trial: &Limits,
        extra_bytes: u64,
    ) -> bool {
        aggregate_within_cap(
            trial.backing_allocated_bytes,
            self.recipe.limits.backing_allocated_bytes,
            self.production.limits.backing_allocated_bytes,
            extra_bytes,
        )
    }
}

pub(super) fn aggregate_within_cap(
    trial_allocated: u64,
    recipe_allocated: u64,
    production_allocated: u64,
    extra_bytes: u64,
) -> bool {
    [
        trial_allocated,
        recipe_allocated,
        production_allocated,
        extra_bytes,
    ]
    .into_iter()
    .try_fold(0u64, u64::checked_add)
    .is_some_and(|sum| sum <= DISK_CEILING)
}

fn distinct<T: Eq>(values: &[T; 3]) -> bool {
    values[0] != values[1] && values[0] != values[2] && values[1] != values[2]
}

fn read_store_host(root: &PathPin, database: &PathPin) -> Result<crate::vm::Id, NativeRefusal> {
    let reason = NativeRefusal::PathCustodyUnverified;
    let length = database.leaf.metadata().map_err(|_| reason)?.len();
    if !(512..=128 * (1 << 20)).contains(&length) {
        return Err(reason);
    }
    let listed = listing_bounded(
        &root.leaf,
        4096,
        Some(Instant::now() + Duration::from_secs(2)),
    )
    .map_err(|_| reason)?;
    if listed.iter().any(|(name, _)| {
        [
            "activity.sqlite3-wal",
            "activity.sqlite3-journal",
            "activity.sqlite3-shm",
        ]
        .iter()
        .any(|excluded| name.eq_ignore_ascii_case(excluded))
    }) {
        return Err(reason);
    }
    inventory::binding(root)?;
    // LocalPath restricts the remaining URI alphabet. The held database
    // excludes all writers and the immutable, read-only SQLite connection
    // neither opens a sidecar nor runs the production store's migrations.
    let path = database
        .path
        .as_str()
        .replace('\\', "/")
        .replace(' ', "%20");
    let uri = format!("file:/{path}?immutable=1&mode=ro");
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
        | rusqlite::OpenFlags::SQLITE_OPEN_URI
        | rusqlite::OpenFlags::SQLITE_OPEN_PRIVATE_CACHE;
    let db = rusqlite::Connection::open_with_flags(uri, flags).map_err(|_| reason)?;
    let host = crate::store::vm::static_vm::host_id(&db).map_err(|_| reason)?;
    inventory::binding(root)?;
    Ok(host)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NetworkEnrollment {
    iso: ReleaseImage,
    kernel: ReleaseImage,
    initrd: ReleaseImage,
    witnesses: super::network::Addresses,
}

pub(super) struct NetworkInputs {
    pub(super) media: [PathPin; 3],
    pub(super) witnesses: super::network::Addresses,
    pub(super) projection: [u8; 32],
}

impl NetworkInputs {
    pub(super) fn hold(
        input: NetworkEnrollment,
        evidence: &LocalPath,
    ) -> Result<Self, NativeRefusal> {
        let resolvers = host_dns_servers().map_err(|_| NativeRefusal::NetworkFenceUnverified)?;
        Self::hold_with_resolvers(input, evidence, &resolvers)
    }

    fn hold_with_resolvers(
        input: NetworkEnrollment,
        evidence: &LocalPath,
        host_resolvers: &[IpAddr],
    ) -> Result<Self, NativeRefusal> {
        use crate::vm::UbuntuRecipe;
        let reason = NativeRefusal::QualificationMissing;
        if !input.witnesses.valid(host_resolvers)
            || input.iso.sha256 != hex::encode(UbuntuRecipe::ISO_SHA256)
        {
            return Err(reason);
        }
        let mut media = Vec::with_capacity(3);
        let mut pins = Vec::with_capacity(3);
        for (input, max) in [
            (input.iso, 8 * GIB),
            (input.kernel, 64 << 20),
            (input.initrd, 512 << 20),
        ] {
            let path = local(&input.path)?;
            if path.overlaps(evidence) {
                return Err(NativeRefusal::PathCustodyUnverified);
            }
            let mut sha256 = [0; 32];
            hex::decode_to_slice(input.sha256, &mut sha256).map_err(|_| reason)?;
            let mut file = PathPin::open(&path, false, false)?;
            let pin = ArtifactPin {
                size: input.size,
                sha256,
            };
            file.verify_content(pin, max)?;
            if media
                .iter()
                .any(|previous: &PathPin| previous.identity == file.identity)
            {
                return Err(NativeRefusal::PathCustodyUnverified);
            }
            media.push(file);
            pins.push(pin);
        }
        // Prove the enrolled kernel and initrd match the exact `/casper/vmlinuz`
        // and `/casper/initrd` extents inside the held authenticated ISO.
        verify_iso_boot_components(&media[0].leaf, pins[1], pins[2])
            .map_err(|_| NativeRefusal::DistributionUnverified)?;
        let projection = crate::vm::qualification_record::digest(
            &serde_json::to_vec(&(
                "arx-vm-network-inputs-v2",
                (pins[0].size, pins[0].sha256),
                (pins[1].size, pins[1].sha256),
                (pins[2].size, pins[2].sha256),
                crate::vm::qemu_args::NETWORK,
                input.witnesses.ipv4.to_string(),
                input.witnesses.ipv6.to_string(),
                input.witnesses.dns.to_string(),
            ))
            .map_err(|_| reason)?,
        );
        Ok(Self {
            media: media.try_into().map_err(|_| reason)?,
            witnesses: input.witnesses,
            projection,
        })
    }
}

/// Bounded ISO-9660 primary-volume reader proving that `kernel` and `initrd`
/// match `/casper/vmlinuz` and `/casper/initrd` inside the held ISO image.
fn verify_iso_boot_components<R: Read + Seek>(
    mut iso: R,
    kernel: ArtifactPin,
    initrd: ArtifactPin,
) -> io::Result<()> {
    const ISO_SECTOR: u64 = 2048;
    const MAX_DIR_BYTES: u64 = 2 * 1024 * 1024;
    let total_len = iso.seek(SeekFrom::End(0))?;
    if total_len < 17 * ISO_SECTOR {
        return Err(io::Error::other("ISO too small"));
    }
    let mut pvd = [0u8; ISO_SECTOR as usize];
    iso.seek(SeekFrom::Start(16 * ISO_SECTOR))?;
    iso.read_exact(&mut pvd)?;
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" || pvd[6] != 1 {
        return Err(io::Error::other(
            "missing ISO-9660 primary volume descriptor",
        ));
    }
    let sector_size = u64::from(u16::from_le_bytes([pvd[128], pvd[129]]));
    if sector_size != ISO_SECTOR {
        return Err(io::Error::other("unsupported ISO-9660 block size"));
    }
    let root_record = &pvd[156..190];
    if root_record[0] < 34 {
        return Err(io::Error::other("invalid ISO root record"));
    }
    let root_lba = u64::from(u32::from_le_bytes(root_record[2..6].try_into().unwrap()));
    let root_len = u64::from(u32::from_le_bytes(root_record[10..14].try_into().unwrap()));
    let (casper_lba, casper_len) = iso_find(
        &mut iso,
        total_len,
        root_lba,
        root_len,
        "CASPER",
        true,
        MAX_DIR_BYTES,
    )?;
    for (entry_name, expected, max_size) in
        [("VMLINUZ", kernel, 64 << 20), ("INITRD", initrd, 512 << 20)]
    {
        let (lba, len) = iso_find(
            &mut iso,
            total_len,
            casper_lba,
            casper_len,
            entry_name,
            false,
            MAX_DIR_BYTES,
        )?;
        if len != expected.size || len == 0 || len > max_size {
            return Err(io::Error::other("ISO component length mismatch"));
        }
        let start = lba
            .checked_mul(ISO_SECTOR)
            .ok_or_else(|| io::Error::other("ISO extent overflow"))?;
        if start.checked_add(len).is_none_or(|end| end > total_len) {
            return Err(io::Error::other("ISO extent out of bounds"));
        }
        iso.seek(SeekFrom::Start(start))?;
        let mut hasher = Sha256::new();
        let copied = io::copy(&mut (&mut iso).take(len), &mut hasher)?;
        if copied != len || hasher.finalize()[..] != expected.sha256 {
            return Err(io::Error::other("ISO component digest mismatch"));
        }
    }
    Ok(())
}

fn iso_find<R: Read + Seek>(
    iso: &mut R,
    total_len: u64,
    dir_lba: u64,
    dir_len: u64,
    wanted: &str,
    expect_dir: bool,
    max_dir_bytes: u64,
) -> io::Result<(u64, u64)> {
    const ISO_SECTOR: u64 = 2048;
    if dir_len == 0 || dir_len > max_dir_bytes {
        return Err(io::Error::other("ISO directory size out of bounds"));
    }
    let start = dir_lba
        .checked_mul(ISO_SECTOR)
        .ok_or_else(|| io::Error::other("ISO directory offset overflow"))?;
    if start.checked_add(dir_len).is_none_or(|end| end > total_len) {
        return Err(io::Error::other("ISO directory out of bounds"));
    }
    iso.seek(SeekFrom::Start(start))?;
    let mut data = vec![0u8; dir_len as usize];
    iso.read_exact(&mut data)?;
    let mut at = 0usize;
    let mut found = None;
    while at < data.len() {
        let len = usize::from(data[at]);
        if len == 0 {
            let next_sector = ((at / ISO_SECTOR as usize) + 1) * ISO_SECTOR as usize;
            if next_sector >= data.len() {
                break;
            }
            at = next_sector;
            continue;
        }
        if len < 34 || at + len > data.len() {
            return Err(io::Error::other("malformed ISO directory record"));
        }
        let rec = &data[at..at + len];
        let name_len = usize::from(rec[32]);
        if 33 + name_len > len {
            return Err(io::Error::other("malformed ISO entry name"));
        }
        let raw_name = &rec[33..33 + name_len];
        let flags = rec[25];
        let is_dir = flags & 0x02 != 0;
        // Strip ISO-9660 version suffix `;1` and trailing dot for comparison.
        if let Ok(text) = std::str::from_utf8(raw_name) {
            let stem = text.split(';').next().unwrap_or(text).trim_end_matches('.');
            if stem.eq_ignore_ascii_case(wanted) {
                // Reject multi-extent (bit 7) or mismatched node type or duplicates.
                if is_dir != expect_dir || flags & 0x80 != 0 {
                    return Err(io::Error::other("unsupported ISO entry"));
                }
                let lba = u64::from(u32::from_le_bytes(rec[2..6].try_into().unwrap()));
                let size = u64::from(u32::from_le_bytes(rec[10..14].try_into().unwrap()));
                if found.replace((lba, size)).is_some() {
                    return Err(io::Error::other("duplicate ISO entry"));
                }
            }
        }
        at += len;
    }
    found.ok_or_else(|| io::Error::other("ISO entry not found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn production_identity_is_read_only_and_refuses_a_sidecar() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = LocalPath::parse(temporary.path().to_str().unwrap()).unwrap();
        let path = directory.join("activity.sqlite3").unwrap();
        let db = rusqlite::Connection::open(path.as_str()).unwrap();
        db.execute_batch(
            "CREATE TABLE vm_state_host(singleton INTEGER PRIMARY KEY,id BLOB NOT NULL);",
        )
        .unwrap();
        let identity = [0x42u8; 16];
        db.execute("INSERT INTO vm_state_host VALUES(1,?1)", [&identity[..]])
            .unwrap();
        drop(db);

        let root = PathPin::open(&directory, true, false).unwrap();
        let database = PathPin::open(&path, false, false).unwrap();
        let before = std::fs::read(path.as_str()).unwrap();
        assert_eq!(read_store_host(&root, &database), Ok(identity));
        assert_eq!(std::fs::read(path.as_str()).unwrap(), before);
        let sidecar = directory.join("activity.sqlite3-wal").unwrap();
        std::fs::write(sidecar.as_str(), []).unwrap();
        assert_eq!(
            read_store_host(&root, &database),
            Err(NativeRefusal::PathCustodyUnverified)
        );
    }

    fn iso_entry(lba: u32, size: u32, is_dir: bool, name: &[u8]) -> Vec<u8> {
        let len = (33 + name.len() + ((33 + name.len()) % 2)).max(34);
        let mut rec = vec![0u8; len];
        rec[0] = len as u8;
        rec[2..6].copy_from_slice(&lba.to_le_bytes());
        rec[6..10].copy_from_slice(&lba.to_be_bytes());
        rec[10..14].copy_from_slice(&size.to_le_bytes());
        rec[14..18].copy_from_slice(&size.to_be_bytes());
        rec[25] = if is_dir { 0x02 } else { 0x00 };
        rec[28..30].copy_from_slice(&1u16.to_le_bytes());
        rec[30..32].copy_from_slice(&1u16.to_be_bytes());
        rec[32] = name.len() as u8;
        rec[33..33 + name.len()].copy_from_slice(name);
        rec
    }

    fn synthetic_ubuntu_iso(kernel: &[u8], initrd: &[u8]) -> Vec<u8> {
        const SECTOR: usize = 2048;
        let mut iso = vec![0u8; 22 * SECTOR];
        // Sector 16: Primary Volume Descriptor.
        let pvd = &mut iso[16 * SECTOR..17 * SECTOR];
        pvd[0] = 1;
        pvd[1..6].copy_from_slice(b"CD001");
        pvd[6] = 1;
        pvd[128..130].copy_from_slice(&(SECTOR as u16).to_le_bytes());
        pvd[130..132].copy_from_slice(&(SECTOR as u16).to_be_bytes());
        let root_rec = iso_entry(18, SECTOR as u32, true, &[0]);
        pvd[156..156 + root_rec.len()].copy_from_slice(&root_rec);
        // Sector 18: root directory containing CASPER at LBA 19.
        let casper_rec = iso_entry(19, SECTOR as u32, true, b"CASPER");
        iso[18 * SECTOR..18 * SECTOR + casper_rec.len()].copy_from_slice(&casper_rec);
        // Sector 19: casper directory containing VMLINUZ.;1 (LBA 20) and INITRD.;1 (LBA 21).
        let vmlinuz_rec = iso_entry(20, kernel.len() as u32, false, b"VMLINUZ.;1");
        let initrd_rec = iso_entry(21, initrd.len() as u32, false, b"INITRD.;1");
        let off = 19 * SECTOR;
        iso[off..off + vmlinuz_rec.len()].copy_from_slice(&vmlinuz_rec);
        iso[off + vmlinuz_rec.len()..off + vmlinuz_rec.len() + initrd_rec.len()]
            .copy_from_slice(&initrd_rec);
        iso[20 * SECTOR..20 * SECTOR + kernel.len()].copy_from_slice(kernel);
        iso[21 * SECTOR..21 * SECTOR + initrd.len()].copy_from_slice(initrd);
        iso
    }

    #[test]
    fn iso_boot_components_bind_exact_kernel_and_initrd_pins() {
        let kernel = b"synthetic-vmlinuz-payload";
        let initrd = b"synthetic-initrd-payload-bytes";
        let iso = synthetic_ubuntu_iso(kernel, initrd);
        let pin = |bytes: &[u8]| ArtifactPin {
            size: bytes.len() as u64,
            sha256: crate::vm::qualification_record::digest(bytes),
        };
        assert!(verify_iso_boot_components(Cursor::new(&iso), pin(kernel), pin(initrd)).is_ok());
        // Substituted kernel or initrd pin is refused even though self-consistent.
        assert!(
            verify_iso_boot_components(Cursor::new(&iso), pin(b"other-kernel-bytes"), pin(initrd))
                .is_err()
        );
        assert!(
            verify_iso_boot_components(Cursor::new(&iso), pin(kernel), pin(b"other-initrd-bytes"))
                .is_err()
        );
        assert!(
            verify_iso_boot_components(Cursor::new(&iso[..1024]), pin(kernel), pin(initrd))
                .is_err()
        );
    }

    #[test]
    fn aggregate_cap_enforces_200_gib_ceiling_across_all_volumes_and_extra() {
        assert!(aggregate_within_cap(GIB, 100 * GIB, 98 * GIB, GIB));
        assert!(!aggregate_within_cap(GIB, 100 * GIB, 98 * GIB, GIB + 1));
        assert!(!aggregate_within_cap(u64::MAX, 1, 0, 0));
    }
}
