use super::*;
fn put32(b: &mut Buffer, o: usize, v: u32) {
    b.0[o..o + 4].copy_from_slice(&v.to_ne_bytes());
}
fn ptr(b: &mut Buffer, o: usize, target: usize) {
    b.0[o..o + size_of::<usize>()].copy_from_slice(&target.to_ne_bytes());
}
fn text(b: &mut Buffer, o: usize, v: &[u16]) {
    for (i, c) in v.iter().enumerate() {
        b.0[o + i * 2..o + i * 2 + 2].copy_from_slice(&c.to_ne_bytes());
    }
}
#[test]
fn bounded_strings_and_multis() {
    let mut b = Buffer::new();
    let base = b.0.as_ptr() as usize;
    text(&mut b, 512, &[65, 0, 66, 0, 0]);
    assert_eq!(
        b.view().multi(base + 512).unwrap_or_default(),
        vec![vec![65], vec![66]]
    );
    assert!(b.view().string(0).unwrap_or_default().is_empty());
    for p in [base - 2, base + 8192, base + 8194, base + 513] {
        assert!(b.view().string(p).is_err());
    }
    b.0.fill(1);
    assert!(b.view().string(base + 512).is_err());
    assert!(b.view().multi(base + 512).is_err());
    b.0.fill(0);
    text(&mut b, 512, &[65; 65]);
    assert!(b.view().multi(base + 512).is_err());
    b.0.fill(0);
    for i in 0..17 {
        text(&mut b, 512 + i * 4, &[65, 0]);
    }
    assert!(b.view().multi(base + 512).is_err());
    text(&mut b, 8190, &[65]);
    assert!(b.view().multi(base + 8190).is_err());
}
#[test]
fn synthetic_base_and_all_config2() {
    let mut b = Buffer::new();
    let base = b.0.as_ptr() as usize;
    put32(
        &mut b,
        offset_of!(QUERY_SERVICE_CONFIGW, dwServiceType),
        TYPE,
    );
    put32(
        &mut b,
        offset_of!(QUERY_SERVICE_CONFIGW, dwStartType),
        START_DEMAND,
    );
    ptr(
        &mut b,
        offset_of!(QUERY_SERVICE_CONFIGW, lpBinaryPathName),
        base + 512,
    );
    text(&mut b, 512, &[65, 0]);
    let c = b.view().base_config().ok().unwrap();
    assert_eq!(c.image, vec![65]);
    assert!(c.account.is_empty());
    for l in [
        Level::Delayed,
        Level::Sid,
        Level::Preshutdown,
        Level::NonCrash,
    ] {
        let mut b = Buffer::new();
        put32(&mut b, 0, 1);
        assert!(b.view().config2(l).is_ok());
    }
    let mut b = Buffer::new();
    let base = b.0.as_ptr() as usize;
    ptr(
        &mut b,
        offset_of!(SERVICE_REQUIRED_PRIVILEGES_INFOW, pmszRequiredPrivileges),
        base + 512,
    );
    text(&mut b, 512, &[65, 0, 0]);
    assert!(
        matches!(b.view().config2(Level::Privileges),Ok(Config2::Privileges(p)) if p==vec![vec![65]])
    );
    let mut b = Buffer::new();
    let base = b.0.as_ptr() as usize;
    put32(&mut b, offset_of!(SERVICE_FAILURE_ACTIONSW, cActions), 3);
    ptr(
        &mut b,
        offset_of!(SERVICE_FAILURE_ACTIONSW, lpsaActions),
        base + 512,
    );
    for (i, (t, d)) in [(1, 5000), (1, 30000), (0, 0)].iter().enumerate() {
        put32(&mut b, 512 + i * 8, *t);
        put32(&mut b, 516 + i * 8, *d);
    }
    assert!(
        matches!(b.view().config2(Level::Failure),Ok(Config2::Failure {actions,..}) if actions==vec![(1,5000),(1,30000),(0,0)])
    );
    put32(&mut b, offset_of!(SERVICE_FAILURE_ACTIONSW, cActions), 9);
    assert!(b.view().config2(Level::Failure).is_err());
    put32(&mut b, offset_of!(SERVICE_FAILURE_ACTIONSW, cActions), 3);
    for p in [base - 4, base + 8192, base + 8190, base + 513] {
        ptr(&mut b, offset_of!(SERVICE_FAILURE_ACTIONSW, lpsaActions), p);
        assert!(b.view().config2(Level::Failure).is_err());
    }
}
#[test]
fn constant_mirrors() {
    assert_eq!(CREATE_BOOT, SERVICE_DEMAND_START);
    use windows_sys::Win32::Foundation::*;
    for (a, b) in [
        (TYPE, SERVICE_WIN32_OWN_PROCESS),
        (START_AUTO, SERVICE_AUTO_START),
        (START_DEMAND, SERVICE_DEMAND_START),
        (STOPPED, SERVICE_STOPPED),
        (START_PENDING, SERVICE_START_PENDING),
        (STOP_PENDING, SERVICE_STOP_PENDING),
        (RUNNING, SERVICE_RUNNING),
        (CONTINUE_PENDING, SERVICE_CONTINUE_PENDING),
        (PAUSE_PENDING, SERVICE_PAUSE_PENDING),
        (PAUSED, SERVICE_PAUSED),
        (ALREADY_RUNNING, ERROR_SERVICE_ALREADY_RUNNING),
        (ABSENT, ERROR_SERVICE_DOES_NOT_EXIST),
        (CANNOT_ACCEPT, ERROR_SERVICE_CANNOT_ACCEPT_CTRL),
        (NOT_ACTIVE, ERROR_SERVICE_NOT_ACTIVE),
        (SPECIFIC_ERROR, ERROR_SERVICE_SPECIFIC_ERROR),
        (MARKED, ERROR_SERVICE_MARKED_FOR_DELETE),
        (EXISTS, ERROR_SERVICE_EXISTS),
        (NEVER_STARTED, ERROR_SERVICE_NEVER_STARTED),
        (DUPLICATE, ERROR_DUPLICATE_SERVICE_NAME),
    ] {
        assert_eq!(a, b);
    }
}

#[test]
fn synthetic_status_process_and_empty_buffers() {
    let mut s: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
    assert!(snapshot(&s).is_err());
    s.dwServiceType = TYPE;
    s.dwCurrentState = STOPPED;
    s.dwWin32ExitCode = NEVER_STARTED;
    let got = snapshot(&s).ok().unwrap();
    assert_eq!(got.state, STOPPED);
    assert_eq!(got.win32_exit, 1077);
    assert_eq!(got.specific_exit, 0);
    let b = Buffer::new();
    assert!(
        matches!(b.view().config2(Level::Privileges),Ok(Config2::Privileges(p)) if p.is_empty())
    );
    assert!(
        matches!(b.view().config2(Level::Failure),Ok(Config2::Failure {actions,reboot,command,..}) if actions.is_empty()&&reboot.is_empty()&&command.is_empty())
    );
    let mut b = Buffer::new();
    let base = b.0.as_ptr() as usize;
    text(&mut b, 8188, &[65, 0]);
    assert!(b.view().multi(base + 8188).is_err());
    text(&mut b, 8188, &[0, 65]);
    assert!(b.view().multi(base + 8188).is_err());
}
#[test]
fn access_masks_are_operation_scoped() {
    let query = SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS;
    assert_eq!(rights(Access::Status), query);
    assert_eq!(
        rights(Access::Install),
        query | SERVICE_CHANGE_CONFIG | SERVICE_START
    );
    assert_eq!(
        rights(Access::Start),
        query | SERVICE_CHANGE_CONFIG | SERVICE_START
    );
    assert_eq!(
        rights(Access::Stop),
        query | SERVICE_CHANGE_CONFIG | SERVICE_STOP
    );
    assert_eq!(
        rights(Access::Uninstall),
        query | SERVICE_CHANGE_CONFIG | SERVICE_STOP | 0x10000
    );
    assert_eq!(SC_ACTION_RESTART, 1);
    assert_eq!(SC_ACTION_NONE, 0);
    assert_eq!(SERVICE_SID_TYPE_UNRESTRICTED, 1);
    assert_eq!(SERVICE_ERROR_NORMAL, 1);
}
