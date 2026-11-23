use anyhow::{Result, ensure};
use axum::body::Bytes;
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::Read,
    path::Path,
};

const FILE_LIMIT: u64 = 8 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 32 * 1024 * 1024;
const ENTRY_LIMIT: usize = 512;
const FILE_COUNT: usize = 256;
const DEPTH: usize = 8;

#[derive(Default)]
pub(super) struct Assets(HashMap<String, Asset>);

pub(super) struct Asset {
    pub content: Bytes,
    pub mime: &'static str,
}

#[derive(Default)]
struct Budget {
    entries: usize,
    bytes: u64,
}

impl Assets {
    pub(super) fn load(root: &Path) -> Result<Self> {
        crate::config::checked_path(root)?;
        ensure!(root.is_dir(), "dashboard assets directory required");
        let mut pins = Vec::new();
        for ancestor in root.ancestors().collect::<Vec<_>>().into_iter().rev() {
            let pin = match (pins.last(), ancestor.file_name()) {
                (Some(parent), Some(name)) => child(parent, ancestor, name)?,
                _ => options().open(ancestor)?,
            };
            check(&pin, true)?;
            pins.push(pin);
        }
        let mut assets = Self::default();
        assets.walk(
            root,
            pins.last().expect("absolute root"),
            "",
            0,
            &mut Budget::default(),
        )?;
        for (path, pin) in root.ancestors().zip(pins.iter().rev()) {
            same_path(path, pin, true)?;
        }
        ensure!(
            assets.0.contains_key("/index.html"),
            "dashboard index required"
        );
        Ok(assets)
    }

    fn walk(
        &mut self,
        path: &Path,
        pin: &File,
        prefix: &str,
        depth: usize,
        budget: &mut Budget,
    ) -> Result<()> {
        ensure!(depth <= DEPTH, "dashboard assets depth exceeded");
        same_path(path, pin, true)?;
        for entry in fs::read_dir(path)? {
            budget.entries += 1;
            ensure!(
                budget.entries <= ENTRY_LIMIT,
                "dashboard asset entries exceeded"
            );
            let entry = entry?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .filter(|name| segment(name))
                .ok_or_else(|| anyhow::anyhow!("invalid dashboard asset name"))?;
            let key = format!("{prefix}/{name}");
            let kind = entry.file_type()?;
            ensure!(
                kind.is_file() || kind.is_dir(),
                "invalid dashboard asset type"
            );
            let handle = child(pin, &entry.path(), OsStr::new(name))?;
            same_path(&entry.path(), &handle, kind.is_dir())?;
            if kind.is_dir() {
                self.walk(&entry.path(), &handle, &key, depth + 1, budget)?;
            } else {
                self.insert(key, path, pin, &entry.path(), handle, budget)?;
            }
        }
        same_path(path, pin, true)
    }

    fn insert(
        &mut self,
        key: String,
        directory: &Path,
        parent: &File,
        path: &Path,
        handle: File,
        budget: &mut Budget,
    ) -> Result<()> {
        ensure!(self.0.len() < FILE_COUNT, "dashboard asset count exceeded");
        let mime = mime(path).ok_or_else(|| anyhow::anyhow!("unsupported dashboard asset type"))?;
        let metadata = handle.metadata()?;
        let size = metadata.len();
        let max = FILE_LIMIT.min(TOTAL_LIMIT - budget.bytes);
        ensure!(size <= max, "dashboard asset size exceeded");
        let mut content = Vec::new();
        (&handle).take(size + 1).read_to_end(&mut content)?;
        ensure!(
            content.len() as u64 == size
                && handle.metadata()?.modified()? == metadata.modified()?,
            "dashboard asset changed"
        );
        same_path(path, &handle, false)?;
        same_path(directory, parent, true)?;
        budget.bytes += size;
        self.0.insert(
            key,
            Asset {
                content: content.into(),
                mime,
            },
        );
        Ok(())
    }

    pub(super) fn get(&self, path: &str) -> Option<&Asset> {
        if path == "/" {
            return self.0.get("/index.html");
        }
        let relative = path.strip_prefix('/')?;
        if relative.len() > 1024 || !relative.split('/').all(segment) {
            return None;
        }
        self.0.get(path)
    }
}

fn segment(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn mime(path: &Path) -> Option<&'static str> {
    Some(match path.extension()?.to_str()? {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        _ => return None,
    })
}

fn check(file: &File, directory: bool) -> Result<(u64, u64)> {
    let metadata = file.metadata()?;
    ensure!(
        if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        },
        "invalid dashboard asset handle"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            directory || metadata.nlink() == 1,
            "dashboard asset hard link"
        );
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::*;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file keeps the borrowed handle open; info is writable storage.
        ensure!(
            unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } != 0,
            "invalid dashboard asset handle"
        );
        ensure!(
            info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "dashboard asset reparse point"
        );
        ensure!(
            directory || info.nNumberOfLinks == 1,
            "dashboard asset hard link"
        );
        Ok((
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ))
    }
}

fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    }
    options
}

#[cfg(windows)]
fn child(_: &File, path: &Path, _: &OsStr) -> Result<File> {
    Ok(options().open(path)?)
}

#[cfg(unix)]
fn child(parent: &File, _: &Path, name: &OsStr) -> Result<File> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    };
    let name = std::ffi::CString::new(name.as_bytes())?;
    // SAFETY: parent stays open and name is a terminated, single path component.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    ensure!(fd >= 0, "dashboard asset open refused");
    // SAFETY: openat returned a new descriptor that File now owns.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn same_path(path: &Path, expected: &File, directory: bool) -> Result<()> {
    crate::config::checked_path(path)?;
    ensure!(
        check(&options().open(path)?, directory)? == check(expected, directory)?,
        "dashboard asset identity changed"
    );
    Ok(())
}
