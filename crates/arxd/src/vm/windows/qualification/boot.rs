use super::*;
use crate::vm::qualification_record::{BootAnchor, Instant2, digest};
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

// ProcessTelemetryIdInformation, documented by Microsoft for Windows 10+.
// Only the fixed header is interpreted. The variable tail can contain
// private data and is discarded without decoding it.
pub(super) fn marker() -> Result<[u8; 32], NativeRefusal> {
    type Query = unsafe extern "system" fn(HANDLE, u32, *mut u8, u32, *mut u32) -> NTSTATUS;
    const CLASS: u32 = 64;
    const HEADER_BYTES: u32 = 92;
    const BOOT_OFFSET: usize = 60;
    let reason = NativeRefusal::QualificationMissing;
    // ntdll is already loaded for this process. No search or additional load.
    let module = unsafe { GetModuleHandleW(wide("ntdll.dll").as_ptr()) };
    if module.is_null() {
        return Err(reason);
    }
    let entry = unsafe { GetProcAddress(module, c"NtQueryInformationProcess".as_ptr().cast()) }
        .ok_or(reason)?;
    // The resolved symbol has the documented NT system-call signature.
    let query: Query = unsafe { std::mem::transmute(entry) };
    let mut buffer = [0u64; 2048];
    let capacity = size_of_val(&buffer) as u32;
    let mut length = 0;
    let bytes = buffer.as_mut_ptr().cast::<u8>();
    // The aligned, initialized output buffer and return length outlive the call.
    let status = unsafe { query(GetCurrentProcess(), CLASS, bytes, capacity, &mut length) };
    if status != 0 || !(HEADER_BYTES..=capacity).contains(&length) {
        return Err(reason);
    }
    let read = |offset| {
        // Every fixed field read is aligned and below the checked header size.
        unsafe { bytes.add(offset).cast::<u32>().read() }
    };
    let boot = read(BOOT_OFFSET);
    if read(0) < HEADER_BYTES
        || read(0) > length
        || read(4) != unsafe { GetCurrentProcessId() }
        || boot == 0
    {
        return Err(reason);
    }
    Ok(digest(
        &[b"arx-windows-boot-v1".as_slice(), &boot.to_le_bytes()].concat(),
    ))
}

pub(super) fn uptime() -> u64 {
    // Unlike the adjusted system clock, uptime is measured from kernel boot.
    unsafe { GetTickCount64() }
}

pub(super) fn capture(origin_mono_ms: u64) -> Result<BootAnchor, NativeRefusal> {
    Ok(BootAnchor {
        marker: marker()?,
        origin_mono_ms,
    })
}

pub(super) fn now() -> Instant2 {
    Instant2 {
        utc: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_secs()),
        mono_ms: uptime(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_reports_a_stable_boot_marker_and_uptime() {
        let first = marker().expect("boot telemetry is available");
        let now = now();
        assert!(marker().is_ok_and(|same| same == first));
        assert!(now.mono_ms <= uptime());
    }
}
