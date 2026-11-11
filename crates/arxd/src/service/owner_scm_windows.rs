//! Narrow native seam. Parsing never dereferences an embedded native pointer.
#![allow(dead_code)] // Library-only until PR-D.
use super::owner_scm::*;
use std::{
    mem::{offset_of, size_of},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{Foundation::GetLastError, System::Services::*};
const CREATE_BOOT: u32 = SERVICE_DEMAND_START;
struct Handle(SC_HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}
pub(super) struct NativeScm {
    manager: Option<Handle>,
    service: Option<Handle>,
}
impl NativeScm {
    pub(super) fn new() -> Self {
        Self {
            manager: None,
            service: None,
        }
    }
    fn service(&self) -> SC_HANDLE {
        self.service.as_ref().expect("opened service").0
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
fn success(v: i32) -> Result<(), Win32> {
    if v != 0 {
        Ok(())
    } else {
        Err(Win32(unsafe { GetLastError() }))
    }
}
fn rights(a: Access) -> u32 {
    SERVICE_QUERY_CONFIG
        | SERVICE_QUERY_STATUS
        | match a {
            Access::Status => 0,
            Access::Start | Access::Install => SERVICE_CHANGE_CONFIG | SERVICE_START,
            Access::Stop => SERVICE_CHANGE_CONFIG | SERVICE_STOP,
            Access::Uninstall => SERVICE_CHANGE_CONFIG | SERVICE_STOP | 0x10000,
        }
}
#[repr(align(8))]
struct Buffer([u8; 8192]);
impl Buffer {
    fn new() -> Self {
        Self([0; 8192])
    }
    fn view(&self) -> View<'_> {
        View {
            bytes: &self.0,
            base: self.0.as_ptr() as usize,
        }
    }
}
struct View<'a> {
    bytes: &'a [u8],
    base: usize,
}
impl View<'_> {
    fn u32(&self, o: usize) -> Result<u32, Fault> {
        Ok(u32::from_ne_bytes(
            self.bytes
                .get(o..o.checked_add(4).ok_or(Fault::Malformed)?)
                .ok_or(Fault::Malformed)?
                .try_into()
                .map_err(|_| Fault::Malformed)?,
        ))
    }
    fn pointer(&self, o: usize) -> Result<usize, Fault> {
        Ok(usize::from_ne_bytes(
            self.bytes
                .get(o..o.checked_add(size_of::<usize>()).ok_or(Fault::Malformed)?)
                .ok_or(Fault::Malformed)?
                .try_into()
                .map_err(|_| Fault::Malformed)?,
        ))
    }
    fn range(&self, p: usize, n: usize, align: usize) -> Result<usize, Fault> {
        let o = p.checked_sub(self.base).ok_or(Fault::Malformed)?;
        if !p.is_multiple_of(align)
            || o >= self.bytes.len()
            || o.checked_add(n).is_none_or(|end| end > self.bytes.len())
        {
            return Err(Fault::Malformed);
        }
        Ok(o)
    }
    fn string(&self, p: usize) -> Result<Vec<u16>, Fault> {
        if p == 0 {
            return Ok(vec![]);
        }
        let mut o = self.range(p, 2, 2)?;
        let mut out = vec![];
        loop {
            let b = self.bytes.get(o..o + 2).ok_or(Fault::Malformed)?;
            let c = u16::from_ne_bytes([b[0], b[1]]);
            if c == 0 {
                return Ok(out);
            }
            out.push(c);
            o += 2;
        }
    }
    fn multi(&self, p: usize) -> Result<Vec<Vec<u16>>, Fault> {
        if p == 0 {
            return Ok(vec![]);
        }
        let mut o = self.range(p, 2, 2)?;
        let mut out = vec![];
        loop {
            let s = self.string(self.base + o)?;
            if s.is_empty() {
                if out.is_empty() {
                    self.range(self.base + o + 2, 2, 2)?;
                    if self.bytes[o + 2..o + 4] != [0, 0] {
                        return Err(Fault::Malformed);
                    }
                }
                return Ok(out);
            }
            if s.len() > 64 || out.len() == 16 {
                return Err(Fault::Malformed);
            }
            o += (s.len() + 1) * 2;
            out.push(s);
        }
    }
    fn base_config(&self) -> Result<BaseConfig, Fault> {
        Ok(BaseConfig {
            service_type: self.u32(offset_of!(QUERY_SERVICE_CONFIGW, dwServiceType))?,
            start_type: self.u32(offset_of!(QUERY_SERVICE_CONFIGW, dwStartType))?,
            image: self
                .string(self.pointer(offset_of!(QUERY_SERVICE_CONFIGW, lpBinaryPathName))?)?,
            account: self
                .string(self.pointer(offset_of!(QUERY_SERVICE_CONFIGW, lpServiceStartName))?)?,
        })
    }
    fn config2(&self, l: Level) -> Result<Config2, Fault> {
        Ok(match l {
            Level::Delayed => Config2::Delayed(self.u32(offset_of!(
                SERVICE_DELAYED_AUTO_START_INFO,
                fDelayedAutostart
            ))?),
            Level::Sid => Config2::Sid(self.u32(offset_of!(SERVICE_SID_INFO, dwServiceSidType))?),
            Level::Privileges => Config2::Privileges(self.multi(self.pointer(offset_of!(
                SERVICE_REQUIRED_PRIVILEGES_INFOW,
                pmszRequiredPrivileges
            ))?)?),
            Level::Preshutdown => Config2::Preshutdown(
                self.u32(offset_of!(SERVICE_PRESHUTDOWN_INFO, dwPreshutdownTimeout))?,
            ),
            Level::NonCrash => Config2::NonCrash(self.u32(offset_of!(
                SERVICE_FAILURE_ACTIONS_FLAG,
                fFailureActionsOnNonCrashFailures
            ))?),
            Level::Failure => {
                let count = self.u32(offset_of!(SERVICE_FAILURE_ACTIONSW, cActions))? as usize;
                if count > 8 {
                    return Err(Fault::Malformed);
                }
                let p = self.pointer(offset_of!(SERVICE_FAILURE_ACTIONSW, lpsaActions))?;
                let mut actions = vec![];
                if count != 0 {
                    let o = self.range(p, count * size_of::<SC_ACTION>(), 4)?;
                    for i in 0..count {
                        actions.push((
                            self.u32(o + i * size_of::<SC_ACTION>() + offset_of!(SC_ACTION, Type))?,
                            self.u32(
                                o + i * size_of::<SC_ACTION>() + offset_of!(SC_ACTION, Delay),
                            )?,
                        ));
                    }
                } else if p != 0 {
                    self.range(p, 0, 4)?;
                }
                Config2::Failure {
                    reset: self.u32(offset_of!(SERVICE_FAILURE_ACTIONSW, dwResetPeriod))?,
                    reboot: self
                        .string(self.pointer(offset_of!(SERVICE_FAILURE_ACTIONSW, lpRebootMsg))?)?,
                    command: self
                        .string(self.pointer(offset_of!(SERVICE_FAILURE_ACTIONSW, lpCommand))?)?,
                    actions,
                }
            }
        })
    }
}
fn snapshot(s: &SERVICE_STATUS_PROCESS) -> Result<Snapshot, Fault> {
    if s.dwServiceType & SERVICE_WIN32_OWN_PROCESS == 0 {
        return Err(Fault::Malformed);
    }
    Ok(Snapshot {
        state: s.dwCurrentState,
        win32_exit: s.dwWin32ExitCode,
        specific_exit: s.dwServiceSpecificExitCode,
    })
}
fn level(l: Level) -> u32 {
    match l {
        Level::Failure => SERVICE_CONFIG_FAILURE_ACTIONS,
        Level::Delayed => SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
        Level::NonCrash => SERVICE_CONFIG_FAILURE_ACTIONS_FLAG,
        Level::Sid => SERVICE_CONFIG_SERVICE_SID_INFO,
        Level::Privileges => SERVICE_CONFIG_REQUIRED_PRIVILEGES_INFO,
        Level::Preshutdown => SERVICE_CONFIG_PRESHUTDOWN_INFO,
    }
}
impl ScmCalls for NativeScm {
    fn open(&mut self, a: Access) -> Result<bool, Win32> {
        self.service = None;
        self.manager = None;
        unsafe {
            let h = OpenSCManagerW(
                null(),
                null(),
                SC_MANAGER_CONNECT
                    | if a == Access::Install {
                        SC_MANAGER_CREATE_SERVICE
                    } else {
                        0
                    },
            );
            if h.is_null() {
                return Err(Win32(GetLastError()));
            }
            self.manager = Some(Handle(h));
            let s = OpenServiceW(h, wide("arxd").as_ptr(), rights(a));
            if s.is_null() {
                let e = GetLastError();
                if e == ABSENT {
                    return Ok(false);
                }
                return Err(Win32(e));
            }
            self.service = Some(Handle(s));
            Ok(true)
        }
    }
    fn create(&mut self, cmdline: &[u16]) -> Result<(), Win32> {
        let mut image = cmdline.to_vec();
        image.push(0);
        unsafe {
            let s = CreateServiceW(
                self.manager.as_ref().expect("opened manager").0,
                wide("arxd").as_ptr(),
                wide("arxd").as_ptr(),
                rights(Access::Install),
                SERVICE_WIN32_OWN_PROCESS,
                CREATE_BOOT,
                SERVICE_ERROR_NORMAL,
                image.as_ptr(),
                null(),
                null_mut(),
                null(),
                null(),
                null(),
            );
            if s.is_null() {
                return Err(Win32(GetLastError()));
            }
            self.service = Some(Handle(s));
            Ok(())
        }
    }
    fn config(&mut self) -> Result<BaseConfig, Fault> {
        let mut b = Buffer::new();
        let mut needed = 0;
        unsafe {
            success(QueryServiceConfigW(
                self.service(),
                b.0.as_mut_ptr().cast(),
                8192,
                &mut needed,
            ))
            .map_err(|e| Fault::Win32(e.0))?;
        }
        b.view().base_config()
    }
    fn config2(&mut self, l: Level) -> Result<Config2, Fault> {
        let mut b = Buffer::new();
        let mut needed = 0;
        unsafe {
            success(QueryServiceConfig2W(
                self.service(),
                level(l),
                b.0.as_mut_ptr(),
                8192,
                &mut needed,
            ))
            .map_err(|e| Fault::Win32(e.0))?;
        }
        b.view().config2(l)
    }
    fn state(&mut self) -> Result<Snapshot, Fault> {
        let mut s: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
        let mut needed = 0;
        unsafe {
            success(QueryServiceStatusEx(
                self.service(),
                SC_STATUS_PROCESS_INFO,
                (&mut s as *mut SERVICE_STATUS_PROCESS).cast(),
                size_of::<SERVICE_STATUS_PROCESS>() as u32,
                &mut needed,
            ))
            .map_err(|e| Fault::Win32(e.0))?;
        }
        snapshot(&s)
    }

    fn set_start(&mut self, b: Boot) -> Result<(), Win32> {
        unsafe {
            success(ChangeServiceConfigW(
                self.service(),
                SERVICE_NO_CHANGE,
                match b {
                    Boot::Auto => SERVICE_AUTO_START,
                    Boot::Demand => SERVICE_DEMAND_START,
                },
                SERVICE_NO_CHANGE,
                null(),
                null(),
                null_mut(),
                null(),
                null(),
                null(),
                null(),
            ))
        }
    }
    fn set_config2(&mut self, v: &Config2) -> Result<(), Win32> {
        // All pointed-to payloads live through this single synchronous call.
        unsafe {
            let s = self.service();
            let l = level(v.level());
            match v {
                Config2::Delayed(x) => {
                    let d = SERVICE_DELAYED_AUTO_START_INFO {
                        fDelayedAutostart: *x as i32,
                    };
                    success(ChangeServiceConfig2W(
                        s,
                        l,
                        (&d as *const SERVICE_DELAYED_AUTO_START_INFO).cast(),
                    ))
                }
                Config2::Sid(x) => {
                    let d = SERVICE_SID_INFO {
                        dwServiceSidType: *x,
                    };
                    success(ChangeServiceConfig2W(
                        s,
                        l,
                        (&d as *const SERVICE_SID_INFO).cast(),
                    ))
                }
                Config2::Preshutdown(x) => {
                    let d = SERVICE_PRESHUTDOWN_INFO {
                        dwPreshutdownTimeout: *x,
                    };
                    success(ChangeServiceConfig2W(
                        s,
                        l,
                        (&d as *const SERVICE_PRESHUTDOWN_INFO).cast(),
                    ))
                }
                Config2::NonCrash(x) => {
                    let d = SERVICE_FAILURE_ACTIONS_FLAG {
                        fFailureActionsOnNonCrashFailures: *x as i32,
                    };
                    success(ChangeServiceConfig2W(
                        s,
                        l,
                        (&d as *const SERVICE_FAILURE_ACTIONS_FLAG).cast(),
                    ))
                }
                Config2::Privileges(values) => {
                    let mut words = vec![];
                    for p in values {
                        words.extend(p);
                        words.push(0);
                    }
                    words.push(0);
                    let d = SERVICE_REQUIRED_PRIVILEGES_INFOW {
                        pmszRequiredPrivileges: words.as_mut_ptr(),
                    };
                    success(ChangeServiceConfig2W(
                        s,
                        l,
                        (&d as *const SERVICE_REQUIRED_PRIVILEGES_INFOW).cast(),
                    ))
                }
                Config2::Failure {
                    reset,
                    reboot,
                    command,
                    actions,
                } => {
                    let mut reboot = reboot.clone();
                    reboot.push(0);
                    let mut command = command.clone();
                    command.push(0);
                    let mut actions: Vec<SC_ACTION> = actions
                        .iter()
                        .map(|(t, d)| SC_ACTION {
                            Type: *t as i32,
                            Delay: *d,
                        })
                        .collect();
                    let d = SERVICE_FAILURE_ACTIONSW {
                        dwResetPeriod: *reset,
                        lpRebootMsg: reboot.as_mut_ptr(),
                        lpCommand: command.as_mut_ptr(),
                        cActions: actions.len() as u32,
                        lpsaActions: actions.as_mut_ptr(),
                    };
                    success(ChangeServiceConfig2W(
                        s,
                        l,
                        (&d as *const SERVICE_FAILURE_ACTIONSW).cast(),
                    ))
                }
            }
        }
    }
    fn start(&mut self) -> Result<(), Win32> {
        unsafe { success(StartServiceW(self.service(), 0, null())) }
    }
    fn control_stop(&mut self) -> Result<(), Win32> {
        let mut s = unsafe { std::mem::zeroed() };
        unsafe { success(ControlService(self.service(), SERVICE_CONTROL_STOP, &mut s)) }
    }
    fn delete(&mut self) -> Result<(), Win32> {
        unsafe { success(DeleteService(self.service())) }
    }
}
#[cfg(test)]
#[path = "owner_scm_windows_tests.rs"]
mod tests;
