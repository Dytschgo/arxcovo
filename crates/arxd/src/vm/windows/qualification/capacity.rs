//! Read-only capacity observations for the enrolled disposable volume.
//! Never attaches, enumerates or opens a physical disk, and every handle used
//! for a query is retained for the life of the preflight.
//!
//! Each volume is anchored once: its drive letter is resolved to a GUID root
//! that is held for the run, every leaf is walked from that held root, and
//! the VHD and device queries use names derived from the same held objects.
//! A drive-letter rebinding therefore cannot join observations of two
//! volumes; a rebinding seen after the observations closes them.

use super::*;
use windows_sys::Wdk::Storage::FileSystem::{
    FileFsFullSizeInformation, NtQueryVolumeInformationFile,
};
use windows_sys::Wdk::System::SystemServices::FILE_FS_FULL_SIZE_INFORMATION;
use windows_sys::Win32::{
    Storage::Vhd::*,
    System::{IO::DeviceIoControl, Ioctl::VOLUME_DISK_EXTENTS},
};

/// Provider subtype 2 denotes a fixed VHDX.
const FIXED_SUBTYPE: u32 = 2;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Free {
    pub caller: u64,
    pub total_free: u64,
    pub total: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct CapacitySample {
    pub provider_subtype: u32,
    pub vhd_virtual: u64,
    pub vhd_physical: u64,
    pub sector: u32,
    pub extent_start: i64,
    pub extent_length: i64,
    pub inner: Free,
    pub outer: Free,
    pub backing_length: u64,
    pub backing_allocated: u64,
}

/// Every comparison is a separate refusal condition.
pub(super) fn check_capacity(s: &CapacitySample) -> Result<Limits, NativeRefusal> {
    let reason = NativeRefusal::DiskCeilingUnverified;
    let start = u64::try_from(s.extent_start).map_err(|_| reason)?;
    let extent = u64::try_from(s.extent_length).map_err(|_| reason)?;
    let end = start.checked_add(extent).ok_or(reason)?;
    if s.provider_subtype != FIXED_SUBTYPE
        || s.vhd_virtual == 0
        || s.vhd_virtual > DISK_CEILING
        || s.vhd_physical < s.vhd_virtual
        || !matches!(s.sector, 512 | 4096)
        || extent == 0
        || end > s.vhd_virtual
        || s.inner.total == 0
        || s.inner.caller > s.inner.total_free
        || s.inner.total_free > s.inner.total
        || s.inner.total > extent
        || s.outer.caller > s.outer.total_free
        || s.outer.total_free > s.outer.total
        // Caller-available is the lower bound (caller <= total free is
        // checked above), so this floor also covers total free space.
        || s.outer.caller < OUTER_FLOOR
        || s.backing_length < s.vhd_virtual
        || s.backing_allocated < s.backing_length
    {
        return Err(reason);
    }
    Ok(Limits {
        inner_caller_free_bytes: s.inner.caller,
        inner_total_free_bytes: s.inner.total_free,
        inner_total_bytes: s.inner.total,
        inner_extent_bytes: extent,
        vhd_virtual_bytes: s.vhd_virtual,
        outer_caller_free_bytes: s.outer.caller,
        outer_total_free_bytes: s.outer.total_free,
        backing_allocated_bytes: s.backing_allocated,
        backing_length_bytes: s.backing_length,
        disk_ceiling_bytes: DISK_CEILING,
        outer_floor_bytes: OUTER_FLOOR,
        disk_budget_bytes: DISK_CEILING.min(extent),
        empty_job_memory_limit_bytes: JOB_MEMORY_BYTES as u64,
        empty_job_cpu_rate: JOB_CPU_RATE,
    })
}

/// Caller-available and total free space through the held directory handle
/// itself, never by reopening a name.
pub(super) fn held_free(dir: &File) -> Result<Free, NativeRefusal> {
    let reason = NativeRefusal::DiskCeilingUnverified;
    let mut info = FILE_FS_FULL_SIZE_INFORMATION::default();
    // Zero is a valid initial state for this output-only status block.
    let mut status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // The fixed-size output matches the requested information class.
    let result = unsafe {
        NtQueryVolumeInformationFile(
            dir.as_raw_handle(),
            &mut status,
            ptr_of(&mut info),
            size_of_val(&info) as u32,
            FileFsFullSizeInformation,
        )
    };
    let unit = u64::from(info.SectorsPerAllocationUnit)
        .checked_mul(u64::from(info.BytesPerSector))
        .filter(|&unit| unit > 0)
        .ok_or(reason)?;
    let bytes = |units: i64| {
        u64::try_from(units)
            .ok()
            .and_then(|units| units.checked_mul(unit))
            .ok_or(reason)
    };
    if result < 0 {
        return Err(reason);
    }
    Ok(Free {
        caller: bytes(info.CallerAvailableAllocationUnits)?,
        total_free: bytes(info.ActualAvailableAllocationUnits)?,
        total: bytes(info.TotalAllocationUnits)?,
    })
}

/// Name resolution behind every volume anchor. Production asks the mount
/// manager and the held handle itself; tests inject rebinding.
pub(super) trait VolumeNames {
    /// The `\\?\Volume{GUID}\` name currently mounted at a drive root.
    fn drive_guid(&self, drive: &str) -> Option<String>;
    /// The GUID-form final path of a held handle.
    fn final_guid_path(&self, file: &File) -> Option<String>;
}

pub(super) struct MountManager;

impl VolumeNames for MountManager {
    fn drive_guid(&self, drive: &str) -> Option<String> {
        volume_guid(drive)
    }

    fn final_guid_path(&self, file: &File) -> Option<String> {
        let mut path = [0u16; 1024];
        // The buffer is sized in UTF-16 units; a longer name is refused.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                path.as_mut_ptr(),
                path.len() as u32,
                VOLUME_NAME_GUID,
            )
        } as usize;
        if length == 0 || length >= path.len() {
            return None;
        }
        String::from_utf16(&path[..length]).ok()
    }
}

/// A volume root opened once through its GUID name and held for the run.
/// Every later leaf on this volume is walked from `root`.
pub(super) struct AnchoredVolume {
    pub drive: String,
    pub guid: String,
    pub guid_bytes: [u8; 16],
    pub root: File,
    pub identity: FileIdentity,
}

/// Resolve the drive letter once and hold the GUID root it named.
pub(super) fn anchor_volume(
    names: &impl VolumeNames,
    drive: &str,
) -> Result<AnchoredVolume, NativeRefusal> {
    let reason = NativeRefusal::PathCustodyUnverified;
    let letter = drive.as_bytes();
    if letter.len() != 3 || !letter[0].is_ascii_alphabetic() || &letter[1..] != b":\\" {
        return Err(reason);
    }
    let guid = names
        .drive_guid(drive)
        .filter(|guid| guid_spelling(guid))
        .ok_or(reason)?;
    let guid_bytes = parse_guid(&guid[11..47]).ok_or(reason)?;
    // Only a fixed local volume (drive type 3) can hold the trial store.
    if unsafe { GetDriveTypeW(wide(&guid).as_ptr()) } != 3 {
        return Err(reason);
    }
    let root = open_directory(&guid).map_err(|_| reason)?;
    let (identity, _) = file_identity(&root, true).map_err(|_| reason)?;
    verify_ntfs(&root).map_err(|_| reason)?;
    Ok(AnchoredVolume {
        drive: drive.to_ascii_uppercase(),
        guid,
        guid_bytes,
        root,
        identity,
    })
}

/// `actual` is the GUID-form name of `relative` on the volume `guid`.
fn on_volume(actual: &str, guid: &str, relative: &str) -> bool {
    actual.len() == guid.len() + relative.len()
        && actual
            .get(..guid.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(guid))
        && actual
            .get(guid.len()..)
            .is_some_and(|rest| rest.eq_ignore_ascii_case(relative))
}

/// Walk `path` from the anchored root's held handle, never from its drive
/// letter, so the leaf provably descends from that exact root. The OS's own
/// GUID-form name for the held leaf must also lie on the anchored volume.
pub(super) fn walk(
    names: &impl VolumeNames,
    volume: &AnchoredVolume,
    path: &LocalPath,
    directory: bool,
    mutable: bool,
) -> Result<PathPin, NativeRefusal> {
    let reason = NativeRefusal::PathCustodyUnverified;
    let raw = path.as_str();
    if raw.len() <= 3 || !raw[..3].eq_ignore_ascii_case(&volume.drive) {
        return Err(reason);
    }
    let relative = &raw[3..];
    let components: Vec<&str> = relative.split('\\').collect();
    let mut ancestors = vec![volume.root.try_clone().map_err(|_| reason)?];
    for (index, name) in components.iter().enumerate() {
        let last = index + 1 == components.len();
        let node = if last && !directory {
            Node::File
        } else {
            Node::Directory
        };
        let parent = ancestors.last().expect("the anchored root is held");
        let file = open_relative(parent, name, node, last && mutable, false).map_err(|_| reason)?;
        let (identity, allocated) =
            file_identity(&file, node == Node::Directory).map_err(|_| reason)?;
        if identity.volume != volume.identity.volume {
            return Err(reason);
        }
        if !last {
            ancestors.push(file);
            continue;
        }
        // Reopen through the same held parent and compare the object.
        let check = open_relative(parent, name, node, mutable, false).map_err(|_| reason)?;
        if file_identity(&check, directory).map_err(|_| reason)?.0 != identity
            || !names
                .final_guid_path(&file)
                .is_some_and(|actual| on_volume(&actual, &volume.guid, relative))
        {
            return Err(reason);
        }
        return Ok(PathPin {
            _ancestors: ancestors,
            leaf: file,
            identity,
            allocated,
            path: path.clone(),
            mutable,
            verified_content: None,
        });
    }
    Err(reason)
}

/// The drive letter still names the anchored volume and its root object.
/// Checked after the observations: a rebinding during them closes the run.
pub(super) fn still_bound(names: &impl VolumeNames, volume: &AnchoredVolume) -> bool {
    names
        .drive_guid(&volume.drive)
        .is_some_and(|guid| guid.eq_ignore_ascii_case(&volume.guid))
        && open_directory(&volume.guid)
            .and_then(|root| file_identity(&root, true))
            .is_ok_and(|(identity, _)| identity == volume.identity)
}

pub(super) struct VhdFacts {
    pub provider_subtype: u32,
    pub virtual_size: u64,
    pub physical_size: u64,
    pub sector: u32,
    pub disk: u32,
}

/// The name the VHD API opens: the held backing leaf's own GUID-form name,
/// on the anchored outer volume, at exactly the enrolled location. A drive
/// letter is never reparsed for the query.
pub(super) fn vhd_query_path(
    names: &impl VolumeNames,
    outer: &AnchoredVolume,
    backing: &PathPin,
) -> Option<String> {
    let raw = backing.path.as_str();
    if !raw.get(..3)?.eq_ignore_ascii_case(&outer.drive)
        || backing.identity.volume != outer.identity.volume
    {
        return None;
    }
    names
        .final_guid_path(&backing.leaf)
        .filter(|path| on_volume(path, &outer.guid, &raw[3..]))
}

/// Query the enrolled VHDX through the held backing object's GUID-form
/// name. The held leaf and its ancestors refuse rename for the whole run,
/// and the name is compared again after the query.
pub(super) fn fixed_vhd(
    names: &impl VolumeNames,
    outer: &AnchoredVolume,
    backing: &PathPin,
) -> Result<(OwnedHandle, VhdFacts), NativeRefusal> {
    let reason = NativeRefusal::DiskCeilingUnverified;
    let path = vhd_query_path(names, outer, backing).ok_or(reason)?;
    let storage_type = VIRTUAL_STORAGE_TYPE {
        DeviceId: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX,
        VendorId: VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
    };
    let parameters = OPEN_VIRTUAL_DISK_PARAMETERS {
        Version: OPEN_VIRTUAL_DISK_VERSION_1,
        Anonymous: OPEN_VIRTUAL_DISK_PARAMETERS_0 {
            Version1: OPEN_VIRTUAL_DISK_PARAMETERS_0_0 {
                RWDepth: OPEN_VIRTUAL_DISK_RW_DEPTH_DEFAULT,
            },
        },
    };
    let mut handle = null_mut();
    // Information access only, no parents, against the held object's name.
    let status = unsafe {
        OpenVirtualDisk(
            &storage_type,
            wide(&path).as_ptr(),
            VIRTUAL_DISK_ACCESS_GET_INFO,
            OPEN_VIRTUAL_DISK_FLAG_NO_PARENTS,
            &parameters,
            &mut handle,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(reason);
    }
    let handle = unsafe { owned(handle) }.map_err(|_| reason)?;
    let subtype = vhd_info(&handle, GET_VIRTUAL_DISK_INFO_PROVIDER_SUBTYPE).ok_or(reason)?;
    let size = vhd_info(&handle, GET_VIRTUAL_DISK_INFO_SIZE).ok_or(reason)?;
    // The reported version selects these union members.
    let (provider_subtype, size) =
        unsafe { (subtype.Anonymous.ProviderSubtype, size.Anonymous.Size) };
    let mut physical = [0u16; 64];
    let mut physical_bytes = size_of_val(&physical) as u32;
    let status = unsafe {
        GetVirtualDiskPhysicalPath(
            handle.as_raw_handle(),
            &mut physical_bytes,
            physical.as_mut_ptr(),
        )
    };
    if status != ERROR_SUCCESS
        || vhd_query_path(names, outer, backing).as_deref() != Some(path.as_str())
    {
        return Err(reason);
    }
    let disk = disk_number(&physical).ok_or(reason)?;
    Ok((
        handle,
        VhdFacts {
            provider_subtype,
            virtual_size: size.VirtualSize,
            physical_size: size.PhysicalSize,
            sector: size.SectorSize,
            disk,
        },
    ))
}

fn vhd_info(
    handle: &OwnedHandle,
    version: GET_VIRTUAL_DISK_INFO_VERSION,
) -> Option<GET_VIRTUAL_DISK_INFO> {
    let mut value = GET_VIRTUAL_DISK_INFO {
        Version: version,
        ..Default::default()
    };
    let mut bytes = size_of_val(&value) as u32;
    let status = unsafe {
        GetVirtualDiskInformation(handle.as_raw_handle(), &mut bytes, &mut value, null_mut())
    };
    (status == ERROR_SUCCESS && value.Version == version).then_some(value)
}

fn disk_number(buffer: &[u16]) -> Option<u32> {
    let end = buffer.iter().position(|&byte| byte == 0)?;
    let path = String::from_utf16(&buffer[..end]).ok()?;
    let number = path.strip_prefix(r"\\.\PhysicalDrive")?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

/// The trial volume's device, opened through the anchored GUID name and
/// retained with the single extent it reported.
pub(super) struct HeldDevice {
    _device: OwnedHandle,
    pub extent_start: i64,
    pub extent_length: i64,
}

fn volume_guid(drive_root: &str) -> Option<String> {
    let mut name = [0u16; 64];
    // The fixed buffer exceeds the 49-unit `\\?\Volume{GUID}\` name.
    let ok = unsafe {
        GetVolumeNameForVolumeMountPointW(
            wide(drive_root).as_ptr(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    let end = name.iter().position(|&unit| unit == 0)?;
    let guid = String::from_utf16(&name[..end]).ok()?;
    (ok != 0 && guid_spelling(&guid)).then_some(guid)
}

fn guid_spelling(name: &str) -> bool {
    name.len() == 49
        && name.starts_with(r"\\?\Volume{")
        && name.ends_with(r"}\")
        && name[11..47].bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

/// Read the single extent of the anchored volume through a device handle
/// opened by the same GUID name; the drive letter is never consulted.
pub(super) fn volume_extent(
    volume: &AnchoredVolume,
    disk: u32,
) -> Result<HeldDevice, NativeRefusal> {
    let reason = NativeRefusal::DiskCeilingUnverified;
    // `\\?\Volume{GUID}` without the trailing separator names the device.
    let device_name = wide(&volume.guid[..volume.guid.len() - 1]);
    // No data access is needed for the read-only extent query.
    let device = unsafe {
        owned(CreateFileW(
            device_name.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        ))
    }
    .map_err(|_| reason)?;
    let mut extents = VOLUME_DISK_EXTENTS::default();
    let mut written = 0;
    let ok = unsafe {
        DeviceIoControl(
            device.as_raw_handle(),
            IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
            null(),
            0,
            ptr_of(&mut extents),
            size_of_val(&extents) as u32,
            &mut written,
            null_mut(),
        )
    };
    let [extent] = extents.Extents;
    if ok == 0
        || written as usize != size_of_val(&extents)
        || extents.NumberOfDiskExtents != 1
        || extent.DiskNumber != disk
    {
        return Err(reason);
    }
    Ok(HeldDevice {
        _device: device,
        extent_start: extent.StartingOffset,
        extent_length: extent.ExtentLength,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn physical_drive_parser_accepts_only_bounded_device_numbers() {
        assert_eq!(disk_number(&wide(r"\\.\PhysicalDrive14")), Some(14));
        for bad in [
            r"\\.\PhysicalDrive",
            r"\\.\PhysicalDrive+2",
            r"\\.\PhysicalDrive4294967296",
            r"\\?\PhysicalDrive1",
        ] {
            assert_eq!(disk_number(&wide(bad)), None);
        }
    }

    #[test]
    fn volume_guid_spelling_is_exact() {
        assert!(guid_spelling(
            r"\\?\Volume{0123abcd-4567-89ab-cdef-0123456789ab}\"
        ));
        for bad in [
            r"\\?\Volume{0123abcd-4567-89ab-cdef-0123456789ab}",
            r"\\?\Volume{0123abcd-4567-89ab-cdef-0123456789ag}\",
            r"\\?\Volume{0123abcd_4567-89ab-cdef-0123456789ab}\",
            r"\\.\Volume{0123abcd-4567-89ab-cdef-0123456789ab}\",
            r"C:\",
        ] {
            assert!(!guid_spelling(bad), "{bad}");
        }
    }

    type Mutation = (&'static str, fn(&mut CapacitySample));

    fn good() -> CapacitySample {
        let free = |caller, total_free, total| Free {
            caller,
            total_free,
            total,
        };
        CapacitySample {
            provider_subtype: FIXED_SUBTYPE,
            vhd_virtual: 100 * GIB,
            vhd_physical: 100 * GIB + 4096,
            sector: 4096,
            extent_start: 1 << 20,
            extent_length: (99 * GIB) as i64,
            inner: free(80 * GIB, 81 * GIB, 98 * GIB),
            outer: free(60 * GIB, 61 * GIB, 400 * GIB),
            backing_length: 100 * GIB + 8192,
            backing_allocated: 100 * GIB + 8192,
        }
    }

    #[test]
    fn capacity_accepts_a_fixed_bounded_volume_and_applies_the_lower_budget() {
        let limits = check_capacity(&good()).unwrap();
        assert_eq!(limits.disk_budget_bytes, 99 * GIB);
        assert_eq!(limits.inner_caller_free_bytes, 80 * GIB);
        assert_eq!(limits.outer_total_free_bytes, 61 * GIB);
    }

    #[test]
    fn each_capacity_comparison_refuses_independently() {
        let cases: [Mutation; 17] = [
            ("dynamic subtype", |s| s.provider_subtype = 3),
            ("zero virtual", |s| s.vhd_virtual = 0),
            ("over ceiling", |s| {
                s.vhd_virtual = DISK_CEILING + 1;
                s.vhd_physical = DISK_CEILING + 1;
                s.backing_length = DISK_CEILING + 1;
                s.backing_allocated = DISK_CEILING + 1;
            }),
            ("physical below virtual", |s| {
                s.vhd_physical = s.vhd_virtual - 1
            }),
            ("sector", |s| s.sector = 1024),
            ("negative extent start", |s| s.extent_start = -1),
            ("extent beyond virtual", |s| {
                s.extent_start = (2 * GIB) as i64
            }),
            ("zero inner total", |s| s.inner.total = 0),
            ("inner caller over free", |s| {
                s.inner.caller = s.inner.total_free + 1
            }),
            ("inner free over total", |s| {
                s.inner.total_free = s.inner.total + 1
            }),
            ("inner total over extent", |s| s.inner.total = 99 * GIB + 1),
            ("outer caller over free", |s| {
                s.outer.caller = s.outer.total_free + 1
            }),
            ("outer free over total", |s| {
                s.outer.total_free = s.outer.total + 1
            }),
            ("caller floor", |s| s.outer.caller = OUTER_FLOOR - 1),
            ("total free below the floor, hence caller too", |s| {
                s.outer.total_free = OUTER_FLOOR - 1;
                s.outer.caller = OUTER_FLOOR - 1;
            }),
            ("backing shorter than virtual", |s| {
                s.backing_length = s.vhd_virtual - 1
            }),
            ("backing not fully allocated", |s| {
                s.backing_allocated = s.backing_length - 1
            }),
        ];
        for (name, mutate) in cases {
            let mut sample = good();
            mutate(&mut sample);
            assert_eq!(
                check_capacity(&sample).map(|_| ()),
                Err(NativeRefusal::DiskCeilingUnverified),
                "{name}"
            );
        }
    }

    #[test]
    fn held_free_space_reads_both_bounds_from_a_handle() {
        let dir = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let file = open_directory(dir.path().to_str().unwrap()).unwrap();
        let free = held_free(&file).unwrap();
        assert!(free.total > 0);
        assert!(free.caller <= free.total_free && free.total_free <= free.total);
    }

    /// A fresh directory named by its long, normalized path: walks compare
    /// the OS's own name, so 8.3 aliases in a runner's temp path are expanded.
    fn long_temp() -> (tempfile::TempDir, LocalPath) {
        let dir = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let long = std::fs::canonicalize(dir.path()).unwrap();
        let text = long
            .to_str()
            .unwrap()
            .strip_prefix(r"\\?\")
            .unwrap()
            .to_owned();
        (dir, LocalPath::parse(&text).unwrap())
    }

    const OTHER: &str = r"\\?\Volume{00000000-0000-0000-0000-000000000001}\";

    /// Real names, except where a test injects a rebinding.
    #[derive(Default)]
    struct Rebinding {
        drive_calls: Cell<u32>,
        /// Every drive resolution after the first names this volume.
        later_drive: Option<&'static str>,
        /// Every held leaf is reported under this volume instead.
        leaf_volume: Option<&'static str>,
    }

    impl VolumeNames for Rebinding {
        fn drive_guid(&self, drive: &str) -> Option<String> {
            let calls = self.drive_calls.get();
            self.drive_calls.set(calls + 1);
            match self.later_drive {
                Some(volume) if calls > 0 => Some(volume.to_owned()),
                _ => MountManager.drive_guid(drive),
            }
        }

        fn final_guid_path(&self, file: &File) -> Option<String> {
            let actual = MountManager.final_guid_path(file)?;
            match self.leaf_volume {
                Some(volume) => Some(format!("{volume}{}", actual.get(49..)?)),
                None => Some(actual),
            }
        }
    }

    #[test]
    fn a_leaf_walked_from_the_guid_anchor_is_the_object_its_name_reaches() {
        let (_dir, path) = long_temp();
        let drive = &path.as_str()[..3];
        let volume = anchor_volume(&MountManager, drive).unwrap();
        assert_eq!(volume.drive, drive.to_ascii_uppercase());
        let pin = walk(&MountManager, &volume, &path, true, false).unwrap();
        assert_eq!(
            pin.identity,
            PathPin::open(&path, true, false).unwrap().identity
        );
        assert!(still_bound(&MountManager, &volume));
        // A path on another drive letter cannot be walked from this anchor.
        let elsewhere = LocalPath::parse(r"Q:\elsewhere").unwrap();
        assert_eq!(
            walk(&MountManager, &volume, &elsewhere, true, false).err(),
            Some(NativeRefusal::PathCustodyUnverified)
        );
    }

    #[test]
    fn a_drive_letter_rebound_after_anchoring_closes_the_observation() {
        let (_dir, path) = long_temp();
        let names = Rebinding {
            later_drive: Some(OTHER),
            ..Rebinding::default()
        };
        let volume = anchor_volume(&names, &path.as_str()[..3]).unwrap();
        // The walk uses only the held root, so the rebinding cannot redirect it.
        assert!(walk(&names, &volume, &path, true, false).is_ok());
        assert!(!still_bound(&names, &volume));
    }

    #[test]
    fn a_leaf_the_os_places_on_another_volume_is_refused() {
        let (_dir, path) = long_temp();
        let names = Rebinding {
            leaf_volume: Some(OTHER),
            ..Rebinding::default()
        };
        let volume = anchor_volume(&names, &path.as_str()[..3]).unwrap();
        assert_eq!(
            walk(&names, &volume, &path, true, false).err(),
            Some(NativeRefusal::PathCustodyUnverified)
        );
    }

    #[test]
    fn the_vhd_query_names_the_held_backing_on_the_anchored_volume() {
        let (dir, parent) = long_temp();
        std::fs::write(dir.path().join("trial.vhdx"), b"synthetic").unwrap();
        let path = parent.join("trial.vhdx").unwrap();
        let volume = anchor_volume(&MountManager, &path.as_str()[..3]).unwrap();
        let backing = walk(&MountManager, &volume, &path, false, true).unwrap();
        let name = vhd_query_path(&MountManager, &volume, &backing).unwrap();
        assert!(on_volume(&name, &volume.guid, &path.as_str()[3..]));
        // The same held leaf reported on another volume is never queried.
        let rebound = Rebinding {
            leaf_volume: Some(OTHER),
            ..Rebinding::default()
        };
        assert_eq!(vhd_query_path(&rebound, &volume, &backing), None);
    }

    #[test]
    fn guid_form_names_must_match_volume_and_location_exactly() {
        let guid = r"\\?\Volume{0123abcd-4567-89ab-cdef-0123456789ab}\";
        let upper = r"\\?\VOLUME{0123ABCD-4567-89AB-CDEF-0123456789AB}\";
        assert!(on_volume(
            &format!(r"{guid}Arx\t.vhdx"),
            guid,
            r"Arx\t.vhdx"
        ));
        assert!(on_volume(
            &format!(r"{upper}ARX\T.VHDX"),
            guid,
            r"Arx\t.vhdx"
        ));
        for actual in [
            format!(r"{OTHER}Arx\t.vhdx"),
            format!(r"{guid}Arx\t.vhdx.bak"),
            format!(r"{guid}Other\t.vhdx"),
            format!(r"{guid}Arx"),
            r"D:\Arx\t.vhdx".to_owned(),
        ] {
            assert!(!on_volume(&actual, guid, r"Arx\t.vhdx"), "{actual}");
        }
    }
}
