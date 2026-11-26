#![cfg(target_env = "msvc")]

use super::*;
use crate::vm::qmp::{ChannelError, Qmp};
use tokio::io::{AsyncBufReadExt, BufReader};
use windows_sys::Win32::System::{
    Console::{GetConsoleCP, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
    StationsAndDesktops::GetProcessWindowStation,
};

const CHILD: &str = "vm::windows::tests::qmp_stdio_fixture::child";
const GREETING: &[u8] =
    b"{\"QMP\":{\"version\":{\"qemu\":{\"major\":11,\"minor\":1,\"micro\":3},\"package\":\"\"},\"capabilities\":[]}}\r\n";
const RELEASE: &[u8] = b"!";
const CAPABILITIES: &[u8] = b"{\"execute\":\"qmp_capabilities\",\"id\":1}\r\n";
const ACK: &[u8] = b"{\"return\":{},\"id\":1}\r\n";
const STDERR: &[u8] = b"fixture stderr marker";
const LIMIT: Duration = Duration::from_secs(5);

fn raw_read(handle: HANDLE, bytes: &mut [u8]) -> io::Result<usize> {
    let mut count = 0;
    // These are synchronous fixture handles; the buffer lives until return.
    succeeded(unsafe {
        ReadFile(
            handle,
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            &mut count,
            null_mut(),
        )
    })?;
    Ok(count as usize)
}

fn raw_write(handle: HANDLE, bytes: &[u8]) {
    let mut count = 0;
    succeeded(unsafe {
        WriteFile(
            handle,
            bytes.as_ptr(),
            bytes.len() as u32,
            &mut count,
            null_mut(),
        )
    })
    .unwrap();
    assert_eq!(count as usize, bytes.len());
}

fn expect_read(handle: HANDLE, expected: &[u8]) {
    let mut bytes = vec![0; expected.len()];
    let mut offset = 0;
    while offset < bytes.len() {
        let count = raw_read(handle, &mut bytes[offset..]).unwrap();
        assert_ne!(count, 0);
        offset += count;
    }
    assert_eq!(bytes, expected);
}

#[test]
#[ignore = "child-only fixture; launched suspended under sealed stdio"]
fn child() {
    let mode = std::env::var("ARX_QMP_STDIO_FIXTURE").unwrap();
    assert_eq!(unsafe { GetConsoleCP() }, 0);
    let [input, output, error] = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
        .map(|id| unsafe { GetStdHandle(id) });
    for handle in [input, output, error] {
        assert_eq!(unsafe { GetFileType(handle) }, FILE_TYPE_PIPE);
    }
    raw_write(error, STDERR);
    match mode.as_str() {
        "greet" | "zero" => {
            if mode == "zero" {
                raw_write(output, &[]);
            }
            raw_write(output, GREETING);
            expect_read(input, CAPABILITIES);
            raw_write(output, ACK);
        }
        "close" => succeeded(unsafe { CloseHandle(output) }).unwrap(),
        "silent" => {}
        _ => panic!("unknown fixture mode"),
    }
    expect_read(input, RELEASE);
    // Do not let libtest write a footer after the deliberate stdout close.
    unsafe { ExitProcess(0) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConsoleProbe {
    Detached,
    Legacy,
    Interactive,
    Bcrypt,
}

impl ConsoleProbe {
    fn refuses_start(self) -> bool {
        self != Self::Detached
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ReadResult {
    Bytes(usize),
    Error(i32),
}

struct ObservedPipe {
    pipe: OwnedHandle,
    results: std::sync::Arc<std::sync::Mutex<Vec<ReadResult>>>,
}

impl Read for ObservedPipe {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let read = raw_read(self.pipe.as_raw_handle(), bytes);
        let result = match &read {
            Ok(count) => ReadResult::Bytes(*count),
            Err(error) => ReadResult::Error(error.raw_os_error().unwrap()),
        };
        let mut results = self.results.lock().unwrap();
        if results.len() < 16 {
            results.push(result);
        }
        read
    }
}

fn inherited(process: &OwnedHandle, expected: HANDLE) -> Result<bool, i32> {
    let mut duplicate = null_mut();
    // Windows preserves numeric handle values when it inherits handles. This
    // duplicate is only a temporary proof and is closed before the first read.
    let duplicate = unsafe {
        let parent = GetCurrentProcess();
        succeeded(DuplicateHandle(
            process.as_raw_handle(),
            expected,
            parent,
            &mut duplicate,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        ))
        .and_then(|()| owned(duplicate))
    }
    .map_err(|error| error.raw_os_error().unwrap())?;
    Ok(unsafe { CompareObjectHandles(duplicate.as_raw_handle(), expected) } != 0)
}
struct ConcurrentSync<'a> {
    before_create: &'a std::sync::Barrier,
    before_resume: &'a std::sync::Barrier,
}

async fn scenario(
    mode: &str,
    token: Option<&VmToken>,
    console: ConsoleProbe,
    peer: Option<&Path>,
    probe_bundle: bool,
    concurrent: Option<ConcurrentSync<'_>>,
) -> Option<Vec<u16>> {
    let restricted = token.is_some();
    let own_image = std::env::current_exe().unwrap();
    let image = peer.unwrap_or(&own_image);
    let local = LocalPath::parse(image.to_str().unwrap()).unwrap();
    let args = if peer.is_some() {
        Vec::new()
    } else {
        ["--exact", CHILD, "--ignored", "--nocapture", "--quiet"]
            .map(str::to_owned)
            .to_vec()
    };
    let mut environment = Vec::new();
    if probe_bundle {
        environment.extend(wide("ARX_QMP_FIXTURE_PROBE_BUNDLE=1"));
    }
    environment.extend(wide(format!("ARX_QMP_STDIO_FIXTURE={mode}")));
    let (_scratch, root) = fixture_root();
    let loader = LocalPath::parse(image.parent().unwrap().to_str().unwrap()).unwrap();
    environment.extend(launch_environment(&loader, &LocalPath::parse(&root).unwrap()).unwrap());
    let (child_io, parent_io) = match token {
        Some(token) => token.stdio().unwrap(),
        None => fixture_stdio(),
    };
    let Stdio {
        input,
        output,
        error,
    } = parent_io;
    let job = Job::with_process_limit(512 << 20, None, Some(1)).unwrap();
    let launch = if let Some(token) =
        token.filter(|_| matches!(console, ConsoleProbe::Detached | ConsoleProbe::Bcrypt))
    {
        Launch::Paused {
            token,
            image: &local,
            args: &args,
        }
    } else {
        Launch::ContainedFixture {
            image,
            args: &args,
            legacy_console: console == ConsoleProbe::Legacy,
            token,
            interactive: console == ConsoleProbe::Interactive,
        }
    };
    if let Some(sync) = &concurrent {
        // Align both OS threads immediately before create_suspended so their
        // LaunchDesktop::new calls genuinely contend on the daemon-wide mutex.
        sync.before_create.wait();
    }
    let (process, thread, desktop) = create_suspended(
        &launch,
        image.parent().unwrap(),
        &environment,
        Assignment::Job(&job),
        &child_io,
    )
    .unwrap_or_else(|failure| panic!("fixture launch restricted={restricted}: {failure:?}"));
    let child_handles = child_io.handles().map(|handle| inherited(&process, handle));
    let desktop_name = desktop.as_ref().map(|lease| lease.path.clone());
    let exists = desktop.as_ref().map(|lease| lease.closure_probe());
    if let Some(exists) = &exists {
        assert!(exists(), "held desktop missing before resume");
    }
    let mut child = Child {
        job,
        process,
        suspended: Some(thread),
        _desktop: desktop,
    };
    let pid = child.verify_live().unwrap();
    assert_eq!(child.job.active().unwrap(), 1);
    if let Some(token) = token {
        verify_child_policy(&child, token).unwrap();
    }
    assert_eq!(child_handles, [Ok(true); 3]);
    drop(child_io);
    println!("restricted={restricted} mode={mode} inherited={child_handles:?}");
    let results = std::sync::Arc::default();
    let cancel = Shutdown::default();
    let (reader, worker) = start_output(
        ObservedPipe {
            pipe: output,
            results: std::sync::Arc::clone(&results),
        },
        cancel.clone(),
        tokio::runtime::Handle::current(),
    )
    .unwrap();
    let workers = WorkerSet {
        cancel,
        workers: vec![worker],
    };
    let mut reader = BufReader::new(reader);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), reader.fill_buf())
            .await
            .is_err()
    );
    if let Some(sync) = &concurrent {
        sync.before_resume.wait();
    }
    child
        .resume_checked(&Shutdown::default(), &mut || Ok(std::time::Instant::now()))
        .unwrap();

    if console.refuses_start() {
        let eof = tokio::time::timeout(LIMIT, reader.read_u8())
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(eof.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(
            results.lock().unwrap().first(),
            Some(&ReadResult::Error(ERROR_BROKEN_PIPE as i32))
        );
        let deadline = tokio::time::Instant::now() + LIMIT;
        child.reap(deadline).await.unwrap();
        assert_eq!(child.exit_code(), Ok(0xC000_0142)); // STATUS_DLL_INIT_FAILED
        workers.join(deadline).await.unwrap();
        drop(child);
        if let Some(exists) = exists {
            assert!(!exists(), "desktop survived refused startup");
        }
        return None;
    }

    // Libtest starts with a blank line and test count. The observer samples
    // from the first real ReadFile; the QMP assertions begin after the preamble.
    if peer.is_none() {
        let mut line = String::new();
        tokio::time::timeout(LIMIT, async {
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim(), "");
            line.clear();
            let count = reader.read_line(&mut line).await.unwrap();
            assert_ne!(
                count,
                0,
                "startup EOF; restricted={restricted} mode={mode} raw={:?} live={} status={:?}",
                results.lock().unwrap(),
                child.verify_live().is_ok(),
                child.exit_code(),
            );
            assert_eq!(line.trim(), "running 1 test");
        })
        .await
        .unwrap();
    }
    let (mut stderr, stderr_worker) = start_output(
        File::from(error),
        workers.cancel.clone(),
        tokio::runtime::Handle::current(),
    )
    .unwrap();
    let mut workers = workers;
    workers.workers.push(stderr_worker);
    let mut marker = [0; STDERR.len()];
    let error_marker = tokio::time::timeout(LIMIT, stderr.read_exact(&mut marker))
        .await
        .unwrap();
    assert!(
        error_marker.is_ok(),
        "fixture missed stderr marker; restricted={restricted} mode={mode} raw={:?} live={} exited={} exit_code={:?}",
        results.lock().unwrap(),
        child.verify_live().is_ok(),
        child.exited().unwrap(),
        child.exit_code()
    );
    assert_eq!(marker.as_slice(), STDERR);

    let input_handle = input.as_raw_handle();
    match mode {
        "greet" | "zero" => {
            let mut greeting = vec![0; GREETING.len()];
            let greeting_read = tokio::time::timeout(LIMIT, reader.read_exact(&mut greeting)).await;
            if let Err(failure) = greeting_read.unwrap() {
                panic!(
                    "fixture greeting failed ({mode}): {failure:?}; raw={:?}",
                    results.lock().unwrap()
                );
            }
            assert_eq!(greeting, GREETING);
            raw_write(input_handle, CAPABILITIES);
            let mut ack = vec![0; ACK.len()];
            tokio::time::timeout(LIMIT, reader.read_exact(&mut ack))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(ack, ACK);
        }
        "close" => {
            let (writer, mut received) = tokio::io::duplex(64);
            let mut observed = None;
            let error = tokio::time::timeout(
                LIMIT,
                Qmp::handshake_diagnosed(
                    &mut reader,
                    writer,
                    &Shutdown::default(),
                    || panic!("capabilities reached"),
                    |check| observed = Some(check),
                ),
            )
            .await
            .unwrap()
            .err();
            assert_eq!(error, Some(ChannelError::Eof));
            assert!(matches!(
                observed,
                Some(crate::vm::qualification_record::PausedCheck::QmpChannel {
                    step: crate::vm::qualification_record::PausedQmpStep::Greeting,
                    error: ChannelError::Eof,
                })
            ));
            assert_eq!(
                received.read_u8().await.unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
            assert_eq!(
                results.lock().unwrap().last(),
                Some(&ReadResult::Error(ERROR_BROKEN_PIPE as i32))
            );
        }
        "silent" => {
            assert!(
                tokio::time::timeout(Duration::from_millis(50), reader.fill_buf())
                    .await
                    .is_err()
            );
        }
        _ => unreachable!(),
    }
    assert_eq!(child.verify_live().unwrap(), pid);
    {
        let reads = results.lock().unwrap();
        match mode {
            "close" => assert_eq!(
                reads.first(),
                Some(&ReadResult::Error(ERROR_BROKEN_PIPE as i32))
            ),
            "silent" => assert!(reads.is_empty()),
            _ => assert!(matches!(reads.first(), Some(ReadResult::Bytes(1..)))),
        }
        if restricted {
            println!("mode={mode} observed_reads={reads:?}");
        }
    }
    raw_write(input_handle, RELEASE);
    let deadline = tokio::time::Instant::now() + LIMIT;
    child.reap(deadline).await.unwrap();
    assert_eq!(child.exit_code(), Ok(0));
    workers.join(deadline).await.unwrap();
    drop(child);
    if let Some(exists) = exists {
        assert!(!exists(), "desktop survived Job and channel settlement");
    }
    desktop_name
}

#[cfg(target_env = "msvc")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Peer {
    Kernel32,
    User32,
    Bcrypt,
    Qemu,
}

#[cfg(target_env = "msvc")]
impl Peer {
    fn define(self) -> &'static str {
        match self {
            Self::Kernel32 => "/DFIXTURE_KERNEL32",
            Self::User32 => "/DFIXTURE_USER32",
            Self::Bcrypt => "/DFIXTURE_BCRYPT",
            Self::Qemu => "/DFIXTURE_QEMU_SHAPE",
        }
    }

    fn link(self) -> &'static [&'static str] {
        match self {
            Self::Kernel32 => &[],
            Self::User32 => &["user32.lib"],
            Self::Bcrypt => &["bcrypt.lib"],
            Self::Qemu => &[
                "user32.lib",
                "advapi32.lib",
                "ole32.lib",
                "winmm.lib",
                "ws2_32.lib",
                "shell32.lib",
                "shlwapi.lib",
                "dnsapi.lib",
                "iphlpapi.lib",
            ],
        }
    }
}

#[cfg(target_env = "msvc")]
fn build_peer(dir: &Path, peer: Peer) -> std::path::PathBuf {
    let tool = |name| {
        cc::windows_registry::find_tool("x86_64-pc-windows-msvc", name)
            .expect("QMP fixture needs the MSVC tools used to build arxd")
            .to_command()
    };
    let source = dir.join("qmp.c");
    std::fs::write(&source, include_bytes!("bare_qmp_fixture.c")).unwrap();
    let image = dir.join("qmp.exe");
    let object = dir.join("qmp.obj");
    let mut compiler = tool("cl.exe");
    compiler
        .current_dir(dir)
        .args(["/nologo", "/W4", "/WX", "/c", peer.define()]);
    if peer == Peer::Qemu {
        compiler.args(["/MD", "/wd4191"]); // Win32 GetProcAddress cast.
    } else {
        compiler.arg("/GS-"); // Deliberately no CRT.
    }
    compiler
        .arg(format!("/Fo{}", object.display()))
        .arg(&source);
    assert!(
        compiler.output().unwrap().status.success(),
        "failed to compile QMP test peer"
    );
    let mut linker = tool("link.exe");
    linker
        .current_dir(dir)
        .args(["/nologo", "/subsystem:console"]);
    if peer != Peer::Qemu {
        linker.args(["/nodefaultlib", "/entry:fixture_main"]);
    }
    linker
        .arg(format!("/out:{}", image.display()))
        .arg(&object)
        .arg("kernel32.lib")
        .args(peer.link());
    assert!(
        linker.output().unwrap().status.success(),
        "failed to link QMP test peer"
    );
    image
}

#[cfg(target_env = "msvc")]
fn copy_fixture_dlls(dir: &Path) -> bool {
    let Some(source) = std::env::var_os("ARX_QMP_FIXTURE_BUNDLE").map(std::path::PathBuf::from)
    else {
        return false;
    };
    assert!(source.is_absolute());
    for name in [
        "libbz2-1.dll",
        "libffi-8.dll",
        "libgio-2.0-0.dll",
        "libglib-2.0-0.dll",
        "libgmodule-2.0-0.dll",
        "libgobject-2.0-0.dll",
        "libiconv-2.dll",
        "libintl-8.dll",
        "libncursesw6.dll",
        "libpcre2-8-0.dll",
        "libpixman-1-0.dll",
        "libslirp-0.dll",
        "libwinpthread-1.dll",
        "libz-1.dll",
        "libzstd.dll",
        "zlib1.dll",
    ] {
        let file = source.join(name);
        let metadata = std::fs::symlink_metadata(&file).expect("fixture DLL missing");
        assert!(
            metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
        );
        std::fs::copy(file, dir.join(name)).expect("failed to copy fixture DLL");
    }
    true
}

#[cfg(target_env = "msvc")]
#[tokio::test]
async fn restricted_launch_stdout_discriminates_greeting_close_and_null_write() {
    // The unrestricted fixture also runs on hosted runners without an owner token.
    scenario("greet", None, ConsoleProbe::Detached, None, false, None).await;
    let (_, sid) = current_user();
    let account = LocalSid::parse(&sid).unwrap();
    let token = match VmToken::derive_current_user(account) {
        Ok(token) => token,
        Err(error) if std::env::var_os("ARX_REQUIRE_RESTRICTED_STDIO_FIXTURE").is_none() => {
            println!("SKIP restricted stdio diagnostic: {error}");
            return;
        }
        Err(error) => panic!("restricted stdio diagnostic requires the owner login: {error}"),
    };
    // Inspect an existing logon station without creating one ahead of the
    // launcher; if it already exists, hold it across every launch and assert
    // its owner and DACL never change.
    let mut station_witness = desktop::StationWitness::open_existing(&token).unwrap();
    let mut station_before = station_witness.as_ref().map(|witness| {
        let (name, flags) = witness.identity().unwrap();
        assert_eq!(flags & 1, 0, "logon station is visible"); // WSF_VISIBLE
        assert!(!name.eq_ignore_ascii_case("WinSta0"));
        (name, witness.security().unwrap())
    });
    let (root, _) = fixture_root();
    let image = build_peer(root.path(), Peer::Qemu);
    let bundled = copy_fixture_dlls(root.path());
    for control in [ConsoleProbe::Legacy, ConsoleProbe::Interactive] {
        scenario("greet", Some(&token), control, Some(&image), bundled, None).await;
    }
    for mode in ["greet", "close", "silent", "zero"] {
        scenario(
            mode,
            Some(&token),
            ConsoleProbe::Detached,
            Some(&image),
            bundled,
            None,
        )
        .await;
        if station_witness.is_none() {
            let witness = desktop::StationWitness::open_existing(&token)
                .unwrap()
                .expect("launcher did not create logon station");
            let (name, flags) = witness.identity().unwrap();
            assert_eq!(flags & 1, 0, "logon station is visible"); // WSF_VISIBLE
            assert!(!name.eq_ignore_ascii_case("WinSta0"));
            station_before = Some((name, witness.security().unwrap()));
            station_witness = Some(witness);
        }
    }
    let (station_name, station_security) =
        station_before.expect("verified logon station witness must exist after launch");

    let station = {
        let _lock = desktop::host_station().unwrap();
        unsafe { GetProcessWindowStation() }
    };
    let before_create = std::sync::Barrier::new(2);
    let before_resume = std::sync::Barrier::new(2);
    let (first, second) = std::thread::scope(|scope| {
        let run = || {
            scope.spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(scenario(
                    "greet",
                    Some(&token),
                    ConsoleProbe::Detached,
                    Some(&image),
                    bundled,
                    Some(ConcurrentSync {
                        before_create: &before_create,
                        before_resume: &before_resume,
                    }),
                ))
            })
        };
        let a = run();
        let b = run();
        (a.join().unwrap(), b.join().unwrap())
    });
    assert!(
        first.is_some() && second.is_some() && first != second,
        "concurrent children shared a desktop"
    );
    {
        let _lock = desktop::host_station().unwrap();
        assert!(
            unsafe { GetProcessWindowStation() } == station,
            "launcher station remained switched"
        );
    }
    let station_prefix = format!("{station_name}\\");
    for path in [&first, &second] {
        let path =
            String::from_utf16(path.as_deref().unwrap().strip_suffix(&[0]).unwrap()).unwrap();
        assert!(
            path.starts_with(&station_prefix) && path.len() > station_prefix.len(),
            "child desktop is not on the verified logon station"
        );
    }
    for (peer, case) in [
        (Peer::Kernel32, ConsoleProbe::Detached),
        (Peer::User32, ConsoleProbe::Detached),
        (Peer::Bcrypt, ConsoleProbe::Bcrypt),
    ] {
        let (root, _) = fixture_root();
        let image = build_peer(root.path(), peer);
        scenario("greet", Some(&token), case, Some(&image), false, None).await;
    }
    assert_eq!(
        station_witness.unwrap().security().unwrap(),
        station_security,
        "launcher changed the shared logon station's owner or DACL"
    );
}
