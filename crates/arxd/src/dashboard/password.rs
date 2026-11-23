use anyhow::{Result, ensure};
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::SaltString,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, IsTerminal, Write},
    path::Path,
};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const MEMORY_KIB: u32 = 64 * 1024;
const ITERATIONS: u32 = 3;
const PASSWORD_FILE: &str = "dashboard-owner.phc";
const LOCK_FILE: &str = "dashboard-owner.lock";

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
    let _lock = lock(dir)?;
    let input = std::io::stdin();
    let guard = input.is_terminal().then(no_echo).transpose()?;
    let password = read_password(input.lock());
    if let Some(guard) = guard {
        guard.restore()?;
        eprintln!();
    }
    write_password(dir, &password?)
}

fn read_password(input: impl BufRead) -> Result<Zeroizing<String>> {
    // A UTF-8 scalar needs at most four bytes, plus a possible CRLF.
    let mut bytes = Zeroizing::new(Vec::with_capacity(4099));
    input.take(4099).read_until(b'\n', &mut bytes)?;
    let raw = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
    let password = std::str::from_utf8(raw)?;
    ensure!(
        !password.is_empty() && password.chars().count() <= 1024,
        "invalid password length"
    );
    Ok(Zeroizing::new(password.into()))
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
    fn restore(&self) -> std::io::Result<()> {
        // SAFETY: the saved termios remains initialized.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &self.0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(windows)]
struct Echo(windows_sys::Win32::Foundation::HANDLE, u32);

#[cfg(windows)]
fn no_echo() -> std::io::Result<Echo> {
    use windows_sys::Win32::System::Console::*;
    let mut original = 0;
    // SAFETY: console standard input is a borrowed process handle.
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        if GetConsoleMode(handle, &mut original) == 0
            || SetConsoleMode(
                handle,
                original & !(ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT),
            ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Echo(handle, original))
    }
}

#[cfg(windows)]
impl Echo {
    fn restore(&self) -> std::io::Result<()> {
        use windows_sys::Win32::System::Console::SetConsoleMode;
        // SAFETY: the standard input handle is still borrowed from the process.
        if unsafe { SetConsoleMode(self.0, self.1) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
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
