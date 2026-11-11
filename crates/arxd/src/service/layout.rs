//! Fixed paths and pure protection policies. Observations require a native adapter.
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    Macos,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Config,
    Identity,
    AccountMissing,
    Rights,
    Workspace,
    Permissions,
    Layout,
    OwnerRequired,
    Manager,
    NativeAdapterMissing,
    NotInstalled,
    Foreign,
    Active,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Config => "service-config",
            Self::Identity => "service-identity",
            Self::AccountMissing => "service-account-missing",
            Self::Rights => "service-rights",
            Self::Workspace => "service-workspace",
            Self::Permissions => "service-permissions",
            Self::Layout => "service-layout",
            Self::OwnerRequired => "service-owner-required",
            Self::NotInstalled => "service-not-installed",
            Self::Foreign => "service-foreign-registration",
            Self::Active => "service-active",
            Self::Manager => "service-manager",
            Self::NativeAdapterMissing => "service-native-adapter-missing",
        })
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    platform: Platform,
    binary: PathBuf,
    config: PathBuf,
    state: PathBuf,
    endpoint: PathBuf,
    artifact: PathBuf,
    private_dirs: Vec<PathBuf>,
}
impl Layout {
    pub fn system() -> Result<Self, Error> {
        #[cfg(windows)]
        {
            let program = known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_ProgramFilesX64)?;
            let data = known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_ProgramData)?;
            Ok(Self::windows(program, data))
        }
        #[cfg(target_os = "linux")]
        {
            Ok(Self::unix(Platform::Linux))
        }
        #[cfg(target_os = "macos")]
        {
            Ok(Self::unix(Platform::Macos))
        }
    }
    #[cfg(any(windows, test))]
    fn windows(program: PathBuf, data: PathBuf) -> Self {
        let base = data.join("Arxcovo");
        Self {
            platform: Platform::Windows,
            binary: program.join("Arxcovo/arxd.exe"),
            config: base.join("config/arxd.json"),
            state: base.join("state"),
            endpoint: PathBuf::from(r"\\.\pipe\arxcovo-arxd"),
            artifact: PathBuf::from("arxd"),
            private_dirs: vec![base.clone(), base.join("config"), base.join("state")],
        }
    }
    #[cfg(any(unix, test))]
    fn unix(platform: Platform) -> Self {
        let (binary, config, state, endpoint, artifact, dirs) = match platform {
            Platform::Linux => (
                "/usr/local/libexec/arxcovo/arxd",
                "/etc/arxcovo/arxd.json",
                "/var/lib/arxcovo",
                "/run/arxcovo/arxd.sock",
                "/etc/systemd/system/arxd.service",
                vec!["/etc/arxcovo", "/var/lib/arxcovo"],
            ),
            _ => (
                "/Library/Application Support/Arxcovo/libexec/arxd",
                "/Library/Application Support/Arxcovo/config/arxd.json",
                "/Library/Application Support/Arxcovo/state",
                "/private/var/run/arxcovo/arxd.sock",
                "/Library/LaunchDaemons/com.arxcovo.arxd.plist",
                vec![
                    "/Library/Application Support/Arxcovo/config",
                    "/Library/Application Support/Arxcovo/state",
                    "/Library/Logs/Arxcovo",
                ],
            ),
        };
        Self {
            platform,
            binary: binary.into(),
            config: config.into(),
            state: state.into(),
            endpoint: endpoint.into(),
            artifact: artifact.into(),
            private_dirs: dirs.into_iter().map(PathBuf::from).collect(),
        }
    }
    #[cfg(test)]
    pub(crate) fn for_test(platform: Platform) -> Self {
        match platform {
            Platform::Windows => {
                Self::windows(r"C:\Program Files".into(), r"C:\ProgramData".into())
            }
            _ => Self::unix(platform),
        }
    }
    pub fn platform(&self) -> Platform {
        self.platform
    }
    pub fn binary(&self) -> &Path {
        &self.binary
    }
    pub fn config(&self) -> &Path {
        &self.config
    }
    pub fn state(&self) -> &Path {
        &self.state
    }
    pub fn endpoint(&self) -> &Path {
        &self.endpoint
    }
    pub fn artifact(&self) -> &Path {
        &self.artifact
    }
    pub fn private_dirs(&self) -> &[PathBuf] {
        &self.private_dirs
    }
    pub fn workspace_disjoint(&self, workspace: &str) -> bool {
        let Some(w) = lexical(workspace, self.platform) else {
            return false;
        };
        if self
            .binary
            .parent()
            .and_then(|p| lexical(&p.to_string_lossy(), self.platform))
            .is_some_and(|p| overlap(&w, &p))
        {
            return false;
        }
        self.private_dirs
            .iter()
            .chain([
                &self.binary,
                &self.config,
                &self.state,
                &self.endpoint,
                &self.artifact,
            ])
            .all(|p| {
                // SCM names and named pipes are not filesystem workspace roots.
                lexical(&p.to_string_lossy(), self.platform).is_none_or(|p| !overlap(&w, &p))
            })
    }
}
fn overlap(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}
fn lexical(value: &str, platform: Platform) -> Option<String> {
    if value.contains('\0') || value.len() > 4096 {
        return None;
    }
    if platform != Platform::Windows && value.contains('\\') {
        return None;
    }
    let mut value = value.replace('\\', "/");
    if platform == Platform::Windows {
        if value
            .chars()
            .any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*') || c.is_control())
        {
            return None;
        }
        if value.as_bytes().get(1) != Some(&b':')
            || value.as_bytes().get(2) != Some(&b'/')
            || !value.as_bytes()[0].is_ascii_alphabetic()
            || value[2..].contains(':')
        {
            return None;
        }
        value.make_ascii_lowercase();
    } else if !value.starts_with('/') || value.starts_with("//") || value.contains('\\') {
        return None;
    }
    if value
        .split('/')
        .any(|c| c == ".." || c == "." || c.ends_with(' ') || c.ends_with('.'))
    {
        return None;
    }
    if platform == Platform::Macos {
        value.make_ascii_lowercase();
    }
    let components: Vec<_> = value.split('/').filter(|s| !s.is_empty()).collect();
    let normalized = components.join("/");
    Some(if platform == Platform::Windows {
        normalized
    } else {
        format!("/{normalized}").trim_end_matches('/').to_owned()
    })
}
#[cfg(windows)]
fn known_folder(id: &windows_sys::core::GUID) -> Result<PathBuf, Error> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{System::Com::CoTaskMemFree, UI::Shell::SHGetKnownFolderPath};
    unsafe {
        let mut p = std::ptr::null_mut();
        if SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p) < 0 || p.is_null() {
            return Err(Error::Layout);
        }
        let mut len = 0;
        while len < 32768 && *p.add(len) != 0 {
            len += 1;
        }
        let result = if len == 32768 {
            Err(Error::Layout)
        } else {
            Ok(PathBuf::from(std::ffi::OsString::from_wide(
                std::slice::from_raw_parts(p, len),
            )))
        };
        CoTaskMemFree(p.cast());
        let path = result?;
        if lexical(&path.to_string_lossy(), Platform::Windows).is_none() {
            return Err(Error::Layout);
        }
        Ok(path)
    }
}

#[derive(Clone, Debug)]
pub enum Protection {
    Windows {
        owner: String,
        protected: bool,
        dacl_present: bool,
        aces: Vec<Ace>,
    },
    Unix {
        uid: u32,
        gid: u32,
        mode: u32,
    },
}
#[derive(Clone, Debug)]
pub enum Ace {
    Allow {
        sid: String,
        mask: u32,
        inherit_only: bool,
    },
    Deny,
    Unsupported,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    PrivateDirectory,
    PrivateFile,
    BinaryIntegrity,
    EndpointDirectory,
}
/// Namespace integrity for an existing Windows path edge. Each next component
/// must also be checked on its own handle (or under the strict final policy).
/// Creation of siblings cannot replace that existing component. Inherit-only
/// grants have no effect here; effective inherited grants are checked on the child.
/// Unknown ACEs and untrusted owners still refuse. Denies never excuse an allow.
#[cfg(any(windows, test))]
pub(crate) fn check_windows_ancestor(p: &Protection) -> Result<(), Error> {
    let Protection::Windows {
        owner,
        dacl_present,
        aces,
        ..
    } = p
    else {
        return Err(Error::Permissions);
    };
    // DELETE, WRITE_DAC, WRITE_OWNER, DELETE_CHILD and GENERIC_ALL.
    // Directory creation/EA/attributes (including GENERIC_WRITE) cannot
    // replace the protected existing child. Every ancestor is nonempty.
    const CONTROL: u32 = 0x10000000 | 0x000d0040;
    if *dacl_present
        && windows_trusted(owner)
        && aces.iter().all(|ace| match ace {
            Ace::Deny => true,
            Ace::Unsupported => false,
            Ace::Allow {
                sid,
                mask,
                inherit_only,
            } => *inherit_only || mask & CONTROL == 0 || windows_trusted(sid),
        })
    {
        Ok(())
    } else {
        Err(Error::Permissions)
    }
}
fn windows_trusted(s: &str) -> bool {
    matches!(
        s,
        "S-1-5-18"
            | "S-1-5-32-544"
            | "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"
    )
}
/// `safe_local` means every ancestor was checked without following reparse points.
pub fn check_protection(
    platform: Platform,
    policy: Policy,
    safe_local: bool,
    p: &Protection,
) -> Result<(), Error> {
    if !safe_local {
        return Err(Error::Permissions);
    }
    let ok = match (platform, p) {
        (
            Platform::Windows,
            Protection::Windows {
                owner,
                protected,
                dacl_present,
                aces,
            },
        ) => {
            let trusted = windows_trusted;
            let private = matches!(policy, Policy::PrivateDirectory | Policy::PrivateFile);
            *dacl_present
                && if private {
                    owner == "S-1-5-18"
                        && *protected
                        && aces.iter().all(|a| match a {
                            Ace::Deny => true,
                            Ace::Allow { sid, .. } => {
                                matches!(sid.as_str(), "S-1-5-18" | "S-1-5-32-544")
                            }
                            Ace::Unsupported => false,
                        })
                } else {
                    trusted(owner)
                        && aces.iter().all(|a| match a {
                            Ace::Deny => true,
                            Ace::Allow { sid, mask, .. } => {
                                mask & (0x10000000 | 0x40000000 | 0x000d0156) == 0 || trusted(sid)
                            }
                            Ace::Unsupported => false,
                        })
                }
        }
        (Platform::Linux | Platform::Macos, Protection::Unix { uid, gid, mode }) => {
            *uid == 0
                && *gid == 0
                && match policy {
                    Policy::PrivateDirectory => *mode == 0o700,
                    Policy::PrivateFile => *mode == 0o600,
                    Policy::EndpointDirectory => *mode == 0o755,
                    Policy::BinaryIntegrity => mode & 0o7022 == 0,
                }
        }
        _ => false,
    };
    if ok { Ok(()) } else { Err(Error::Permissions) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_namespace_edges_reject_control_but_allow_sibling_creation() {
        let fixture = |mask, inherit_only| Protection::Windows {
            owner: "S-1-5-18".into(),
            protected: false,
            dacl_present: true,
            aces: vec![Ace::Allow {
                sid: "S-1-1-0".into(),
                mask,
                inherit_only,
            }],
        };
        for mask in [0, 2, 4, 6, 0x116, 0x40000000, 0x1200a9] {
            assert!(check_windows_ancestor(&fixture(mask, false)).is_ok());
        }
        // Separate rights, not only FULL_CONTROL: each confers control or
        // modification authority beyond creating a new sibling.
        for mask in [0x40, 0x10000, 0x40000, 0x80000, 0x10000000] {
            assert_eq!(
                check_windows_ancestor(&fixture(mask, false)),
                Err(Error::Permissions)
            );
            assert!(check_windows_ancestor(&fixture(mask, true)).is_ok());
            // A grant becoming effective on the next component must refuse.
            assert!(
                check_protection(
                    Platform::Windows,
                    Policy::BinaryIntegrity,
                    true,
                    &fixture(mask, false)
                )
                .is_err()
            );
        }
        for policy in [Policy::PrivateFile, Policy::PrivateDirectory] {
            // Even harmless or inherit-only foreign grants remain forbidden
            // on the final private object.
            for mask in [0, 4, 0x1f01ff] {
                for inherit_only in [false, true] {
                    let mut p = fixture(mask, inherit_only);
                    if let Protection::Windows { protected, .. } = &mut p {
                        *protected = true;
                    }
                    assert!(check_protection(Platform::Windows, policy, true, &p).is_err());
                }
            }
        }
        let mut bad_owner = fixture(0, false);
        if let Protection::Windows { owner, .. } = &mut bad_owner {
            *owner = "S-1-1-0".into();
        }
        assert!(check_windows_ancestor(&bad_owner).is_err());
        let mut missing = fixture(0, false);
        if let Protection::Windows { dacl_present, .. } = &mut missing {
            *dacl_present = false;
        }
        assert!(check_windows_ancestor(&missing).is_err());
        let mut unknown = fixture(0, false);
        if let Protection::Windows { aces, .. } = &mut unknown {
            aces.push(Ace::Unsupported);
        }
        assert!(check_windows_ancestor(&unknown).is_err());
    }
    #[test]
    fn disjoint_paths_are_component_aware() {
        for os in [Platform::Windows, Platform::Linux, Platform::Macos] {
            let l = Layout::for_test(os);
            assert!(!l.workspace_disjoint(&l.state().to_string_lossy()));
            assert!(!l.workspace_disjoint("/"));
            assert!(!l.workspace_disjoint("relative"));
            assert!(!l.workspace_disjoint("/work/../etc"));
        }
        assert!(
            !Layout::for_test(Platform::Macos)
                .workspace_disjoint("/LIBRARY/Application Support/Arxcovo/state/child")
        );
        let l = Layout::for_test(Platform::Windows);
        assert!(!l.workspace_disjoint(r"c:\PROGRAMDATA\Arxcovo\state\child"));
        assert!(!l.workspace_disjoint(r"\\server\share"));
        assert!(l.workspace_disjoint(r"D:\work"));
        assert!(!Layout::for_test(Platform::Linux).workspace_disjoint("/etc//arxcovo/child"));
        assert!(Layout::for_test(Platform::Linux).workspace_disjoint("/var/lib/arxcovo-build"));
    }
    #[test]
    fn strict_private_and_integrity_are_distinct() {
        let mut p = Protection::Windows {
            owner: "S-1-5-18".into(),
            protected: true,
            dacl_present: true,
            aces: vec![],
        };
        assert!(check_protection(Platform::Windows, Policy::PrivateDirectory, true, &p).is_ok());
        if let Protection::Windows { aces, .. } = &mut p {
            aces.push(Ace::Allow {
                sid: "S-1-1-0".into(),
                mask: 0,
                inherit_only: true,
            });
        }
        assert_eq!(
            check_protection(Platform::Windows, Policy::PrivateDirectory, true, &p),
            Err(Error::Permissions)
        );
        assert!(check_protection(Platform::Windows, Policy::BinaryIntegrity, true, &p).is_ok());
        let writable = Protection::Windows {
            owner: "S-1-5-18".into(),
            protected: true,
            dacl_present: true,
            aces: vec![Ace::Allow {
                sid: "S-1-1-0".into(),
                mask: 0x40000000,
                inherit_only: true,
            }],
        };
        assert!(
            check_protection(Platform::Windows, Policy::BinaryIntegrity, true, &writable).is_err()
        );
        if let Protection::Windows { owner, .. } = &mut p {
            *owner = "S-1-5-32-544".into();
        }
        assert!(check_protection(Platform::Windows, Policy::PrivateDirectory, true, &p).is_err());
        for (policy, mode) in [
            (Policy::PrivateDirectory, 0o700),
            (Policy::PrivateFile, 0o600),
            (Policy::EndpointDirectory, 0o755),
        ] {
            assert!(
                check_protection(
                    Platform::Linux,
                    policy,
                    true,
                    &Protection::Unix {
                        uid: 0,
                        gid: 0,
                        mode
                    }
                )
                .is_ok()
            );
            assert!(
                check_protection(
                    Platform::Linux,
                    policy,
                    true,
                    &Protection::Unix {
                        uid: 1,
                        gid: 0,
                        mode
                    }
                )
                .is_err()
            );
            assert!(
                check_protection(
                    Platform::Linux,
                    policy,
                    false,
                    &Protection::Unix {
                        uid: 0,
                        gid: 0,
                        mode
                    }
                )
                .is_err()
            );
        }
    }
}
