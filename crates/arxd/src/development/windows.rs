#[cfg(test)]
macro_rules! drain_at {
    ($role:ident,$out:expr,$err:expr,$pass:expr,$call:expr) => {{
        diag::role(diag::Role::$role);
        diag::totals($out, $err, $pass);
        $call
    }};
}
#[cfg(not(test))]
macro_rules! drain_at {
    ($role:ident,$out:expr,$err:expr,$pass:expr,$call:expr) => {{ $call }};
}

#[cfg(test)]
macro_rules! diagnostic { ($($body:tt)*) => {{ $($body)* }}; }
#[cfg(not(test))]
macro_rules! diagnostic {
    ($($body:tt)*) => {{}};
}
#[cfg(test)]
macro_rules! stage {
    ($stage:expr) => {{
        if let Some(error) = diag::stage($stage) {
            diag::capture(|| error, true, None);
            diagnostic_snapshot();
            return Err(Reason::ExecutionFailed);
        }
    }};
}
#[cfg(not(test))]
macro_rules! stage {
    ($($body:tt)*) => {{}};
}
#[cfg(test)]
fn diagnostic_failure(valid: bool, wait: Option<u32>) {
    diag::failure(
        || unsafe { GetLastError() },
        valid,
        wait,
        diagnostic_snapshot,
    );
}
#[cfg(test)]
fn diagnostic_snapshot() {
    let (process, job) = diag::borrowed();
    diag::enrich(|r| unsafe {
        // Entry elapsed time remains missing: no added success-path clock API call.
        let mut count = 0;
        if GetProcessHandleCount(GetCurrentProcess(), &mut count) != 0 {
            r.handles = Some(count);
        }
        if process != 0 {
            let wait = WaitForSingleObject(process as HANDLE, 0);
            if wait != WAIT_FAILED {
                r.child_wait = Some(wait);
            }
            let mut code = 0;
            if GetExitCodeProcess(process as HANDLE, &mut code) != 0 {
                r.child_code = Some(code);
            }
        }
        if job != 0 {
            let mut a: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = zeroed();
            if QueryInformationJobObject(
                job as HANDLE,
                JobObjectBasicAccountingInformation,
                (&mut a as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of_val(&a) as u32,
                null_mut(),
            ) != 0
            {
                r.active = Some(a.ActiveProcesses);
                r.total = Some(a.TotalProcesses);
                r.terminated = Some(a.TotalTerminatedProcesses);
            }
            let mut v: JOBOBJECT_LIMIT_VIOLATION_INFORMATION = zeroed();
            if QueryInformationJobObject(
                job as HANDLE,
                JobObjectLimitViolationInformation,
                (&mut v as *mut JOBOBJECT_LIMIT_VIOLATION_INFORMATION).cast(),
                size_of_val(&v) as u32,
                null_mut(),
            ) != 0
            {
                r.violation = Some(v.ViolationLimitFlags);
            }
        }
    });
}
#[cfg(test)]
fn run_ok(
    context: diag::Context,
    index: u32,
    execution: &Execution,
    r: &Request,
    cancel: &Cancellation,
) -> ResultData {
    let _scope = diag::Scope::enter();
    run_inner(execution, r, cancel, true)
        .unwrap_or_else(|reason| panic!("{}", diag::line(context, index, reason, diag::record())))
}

#[cfg(test)]
mod diag {
    use super::Reason;
    use std::cell::RefCell;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Stage {
        DaemonTokenOpen,
        DaemonTokenInfo,
        ChildTokenOpen,
        ChildTokenInfo,
        PipeOutCreate,
        PipeErrCreate,
        PipeOutNoinherit,
        PipeErrNoinherit,
        JobCreate,
        JobLimits,
        JobUi,
        NulOpen,
        AttrSize,
        AttrInit,
        AttrUpdate,
        CreateProcess,
        AssignJob,
        Resume,
        LoopOutPeek,
        LoopErrPeek,
        LoopOutRead,
        LoopErrRead,
        FinalOutPeek,
        FinalErrPeek,
        FinalOutRead,
        FinalErrRead,
        Wait,
        ExitCode,
    }
    impl Stage {
        pub const ALL: [Self; 28] = [
            Self::DaemonTokenOpen,
            Self::DaemonTokenInfo,
            Self::ChildTokenOpen,
            Self::ChildTokenInfo,
            Self::PipeOutCreate,
            Self::PipeErrCreate,
            Self::PipeOutNoinherit,
            Self::PipeErrNoinherit,
            Self::JobCreate,
            Self::JobLimits,
            Self::JobUi,
            Self::NulOpen,
            Self::AttrSize,
            Self::AttrInit,
            Self::AttrUpdate,
            Self::CreateProcess,
            Self::AssignJob,
            Self::Resume,
            Self::LoopOutPeek,
            Self::LoopErrPeek,
            Self::LoopOutRead,
            Self::LoopErrRead,
            Self::FinalOutPeek,
            Self::FinalErrPeek,
            Self::FinalOutRead,
            Self::FinalErrRead,
            Self::Wait,
            Self::ExitCode,
        ];
        pub fn label(self) -> &'static str {
            match self {
                Self::DaemonTokenOpen => "daemon_token_open",
                Self::DaemonTokenInfo => "daemon_token_info",
                Self::ChildTokenOpen => "child_token_open",
                Self::ChildTokenInfo => "child_token_info",
                Self::PipeOutCreate => "pipe_out_create",
                Self::PipeErrCreate => "pipe_err_create",
                Self::PipeOutNoinherit => "pipe_out_noinherit",
                Self::PipeErrNoinherit => "pipe_err_noinherit",
                Self::JobCreate => "job_create",
                Self::JobLimits => "job_limits",
                Self::JobUi => "job_ui",
                Self::NulOpen => "nul_open",
                Self::AttrSize => "attr_size",
                Self::AttrInit => "attr_init",
                Self::AttrUpdate => "attr_update",
                Self::CreateProcess => "create_process",
                Self::AssignJob => "assign_job",
                Self::Resume => "resume",
                Self::LoopOutPeek => "loop_out_peek",
                Self::LoopErrPeek => "loop_err_peek",
                Self::LoopOutRead => "loop_out_read",
                Self::LoopErrRead => "loop_err_read",
                Self::FinalOutPeek => "final_out_peek",
                Self::FinalErrPeek => "final_err_peek",
                Self::FinalOutRead => "final_out_read",
                Self::FinalErrRead => "final_err_read",
                Self::Wait => "wait",
                Self::ExitCode => "exit_code",
            }
        }
    }
    #[derive(Clone, Copy)]
    pub enum Context {
        SlotReuse,
        Unduplicable,
        Tree,
        Overflow,
        Handles,
        Cancel,
        Shutdown,
        Fresh,
        Fail,
        Echo,
        Cwd,
        Direct,
        Leak,
    }
    impl Context {
        fn label(self) -> &'static str {
            match self {
                Self::SlotReuse => "slot_reuse",
                Self::Unduplicable => "unduplicable",
                Self::Tree => "tree",
                Self::Overflow => "overflow",
                Self::Handles => "handles",
                Self::Cancel => "cancel",
                Self::Shutdown => "shutdown",
                Self::Fresh => "fresh",
                Self::Fail => "fail",
                Self::Echo => "echo",
                Self::Cwd => "cwd",
                Self::Direct => "direct",
                Self::Leak => "leak",
            }
        }
    }
    #[derive(Clone, Copy)]
    pub enum Role {
        Daemon,
        Child,
        Out,
        Err,
        LoopOut,
        LoopErr,
        FinalOut,
        FinalErr,
    }
    #[derive(Clone, Copy)]
    pub enum Op {
        TokenOpen,
        TokenInfo,
        PipeCreate,
        PipeNoinherit,
        Peek,
        Read,
    }
    pub fn resolve(role: Role, op: Op) -> Stage {
        use {Op::*, Role::*};
        match (role, op) {
            (Daemon, TokenOpen) => Stage::DaemonTokenOpen,
            (Daemon, TokenInfo) => Stage::DaemonTokenInfo,
            (Child, TokenOpen) => Stage::ChildTokenOpen,
            (Child, TokenInfo) => Stage::ChildTokenInfo,
            (Out, PipeCreate) => Stage::PipeOutCreate,
            (Err, PipeCreate) => Stage::PipeErrCreate,
            (Out, PipeNoinherit) => Stage::PipeOutNoinherit,
            (Err, PipeNoinherit) => Stage::PipeErrNoinherit,
            (LoopOut, Peek) => Stage::LoopOutPeek,
            (LoopOut, Read) => Stage::LoopOutRead,
            (LoopErr, Peek) => Stage::LoopErrPeek,
            (LoopErr, Read) => Stage::LoopErrRead,
            (FinalOut, Peek) => Stage::FinalOutPeek,
            (FinalOut, Read) => Stage::FinalOutRead,
            (FinalErr, Peek) => Stage::FinalErrPeek,
            (FinalErr, Read) => Stage::FinalErrRead,
            _ => panic!("invalid diagnostic role"),
        }
    }
    #[derive(Clone, Copy, Default)]
    pub struct Record {
        pub stage: Option<Stage>,
        pub error: Option<u32>,
        pub valid: bool,
        pub wait: Option<u32>,
        pub ms: Option<u64>,
        pub out: Option<u64>,
        pub err: Option<u64>,
        pub pass: Option<u32>,
        pub handles: Option<u32>,
        pub child_wait: Option<u32>,
        pub child_code: Option<u32>,
        pub active: Option<u32>,
        pub total: Option<u32>,
        pub terminated: Option<u32>,
        pub violation: Option<u32>,
    }
    #[derive(Default)]
    struct State {
        scoped: bool,
        running: bool,
        armed: Option<(Stage, u32)>,
        captured: Option<Record>,
        pending: Option<Stage>,
        role: Option<Role>,
        // Borrowed handles are excluded from Record and formatting.
        process: usize,
        job: usize,
        out: Option<u64>,
        err: Option<u64>,
        pass: Option<u32>,
        calls: u32,
    }
    thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
    pub struct Scope(std::marker::PhantomData<std::rc::Rc<()>>);
    impl Scope {
        pub fn enter() -> Self {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                assert!(!s.scoped && !s.running, "nested diagnostic scope");
                *s = State {
                    scoped: true,
                    ..State::default()
                };
            });
            Self(std::marker::PhantomData)
        }
    }
    impl Drop for Scope {
        fn drop(&mut self) {
            STATE.with(|s| *s.borrow_mut() = State::default());
        }
    }
    pub struct Invocation(std::marker::PhantomData<std::rc::Rc<()>>);
    impl Invocation {
        pub fn enter() -> Self {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                assert!(!s.running, "reentrant diagnostic invocation");
                s.running = true;
                s.captured = None;
                s.pending = None;
                s.role = None;
                s.process = 0;
                s.job = 0;
                s.out = None;
                s.err = None;
                s.pass = None;
                s.calls = 0;
            });
            Self(std::marker::PhantomData)
        }
    }
    impl Drop for Invocation {
        fn drop(&mut self) {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                if s.scoped {
                    s.running = false;
                    s.pending = None;
                    s.role = None;
                    s.process = 0;
                    s.job = 0;
                    s.armed = None;
                } else {
                    *s = State::default();
                }
            });
        }
    }
    #[allow(dead_code)] // Future separately authorized replay only; no native replay tests here.
    pub fn arm(stage: Stage, error: u32) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            assert!(s.scoped && !s.running);
            s.armed = Some((stage, error));
        });
    }
    pub fn role(role: Role) {
        STATE.with(|s| s.borrow_mut().role = Some(role));
    }
    pub fn operation(op: Op) -> Stage {
        STATE.with(|s| resolve(s.borrow().role.expect("diagnostic role"), op))
    }
    pub fn stage(stage: Stage) -> Option<u32> {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.pending = Some(stage);
            if s.running && s.armed.is_some_and(|a| a.0 == stage) {
                s.armed.take().map(|a| a.1)
            } else {
                None
            }
        })
    }
    pub fn handles(process: usize, job: usize) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.process = process;
            s.job = job;
        });
    }
    pub fn borrowed() -> (usize, usize) {
        STATE.with(|s| {
            let s = s.borrow();
            (s.process, s.job)
        })
    }
    pub fn totals(out: u64, err: u64, pass: Option<u32>) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.out = Some(out);
            s.err = Some(err);
            s.pass = pass;
        });
    }
    pub fn create_call() {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.calls = s.calls.saturating_add(1);
        });
    }
    #[allow(dead_code)] // Available to future native assertions, no native proof earned here.
    pub fn create_calls() -> u32 {
        STATE.with(|s| s.borrow().calls)
    }
    pub fn capture(reader: impl FnOnce() -> u32, valid: bool, wait: Option<u32>) {
        let error = reader(); // First, before TLS, snapshots or cleanup.
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.running && s.captured.is_none() {
                s.captured = Some(Record {
                    stage: s.pending,
                    error: Some(error),
                    valid,
                    wait,
                    ms: None,
                    out: s.out,
                    err: s.err,
                    pass: s.pass,
                    ..Record::default()
                });
            }
        });
    }
    pub fn failure(
        reader: impl FnOnce() -> u32,
        valid: bool,
        wait: Option<u32>,
        observe: impl FnOnce(),
    ) {
        capture(reader, valid, wait);
        observe();
    }
    pub fn enrich(f: impl FnOnce(&mut Record)) {
        if let Some(mut r) = record() {
            f(&mut r);
            STATE.with(|s| s.borrow_mut().captured = Some(r));
        }
    }
    pub fn record() -> Option<Record> {
        STATE.with(|s| s.borrow().captured)
    }
    fn n<T: std::fmt::Display>(v: Option<T>) -> String {
        v.map_or_else(|| "-".into(), |v| v.to_string())
    }
    pub fn line(context: Context, index: u32, reason: Reason, record: Option<Record>) -> String {
        let r = if reason == Reason::ExecutionFailed {
            record.unwrap_or_default()
        } else {
            Record::default()
        };
        format!(
            "c={} r={} s={} i={} e={} v={} w={} ms={} o={} d={} p={} h={} cw={} cc={} ja={} jt={} jk={} jv={}",
            context.label(),
            reason.as_str(),
            r.stage.map_or("-", Stage::label),
            index,
            n(r.error),
            u8::from(r.valid),
            n(r.wait),
            n(r.ms),
            n(r.out),
            n(r.err),
            n(r.pass),
            n(r.handles),
            n(r.child_wait),
            n(r.child_code),
            n(r.active),
            n(r.total),
            n(r.terminated),
            n(r.violation)
        )
    }

    #[cfg(test)]
    mod pure {
        use super::*;
        // Independent accepted baseline inventory, not generated from Stage::ALL.
        const MANIFEST: &[(u32, &[&str])] = &[
            (64, &["daemon_token_info", "child_token_info"]),
            (89, &["daemon_token_open", "child_token_open"]),
            (221, &["job_create"]),
            (231, &["job_limits"]),
            (240, &["job_ui"]),
            (286, &["pipe_out_create", "pipe_err_create"]),
            (289, &["pipe_out_noinherit", "pipe_err_noinherit"]),
            (
                324,
                &[
                    "loop_out_peek",
                    "loop_err_peek",
                    "final_out_peek",
                    "final_err_peek",
                ],
            ),
            (
                329,
                &[
                    "loop_out_read",
                    "loop_err_read",
                    "final_out_read",
                    "final_err_read",
                ],
            ),
            (414, &["nul_open"]),
            (420, &["attr_size"]),
            (423, &["attr_init"]),
            (430, &["attr_update"]),
            (448, &["create_process"]),
            (471, &["assign_job"]),
            (478, &["resume"]),
            (508, &["wait"]),
            (512, &["exit_code"]),
        ];
        fn mapping_valid(labels: &[&str]) -> bool {
            let mut expected: Vec<_> = MANIFEST
                .iter()
                .flat_map(|(_, v)| v.iter().copied())
                .collect();
            let mut actual = labels.to_vec();
            expected.sort();
            actual.sort();
            actual == expected
        }
        #[test]
        fn independent_mapping_and_module_mutants() {
            assert_eq!(MANIFEST.len(), 18);
            let labels: Vec<_> = Stage::ALL.iter().map(|s| s.label()).collect();
            assert!(mapping_valid(&labels));
            for i in 0..labels.len() {
                let mut deleted = labels.clone();
                deleted.remove(i);
                assert!(!mapping_valid(&deleted));
                let mut mislabeled = labels.clone();
                mislabeled[i] = "wrong";
                assert!(!mapping_valid(&mislabeled));
            }
            assert_eq!(
                resolve(Role::Daemon, Op::TokenOpen).label(),
                "daemon_token_open"
            );
            assert_eq!(
                resolve(Role::Daemon, Op::TokenInfo).label(),
                "daemon_token_info"
            );
            assert_eq!(
                resolve(Role::Child, Op::TokenOpen).label(),
                "child_token_open"
            );
            assert_eq!(
                resolve(Role::Child, Op::TokenInfo).label(),
                "child_token_info"
            );
            assert_eq!(
                resolve(Role::Out, Op::PipeCreate).label(),
                "pipe_out_create"
            );
            assert_eq!(
                resolve(Role::Err, Op::PipeCreate).label(),
                "pipe_err_create"
            );
            assert_eq!(
                resolve(Role::Out, Op::PipeNoinherit).label(),
                "pipe_out_noinherit"
            );
            assert_eq!(
                resolve(Role::Err, Op::PipeNoinherit).label(),
                "pipe_err_noinherit"
            );
            assert_eq!(resolve(Role::LoopOut, Op::Peek).label(), "loop_out_peek");
            assert_eq!(resolve(Role::LoopOut, Op::Read).label(), "loop_out_read");
            assert_eq!(resolve(Role::LoopErr, Op::Peek).label(), "loop_err_peek");
            assert_eq!(resolve(Role::LoopErr, Op::Read).label(), "loop_err_read");
            assert_eq!(resolve(Role::FinalOut, Op::Peek).label(), "final_out_peek");
            assert_eq!(resolve(Role::FinalOut, Op::Read).label(), "final_out_read");
            assert_eq!(resolve(Role::FinalErr, Op::Peek).label(), "final_err_peek");
            assert_eq!(resolve(Role::FinalErr, Op::Read).label(), "final_err_read");
        }
        #[test]
        fn lifecycle_one_shot_unwind_and_counter() {
            {
                let _scope = Scope::enter();
                arm(Stage::Resume, 55);
                {
                    let _run = Invocation::enter();
                    assert!(record().is_none());
                    assert_eq!(stage(Stage::Wait), None);
                    assert_eq!(stage(Stage::Resume), Some(55));
                    assert_eq!(stage(Stage::Resume), None);
                    create_call();
                    assert_eq!(create_calls(), 1);
                    assert!(std::panic::catch_unwind(Invocation::enter).is_err());
                    capture(|| 55, true, None);
                    assert_eq!(record().unwrap().error, Some(55));
                }
                assert_eq!(record().unwrap().error, Some(55));
                {
                    let _run = Invocation::enter();
                    assert!(record().is_none());
                }
            }
            assert!(record().is_none());
            assert!(
                std::panic::catch_unwind(|| {
                    let _scope = Scope::enter();
                    let _run = Invocation::enter();
                    capture(|| 7, true, None);
                    panic!("synthetic unwind");
                })
                .is_err()
            );
            assert!(record().is_none());
            assert!(!STATE.with(|s| s.borrow().running));
        }
        #[test]
        fn error_reader_precedes_mock_cleanup_and_preserves_first() {
            let _scope = Scope::enter();
            let _run = Invocation::enter();
            stage(Stage::Wait);
            let mock = std::cell::Cell::new(123u32);
            failure(|| mock.get(), true, Some(u32::MAX), || mock.set(999)); // Pure mock cleanup/error clobber, no Win32 call.
            capture(|| mock.get(), true, None);
            assert_eq!(record().unwrap().error, Some(123));
            assert_eq!(record().unwrap().wait, Some(u32::MAX));
            assert_eq!(mock.get(), 999);
        }
        #[test]
        fn complete_format_bounds_indices_validity_and_privacy() {
            let max = Record {
                stage: Some(Stage::PipeErrNoinherit),
                error: Some(u32::MAX),
                valid: true,
                wait: Some(u32::MAX),
                ms: Some(u64::MAX),
                out: Some(u64::MAX),
                err: Some(u64::MAX),
                pass: Some(u32::MAX),
                handles: Some(u32::MAX),
                child_wait: Some(u32::MAX),
                child_code: Some(u32::MAX),
                active: Some(u32::MAX),
                total: Some(u32::MAX),
                terminated: Some(u32::MAX),
                violation: Some(u32::MAX),
            };
            for stage in Stage::ALL {
                for context in [
                    Context::SlotReuse,
                    Context::Unduplicable,
                    Context::Tree,
                    Context::Overflow,
                    Context::Handles,
                    Context::Cancel,
                    Context::Shutdown,
                    Context::Fresh,
                    Context::Fail,
                    Context::Echo,
                    Context::Cwd,
                    Context::Direct,
                    Context::Leak,
                ] {
                    let line = line(
                        context,
                        u32::MAX,
                        Reason::ExecutionFailed,
                        Some(Record {
                            stage: Some(stage),
                            ..max
                        }),
                    );
                    assert!(line.len() <= 384, "{} {}", line.len(), line);
                    assert!(line.contains("i=4294967295 e=4294967295 v=1"));
                }
            }
            for index in [3, 7] {
                assert_eq!(
                    line(Context::Leak, index, Reason::ExecutionFailed, None),
                    format!(
                        "c=leak r=execution_failed s=- i={index} e=- v=0 w=- ms=- o=- d=- p=- h=- cw=- cc=- ja=- jt=- jk=- jv=-"
                    )
                );
            }
            for reason in [
                Reason::InvalidArguments,
                Reason::IdentityMismatch,
                Reason::CleanupUnconfirmed,
            ] {
                let text = line(Context::Leak, 7, reason, Some(max));
                assert!(text.contains(&format!("r={} s=- i=7 e=- v=0", reason.as_str())));
            }
            let text = line(
                Context::Unduplicable,
                7,
                Reason::ExecutionFailed,
                Some(Record {
                    valid: false,
                    ..max
                }),
            );
            assert!(text.contains("v=0"));
            // Synthetic private payloads cannot be supplied to this typed interface.
            for payload in [
                r"C:\private\sentinel",
                "--secret-argv",
                "S-1-5-21-SECRET",
                "ENV_SECRET=abc",
                "raw-output-secret",
                "0xdeadbeef",
                "Local\\private-event",
            ] {
                assert!(!text.contains(payload));
            }
        }
    }
}
use super::*;
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::*,
    Storage::FileSystem::*,
    System::{JobObjects::*, Pipes::*, Threading::*},
};
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn ok(n: i32) -> std::result::Result<(), Reason> {
    if n == 0 {
        diagnostic!(diagnostic_failure(true, None));
        Err(Reason::ExecutionFailed)
    } else {
        Ok(())
    }
}
fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}
struct Facts {
    user: Vec<u8>,
    service: bool,
    elevated: bool,
    integrity: u32,
    admin: bool,
}
fn policy(f: &Facts) -> std::result::Result<(), &'static str> {
    if f.service {
        Err("service_identity")
    } else if f.elevated {
        Err("elevated_identity")
    } else if f.integrity != 8192 {
        Err("integrity_not_medium")
    } else if f.admin {
        Err("admin_group_enabled")
    } else {
        Ok(())
    }
}
unsafe fn info(
    token: HANDLE,
    class: TOKEN_INFORMATION_CLASS,
) -> std::result::Result<Vec<usize>, Reason> {
    unsafe {
        let mut len = 0;
        GetTokenInformation(token, class, null_mut(), 0, &mut len);
        if len == 0 || len > 65536 {
            return Err(Reason::PreflightFailed);
        }
        let mut buf = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
        stage!(diag::operation(diag::Op::TokenInfo));
        ok(GetTokenInformation(
            token,
            class,
            buf.as_mut_ptr().cast(),
            len,
            &mut len,
        ))?;
        Ok(buf)
    }
}
unsafe fn sid_bytes(s: PSID) -> std::result::Result<Vec<u8>, Reason> {
    unsafe {
        if IsValidSid(s) == 0 {
            return Err(Reason::PreflightFailed);
        }
        let n = GetLengthSid(s);
        if n > 68 {
            return Err(Reason::PreflightFailed);
        }
        Ok(std::slice::from_raw_parts(s.cast::<u8>(), n as usize).to_vec())
    }
}
fn facts(process: HANDLE) -> std::result::Result<Facts, Reason> {
    unsafe {
        let mut token = null_mut();
        stage!(diag::operation(diag::Op::TokenOpen));
        ok(OpenProcessToken(process, TOKEN_QUERY, &mut token))?;
        let token = Handle(token);
        let u = info(token.0, TokenUser)?;
        let user = sid_bytes((*(u.as_ptr().cast::<TOKEN_USER>())).User.Sid)?;
        let e = info(token.0, TokenElevation)?;
        let elevated = (*(e.as_ptr().cast::<TOKEN_ELEVATION>())).TokenIsElevated != 0;
        let i = info(token.0, TokenIntegrityLevel)?;
        let sid = (*(i.as_ptr().cast::<TOKEN_MANDATORY_LABEL>())).Label.Sid;
        if IsValidSid(sid) == 0 {
            return Err(Reason::PreflightFailed);
        }
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return Err(Reason::PreflightFailed);
        }
        let integrity = *GetSidSubAuthority(sid, (count - 1) as u32);
        let g = info(token.0, TokenGroups)?;
        let groups = &*g.as_ptr().cast::<TOKEN_GROUPS>();
        let count = groups.GroupCount as usize;
        let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
        if count > 1024
            || offset + count * size_of::<SID_AND_ATTRIBUTES>() > g.len() * size_of::<usize>()
        {
            return Err(Reason::PreflightFailed);
        }
        let mut admin = false;
        for group in std::slice::from_raw_parts(groups.Groups.as_ptr(), count) {
            if IsWellKnownSid(group.Sid, WinBuiltinAdministratorsSid) != 0
                && group.Attributes&0x10 /* SE_GROUP_USE_FOR_DENY_ONLY */==0
            {
                admin = true;
            }
        }
        let service = [WinLocalSystemSid, WinLocalServiceSid, WinNetworkServiceSid]
            .iter()
            .any(|kind| IsWellKnownSid((*(u.as_ptr().cast::<TOKEN_USER>())).User.Sid, *kind) != 0);
        Ok(Facts {
            user,
            service,
            elevated,
            integrity,
            admin,
        })
    }
}
pub(super) fn preflight() -> std::result::Result<(), Reason> {
    diagnostic!(diag::role(diag::Role::Daemon));
    let f = facts(unsafe { GetCurrentProcess() })?;
    policy(&f).map_err(|_| Reason::PreflightFailed)
}
/// The current token's profile directory, for best-effort output redaction only.
pub(super) fn profile() -> Option<String> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let token = Handle(token);
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        if windows_sys::Win32::UI::Shell::GetUserProfileDirectoryW(
            token.0,
            buf.as_mut_ptr(),
            &mut n,
        ) == 0
        {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0)?;
        String::from_utf16(&buf[..len]).ok()
    }
}
// Opening each component without delete/write sharing keeps checked directories
// stable until the suspended child has been assigned and resumed.
fn pin(path: &Path) -> std::result::Result<Vec<std::fs::File>, Reason> {
    crate::config::checked_path(path).map_err(|_| Reason::InvalidArguments)?;
    let mut prefix = PathBuf::new();
    let mut pins = Vec::new();
    for part in path.components() {
        prefix.push(part);
        if matches!(part, std::path::Component::Prefix(_)) {
            continue;
        }
        pins.push(open_pinned(&prefix)?);
    }
    Ok(pins)
}
/// The handle-based check is the backstop for a component replaced by a
/// reparse point after `checked_path` inspected it by name.
fn open_pinned(path: &Path) -> std::result::Result<std::fs::File, Reason> {
    use std::os::windows::fs::MetadataExt;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|_| Reason::InvalidArguments)?;
    if f.metadata()
        .map_err(|_| Reason::InvalidArguments)?
        .file_attributes()
        & FILE_ATTRIBUTE_REPARSE_POINT
        != 0
    {
        return Err(Reason::InvalidArguments);
    }
    Ok(f)
}
fn resolve(
    settings: &Settings,
    tool: &str,
) -> std::result::Result<(PathBuf, Vec<std::fs::File>), Reason> {
    for dir in settings.environment["PATH"].split(';') {
        let p = Path::new(dir).join(format!("{tool}.exe"));
        if p.is_file() {
            let pins = pin(&p)?;
            return Ok((p, pins));
        }
    }
    Err(Reason::Unavailable)
}
struct Job(Handle);
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            TerminateJobObject(self.0.0, 1);
        }
    }
}
impl Job {
    fn new(l: &Limits) -> std::result::Result<Self, Reason> {
        unsafe {
            stage!(diag::Stage::JobCreate);
            let h = CreateJobObjectW(null(), null());
            if h.is_null() {
                diagnostic!(diagnostic_failure(true, None));
                return Err(Reason::ExecutionFailed);
            }
            let job = Self(Handle(h));
            diagnostic!(diag::handles(0, h as usize));
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_JOB_MEMORY
                | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
            limits.BasicLimitInformation.ActiveProcessLimit = l.pids;
            limits.JobMemoryLimit = l.memory_bytes;
            stage!(diag::Stage::JobLimits);
            ok(SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
            ))?;
            let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
                UIRestrictionsClass: 0xff, /* all eight UI limits */
            };
            stage!(diag::Stage::JobUi);
            ok(SetInformationJobObject(
                h,
                JobObjectBasicUIRestrictions,
                (&ui as *const JOBOBJECT_BASIC_UI_RESTRICTIONS).cast(),
                size_of_val(&ui) as u32,
            ))?;
            Ok(job)
        }
    }
    fn stop(&self) -> bool {
        unsafe {
            if TerminateJobObject(self.0.0, 1) == 0 {
                return false;
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let mut a: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = zeroed();
                if QueryInformationJobObject(
                    self.0.0,
                    JobObjectBasicAccountingInformation,
                    (&mut a as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    size_of_val(&a) as u32,
                    null_mut(),
                ) == 0
                {
                    return false;
                }
                if a.ActiveProcesses == 0 {
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
fn pipe() -> std::result::Result<(Handle, Handle), Reason> {
    unsafe {
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let (mut r, mut w) = (null_mut(), null_mut());
        stage!(diag::operation(diag::Op::PipeCreate));
        ok(CreatePipe(&mut r, &mut w, &sa, 0))?;
        let r = Handle(r);
        let w = Handle(w);
        stage!(diag::operation(diag::Op::PipeNoinherit));
        ok(SetHandleInformation(r.0, HANDLE_FLAG_INHERIT, 0))?;
        Ok((r, w))
    }
}
struct Attributes {
    buf: Vec<usize>,
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.buf.as_mut_ptr().cast());
        }
    }
}
/// Raw bytes kept past the requested limit so a path crossing the cut is still
/// whole when redacted; the returned text is then cut to `output_bytes`.
const REDACTION_MARGIN: usize = 4096;
#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    total: u64,
}
impl Captured {
    fn dropped(&self, limit: usize) -> u64 {
        self.total.saturating_sub(limit as u64)
    }
}
/// Returns true once the stream has produced more than `limit` bytes.
fn drain(h: HANDLE, c: &mut Captured, limit: usize) -> std::result::Result<bool, Reason> {
    unsafe {
        let mut available = 0;
        #[cfg(test)]
        if let Some(error) = diag::stage(diag::operation(diag::Op::Peek)) {
            if error == ERROR_BROKEN_PIPE {
                return Ok(c.total > limit as u64);
            }
            diag::failure(|| error, true, None, diagnostic_snapshot);
            return Err(Reason::ExecutionFailed);
        }
        if PeekNamedPipe(h, null_mut(), 0, null_mut(), &mut available, null_mut()) == 0 {
            let error = GetLastError();
            if error == ERROR_BROKEN_PIPE {
                return Ok(c.total > limit as u64);
            }
            diagnostic!(diag::failure(|| error, true, None, diagnostic_snapshot));
            return Err(Reason::ExecutionFailed);
        }
        if available > 0 {
            let mut b = [0u8; 4096];
            let mut n = 0;
            stage!(diag::operation(diag::Op::Read));
            ok(ReadFile(
                h,
                b.as_mut_ptr(),
                available.min(4096),
                &mut n,
                null_mut(),
            ))?;
            let keep = (limit + REDACTION_MARGIN).saturating_sub(c.bytes.len());
            c.bytes.extend_from_slice(&b[..(n as usize).min(keep)]);
            c.total += u64::from(n);
        }
        Ok(c.total > limit as u64)
    }
}
pub(super) fn run(
    execution: &Execution,
    r: &Request,
    cancel: &Cancellation,
) -> std::result::Result<ResultData, Reason> {
    run_inner(execution, r, cancel, {
        #[cfg(test)]
        {
            execution.policy_fixture
        }
        #[cfg(not(test))]
        {
            false
        }
    })
}
fn run_inner(
    execution: &Execution,
    r: &Request,
    cancel: &Cancellation,
    fixture: bool,
) -> std::result::Result<ResultData, Reason> {
    #[cfg(test)]
    let _invocation = diag::Invocation::enter();
    #[cfg_attr(not(test), allow(unused_mut))]
    let mut daemon = {
        diagnostic!(diag::role(diag::Role::Daemon));
        facts(unsafe { GetCurrentProcess() })?
    };
    // The only bypass is a unit-test call; no binary/config/CLI can select it.
    if !(cfg!(test) && fixture) {
        policy(&daemon).map_err(|_| Reason::PreflightFailed)?;
    }
    #[cfg(test)]
    if execution.tamper_identity.load(Ordering::Acquire) {
        // Flip one byte of the in-memory copy only; nothing is logged or stored.
        if let Some(last) = daemon.user.last_mut() {
            *last ^= 1;
        }
    }
    let settings = &execution.settings;
    let (exe, _exe_pins) = resolve(settings, &r.tool)?;
    let cwd = settings.workspace.join(&r.cwd);
    let _cwd_pins = pin(&cwd)?;
    if !cwd.is_dir() {
        return Err(Reason::InvalidArguments);
    }
    let mut command = std::iter::once(quote(exe.to_str().ok_or(Reason::InvalidArguments)?))
        .chain(r.args.iter().map(|s| quote(s)))
        .collect::<Vec<_>>()
        .join(" ")
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    if command.len() > 32767 {
        return Err(Reason::InvalidArguments);
    }
    diagnostic!(diag::role(diag::Role::Out));
    let (out_r, out_w) = pipe()?;
    diagnostic!(diag::role(diag::Role::Err));
    let (err_r, err_w) = pipe()?;
    let job = Job::new(&settings.limits)?;
    unsafe {
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        stage!(diag::Stage::NulOpen);
        let nul = Handle(CreateFileW(
            wide(std::ffi::OsStr::new("NUL")).as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &sa,
            OPEN_EXISTING,
            0,
            null_mut(),
        ));
        if nul.0 == INVALID_HANDLE_VALUE {
            diagnostic!(diagnostic_failure(true, None));
            return Err(Reason::ExecutionFailed);
        }
        let mut handles = [nul.0, out_w.0, err_w.0];
        let mut size = 0;
        stage!(diag::Stage::AttrSize);
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
        if size == 0 || size > 65536 {
            diagnostic!(diagnostic_failure(false, None));
            return Err(Reason::ExecutionFailed);
        }
        let mut buf = vec![0usize; size.div_ceil(size_of::<usize>())];
        stage!(diag::Stage::AttrInit);
        ok(InitializeProcThreadAttributeList(
            buf.as_mut_ptr().cast(),
            1,
            0,
            &mut size,
        ))?;
        let mut attributes = Attributes { buf };
        stage!(diag::Stage::AttrUpdate);
        ok(UpdateProcThreadAttribute(
            attributes.buf.as_mut_ptr().cast(),
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_mut_ptr().cast(),
            size_of_val(&handles),
            null_mut(),
            null(),
        ))?;
        let mut startup: STARTUPINFOEXW = zeroed();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = nul.0;
        startup.StartupInfo.hStdOutput = out_w.0;
        startup.StartupInfo.hStdError = err_w.0;
        startup.lpAttributeList = attributes.buf.as_mut_ptr().cast();
        let mut pi: PROCESS_INFORMATION = zeroed();
        let env = environment(settings);
        stage!(diag::Stage::CreateProcess);
        diagnostic!(diag::create_call());
        ok(CreateProcessW(
            wide(exe.as_os_str()).as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED
                | CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT,
            env.as_ptr().cast::<c_void>(),
            wide(cwd.as_os_str()).as_ptr(),
            &startup.StartupInfo,
            &mut pi,
        ))?;
        let process = Handle(pi.hProcess);
        let thread = Handle(pi.hThread);
        diagnostic!(diag::handles(process.0 as usize, job.0.0 as usize));
        drop(out_w);
        drop(err_w);
        drop(nul);
        let result = (|| {
            stage!(diag::Stage::AssignJob);
            if AssignProcessToJobObject(job.0.0, process.0) == 0 {
                diagnostic!(diag::capture(|| GetLastError(), true, None));
                TerminateProcess(process.0, 1);
                diagnostic!(diagnostic_snapshot());
                return Err(Reason::ExecutionFailed);
            }
            diagnostic!(diag::role(diag::Role::Child));
            let child = facts(process.0)?;
            if child.user != daemon.user || (!(cfg!(test) && fixture) && policy(&child).is_err()) {
                return Err(Reason::IdentityMismatch);
            }
            stage!(diag::Stage::Resume);
            if ResumeThread(thread.0) == u32::MAX {
                diagnostic!(diagnostic_failure(true, None));
                return Err(Reason::ExecutionFailed);
            }
            let start = Instant::now();
            let mut stdout = Captured::default();
            let mut stderr = Captured::default();
            let mut reason = StopReason::Exited;
            loop {
                if drain_at!(
                    LoopOut,
                    stdout.total,
                    stderr.total,
                    None,
                    drain(out_r.0, &mut stdout, r.output_bytes)
                )? || drain_at!(
                    LoopErr,
                    stdout.total,
                    stderr.total,
                    None,
                    drain(err_r.0, &mut stderr, r.output_bytes)
                )? {
                    reason = StopReason::OutputLimit;
                    break;
                }
                if execution.shutdown.is_cancelled() {
                    reason = StopReason::Shutdown;
                    break;
                }
                if cancel.is_cancelled() {
                    reason = StopReason::Cancelled;
                    break;
                }
                if start.elapsed() >= Duration::from_secs(r.timeout_s) {
                    reason = StopReason::Timeout;
                    break;
                }
                stage!(diag::Stage::Wait);
                let wait = WaitForSingleObject(process.0, 10);
                if wait == WAIT_OBJECT_0 {
                    break;
                }
                if wait != WAIT_TIMEOUT {
                    diagnostic!(diagnostic_failure(wait == WAIT_FAILED, Some(wait)));
                    return Err(Reason::ExecutionFailed);
                }
            }
            let mut code = 0;
            stage!(diag::Stage::ExitCode);
            ok(GetExitCodeProcess(process.0, &mut code))?;
            let cleanup = job.stop();
            if !cleanup {
                return Err(Reason::CleanupUnconfirmed);
            }
            // Bounded final read: pipes cannot hold us open, even if a handle escaped.
            for _pass in 0..32 {
                let overflow = drain_at!(
                    FinalOut,
                    stdout.total,
                    stderr.total,
                    Some(_pass),
                    drain(out_r.0, &mut stdout, r.output_bytes)
                )? | drain_at!(
                    FinalErr,
                    stdout.total,
                    stderr.total,
                    Some(_pass),
                    drain(err_r.0, &mut stderr, r.output_bytes)
                )?;
                if overflow && reason == StopReason::Exited {
                    reason = StopReason::OutputLimit;
                }
            }
            Ok(ResultData {
                execution_mode: MODE,
                claim: CLAIM,
                dedicated: false,
                stop_reason: reason,
                exit_code: if reason == StopReason::Exited {
                    Some(code as i32)
                } else {
                    None
                },
                cleanup_confirmed: true,
                matches_current_user: true,
                stdout: bounded_text(
                    redact(
                        String::from_utf8_lossy(&stdout.bytes).into_owned(),
                        settings,
                    ),
                    r.output_bytes,
                ),
                stderr: bounded_text(
                    redact(
                        String::from_utf8_lossy(&stderr.bytes).into_owned(),
                        settings,
                    ),
                    r.output_bytes,
                ),
                stdout_dropped_bytes: stdout.dropped(r.output_bytes),
                stderr_dropped_bytes: stderr.dropped(r.output_bytes),
            })
        })();
        if result.is_err() {
            TerminateProcess(process.0, 1);
            job.stop();
            execution.quarantined.store(true, Ordering::Release);
        }
        result
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[test]
    fn token_policy_refuses_every_unsafe_fact() {
        let mut f = Facts {
            user: vec![],
            service: false,
            elevated: false,
            integrity: 8192,
            admin: false,
        };
        assert!(policy(&f).is_ok());
        f.service = true;
        assert_eq!(policy(&f), Err("service_identity"));
        f.service = false;
        f.elevated = true;
        assert_eq!(policy(&f), Err("elevated_identity"));
        f.elevated = false;
        for rid in [0, 4096, 8193, 12288, 16384] {
            f.integrity = rid;
            assert_eq!(policy(&f), Err("integrity_not_medium"));
        }
        f.integrity = 8192;
        f.admin = true;
        assert_eq!(policy(&f), Err("admin_group_enabled"));
    }
    #[test]
    fn quoter_roundtrips_windows_parser() {
        unsafe {
            use windows_sys::Win32::UI::Shell::CommandLineToArgvW;
            let args = [
                "app.exe",
                "",
                "a b",
                "x\"y",
                "trailing\\",
                "\\\\\"quoted",
                "\u{e9}\u{4e2d}\u{1f600} \\\\",
            ];
            let line = args.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ");
            let mut count = 0;
            let ptr = CommandLineToArgvW(wide(std::ffi::OsStr::new(&line)).as_ptr(), &mut count);
            assert!(!ptr.is_null());
            assert_eq!(count, args.len() as i32);
            for (i, a) in args.iter().enumerate() {
                let p = *ptr.add(i);
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                assert_eq!(
                    String::from_utf16_lossy(std::slice::from_raw_parts(p, n)),
                    *a
                );
            }
            LocalFree(ptr.cast());
        }
    }
    #[test]
    fn fixture_child() {
        // The action comes from the fixture environment or, for SDK tests, from an
        // extra libtest filter argument `action=<name>` that matches no test.
        let action = std::env::var("ARX_UNIT_ACTION")
            .ok()
            .or_else(|| std::env::args().find_map(|a| a.strip_prefix("action=").map(String::from)));
        let Some(action) = action else {
            return;
        };
        match action.as_str() {
            "overflow" => {
                print!("{}", "x".repeat(32768));
            }
            // Synthetic markers only: the working directory must come back redacted.
            "echo" => {
                let cwd = std::env::current_dir().unwrap();
                println!("ARX-ECHO-MARKER {} S-1-5-21-7-8-9-1001", cwd.display());
                eprintln!("ARX-ERR-MARKER");
            }
            "touch" => {
                std::fs::write("touched", b"1").unwrap();
                println!("ARX-TOUCH-MARKER");
            }
            "fail" => std::process::exit(7),
            "tree" => {
                let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "development::windows::tests::fixture_child",
                        "--nocapture",
                    ])
                    .env("ARX_UNIT_ACTION", "sleep")
                    .spawn()
                    .unwrap();
                let _ = child.wait();
            }
            "sleep" => std::thread::sleep(Duration::from_secs(30)),
            "handles" => unsafe {
                use windows_sys::Win32::System::Console::*;
                let mut sentinel = std::env::var("ARX_UNIT_HANDLE")
                    .unwrap()
                    .parse::<usize>()
                    .unwrap() as HANDLE;
                // Deterministic seam for the intermittent case: Windows' own ETW
                // registrations sit in valid slots yet refuse duplication, and the
                // parent's sentinel value sometimes lands on one of them.
                if std::env::var_os("ARX_UNIT_UNDUPLICABLE").is_some() {
                    sentinel = (4..=0x4000usize)
                        .step_by(4)
                        .map(|v| v as HANDLE)
                        .find(|&h| {
                            let (mut flags, mut copy) = (0, null_mut());
                            GetHandleInformation(h, &mut flags) != 0
                                && if DuplicateHandle(
                                    GetCurrentProcess(),
                                    h,
                                    GetCurrentProcess(),
                                    &mut copy,
                                    0,
                                    0,
                                    DUPLICATE_SAME_ACCESS,
                                ) != 0
                                {
                                    drop(Handle(copy));
                                    false
                                } else {
                                    GetLastError() == ERROR_NOT_SUPPORTED
                                }
                        })
                        .expect("no valid unduplicable slot in child");
                }
                // Model loader/runtime reuse of the parent's numeric slot, using
                // real child-local kernel objects rather than mocking Win32.
                let mut held = Vec::new();
                if std::env::var_os("ARX_UNIT_REUSE").is_some() {
                    let mut flags = 0;
                    for _ in 0..4096 {
                        if GetHandleInformation(sentinel, &mut flags) != 0 {
                            break;
                        }
                        let event = Handle(CreateEventW(null(), 1, 0, null()));
                        assert!(!event.0.is_null());
                        held.push(event);
                    }
                    assert_ne!(
                        GetHandleInformation(sentinel, &mut flags),
                        0,
                        "reuse fixture did not occupy slot"
                    );
                }
                // Snapshot BEFORE opening the reference: OpenEventW itself may
                // otherwise occupy an absent sentinel slot and fake a leak.
                let mut candidate = null_mut();
                let present = DuplicateHandle(
                    GetCurrentProcess(),
                    sentinel,
                    GetCurrentProcess(),
                    &mut candidate,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                ) != 0;
                // A valid slot whose object refuses duplication (ERROR_NOT_SUPPORTED,
                // e.g. an EtwRegistration) is still occupied: compare it in place.
                // The sentinel is a duplicable event, so a leak takes the copy path.
                let in_place = !present
                    && GetLastError() == ERROR_NOT_SUPPORTED
                    && GetHandleInformation(sentinel, &mut 0) != 0;
                let candidate = present.then(|| Handle(candidate));
                let name = std::env::var("ARX_UNIT_EVENT").unwrap();
                let reference = Handle(OpenEventW(
                    SYNCHRONIZE,
                    0,
                    wide(std::ffi::OsStr::new(&name)).as_ptr(),
                ));
                assert!(!reference.0.is_null(), "sentinel reference unavailable");
                if in_place {
                    assert_ne!(reference.0, sentinel, "occupied slot vacated during check");
                }
                let slot = candidate.as_ref().map(|c| c.0);
                if let Some(slot) = slot.or(in_place.then_some(sentinel)) {
                    assert_eq!(
                        CompareObjectHandles(slot, reference.0),
                        0,
                        "unintended inherited sentinel object"
                    );
                    println!("ARX-HANDLE-REUSED-DISTINCT");
                }
                assert_eq!(GetFileType(GetStdHandle(STD_INPUT_HANDLE)), FILE_TYPE_CHAR);
                assert_eq!(GetFileType(GetStdHandle(STD_OUTPUT_HANDLE)), FILE_TYPE_PIPE);
                assert_eq!(GetFileType(GetStdHandle(STD_ERROR_HANDLE)), FILE_TYPE_PIPE);
            },
            _ => panic!("invalid fixture"),
        }
    }
    pub(crate) const CHILD: &str = "development::windows::tests::fixture_child";
    /// Workspace = PATH = a private temp copy of this unit-test binary named
    /// `cargo.exe`. An empty `action` lets callers choose it per request.
    pub(crate) fn fixture_settings(action: &str) -> (tempfile::TempDir, Settings) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::copy(std::env::current_exe().unwrap(), root.join("cargo.exe")).unwrap();
        assert!(
            crate::config::checked_path(&root).is_ok(),
            "fixture root path check"
        );
        assert!(pin(&root).is_ok(), "fixture root pin");
        assert!(pin(&root.join("cargo.exe")).is_ok(), "fixture exe pin");
        let mut environment = BTreeMap::from([("PATH".into(), root.to_str().unwrap().into())]);
        if !action.is_empty() {
            environment.insert("ARX_UNIT_ACTION".into(), action.into());
        }
        let settings = Settings {
            mode: Mode::CurrentUser,
            acknowledge: Acknowledge::Shared,
            workspace: root,
            environment,
            limits: Limits {
                memory_bytes: 512 * 1024 * 1024,
                pids: 16,
            },
        };
        (dir, settings)
    }
    fn fixture_execution(settings: Settings) -> Execution {
        Execution::policy_fixture(settings)
    }
    fn fixture(action: &str) -> (tempfile::TempDir, Execution, Request) {
        let (dir, settings) = fixture_settings(action);
        let r = Request {
            tool: "cargo".into(),
            args: vec!["--exact".into(), CHILD.into(), "--nocapture".into()],
            cwd: String::new(),
            timeout_s: 1,
            output_bytes: 8192,
        };
        (dir, fixture_execution(settings), r)
    }
    #[test]
    fn handle_slot_reuse_is_not_inheritance() {
        let (_dir, mut e, r) = fixture("handles");
        let _sentinel = sentinel_event(&mut e);
        e.settings
            .environment
            .insert("ARX_UNIT_REUSE".into(), "1".into());
        let result = run_ok(
            diag::Context::SlotReuse,
            0,
            &e,
            &r,
            &Cancellation::default(),
        );
        assert!(result.cleanup_confirmed && result.matches_current_user);
        assert_eq!(result.stop_reason, StopReason::Exited);
        assert_eq!(result.exit_code, Some(0), "stderr={}", result.stderr);
        assert!(
            result.stdout.contains("ARX-HANDLE-REUSED-DISTINCT"),
            "stdout={} stderr={}",
            result.stdout,
            result.stderr
        );
    }
    #[test]
    fn unduplicable_occupied_slot_is_compared_not_skipped() {
        let (_dir, mut e, r) = fixture("handles");
        let _sentinel = sentinel_event(&mut e);
        e.settings
            .environment
            .insert("ARX_UNIT_UNDUPLICABLE".into(), "1".into());
        let result = run_ok(
            diag::Context::Unduplicable,
            0,
            &e,
            &r,
            &Cancellation::default(),
        );
        assert!(result.cleanup_confirmed && result.matches_current_user);
        assert_eq!(result.stop_reason, StopReason::Exited);
        assert_eq!(result.exit_code, Some(0), "stderr={}", result.stderr);
        assert!(
            result.stdout.contains("ARX-HANDLE-REUSED-DISTINCT"),
            "stdout={} stderr={}",
            result.stdout,
            result.stderr
        );
    }
    fn sentinel_event(e: &mut Execution) -> Handle {
        let name = format!("Local\\Arxcovo-test-{:032x}", rand::random::<u128>());
        let sentinel = unsafe {
            Handle(CreateEventW(
                null(),
                1,
                0,
                wide(std::ffi::OsStr::new(&name)).as_ptr(),
            ))
        };
        assert!(!sentinel.0.is_null());
        unsafe {
            ok(SetHandleInformation(
                sentinel.0,
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            ))
            .unwrap();
        }
        e.settings.environment.insert("ARX_UNIT_EVENT".into(), name);
        e.settings
            .environment
            .insert("ARX_UNIT_HANDLE".into(), (sentinel.0 as usize).to_string());
        sentinel
    }
    #[test]
    fn actual_job_tree_timeout_cancel_shutdown_overflow_and_handles() {
        for action in ["tree", "overflow", "handles", "cancel", "shutdown"] {
            let (_dir, mut e, r) = fixture(if action == "cancel" || action == "shutdown" {
                "tree"
            } else {
                action
            });
            let cancel = Cancellation::default();
            let _sentinel = (action == "handles").then(|| sentinel_event(&mut e));
            if action == "cancel" {
                cancel.cancel();
            }
            if action == "shutdown" {
                e.shutdown.cancel();
            }
            let result = run_ok(
                match action {
                    "tree" => diag::Context::Tree,
                    "overflow" => diag::Context::Overflow,
                    "handles" => diag::Context::Handles,
                    "cancel" => diag::Context::Cancel,
                    "shutdown" => diag::Context::Shutdown,
                    _ => unreachable!(),
                },
                0,
                &e,
                &r,
                &cancel,
            );
            assert!(result.cleanup_confirmed);
            assert!(result.matches_current_user);
            let expected = match action {
                "overflow" => StopReason::OutputLimit,
                "cancel" => StopReason::Cancelled,
                "shutdown" => StopReason::Shutdown,
                "handles" => StopReason::Exited,
                _ => StopReason::Timeout,
            };
            assert_eq!(result.stop_reason, expected, "{action}");
            if action == "handles" {
                assert_eq!(
                    result.exit_code,
                    Some(0),
                    "stdout={} stderr={}",
                    result.stdout,
                    result.stderr
                );
            }
        }
    }
    #[test]
    fn production_preflight_matches_independent_elevation_query() {
        unsafe {
            let mut token = null_mut();
            assert_ne!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
                0
            );
            let token = Handle(token);
            let mut elevation: TOKEN_ELEVATION = zeroed();
            let mut n = 0;
            assert_ne!(
                GetTokenInformation(
                    token.0,
                    TokenElevation,
                    (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                    size_of_val(&elevation) as u32,
                    &mut n
                ),
                0
            );
            if elevation.TokenIsElevated != 0 {
                assert!(preflight().is_err());
            }
        }
    }
    #[test]
    fn child_token_mismatch_never_resumes_and_quarantines() {
        let (_dir, e, r) = fixture("touch");
        let touched = e.settings.workspace.join("touched");
        e.tamper_identity(true);
        assert!(matches!(
            run_inner(&e, &r, &Cancellation::default(), true),
            Err(Reason::IdentityMismatch)
        ));
        std::thread::sleep(Duration::from_millis(500));
        assert!(!touched.exists(), "suspended child must never run");
        assert!(e.quarantined.load(Ordering::Acquire));
        e.tamper_identity(false);
        assert!(matches!(
            e.work(Some(&r), &Cancellation::default()),
            Err(Reason::Quarantined)
        ));
        assert_eq!(e.capability()["reason"], "quarantined");
        // Control: the same settings without the fault run the child to completion.
        let fresh = fixture_execution(e.settings.clone());
        let done = run_ok(
            diag::Context::Fresh,
            0,
            &fresh,
            &r,
            &Cancellation::default(),
        );
        assert_eq!(done.exit_code, Some(0));
        assert!(touched.exists());
        assert!(!fresh.quarantined.load(Ordering::Acquire));
    }
    #[test]
    fn results_count_dropped_output_and_report_nonzero_exit() {
        let (_dir, e, mut r) = fixture("overflow");
        r.output_bytes = 100;
        let o = run_ok(diag::Context::Overflow, 0, &e, &r, &Cancellation::default());
        assert_eq!(o.stop_reason, StopReason::OutputLimit);
        assert_eq!(o.exit_code, None);
        assert!(o.stdout.len() <= 100 && o.stdout_dropped_bytes > 0);
        assert_eq!(o.stderr_dropped_bytes, 0);
        let (_dir, e, r) = fixture("fail");
        let o = run_ok(diag::Context::Fail, 0, &e, &r, &Cancellation::default());
        assert_eq!((o.stop_reason, o.exit_code), (StopReason::Exited, Some(7)));
        assert_eq!(outcome(&o), Outcome::Failed);
        let (_dir, e, r) = fixture("echo");
        let o = run_ok(diag::Context::Echo, 0, &e, &r, &Cancellation::default());
        assert!(o.stdout.contains("ARX-ECHO-MARKER <workspace>"));
        assert!(o.stdout.contains("<sid>") && !o.stdout.contains("S-1-5-21"));
        let root = e.settings.workspace.to_string_lossy().to_ascii_lowercase();
        assert!(!o.stdout.to_ascii_lowercase().contains(&root));
        assert!(o.stderr.contains("ARX-ERR-MARKER"));
        assert_eq!(outcome(&o), Outcome::Succeeded);
    }
    /// Ordinary unprivileged directory junction. cmd.exe is a fixed test tool
    /// addressed with backslashes only: cmd scans its whole command line for
    /// switches, so `System32/cmd.exe` was parsed as `/c md.exe ...`.
    fn junction(parent: &Path, name: &str, target: &str) {
        use std::os::windows::{fs::MetadataExt, process::CommandExt};
        assert!(
            name.bytes()
                .chain(target.bytes())
                .all(|b| b.is_ascii_alphanumeric())
        );
        let cmd = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("cmd.exe");
        let made = std::process::Command::new(cmd)
            .current_dir(parent)
            .raw_arg(format!("/d /c mklink /J {name} {target}"))
            .output()
            .unwrap()
            .status
            .success();
        assert!(made, "junction fixture creation failed");
        let attributes = std::fs::symlink_metadata(parent.join(name))
            .unwrap()
            .file_attributes();
        assert_ne!(attributes & FILE_ATTRIBUTE_REPARSE_POINT, 0);
    }
    #[test]
    fn junction_cwd_and_tool_refused_and_handles_do_not_leak() {
        let (dir, e, r) = fixture("echo");
        let root = e.settings.workspace.clone();
        std::fs::create_dir_all(root.join("outside").join("sub")).unwrap();
        // Control: the identical request in an ordinary directory runs and exits 0,
        // so each refusal below is caused by the reparse component alone.
        let mut good = r.clone();
        good.cwd = "outside\\sub".into();
        let ok = run_ok(diag::Context::Cwd, 0, &e, &good, &Cancellation::default());
        assert_eq!(
            (ok.stop_reason, ok.exit_code),
            (StopReason::Exited, Some(0))
        );
        junction(&root, "junction", "outside");
        junction(&root.join("outside"), "nested", "sub");
        for cwd in [
            "junction",
            "junction\\sub",
            "outside\\nested",
            "outside/nested",
        ] {
            let mut bad = r.clone();
            bad.cwd = cwd.into();
            assert!(bad.validate());
            assert!(matches!(
                run_inner(&e, &bad, &Cancellation::default(), true),
                Err(Reason::InvalidArguments)
            ));
        }
        assert!(pin(&root.join("junction")).is_err());
        // The handle backstop alone (no name-based pre-check) refuses the junction.
        assert!(open_pinned(&root.join("junction")).is_err());
        assert!(
            open_pinned(&root.join("outside")).is_ok(),
            "positive control"
        );
        // Executable reparse: the configured PATH directory is reached via a junction.
        let tools = root.join("tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::copy(root.join("cargo.exe"), tools.join("cargo.exe")).unwrap();
        junction(&root, "toollink", "tools");
        let mut via = fixture_execution(e.settings.clone());
        via.settings.environment.insert(
            "PATH".into(),
            root.join("toollink").to_str().unwrap().into(),
        );
        assert!(matches!(
            run_inner(&via, &r, &Cancellation::default(), true),
            Err(Reason::InvalidArguments)
        ));
        via.settings
            .environment
            .insert("PATH".into(), tools.to_str().unwrap().into());
        let direct = run_ok(diag::Context::Direct, 0, &via, &r, &Cancellation::default());
        assert_eq!(direct.exit_code, Some(0));
        // Pre-spawn refusals never quarantine: nothing was started.
        assert!(!e.quarantined.load(Ordering::Acquire));
        assert!(!via.quarantined.load(Ordering::Acquire));
        for name in ["junction", "toollink"] {
            std::fs::remove_dir(root.join(name)).unwrap();
        }
        std::fs::remove_dir(root.join("outside").join("nested")).unwrap();
        drop(dir);
        let mut before = 0;
        unsafe {
            assert_ne!(GetProcessHandleCount(GetCurrentProcess(), &mut before), 0);
        }
        for index in 0..8 {
            let (_dir, e, r) = fixture("overflow");
            assert!(
                run_ok(diag::Context::Leak, index, &e, &r, &Cancellation::default())
                    .cleanup_confirmed
            );
        }
        let mut after = 0;
        unsafe {
            assert_ne!(GetProcessHandleCount(GetCurrentProcess(), &mut after), 0);
        }
        assert!(
            after <= before + 4,
            "native handles leaked: {before} -> {after}"
        );
    }
}
