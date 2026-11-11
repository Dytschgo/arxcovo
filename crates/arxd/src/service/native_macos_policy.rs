use super::Error;
const LOCAL: &str = "/Local/Default";
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Record {
    pub name: String,
    pub uid: String,
    pub gid: String,
    pub uuid: String,
    pub node: String,
    pub origin: Option<String>,
    pub shell: String,
}
pub(super) fn decimal(value: &str) -> Result<u32, Error> {
    let n = value.parse::<u32>().map_err(|_| Error::Identity)?;
    if n.to_string() != value {
        return Err(Error::Identity);
    }
    Ok(n)
}
pub(super) fn classify(r: &Record, requested: &str) -> Result<(u32, u32), Error> {
    let (uid, gid) = (decimal(&r.uid)?, decimal(&r.gid)?);
    // Native Users type + local origin + ordinary login profile. Mobile/cache
    // records with an original directory node are deliberately unsupported.
    if r.node != LOCAL
        || r.origin.is_some()
        || uid < 501
        || uid >= i32::MAX as u32
        || gid == 0
        || r.name.is_empty()
        || r.name.len() > 256
        || r.name == "root"
        || uid == 65534
        || r.name.starts_with('_')
        || r.name.chars().any(|c| c.is_control() || c == ':')
        || !matches!(
            r.shell.as_str(),
            "/bin/zsh" | "/bin/bash" | "/bin/sh" | "/bin/ksh" | "/bin/csh" | "/bin/tcsh"
        )
        || r.uuid.len() != 36
        || !r.uuid.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
        || r.uuid
            .bytes()
            .filter(|c| c.is_ascii_hexdigit())
            .all(|c| c == b'0')
        || (requested != r.name && requested != r.uid)
    {
        return Err(Error::Identity);
    }
    Ok((uid, gid))
}

pub(super) fn revision(r: &Record, admin: bool) -> String {
    format!("{}:{}:{}:{}:{}", r.name, r.uid, r.gid, r.uuid, admin)
}
pub(super) fn unique(n: isize) -> Result<(), Error> {
    if n != 1 { Err(Error::Identity) } else { Ok(()) }
}
pub(super) fn record_type(actual: &str, expected: &str) -> Result<(), Error> {
    if actual != expected {
        Err(Error::Identity)
    } else {
        Ok(())
    }
}
pub(super) fn count(n: isize, max: isize) -> Result<isize, Error> {
    if !(0..=max).contains(&n) {
        Err(Error::Identity)
    } else {
        Ok(n)
    }
}
pub(super) fn membership(value: bool, error: bool) -> Result<bool, Error> {
    if error {
        Err(Error::Identity)
    } else {
        Ok(value)
    }
}
pub(super) fn acl_empty(result: i32, invalid: bool) -> Result<(), Error> {
    if result == -1 && invalid {
        Ok(())
    } else {
        Err(Error::Permissions)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Record {
        Record {
            name: "fixture".into(),
            uid: "501".into(),
            gid: "20".into(),
            uuid: "12345678-1234-1234-1234-123456789abc".into(),
            node: LOCAL.into(),
            origin: None,
            shell: "/bin/zsh".into(),
        }
    }
    #[test]
    fn revision_changes_for_rename_replacement_and_membership() {
        let r = fixture();
        let pinned = revision(&r, false);
        assert!(pinned == revision(&r, false));
        assert!(pinned != revision(&r, true));
        let mut renamed = r.clone();
        renamed.name = "renamed".into();
        assert!(pinned != revision(&renamed, false));
        let mut replaced = r;
        replaced.uuid = "87654321-1234-1234-1234-123456789abc".into();
        assert!(pinned != revision(&replaced, false));
    }
    #[test]
    fn native_result_contracts_refuse_ambiguity_errors_and_oversize() {
        assert!(record_type("Users", "Users").is_ok());
        assert!(record_type("Groups", "Users").is_err());
        assert!(unique(0).is_err());
        assert!(unique(2).is_err());
        assert!(unique(1).is_ok());
        assert!(count(-1, 2).is_err());
        assert!(count(3, 2).is_err());
        assert!(count(2, 2).is_ok());
        assert!(membership(true, false) == Ok(true));
        assert!(membership(false, false) == Ok(false));
        assert!(membership(false, true).is_err());
        assert!(membership(true, true).is_err());
        assert!(acl_empty(-1, true).is_ok());
        assert!(acl_empty(0, true).is_err());
        assert!(acl_empty(-1, false).is_err());
    }
    #[test]
    fn local_ordinary_classification_negatives() {
        let r = fixture();
        assert!(classify(&r, "501").is_ok());
        for (field, value) in [
            ("node", "/LDAPv3/example"),
            ("uid", "0"),
            ("uid", "500"),
            ("uid", "0501"),
            ("uid", "4294967296"),
            ("gid", "0"),
            ("name", "_daemon"),
            ("shell", "/usr/bin/false"),
            ("uuid", "bad"),
        ] {
            let mut r = fixture();
            match field {
                "node" => r.node = value.into(),
                "uid" => r.uid = value.into(),
                "gid" => r.gid = value.into(),
                "name" => r.name = value.into(),
                "shell" => r.shell = value.into(),
                _ => r.uuid = value.into(),
            };
            assert!(classify(&r, "501").is_err());
        }
        let mut r = fixture();
        r.origin = Some(LOCAL.into());
        assert!(classify(&r, "501").is_err());
        assert!(classify(&fixture(), "different").is_err());
    }
}
