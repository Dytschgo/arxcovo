//! Read-only native observations. No owner operations or service entry wiring.
use super::{
    config::{Account, MAX_CONFIG_BYTES, Resolver, ServiceConfig},
    layout::{Error, Layout, Policy, Protection, check_protection},
};
use std::{fs::File, io::Read, path::Path};
#[cfg(unix)]
#[path = "native_unix.rs"]
mod os;
#[cfg(windows)]
#[path = "native_windows.rs"]
mod os;

pub struct NativeResolver;
impl Resolver for NativeResolver {
    fn account(&self, value: &str) -> Result<Account, Error> {
        if value.is_empty() || value.len() > 256 || value.contains('\0') {
            return Err(Error::Identity);
        }
        os::account(value)
    }
    fn workspace_safe(&self, value: &str) -> Result<bool, Error> {
        os::open(Path::new(value), true, false, false)
            .map(|_| true)
            .map_err(|_| Error::Workspace)
    }
}

/// A handle and its pinned ancestry are kept alive together. Observations apply
/// to this opened object; they are not authority for a later path-based open.
pub struct Observed {
    file: File,
    _ancestors: Vec<File>,
    protection: Protection,
}
impl Observed {
    pub fn protection(&self) -> &Protection {
        &self.protection
    }
}

fn checked(path: &Path, directory: bool, policy: Policy, read: bool) -> Result<Observed, Error> {
    let observed = os::open(path, directory, read, true)?;
    check_protection(os::PLATFORM, policy, true, &observed.protection)?;
    Ok(observed)
}

/// Observe the fixed service layout. No arbitrary path override is exposed.
pub fn observe_layout(layout: &Layout) -> Result<Vec<Observed>, Error> {
    if layout.platform() != os::PLATFORM {
        return Err(Error::Layout);
    }
    let mut result = Vec::new();
    for path in layout.private_dirs() {
        result.push(checked(path, true, Policy::PrivateDirectory, false)?);
    }
    result.push(checked(
        layout.binary().parent().ok_or(Error::Layout)?,
        true,
        Policy::BinaryIntegrity,
        false,
    )?);
    result.push(checked(
        layout.binary(),
        false,
        Policy::BinaryIntegrity,
        false,
    )?);
    result.push(checked(layout.config(), false, Policy::PrivateFile, false)?);
    #[cfg(unix)]
    result.push(checked(
        layout.endpoint().parent().ok_or(Error::Layout)?,
        true,
        Policy::EndpointDirectory,
        false,
    )?);
    Ok(result)
}

/// Protection and bytes come from the same no-follow handle. Only the fixed
/// config is read. The existing parser performs fresh identity/pin checks.
pub fn read_config(layout: &Layout) -> Result<ServiceConfig, Error> {
    if layout.platform() != os::PLATFORM {
        return Err(Error::Layout);
    }
    let mut parents = Vec::new();
    for path in layout.private_dirs() {
        parents.push(checked(path, true, Policy::PrivateDirectory, false)?);
    }
    let mut opened = checked(layout.config(), false, Policy::PrivateFile, true)?;
    let bytes = bounded_read(&mut opened.file)?;
    // Reobserve the handle after reading, never reopen the symbolic path.
    check_protection(
        os::PLATFORM,
        Policy::PrivateFile,
        true,
        &os::protection(&opened.file)?,
    )?;
    ServiceConfig::parse(&bytes, layout, &NativeResolver)
}
fn bounded_read(file: &mut impl Read) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Config)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(Error::Config);
    }
    Ok(bytes)
}

/// Reject lexical aliases before the OS sees them. In particular Path::components
/// normalizes dots, so it must not be used as the validator.
fn components(path: &Path, windows: bool) -> Result<Vec<String>, Error> {
    let s = path.to_str().ok_or(Error::Layout)?;
    if s.len() > 4096 || s.contains('\0') {
        return Err(Error::Layout);
    }
    let normalized;
    let tail = if windows {
        normalized = s.replace('\\', "/");
        let b = normalized.as_bytes();
        if b.len() < 3 || !b[0].is_ascii_alphabetic() || &b[1..3] != b":/" {
            return Err(Error::Layout);
        }
        &normalized[3..]
    } else {
        if !s.starts_with('/') || s.contains('\\') {
            return Err(Error::Layout);
        }
        &s[1..]
    };
    let mut out = Vec::new();
    if tail.is_empty() {
        return Ok(out);
    }
    for c in tail.split('/') {
        if c.is_empty()
            || c == "."
            || c == ".."
            || c.chars().any(char::is_control)
            || (windows
                && (c.ends_with([' ', '.']) || c.contains([':', '<', '>', '"', '|', '?', '*'])))
        {
            return Err(Error::Layout);
        }
        if windows {
            let stem = c.split('.').next().unwrap_or("").to_ascii_uppercase();
            if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ["COM", "LPT"].iter().any(|p| {
                    stem.strip_prefix(p).is_some_and(|n| {
                        matches!(
                            n,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                        )
                    })
                })
            {
                return Err(Error::Layout);
            }
        }
        out.push(c.to_owned());
    }
    if out.len() > 128 {
        return Err(Error::Layout);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexical_aliases_refuse_before_open() {
        for p in [
            "relative", "/a/../b", "/a/./b", "//a", "/a//b", "/a/", "/a\0b",
        ] {
            assert!(components(Path::new(p), false).is_err());
        }
        for p in [
            r"C:relative",
            r"\\host\share",
            r"\\?\C:\a",
            r"C:\a\..\b",
            r"C:\a:stream",
            r"C:\NUL.txt",
            r"C:\COM1",
            r"C:\a.",
        ] {
            assert!(components(Path::new(p), true).is_err());
        }
        assert!(components(Path::new(r"C:\ordinary\file"), true).is_ok());
    }
    #[test]
    fn reads_are_bounded_even_if_metadata_lies() {
        assert_eq!(
            bounded_read(&mut &vec![0; MAX_CONFIG_BYTES][..])
                .unwrap()
                .len(),
            MAX_CONFIG_BYTES
        );
        assert_eq!(
            bounded_read(&mut &vec![0; MAX_CONFIG_BYTES + 1][..]),
            Err(Error::Config)
        );
    }
    #[test]
    fn policy_negatives_cover_every_required_observation() {
        use super::super::layout::{Ace, Platform};
        let win = |owner: &str, protected, dacl_present, aces| Protection::Windows {
            owner: owner.into(),
            protected,
            dacl_present,
            aces,
        };
        for p in [
            win("S-1-5-32-544", true, true, vec![]),
            win("S-1-5-18", false, true, vec![]),
            win("S-1-5-18", true, false, vec![]),
            win("S-1-5-18", true, true, vec![Ace::Unsupported]),
            win(
                "S-1-5-18",
                true,
                true,
                vec![Ace::Allow {
                    sid: "S-1-1-0".into(),
                    writes: false,
                    inherit_only: true,
                }],
            ),
        ] {
            assert_eq!(
                check_protection(Platform::Windows, Policy::PrivateFile, true, &p),
                Err(Error::Permissions)
            );
        }
        for (uid, gid, mode, safe) in [
            (1, 0, 0o600, true),
            (0, 1, 0o600, true),
            (0, 0, 0o644, true),
            (0, 0, 0o600, false),
        ] {
            assert_eq!(
                check_protection(
                    Platform::Linux,
                    Policy::PrivateFile,
                    safe,
                    &Protection::Unix { uid, gid, mode }
                ),
                Err(Error::Permissions)
            );
        }
    }
    #[test]
    fn rights_both_directions_and_pins_revalidate() {
        use super::super::{
            config::tests::{FakeResolver, fixture, parse},
            layout::Platform,
        };
        let mut v = fixture();
        v["rights"] = "admin".into();
        assert!(parse(v.clone(), &FakeResolver::default()).is_err());
        let c = parse(
            v,
            &FakeResolver {
                admin: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            c.revalidate(&Layout::for_test(Platform::Linux), &FakeResolver::default())
                .is_err()
        );
        let c = parse(fixture(), &FakeResolver::default()).unwrap();
        for r in [
            FakeResolver {
                admin: true,
                ..Default::default()
            },
            FakeResolver {
                changed: true,
                ..Default::default()
            },
            FakeResolver {
                safe: false,
                ..Default::default()
            },
            FakeResolver {
                group: true,
                ..Default::default()
            },
        ] {
            assert!(
                c.revalidate(&Layout::for_test(Platform::Linux), &r)
                    .is_err()
            );
        }
    }
    #[test]
    fn native_workspace_rejects_missing_and_aliases() {
        let r = NativeResolver;
        assert!(r.workspace_safe("relative-secret-marker").is_err());
        let temp = tempfile::tempdir().unwrap();
        // macOS /var is a link: this intentionally refuses rather than canonicalizing.
        let result = r.workspace_safe(temp.path().to_str().unwrap());
        #[cfg(not(target_os = "macos"))]
        assert_eq!(result, Ok(true));
        #[cfg(target_os = "macos")]
        let _ = result;
        assert!(
            r.workspace_safe(temp.path().join("missing").to_str().unwrap())
                .is_err()
        );
        assert!(r.account("secret-marker\0").is_err());
    }
}
