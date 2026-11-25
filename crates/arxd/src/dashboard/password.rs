use anyhow::{Context, Result, bail, ensure};
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::SaltString,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, IsTerminal, Read, Write},
    path::Path,
};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const MEMORY_KIB: u32 = 64 * 1024;
const ITERATIONS: u32 = 3;
const PASSWORD_FILE: &str = "dashboard-owner.phc";
const LOCK_FILE: &str = "dashboard-owner.lock";
const MAX_PASSWORD_CHARS: usize = 1024;
// A UTF-8 scalar takes up to four bytes. Allow CRLF and one overflow byte.
const PASSWORD_READ_LIMIT: usize = 4 * MAX_PASSWORD_CHARS + 3;
const PROMPT_ATTEMPTS: usize = 3;
const CHOOSE_PROMPT: &str =
    "Choose the dashboard password for user 'owner' (1–1024 UTF-8 characters, input hidden): ";
const REPEAT_PROMPT: &str = "Repeat password (input hidden): ";
const PROMPT_IO_ERROR: &str = "Could not display the dashboard password prompt.";
const SUCCESS: &str = "Dashboard password saved for 'owner'. Sign in at the configured dashboard URL (http://localhost:<port> for local HTTP).";

fn parameters() -> Params {
    Params::new(MEMORY_KIB, ITERATIONS, 1, Some(32)).expect("fixed Argon2id parameters")
}

fn hash(password: &str, params: Params) -> Result<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| anyhow::anyhow!("randomness unavailable"))?;
    let salt = SaltString::encode_b64(&bytes).map_err(|_| anyhow::anyhow!("invalid salt"))?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password(password.as_bytes(), &salt)
        .map(|value| value.to_string())
        .map_err(|_| anyhow::anyhow!("password hashing unavailable"))
}

pub(super) struct Verifier(String);

impl Verifier {
    pub(super) fn load(dir: &Path) -> Result<(Self, File)> {
        let lock = lock(dir)?;
        let text = String::from_utf8(crate::local::private_file(&dir.join(PASSWORD_FILE))?)?;
        ensure!(text.len() <= 256, "invalid password hash");
        let phc = PasswordHash::new(&text).map_err(|_| anyhow::anyhow!("invalid password hash"))?;
        ensure!(
            phc.algorithm == argon2::ARGON2ID_IDENT
                && phc.version == Some(19)
                && Params::try_from(&phc).is_ok_and(|params| params == parameters())
                && phc.hash.is_some_and(|output| output.len() == 32)
                && phc.salt.is_some_and(|salt| salt.len() == 22),
            "invalid password hash"
        );
        Ok((Self(text), lock))
    }

    pub(super) fn accepts(&self, username: &str, password: &str) -> bool {
        // Do the same Argon2id verification for every admitted username.
        // password-hash compares the derived output in constant time.
        let matched = PasswordHash::new(&self.0).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        });
        let mut candidate = [0; 5];
        if username.len() == candidate.len() {
            candidate.copy_from_slice(username.as_bytes());
        }
        matched & bool::from(candidate.ct_eq(b"owner"))
    }

    #[cfg(test)]
    pub(super) fn cheap(password: &str) -> Self {
        Self(hash(password, Params::new(8, 1, 1, Some(32)).unwrap()).unwrap())
    }
}

fn check_private(path: &Path) -> Result<()> {
    crate::config::checked_path(path)?;
    crate::security::private(path)?;
    crate::local::owned(path)
}

/// Creates a new private file for reading and writing; it never opens an
/// existing one.
#[cfg(unix)]
fn create_new(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Creates a new file whose owner is the current token user from the
/// moment it exists. An elevated administrator token would otherwise default
/// new objects to the Administrators group, which the private-file owner
/// check refuses. The descriptor names only the owner, so the file still
/// inherits the private directory's DACL.
#[cfg(windows)]
fn create_new(path: &Path) -> std::io::Result<File> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows_sys::Win32::{
        Foundation::{INVALID_HANDLE_VALUE, LocalFree},
        Security::{Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, *},
        Storage::FileSystem::*,
    };
    let identity = crate::local::current_identity().map_err(std::io::Error::other)?;
    let sddl: Vec<u16> = format!("O:{identity}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: both strings are NUL-terminated and outlive the calls; the
    // descriptor is allocated by the conversion, used only while live and
    // freed once; a valid returned handle is owned by the new File.
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let handle = CreateFileW(
            wide.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        let error = std::io::Error::last_os_error();
        LocalFree(descriptor);
        if handle == INVALID_HANDLE_VALUE {
            return Err(error);
        }
        Ok(File::from_raw_handle(handle))
    }
}

fn lock(dir: &Path) -> Result<File> {
    check_private(dir)?;
    ensure!(dir.is_dir(), "private state directory required");
    let path = dir.join(LOCK_FILE);
    let file = match create_new(&path) {
        Ok(file) => {
            // Never leave a freshly created lock that later runs refuse.
            if let Err(error) = check_private(&path) {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            file
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            check_private(&path)?;
            ensure!(path.is_file(), "invalid password lock");
            OpenOptions::new().read(true).write(true).open(&path)?
        }
        Err(error) => return Err(error.into()),
    };
    check_private(&path)?;
    file.try_lock()?;
    Ok(file)
}

pub fn set_password(dir: &Path) -> Result<()> {
    let _lock = lock(dir).context(
        "Could not lock the dashboard password file. Check the private state directory and stop the dashboard daemon before retrying.",
    )?;
    let stdin = io::stdin();
    let terminal = stdin.is_terminal();
    let mut input = stdin.lock();
    let password = if terminal {
        let mut guard = no_echo().context("Could not hide input on the console.")?;
        let mode = if cfg!(windows) {
            InputMode::WindowsConsole { skip_lf: false }
        } else {
            InputMode::Terminal
        };
        let password = confirm_password(&mut input, &mut io::stderr().lock(), mode);
        guard
            .restore()
            .context("Could not restore the console input mode.")?;
        password?
    } else {
        read_password(&mut input, &mut InputMode::Piped)?
    };
    write_password(dir, &password).context("Could not save the dashboard password.")?;
    let _ = writeln!(io::stdout().lock(), "{SUCCESS}");
    Ok(())
}

enum InputMode {
    Piped,
    Terminal,
    WindowsConsole { skip_lf: bool },
}

fn confirm_password(
    input: &mut impl BufRead,
    output: &mut impl Write,
    mut mode: InputMode,
) -> Result<Zeroizing<String>> {
    for attempt in 1..=PROMPT_ATTEMPTS {
        let mut ask = |prompt| {
            write!(output, "{prompt}").context(PROMPT_IO_ERROR)?;
            output.flush().context(PROMPT_IO_ERROR)?;
            let password = read_password(input, &mut mode);
            writeln!(output).context(PROMPT_IO_ERROR)?;
            password
        };
        let password = ask(CHOOSE_PROMPT)?;
        let confirmation = ask(REPEAT_PROMPT)?;
        if password.as_bytes() == confirmation.as_bytes() {
            return Ok(password);
        }
        if attempt < PROMPT_ATTEMPTS {
            writeln!(output, "Passwords do not match. Try again.").context(PROMPT_IO_ERROR)?;
        }
    }
    bail!("Passwords did not match after {PROMPT_ATTEMPTS} attempts.")
}

fn read_password(input: &mut impl BufRead, mode: &mut InputMode) -> Result<Zeroizing<String>> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(PASSWORD_READ_LIMIT));
    let mut input = input.take(PASSWORD_READ_LIMIT as u64);
    match mode {
        InputMode::WindowsConsole { skip_lf } => read_windows_line(&mut input, &mut bytes, skip_lf),
        _ => input.read_until(b'\n', &mut bytes).map(|_| ()),
    }
    .context("Could not read the password from standard input.")?;
    if bytes.is_empty() {
        match mode {
            InputMode::Piped => bail!(
                "No password received. Use a console or supply one UTF-8 line on standard input."
            ),
            _ => bail!("Password entry ended before a line was received."),
        }
    }
    let raw = strip_terminator(&bytes);
    ensure!(
        raw.len() <= 4 * MAX_PASSWORD_CHARS,
        "Password is too long. Use at most {MAX_PASSWORD_CHARS} UTF-8 characters."
    );
    let password = std::str::from_utf8(raw).context("Password must be valid UTF-8.")?;
    ensure!(
        !password.is_empty(),
        "Password is too short. Enter at least one character."
    );
    ensure!(
        password.chars().count() <= MAX_PASSWORD_CHARS,
        "Password is too long. Use at most {MAX_PASSWORD_CHARS} UTF-8 characters."
    );
    Ok(Zeroizing::new(password.into()))
}

fn strip_terminator(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .or_else(|| bytes.strip_suffix(b"\r"))
        .unwrap_or(bytes)
}

fn windows_line_end(bytes: &[u8]) -> Option<usize> {
    let index = bytes
        .iter()
        .position(|byte| matches!(byte, b'\r' | b'\n'))?;
    Some(
        index
            + if bytes[index..].starts_with(b"\r\n") {
                2
            } else {
                1
            },
    )
}

fn read_windows_line(
    input: &mut impl BufRead,
    bytes: &mut Vec<u8>,
    skip_lf: &mut bool,
) -> io::Result<()> {
    // A bare CR ends the read immediately. If CRLF straddles a buffer boundary,
    // discard its LF on the next line read, without waiting for it after CR.
    if std::mem::take(skip_lf) && input.fill_buf()?.starts_with(b"\n") {
        input.consume(1);
    }
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(());
        }
        if let Some(end) = windows_line_end(chunk) {
            bytes.extend_from_slice(&chunk[..end]);
            *skip_lf = end == chunk.len() && chunk.ends_with(b"\r");
            input.consume(end);
            return Ok(());
        }
        bytes.extend_from_slice(chunk);
        let len = chunk.len();
        input.consume(len);
    }
}

fn write_password(dir: &Path, password: &str) -> Result<()> {
    let value = hash(password, parameters())?;
    let path = dir.join(PASSWORD_FILE);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            check_private(&path)?;
            ensure!(metadata.is_file(), "invalid password file");
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temp = dir.join(format!("dashboard-password-{}.tmp", super::random_token()?));
    let mut file = create_new(&temp)?;
    let write = (|| {
        check_private(&temp)?;
        file.write_all(value.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &path)?;
        #[cfg(unix)]
        File::open(dir)?.sync_all()?;
        Ok(())
    })();
    if write.is_err() {
        let _ = fs::remove_file(temp);
    }
    write
}

#[cfg(unix)]
struct Echo(libc::termios);

#[cfg(unix)]
fn no_echo() -> std::io::Result<Echo> {
    let mut mode = std::mem::MaybeUninit::uninit();
    // SAFETY: stdin is checked as a terminal and mode has writable storage.
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, mode.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let original = unsafe { mode.assume_init() };
    let mut hidden = original;
    hidden.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ISIG);
    // SAFETY: the termios structure is initialized and stdin is borrowed.
    if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &hidden) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Echo(original))
}

#[cfg(unix)]
impl Echo {
    fn restore(&mut self) -> std::io::Result<()> {
        // SAFETY: the saved termios remains initialized.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &self.0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(windows)]
use windows_console::{Echo, no_echo};

#[cfg(windows)]
mod windows_console {
    use std::{io, sync::Mutex};
    use windows_sys::Win32::{Foundation::HANDLE, System::Console::*};

    pub(super) struct Echo(Option<HANDLE>);
    static SAVED_MODE: Mutex<Option<u32>> = Mutex::new(None);

    fn prompt_modes(original: u32) -> (u32, u32) {
        // Keep processed input for Ctrl+C and keep the console's line editing.
        (original & !ENABLE_ECHO_INPUT, original)
    }

    unsafe extern "system" fn restore_on_interrupt(event: u32) -> i32 {
        // Windows terminates the process on a separate thread for Ctrl+C/Break,
        // so Drop cannot restore the input mode in that case.
        if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT)
            && let Ok(saved) = SAVED_MODE.lock()
            && let Some(original) = *saved
        {
            // SAFETY: the CLI leaves its borrowed stdin handle in place.
            unsafe { SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), original) };
        }
        // Continue dispatch so the default cancellation behavior still runs.
        0
    }

    pub(super) fn no_echo() -> io::Result<Echo> {
        let mut saved = SAVED_MODE
            .lock()
            .map_err(|_| io::Error::other("console mode lock poisoned"))?;
        if saved.is_some() {
            return Err(io::Error::other("console input is already hidden"));
        }
        // SAFETY: stdin is borrowed, original has writable storage and the
        // handler can only access the saved mode while holding the same lock.
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            let mut original = 0;
            if GetConsoleMode(handle, &mut original) == 0
                || SetConsoleCtrlHandler(Some(restore_on_interrupt), 1) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let (hidden, restore) = prompt_modes(original);
            if SetConsoleMode(handle, hidden) == 0 {
                let error = io::Error::last_os_error();
                SetConsoleCtrlHandler(Some(restore_on_interrupt), 0);
                return Err(error);
            }
            *saved = Some(restore);
            Ok(Echo(Some(handle)))
        }
    }

    impl Echo {
        pub(super) fn restore(&mut self) -> io::Result<()> {
            if let Some(handle) = self.0 {
                let mut saved = SAVED_MODE
                    .lock()
                    .map_err(|_| io::Error::other("console mode lock poisoned"))?;
                let original = saved.ok_or_else(|| io::Error::other("missing console mode"))?;
                // SAFETY: the saved stdin handle is still borrowed from the process.
                unsafe {
                    if SetConsoleMode(handle, original) == 0 {
                        return Err(io::Error::last_os_error());
                    }
                    *saved = None;
                    self.0 = None;
                    SetConsoleCtrlHandler(Some(restore_on_interrupt), 0);
                }
            }
            Ok(())
        }
    }

    #[test]
    fn windows_modes_preserve_input_flags_and_restore_the_original() {
        let flags = ENABLE_PROCESSED_INPUT | ENABLE_LINE_INPUT | ENABLE_MOUSE_INPUT;
        assert_eq!(
            prompt_modes(flags | ENABLE_ECHO_INPUT),
            (flags, flags | ENABLE_ECHO_INPUT)
        );
        assert_eq!(
            prompt_modes(ENABLE_MOUSE_INPUT),
            (ENABLE_MOUSE_INPUT, ENABLE_MOUSE_INPUT)
        );
    }
}

impl Drop for Echo {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_terminator_is_stripped_and_interior_cr_is_preserved() {
        for (line, password) in [
            (b"abc\r".as_slice(), b"abc".as_slice()),
            (b"abc\n", b"abc"),
            (b"abc\r\n", b"abc"),
            (b"\r\n", b""),
            (b"abc\r\r\n", b"abc\r"),
            (b"a\rbc\n", b"a\rbc"),
            (b"abc", b"abc"),
        ] {
            assert_eq!(strip_terminator(line), password);
        }
    }

    #[test]
    fn windows_console_finds_the_first_terminator() {
        assert_eq!(windows_line_end(b"abc\r"), Some(4));
        assert_eq!(windows_line_end(b"abc\n"), Some(4));
        assert_eq!(windows_line_end(b"abc\r\nnext"), Some(5));
        assert_eq!(windows_line_end(b"\r\n"), Some(2));
        assert_eq!(windows_line_end(b"abc\rnext\n"), Some(4));
        assert_eq!(windows_line_end(b"abc"), None);
    }

    #[test]
    fn confirmation_retries_a_mismatch_and_accepts_a_match() {
        // Force CR and LF into different reads for the first line.
        let mut input = io::BufReader::with_capacity(4, b"one\r\ntwo\rthree\rthree\r".as_slice());
        let mut output = Vec::new();
        let password = confirm_password(
            &mut input,
            &mut output,
            InputMode::WindowsConsole { skip_lf: false },
        )
        .unwrap();
        assert_eq!(password.as_str(), "three");
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!(
                "{CHOOSE_PROMPT}\n{REPEAT_PROMPT}\nPasswords do not match. Try again.\n{CHOOSE_PROMPT}\n{REPEAT_PROMPT}\n"
            )
        );
    }

    #[test]
    fn confirmation_fails_after_three_mismatches() {
        let mut input = b"one\ntwo\none\ntwo\none\ntwo\nunused\n".as_slice();
        let mut output = Vec::new();
        let error = confirm_password(&mut input, &mut output, InputMode::Terminal).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Passwords did not match after 3 attempts."
        );
        assert_eq!(input, b"unused\n");
    }

    #[test]
    fn password_validation_reports_safe_messages() {
        for (input, message) in [
            (
                b"\n".to_vec(),
                "Password is too short. Enter at least one character.",
            ),
            (
                vec![b'x'; 1025],
                "Password is too long. Use at most 1024 UTF-8 characters.",
            ),
            (vec![0xff], "Password must be valid UTF-8."),
            (
                vec![],
                "No password received. Use a console or supply one UTF-8 line on standard input.",
            ),
        ] {
            let error = read_password(&mut input.as_slice(), &mut InputMode::Piped).unwrap_err();
            assert_eq!(error.to_string(), message);
        }
        struct BrokenStdin;
        impl Read for BrokenStdin {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("must not expose the inner error"))
            }
        }
        let mut unreadable = io::BufReader::new(BrokenStdin);
        let error = read_password(&mut unreadable, &mut InputMode::Piped).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Could not read the password from standard input."
        );
    }

    #[test]
    fn piping_keeps_an_interior_cr_and_reads_only_the_first_line() {
        let mut input = b"a\rb\r\nunused\n".as_slice();
        assert_eq!(
            read_password(&mut input, &mut InputMode::Piped)
                .unwrap()
                .as_str(),
            "a\rb"
        );
        assert_eq!(input, b"unused\n");
    }

    #[test]
    fn new_private_files_are_owned_at_creation_and_never_reopened() {
        let dir = crate::service::application::tests::private_temp();
        let path = dir.path().join("probe");
        let mut file = create_new(&path).unwrap();
        // On an elevated Windows token this fails unless the owner was set
        // to the token user when the file was created.
        check_private(&path).unwrap();
        file.write_all(b"first").unwrap();
        drop(file);
        assert_eq!(
            create_new(&path).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&path).unwrap(), b"first");
    }
}
