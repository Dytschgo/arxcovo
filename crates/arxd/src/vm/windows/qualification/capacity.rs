//! Read-only capacity observations for the enrolled disposable volume.
//! Never attaches, enumerates or opens a physical disk, and every handle used
//! for a query is retained for the life of the preflight.

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

pub(super) struct VhdFacts {
    pub provider_subtype: u32,
    pub virtual_size: u64,
    pub physical_size: u64,
    pub sector: u32,
    pub disk: u32,
}

/// Query the enrolled VHDX. The API takes a name; the held backing pin and
/// its held ancestors refuse rename and delete for the whole preflight, so
/// the name stays bound to the pinned object while the handle is retained.
pub(super) fn fixed_vhd(backing: &PathPin) -> Result<(OwnedHandle, VhdFacts), NativeRefusal> {
    let reason = NativeRefusal::DiskCeilingUnverified;
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
    // Information access only, no parents, against the held backing name.
    let status = unsafe {
        OpenVirtualDisk(
            &storage_type,
            wide(backing.path.as_str()).as_ptr(),
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
    if status != ERROR_SUCCESS {
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

/// The trial volume, bound by its stable volume GUID rather than the drive
/// letter, with both the GUID root and the device handle retained.
pub(super) struct HeldVolume {
    _root: File,
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

/// Bind the held drive-root pin to its GUID volume, read the single extent
/// through a retained device handle, then prove the drive still resolves to
/// the same GUID and root object.
pub(super) fn bind_volume(drive_root: &PathPin, disk: u32) -> Result<HeldVolume, NativeRefusal> {
    let reason = NativeRefusal::DiskCeilingUnverified;
    let drive = &drive_root.path.as_str()[..3];
    let guid = volume_guid(drive).ok_or(reason)?;
    let root = open_directory(&guid).map_err(|_| reason)?;
    if file_identity(&root, true).map_err(|_| reason)?.0 != drive_root.identity {
        return Err(reason);
    }
    // `\\?\Volume{GUID}` without the trailing separator names the device.
    let device_name = wide(&guid[..guid.len() - 1]);
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
        || volume_guid(drive).as_deref() != Some(guid.as_str())
        || file_identity(&open_directory(&guid).map_err(|_| reason)?, true)
            .map_err(|_| reason)?
            .0
            != drive_root.identity
    {
        return Err(reason);
    }
    Ok(HeldVolume {
        _root: root,
        _device: device,
        extent_start: extent.StartingOffset,
        extent_length: extent.ExtentLength,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
