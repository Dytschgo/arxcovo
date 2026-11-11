//! Read-only local OpenDirectory observations. No NSS or command fallback.
//! FFI contracts: Apple's DirectoryService CFOpenDirectory.h and SDK
//! odconstants.h; CoreFoundation Create/Copy ownership applies throughout.
use super::super::config::AccountKind;
use super::*;
use std::ffi::{CString, c_void};
#[repr(C)]
struct Range {
    location: isize,
    length: isize,
}
type Ref = *const c_void;
const NIL: Ref = std::ptr::null();
#[path = "native_macos_policy.rs"]
mod policy;
use policy::{Record, classify, decimal};
const LOCAL: &str = "/Local/Default";
const USERS: &str = "dsRecTypeStandard:Users";
const GROUPS: &str = "dsRecTypeStandard:Groups";
const NAME: &str = "dsAttrTypeStandard:RecordName";
const UID: &str = "dsAttrTypeStandard:UniqueID";
const GID: &str = "dsAttrTypeStandard:PrimaryGroupID";
const UUID: &str = "dsAttrTypeStandard:GeneratedUID";
const NODE: &str = "dsAttrTypeStandard:AppleMetaNodeLocation";
const ORIGIN: &str = "dsAttrTypeStandard:OriginalNodeName";
const SHELL: &str = "dsAttrTypeStandard:UserShell";
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Ref);
    fn CFRetain(value: Ref) -> Ref;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFStringCreateWithCString(allocator: Ref, value: *const i8, encoding: u32) -> Ref;
    fn CFStringGetLength(value: Ref) -> isize;
    fn CFStringGetCharacters(value: Ref, range: Range, out: *mut u16);
    fn CFArrayGetCount(value: Ref) -> isize;
    fn CFArrayGetValueAtIndex(value: Ref, index: isize) -> Ref;
    fn CFArrayCreate(allocator: Ref, values: *const Ref, count: isize, callbacks: Ref) -> Ref;
    fn CFDictionaryGetCount(value: Ref) -> isize;
    fn CFDictionaryGetValue(value: Ref, key: Ref) -> Ref;
}
#[link(name = "OpenDirectory", kind = "framework")]
unsafe extern "C" {
    fn ODNodeCreateWithName(allocator: Ref, session: Ref, name: Ref, error: *mut Ref) -> Ref;
    fn ODQueryCreateWithNode(
        allocator: Ref,
        node: Ref,
        types: Ref,
        attribute: Ref,
        match_type: u32,
        value: Ref,
        attributes: Ref,
        maximum: isize,
        error: *mut Ref,
    ) -> Ref;
    fn ODQueryCopyResults(query: Ref, partial: bool, error: *mut Ref) -> Ref;
    fn ODRecordGetTypeID() -> usize;
    fn ODRecordGetRecordType(record: Ref) -> Ref;
    fn ODRecordGetRecordName(record: Ref) -> Ref;
    fn ODRecordCopyDetails(record: Ref, attributes: Ref, error: *mut Ref) -> Ref;
    fn ODRecordContainsMember(group: Ref, member: Ref, error: *mut Ref) -> bool;
}
struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}
fn owned(value: Ref) -> Result<Owned, Error> {
    if value.is_null() {
        Err(Error::Identity)
    } else {
        Ok(Owned(value))
    }
}
// Release even an unexpected simultaneous result+error. Never format CFError.
fn result(value: Ref, error: Ref) -> Result<Owned, Error> {
    let value = owned(value);
    if !error.is_null() {
        unsafe { CFRelease(error) };
        return Err(Error::Identity);
    }
    value
}
fn string(value: &str) -> Result<Owned, Error> {
    if value.is_empty() || value.len() > 1024 {
        return Err(Error::Identity);
    }
    let value = CString::new(value).map_err(|_| Error::Identity)?;
    owned(unsafe { CFStringCreateWithCString(NIL, value.as_ptr(), 0x08000100) }) // UTF8
}
fn text(value: Ref) -> Result<String, Error> {
    if value.is_null()
        || unsafe { CFGetTypeID(value) != CFStringGetTypeID() }
        || !(1..=1024).contains(&unsafe { CFStringGetLength(value) })
    {
        return Err(Error::Identity);
    }
    let n = unsafe { CFStringGetLength(value) };
    let mut units = [0u16; 1024];
    unsafe {
        CFStringGetCharacters(
            value,
            Range {
                location: 0,
                length: n,
            },
            units.as_mut_ptr(),
        )
    };
    let out = String::from_utf16(&units[..n as usize]).map_err(|_| Error::Identity)?;
    if out.chars().any(char::is_control) {
        return Err(Error::Identity);
    }
    Ok(out)
}
fn array(value: Ref, max: isize) -> Result<isize, Error> {
    if value.is_null() || unsafe { CFGetTypeID(value) != CFArrayGetTypeID() } {
        return Err(Error::Identity);
    }
    let n = unsafe { CFArrayGetCount(value) };
    policy::count(n, max)
}
fn attribute(details: Ref, key: &str, optional: bool) -> Result<Option<String>, Error> {
    let key = string(key)?;
    let values = unsafe { CFDictionaryGetValue(details, key.0) };
    if values.is_null() && optional {
        return Ok(None);
    }
    if array(values, 1)? != 1 {
        return Err(Error::Identity);
    }
    text(unsafe { CFArrayGetValueAtIndex(values, 0) }).map(Some)
}
fn query(node: Ref, kind: &str, attr: &str, value: &str) -> Result<Owned, Error> {
    let (kind, attr, value, returned) =
        (string(kind)?, string(attr)?, string(value)?, string(NAME)?);
    let mut error = NIL;
    // SDK kODMatchEqualTo = 0x2001. Two results, not one, detects duplicates.
    let query = result(
        unsafe {
            ODQueryCreateWithNode(
                NIL, node, kind.0, attr.0, 0x2001, value.0, returned.0, 2, &mut error,
            )
        },
        error,
    )?;
    error = NIL;
    let results = result(
        unsafe { ODQueryCopyResults(query.0, false, &mut error) },
        error,
    )?;
    policy::unique(array(results.0, 2)?)?;
    let record = unsafe { CFArrayGetValueAtIndex(results.0, 0) };
    if record.is_null() || unsafe { CFGetTypeID(record) != ODRecordGetTypeID() } {
        return Err(Error::Identity);
    }
    policy::record_type(
        &text(unsafe { ODRecordGetRecordType(record) })?,
        &text(kind.0)?,
    )?;
    owned(unsafe { CFRetain(record) })
}
fn record(value: Ref) -> Result<Record, Error> {
    let keys = [NAME, UID, GID, UUID, NODE, ORIGIN, SHELL].map(string);
    let keys: Vec<_> = keys.into_iter().collect::<Result<_, _>>()?;
    let refs: Vec<_> = keys.iter().map(|v| v.0).collect();
    // NULL callbacks: this borrowed array lives less than its retained keys.
    let attrs = owned(unsafe { CFArrayCreate(NIL, refs.as_ptr(), refs.len() as isize, NIL) })?;
    let mut error = NIL;
    let details = result(
        unsafe { ODRecordCopyDetails(value, attrs.0, &mut error) },
        error,
    )?;
    if unsafe { CFGetTypeID(details.0) != CFDictionaryGetTypeID() }
        || !(1..=16).contains(&unsafe { CFDictionaryGetCount(details.0) })
    {
        return Err(Error::Identity);
    }
    let required = |key| attribute(details.0, key, false)?.ok_or(Error::Identity);
    Ok(Record {
        name: required(NAME)?,
        uid: required(UID)?,
        gid: required(GID)?,
        uuid: required(UUID)?,
        node: required(NODE)?,
        origin: attribute(details.0, ORIGIN, true)?,
        shell: required(SHELL)?,
    })
}
fn observe(value: &str) -> Result<(Account, Record), Error> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(Error::Identity);
    }
    let local = string(LOCAL)?;
    let mut error = NIL;
    let node = result(
        unsafe { ODNodeCreateWithName(NIL, NIL, local.0, &mut error) },
        error,
    )?;
    let by = if value.bytes().all(|c| c.is_ascii_digit()) {
        decimal(value)?;
        UID
    } else {
        NAME
    };
    let user = query(node.0, USERS, by, value)?;
    let r = record(user.0)?;
    let (uid, gid) = classify(&r, value)?;
    // Both keys must uniquely resolve to the same local incarnation.
    for (key, value) in [
        (NAME, r.name.as_str()),
        (UID, r.uid.as_str()),
        (UUID, r.uuid.as_str()),
    ] {
        let other = query(node.0, USERS, key, value)?;
        if record(other.0)? != r {
            return Err(Error::Identity);
        }
    }
    let group = query(node.0, GROUPS, GID, "80")?;
    if text(unsafe { ODRecordGetRecordName(group.0) })? != "admin" {
        return Err(Error::Identity);
    }
    error = NIL;
    let admin = unsafe { ODRecordContainsMember(group.0, user.0, &mut error) };
    let failed = !error.is_null();
    if failed {
        unsafe { CFRelease(error) };
    }
    let admin = policy::membership(admin, failed)?;
    Ok((
        Account {
            identity: uid.to_string(),
            gid: Some(gid),
            kind: AccountKind::User,
            administrative: admin,
        },
        r,
    ))
}
pub fn account(value: &str) -> Result<Account, Error> {
    observe(value).map(|v| v.0)
}
pub fn revision(value: &str) -> Result<String, Error> {
    let (a, r) = observe(value)?;
    Ok(policy::revision(&r, a.administrative))
}
#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" {
        fn mbr_uid_to_uuid(uid: u32, uuid: *mut u8) -> i32;
        fn mbr_gid_to_uuid(gid: u32, uuid: *mut u8) -> i32;
        fn mbr_check_membership(user: *const u8, group: *const u8, member: *mut i32) -> i32;
    }
    #[test]
    fn native_current_local_identity_positive() {
        // NSS supplies only a candidate key. Passing requires all OD checks.
        let key = unsafe { libc::geteuid() }.to_string();
        let a = account(&key).expect("native local ordinary identity required");
        assert!(a.kind == AccountKind::User);
        assert!(a.identity == key);
        let (mut user, mut group, mut member) = ([0u8; 16], [0u8; 16], 0);
        assert!(unsafe { mbr_uid_to_uuid(libc::geteuid(), user.as_mut_ptr()) } == 0);
        assert!(unsafe { mbr_gid_to_uuid(80, group.as_mut_ptr()) } == 0);
        assert!(unsafe { mbr_check_membership(user.as_ptr(), group.as_ptr(), &mut member) } == 0);
        assert!(a.administrative == (member != 0));
        assert!(revision(&key).is_ok());
        assert!(account("0").is_err());
        assert!(account("arxcovo-test-nonexistent").is_err());
    }
}

// Inspect the filesec property explicitly: acl_get_fd_np alone conflates a
// missing ACL property with retrieval failure. See Libc acl_file.c/filesec.c.
unsafe extern "C" {
    fn filesec_init() -> Ref;
    fn filesec_free(value: Ref);
    #[cfg_attr(target_arch = "x86_64", link_name = "fstatx_np$INODE64")]
    fn fstatx_np(fd: i32, stat: *mut libc::stat, security: Ref) -> i32;
    fn filesec_query_property(value: Ref, property: i32, present: *mut i32) -> i32;
    fn filesec_get_property(value: Ref, property: i32, out: *mut Ref) -> i32;
    fn acl_valid(value: Ref) -> i32;
    fn acl_get_entry(value: Ref, index: i32, entry: *mut Ref) -> i32;
    fn acl_free(value: Ref) -> i32;
}
pub(super) fn empty_acl(fd: i32) -> Result<(), Error> {
    struct Security(Ref);
    impl Drop for Security {
        fn drop(&mut self) {
            unsafe { filesec_free(self.0) }
        }
    }
    struct Acl(Ref);
    impl Drop for Acl {
        fn drop(&mut self) {
            unsafe {
                acl_free(self.0);
            }
        }
    }
    let raw = unsafe { filesec_init() };
    if raw.is_null() {
        return Err(Error::Permissions);
    }
    let security = Security(raw);
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let mut present = 0;
    if unsafe { fstatx_np(fd, stat.as_mut_ptr(), security.0) } != 0
        || unsafe { filesec_query_property(security.0, 5, &mut present) } != 0
    {
        return Err(Error::Permissions);
    }
    if !policy::acl_present(present) {
        return Ok(());
    }
    let mut raw = NIL;
    if unsafe { filesec_get_property(security.0, 5, &mut raw) } != 0
        || raw.is_null()
        || raw as usize == 1
    {
        return Err(Error::Permissions);
    }
    let acl = Acl(raw);
    if unsafe { acl_valid(acl.0) } != 0 {
        return Err(Error::Permissions);
    }
    let mut entry = NIL;
    let result = unsafe { acl_get_entry(acl.0, 0, &mut entry) }; // ACL_FIRST_ENTRY
    let errno = std::io::Error::last_os_error().raw_os_error();
    // Apple returns 0 for an entry, -1/EINVAL for an exhausted valid ACL.
    policy::acl_empty(result, errno == Some(libc::EINVAL))
}
#[cfg(test)]
mod acl_tests {
    use super::*;
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn acl_init(count: i32) -> Ref;
        fn acl_set_fd_np(fd: i32, acl: Ref, kind: i32) -> i32;
        fn acl_from_text(text: *const i8) -> Ref;
    }
    #[test]
    fn native_temporary_acl_absent_empty_nonempty_error() {
        let f = tempfile::tempfile().unwrap();
        assert!(empty_acl(f.as_raw_fd()).is_ok());
        let empty = unsafe { acl_init(0) };
        assert!(!empty.is_null());
        assert!(unsafe { acl_set_fd_np(f.as_raw_fd(), empty, 0x100) } == 0);
        unsafe {
            acl_free(empty);
        }
        assert!(empty_acl(f.as_raw_fd()).is_ok());
        let acl = unsafe {
            acl_from_text(
                c"!#acl 1\ngroup:ABCDEFAB-CDEF-ABCD-EFAB-CDEF0000000C:everyone:12:allow:read\n"
                    .as_ptr(),
            )
        };
        assert!(!acl.is_null());
        assert!(unsafe { acl_set_fd_np(f.as_raw_fd(), acl, 0x100) } == 0);
        unsafe {
            acl_free(acl);
        }
        assert!(empty_acl(f.as_raw_fd()).is_err());
        assert!(empty_acl(-1).is_err());
        // A caller-owned file is an observation fixture, never root policy evidence.
        assert!(super::super::os::protection(&f).is_err());
        let clean = tempfile::tempfile().unwrap();
        let p = super::super::os::protection(&clean).unwrap();
        assert!(
            check_protection(super::super::os::PLATFORM, Policy::PrivateFile, true, &p).is_err()
        );
    }
}
