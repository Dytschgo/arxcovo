//! Offline layout ownership, under the explicit no-external-writer premise.
//! Handles and identity rechecks detect replacement; this is not a sandbox
//! against an arbitrary writer running as the same account.
#![allow(dead_code)]
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions},
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Mode {
    CreateFounder,
    Join,
    RecoverFounder,
    RecoverJoiner,
}
impl Mode {
    pub(super) fn recovery(self) -> bool {
        matches!(self, Self::RecoverFounder | Self::RecoverJoiner)
    }
    pub(super) fn founder(self) -> bool {
        matches!(self, Self::CreateFounder | Self::RecoverFounder)
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    pub mode: Mode,
    pub config: PathBuf,
    pub state: PathBuf,
    pub credentials: PathBuf,
    pub workspace: PathBuf,
    pub peer: String,
    pub enrollment: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    Plan,
    Layout,
    Ownership,
    Unavailable,
    Identity,
    Store,
    Clock,
    Provenance,
}
type Result<T> = std::result::Result<T, Refusal>;
fn endpoint(s: &str) -> Result<SocketAddr> {
    let a: SocketAddr = s.parse().map_err(|_| Refusal::Plan)?;
    if matches!(a,SocketAddr::V6(v) if v.scope_id()!=0) {
        return Err(Refusal::Plan);
    }
    let safe = match a.ip() {
        std::net::IpAddr::V4(v) => !v.is_link_local(),
        std::net::IpAddr::V6(v) => !v.is_unicast_link_local() && v.to_ipv4_mapped().is_none(),
    };
    if s.len() > 64
        || s != a.to_string()
        || a.port() == 0
        || a.ip().is_unspecified()
        || a.ip().is_multicast()
        || !safe
    {
        return Err(Refusal::Plan);
    }
    Ok(a)
}
impl Plan {
    #[cfg(target_os = "linux")]
    pub(super) fn recover_joiner(&self) -> Self {
        Self {
            mode: Mode::RecoverJoiner,
            config: self.config.clone(),
            state: self.state.clone(),
            credentials: self.credentials.clone(),
            workspace: self.workspace.clone(),
            peer: self.peer.clone(),
            enrollment: self.enrollment.clone(),
        }
    }
    pub(super) fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 16384 {
            return Err(Refusal::Plan);
        }
        let p: Self = serde_json::from_slice(bytes).map_err(|_| Refusal::Plan)?;
        p.validate()?;
        Ok(p)
    }
    fn validate(&self) -> Result<()> {
        let p = self;
        if endpoint(&p.peer)? == endpoint(&p.enrollment)? {
            return Err(Refusal::Plan);
        }
        for path in [&p.config, &p.state, &p.credentials, &p.workspace] {
            crate::config::checked_path(path).map_err(|_| Refusal::Layout)?;
        }
        let paths = [&p.config, &p.state, &p.credentials, &p.workspace];
        for (i, a) in paths.iter().enumerate() {
            for b in &paths[i + 1..] {
                if a.starts_with(b) || b.starts_with(a) {
                    return Err(Refusal::Layout);
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct ObjectId(u64, u64);
#[cfg(unix)]
fn object(file: &File) -> Result<ObjectId> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata().map_err(|_| Refusal::Unavailable)?;
    if m.is_file() && m.nlink() != 1 {
        return Err(Refusal::Layout);
    }
    Ok(ObjectId(m.dev(), m.ino()))
}
#[cfg(windows)]
fn object(file: &File) -> Result<ObjectId> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(Refusal::Unavailable);
    }
    if info.dwFileAttributes & 0x400 != 0
        || (info.dwFileAttributes & 0x10 == 0 && info.nNumberOfLinks != 1)
    {
        return Err(Refusal::Layout);
    }
    Ok(ObjectId(
        info.dwVolumeSerialNumber as u64,
        (info.nFileIndexHigh as u64) << 32 | info.nFileIndexLow as u64,
    ))
}
#[cfg(not(any(unix, windows)))]
fn object(_: &File) -> Result<ObjectId> {
    Err(Refusal::Unavailable)
}
fn options(directory: bool) -> OpenOptions {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.share_mode(3)
            .custom_flags(0x00200000 | if directory { 0x02000000 } else { 0 });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | if directory { libc::O_DIRECTORY } else { 0 });
    }
    o
}
pub(super) struct Guard {
    path: PathBuf,
    file: File,
    id: ObjectId,
}
impl Guard {
    pub(super) fn bound_duplicate(&self) -> Result<File> {
        self.recheck()?;
        self.file.try_clone().map_err(|_| Refusal::Unavailable)
    }
    pub(super) fn file(&self) -> &File {
        &self.file
    }
    pub(super) fn created(path: PathBuf, file: File) -> Result<Self> {
        let id = object(&file)?;
        let guard = Self { path, file, id };
        guard.recheck()?;
        crate::security::private(&guard.path).map_err(|_| Refusal::Layout)?;
        Ok(guard)
    }
    pub(super) fn rename_to(&mut self, path: PathBuf) {
        self.path = path;
    }
    pub(super) fn read_at(&self, max: u64) -> Result<Vec<u8>> {
        let len = self.file.metadata().map_err(|_| Refusal::Layout)?.len();
        if len > max {
            return Err(Refusal::Identity);
        }
        let mut bytes = vec![0; len as usize];
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            self.file
                .read_exact_at(&mut bytes, 0)
                .map_err(|_| Refusal::Layout)?;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            let mut offset = 0;
            while offset < bytes.len() {
                let n = self
                    .file
                    .seek_read(&mut bytes[offset..], offset as u64)
                    .map_err(|_| Refusal::Layout)?;
                if n == 0 {
                    return Err(Refusal::Layout);
                }
                offset += n;
            }
        }
        #[cfg(not(any(unix, windows)))]
        return Err(Refusal::Unavailable);
        Ok(bytes)
    }
    fn open(path: &Path, directory: bool, private: bool) -> Result<Self> {
        crate::config::checked_path(path).map_err(|_| Refusal::Layout)?;
        if private {
            crate::security::private(path).map_err(|_| Refusal::Layout)?;
        }
        let file = options(directory).open(path).map_err(|_| Refusal::Layout)?;
        let m = file.metadata().map_err(|_| Refusal::Layout)?;
        if m.is_dir() != directory || (!directory && !m.is_file()) {
            return Err(Refusal::Layout);
        }
        let id = object(&file)?;
        Ok(Self {
            path: path.into(),
            file,
            id,
        })
    }
    pub(super) fn recheck(&self) -> Result<()> {
        let other = Self::open(
            &self.path,
            self.file.metadata().map_err(|_| Refusal::Layout)?.is_dir(),
            false,
        )?;
        if other.id != self.id || object(&self.file)? != self.id {
            return Err(Refusal::Layout);
        }
        Ok(())
    }
}
pub(super) struct Owner {
    pub plan: Plan,
    guards: Vec<Guard>,
    // The real OS lock remains held until this object is dropped.
    lock: File,
    entries: Vec<String>,
    published_join: bool,
    pub(super) publication: super::issuance_publication::Attempt,
    pub(super) bound_attempt: super::bound_open::BoundAttempt,
}
/// Holds independent identity references, never the locked Owner descriptor.
#[cfg(target_os = "linux")]
pub(super) struct ReopenObjects(Vec<Guard>);

#[cfg(target_os = "linux")]
impl Owner {
    pub(super) fn pin_reopen_objects(&self) -> Result<ReopenObjects> {
        self.recheck()?;
        if !self.published_join || !self.has_stores() {
            return Err(Refusal::Provenance);
        }
        let core = self
            .guards
            .iter()
            .filter(|guard| {
                !guard
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.ends_with("-journal")
                            || name.ends_with("-wal")
                            || name.ends_with("-shm")
                    })
            })
            .map(|guard| {
                let directory = guard.file.metadata().map_err(|_| Refusal::Layout)?.is_dir();
                let pin = Guard::open(&guard.path, directory, false)?;
                if pin.id != guard.id {
                    return Err(Refusal::Ownership);
                }
                Ok(pin)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ReopenObjects(core))
    }

    pub(super) fn verify_reopen_objects(&self, pinned: &ReopenObjects) -> Result<()> {
        self.recheck()?;
        for guard in &pinned.0 {
            guard.recheck()?;
            if !self
                .guards
                .iter()
                .any(|g| g.path == guard.path && g.id == guard.id)
            {
                return Err(Refusal::Ownership);
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Plan {
    /// The private lane selects the operation without rewriting the pinned config.
    pub(super) fn with_history_intent(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }
}

#[cfg(target_os = "linux")]
impl Owner {
    pub(super) fn history_config_digest(&self) -> Result<[u8; 32]> {
        use sha2::{Digest, Sha256};
        let guard = self.history_guard(&self.plan.config)?;
        use std::os::unix::fs::FileExt;
        let file = guard.file();
        let len = file.metadata().map_err(|_| Refusal::Layout)?.len();
        if len > 16384 {
            return Err(Refusal::Plan);
        }
        let mut bytes = super::pki::custody::voter_transfer::Wipe(vec![0; len as usize]);
        file.read_exact_at(&mut bytes.0, 0)
            .map_err(|_| Refusal::Plan)?;
        guard.recheck()?;
        if file.metadata().map_err(|_| Refusal::Layout)?.len() != len {
            return Err(Refusal::Plan);
        }
        Ok(Sha256::digest(&bytes.0).into())
    }

    pub(super) fn history_base_objects(&self) -> Result<[[u64; 2]; 5]> {
        let path = self.plan.state.join("cluster-owner.lock");
        let outer = self.history_guard(&path)?;
        if outer.id != object(&self.lock)? || !outer.read_at(0)?.is_empty() {
            return Err(Refusal::Ownership);
        }
        let mut identities = [[0; 2]; 5];
        for (identity, path) in identities.iter_mut().zip([
            &self.plan.config,
            &self.plan.state,
            &self.plan.credentials,
            &self.plan.workspace,
            &path,
        ]) {
            let id = self.history_guard(path)?.id;
            *identity = [id.0, id.1];
        }
        Ok(identities)
    }

    pub(super) fn history_objects(&self, include_stores: bool) -> Result<Vec<[u64; 2]>> {
        let mut identities = self.history_base_objects()?.to_vec();
        let mut paths = vec![
            self.plan.credentials.join("identity.json"),
            self.plan.credentials.join("ca.der"),
            self.plan.credentials.join("member-key"),
        ];
        if include_stores {
            paths.extend([
                self.plan.state.join("cluster-state.lock"),
                self.plan.state.join("cluster-log.lock"),
                self.plan.state.join("cluster-state.sqlite3"),
                self.plan.state.join("cluster-log.sqlite3"),
            ]);
        }
        for path in paths {
            let id = self.history_guard(&path)?.id;
            identities.push([id.0, id.1]);
        }
        Ok(identities)
    }

    fn history_guard(&self, path: &Path) -> Result<&Guard> {
        let guard = self
            .guards
            .iter()
            .find(|guard| guard.path == path)
            .ok_or(Refusal::Layout)?;
        guard.recheck()?;
        if path != self.plan.workspace {
            crate::security::private(path).map_err(|_| Refusal::Layout)?;
        }
        Ok(guard)
    }
}

const STATE: &[&str] = &[
    "cluster-owner.lock",
    "cluster-state.lock",
    "cluster-log.lock",
    "cluster-state.sqlite3",
    "cluster-log.sqlite3",
    "cluster-state.sqlite3-journal",
    "cluster-log.sqlite3-journal",
    "cluster-state.sqlite3-wal",
    "cluster-log.sqlite3-wal",
    "cluster-state.sqlite3-shm",
    "cluster-log.sqlite3-shm",
];
impl Owner {
    pub(super) fn acquire(plan: Plan) -> Result<Self> {
        plan.validate()?;
        let mut guards = Vec::new();
        let mut root_ids = Vec::new();
        let mut ancestor_ids = Vec::new();
        for (path, directory) in [
            (&plan.config, false),
            (&plan.state, true),
            (&plan.credentials, true),
            (&plan.workspace, true),
        ] {
            let g = Guard::open(path, directory, path != &plan.workspace)?;
            if guards.iter().any(|old: &Guard| old.id == g.id) {
                return Err(Refusal::Layout);
            }
            root_ids.push(g.id);
            guards.push(g);
            let mut ancestor = path.parent();
            while let Some(p) = ancestor {
                if p.as_os_str().is_empty() {
                    break;
                }
                let g = Guard::open(p, true, false)?;
                ancestor_ids.push(g.id);
                guards.push(g);
                ancestor = p.parent();
            }
        }
        if root_ids.iter().any(|id| ancestor_ids.contains(id)) {
            return Err(Refusal::Layout);
        }
        let mut entries = Vec::new();
        for e in fs::read_dir(&plan.state)
            .map_err(|_| Refusal::Layout)?
            .take(STATE.len() + 1)
        {
            let e = e.map_err(|_| Refusal::Layout)?;
            let name = e.file_name().into_string().map_err(|_| Refusal::Layout)?;
            if !STATE.contains(&name.as_str()) {
                return Err(Refusal::Layout);
            }
            let g = Guard::open(&e.path(), false, true)?;
            if guards.iter().any(|old| old.id == g.id) {
                return Err(Refusal::Layout);
            }
            guards.push(g);
            entries.push(name);
        }
        let credential_entries: Vec<_> = fs::read_dir(&plan.credentials)
            .map_err(|_| Refusal::Layout)?
            .take(4)
            .collect();
        if !plan.mode.recovery() && !credential_entries.is_empty() {
            return Err(Refusal::Layout);
        }
        if plan.mode.recovery() || !entries.is_empty() {
            for name in [
                "cluster-owner.lock",
                "cluster-state.lock",
                "cluster-log.lock",
                "cluster-state.sqlite3",
                "cluster-log.sqlite3",
            ] {
                if !entries.iter().any(|v| v == name) {
                    return Err(Refusal::Layout);
                }
            }
            for name in &entries {
                if name.ends_with("-shm")
                    || ((name.ends_with("-wal") || name.ends_with("-journal"))
                        && fs::metadata(plan.state.join(name))
                            .map_err(|_| Refusal::Layout)?
                            .len()
                            != 0)
                {
                    return Err(Refusal::Store);
                }
            }
        }
        if plan.mode.recovery() {
            let mut names = Vec::new();
            for e in credential_entries {
                let e = e.map_err(|_| Refusal::Layout)?;
                let name = e.file_name().into_string().map_err(|_| Refusal::Layout)?;
                if name != "identity.json"
                    && name != "ca.der"
                    && !super::join_receipt::basename(&name)
                {
                    return Err(Refusal::Layout);
                }
                let g = Guard::open(&e.path(), false, true)?;
                if guards.iter().any(|old| old.id == g.id) {
                    return Err(Refusal::Layout);
                }
                guards.push(g);
                names.push(name);
            }
            if names.len() != 3
                || !names.iter().any(|v| v == "identity.json")
                || !names.iter().any(|v| v == "ca.der")
            {
                return Err(Refusal::Identity);
            }
        }
        let lock_path = plan.state.join("cluster-owner.lock");
        let mut o = options(false);
        o.write(true);
        if entries.is_empty() {
            o.create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                o.mode(0o600);
            }
        }
        let lock = o.open(&lock_path).map_err(|_| Refusal::Ownership)?;
        lock.try_lock().map_err(|_| Refusal::Ownership)?;
        guards.push(Guard::open(&lock_path, false, true)?);
        if !entries.iter().any(|v| v == "cluster-owner.lock") {
            entries.push("cluster-owner.lock".into());
        }
        let owner = Self {
            plan,
            guards,
            lock,
            entries,
            published_join: false,
            publication: Default::default(),
            bound_attempt: super::bound_open::BoundAttempt::Ready,
        };
        owner.recheck()?;
        Ok(owner)
    }
    pub(super) fn recheck(&self) -> Result<()> {
        for g in &self.guards {
            g.recheck()?;
            if g.path.parent() == Some(self.plan.state.as_path())
                && g.path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with("-journal"))
                && g.file.metadata().map_err(|_| Refusal::Layout)?.len() != 0
            {
                return Err(Refusal::Store);
            }
        }
        let mut entries = fs::read_dir(&self.plan.state)
            .map_err(|_| Refusal::Layout)?
            .take(STATE.len() + 1)
            .map(|e| {
                e.map_err(|_| Refusal::Layout)?
                    .file_name()
                    .into_string()
                    .map_err(|_| Refusal::Layout)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut expected = self.entries.clone();
        entries.sort();
        expected.sort();
        if entries != expected {
            return Err(Refusal::Layout);
        }
        let count = fs::read_dir(&self.plan.credentials)
            .map_err(|_| Refusal::Layout)?
            .take(4)
            .count();
        if count
            != if self.plan.mode.recovery() || self.published_join {
                3
            } else {
                0
            }
        {
            return Err(Refusal::Layout);
        }
        Ok(())
    }
    /// Original identities only; live store ownership composition remains deferred.
    pub(super) fn publication_originals(&self) -> Result<()> {
        for g in &self.guards {
            g.recheck()?;
        }
        for p in [&self.plan.credentials, &self.plan.state, &self.plan.config] {
            crate::security::private(p).map_err(|_| Refusal::Layout)?;
        }
        Ok(())
    }
    pub(super) fn publication_directory(&self) -> Result<File> {
        self.guards
            .iter()
            .find(|g| g.path == self.plan.credentials)
            .ok_or(Refusal::Layout)?
            .bound_duplicate()
    }
    pub(super) fn publication_capacity(&mut self) {
        self.guards.reserve(3);
    }
    pub(super) fn publication_install(&mut self, delta: &mut Vec<Guard>) {
        self.guards.append(delta);
    }
    pub(super) fn inspect_validate(&self) -> Result<()> {
        self.recheck()?;
        for name in [
            "cluster-owner.lock",
            "cluster-state.lock",
            "cluster-log.lock",
            "cluster-state.sqlite3",
            "cluster-log.sqlite3",
        ] {
            if !self.entries.iter().any(|entry| entry == name) {
                return Err(Refusal::Layout);
            }
        }
        for g in &self.guards {
            if g.path.parent() == Some(self.plan.state.as_path()) {
                crate::security::private(&g.path).map_err(|_| Refusal::Layout)?;
                let name = g
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or(Refusal::Layout)?;
                if name.ends_with("-shm")
                    || ((name.ends_with("-wal") || name.ends_with("-journal"))
                        && g.file.metadata().map_err(|_| Refusal::Layout)?.len() != 0)
                {
                    return Err(Refusal::Store);
                }
            }
        }
        self.recheck()
    }
    /// Accept only the original fresh Join owner and its empty lock-only state.
    #[cfg(any(test, target_os = "linux"))]
    pub(super) fn validate_fresh_join(&self) -> Result<()> {
        self.recheck()?;
        if self.plan.mode != Mode::Join || self.entries != ["cluster-owner.lock"] {
            return Err(Refusal::Provenance);
        }
        let path = self.plan.state.join("cluster-owner.lock");
        let guard = self
            .guards
            .iter()
            .find(|guard| guard.path == path)
            .ok_or(Refusal::Ownership)?;
        if guard.id != object(&self.lock)? || !guard.read_at(0)?.is_empty() {
            return Err(Refusal::Ownership);
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    pub(super) fn stage_published_join(&mut self) -> Result<()> {
        if self.plan.mode != Mode::Join
            || self.published_join
            || self.entries != ["cluster-owner.lock"]
        {
            return Err(Refusal::Provenance);
        }
        for name in ["ca.der", "identity.json", "member-key"] {
            self.read(name, 16384)?;
        }
        let lock = self
            .guards
            .iter()
            .find(|guard| guard.path == self.plan.state.join("cluster-owner.lock"))
            .ok_or(Refusal::Ownership)?;
        if lock.id != object(&self.lock)? || !lock.read_at(0)?.is_empty() {
            return Err(Refusal::Ownership);
        }
        self.published_join = true;
        self.recheck()
    }

    pub(super) fn published_baseline(&self) -> Result<()> {
        self.recheck()?;
        if !self.published_join
            || self.plan.mode != Mode::Join
            || self.entries != ["cluster-owner.lock"]
        {
            return Err(Refusal::Provenance);
        }
        Ok(())
    }

    pub(super) fn fresh_baseline(&self) -> Result<()> {
        self.recheck()?;
        if self.entries != ["cluster-owner.lock"] || self.plan.mode != Mode::CreateFounder {
            return Err(Refusal::Provenance);
        }
        Ok(())
    }
    /// Fixed fresh-only delta; preserve all original guards and the outer lock.
    pub(super) fn capture_created_stores(&mut self) -> Result<()> {
        if self.entries != ["cluster-owner.lock"] || self.plan.mode != Mode::CreateFounder {
            return Err(Refusal::Provenance);
        }
        for g in &self.guards {
            g.recheck()?;
        }
        if fs::read_dir(&self.plan.credentials)
            .map_err(|_| Refusal::Layout)?
            .next()
            .is_some()
        {
            return Err(Refusal::Layout);
        }
        let required = [
            "cluster-owner.lock",
            "cluster-log.lock",
            "cluster-state.lock",
            "cluster-log.sqlite3",
            "cluster-state.sqlite3",
        ];
        let journals = [
            "cluster-log.sqlite3-journal",
            "cluster-state.sqlite3-journal",
        ];
        let mut entries = Vec::new();
        let mut staged: Vec<Guard> = Vec::new();
        for entry in fs::read_dir(&self.plan.state)
            .map_err(|_| Refusal::Layout)?
            .take(8)
        {
            let entry = entry.map_err(|_| Refusal::Layout)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Refusal::Layout)?;
            if !required.contains(&name.as_str()) && !journals.contains(&name.as_str()) {
                return Err(Refusal::Layout);
            }
            if name != "cluster-owner.lock" {
                let g = Guard::open(&entry.path(), false, true)?;
                if self.guards.iter().chain(&staged).any(|old| old.id == g.id) {
                    return Err(Refusal::Layout);
                }
                if journals.contains(&name.as_str())
                    && g.file.metadata().map_err(|_| Refusal::Layout)?.len() != 0
                {
                    return Err(Refusal::Store);
                }
                staged.push(g);
                #[cfg(test)]
                tests::capture_stage(staged.len(), self)?;
            }
            entries.push(name);
        }
        if required
            .iter()
            .any(|name| !entries.iter().any(|e| e == name))
        {
            return Err(Refusal::Layout);
        }
        for g in self.guards.iter().chain(&staged) {
            g.recheck()?;
            if g.path.parent() == Some(self.plan.state.as_path())
                && g.path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with("-journal"))
                && g.file.metadata().map_err(|_| Refusal::Layout)?.len() != 0
            {
                return Err(Refusal::Store);
            }
        }
        self.guards.extend(staged);
        self.entries = entries;
        Ok(())
    }
    pub(super) fn bound_guard(&self, name: &str) -> Result<File> {
        let path = self.plan.state.join(name);
        let guard = self
            .guards
            .iter()
            .find(|g| g.path == path)
            .ok_or(Refusal::Layout)?;
        guard.recheck()?;
        guard.file.try_clone().map_err(|_| Refusal::Unavailable)
    }
    pub(super) fn bound_create(&self, name: &str) -> Result<Guard> {
        if ![
            "cluster-state.lock",
            "cluster-log.lock",
            "cluster-state.sqlite3",
            "cluster-log.sqlite3",
        ]
        .contains(&name)
        {
            return Err(Refusal::Layout);
        }
        // checked_path requires an existing object. Only the fixed leaf is new.
        crate::config::checked_path(&self.plan.state).map_err(|_| Refusal::Layout)?;
        for guard in &self.guards {
            guard.recheck()?;
        }
        let path = self.plan.state.join(name);
        let mut o = options(false);
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let file = o.open(&path).map_err(|_| Refusal::Layout)?;
        let id = object(&file)?;
        Ok(Guard { path, file, id })
    }
    pub(super) fn bound_validate(&self, staged: &[Guard], complete: bool) -> Result<()> {
        for g in self.guards.iter().chain(staged) {
            g.recheck()?;
            if g.path.parent() == Some(self.plan.state.as_path()) {
                crate::security::private(&g.path).map_err(|_| Refusal::Layout)?;
            }
        }
        let mut expected = self.entries.clone();
        for g in staged {
            let name = g
                .path
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or(Refusal::Layout)?;
            if expected.iter().any(|v| v == name) {
                return Err(Refusal::Layout);
            }
            expected.push(name.to_owned());
        }
        let mut actual = Vec::new();
        for entry in fs::read_dir(&self.plan.state)
            .map_err(|_| Refusal::Layout)?
            .take(STATE.len() + 1)
        {
            let e = entry.map_err(|_| Refusal::Layout)?;
            let name = e.file_name().into_string().map_err(|_| Refusal::Layout)?;
            if !expected.contains(&name) {
                if !complete
                    || ![
                        "cluster-state.sqlite3-journal",
                        "cluster-log.sqlite3-journal",
                    ]
                    .contains(&name.as_str())
                {
                    return Err(Refusal::Layout);
                }
                let g = Guard::open(&e.path(), false, true)?;
                if g.file.metadata().map_err(|_| Refusal::Layout)?.len() != 0 {
                    return Err(Refusal::Store);
                }
            } else if name.ends_with("-wal")
                || name.ends_with("-shm")
                || (name.ends_with("-journal")
                    && e.metadata().map_err(|_| Refusal::Layout)?.len() != 0)
            {
                return Err(Refusal::Store);
            }
            actual.push(name);
        }
        if expected.iter().any(|n| !actual.contains(n)) {
            return Err(Refusal::Layout);
        }
        if fs::read_dir(&self.plan.credentials)
            .map_err(|_| Refusal::Layout)?
            .take(4)
            .count()
            != if self.plan.mode.recovery() || self.published_join {
                3
            } else {
                0
            }
        {
            return Err(Refusal::Layout);
        }
        Ok(())
    }
    pub(super) fn bound_install(&mut self, staged: &mut Vec<Guard>) -> Result<()> {
        #[cfg(test)]
        super::bound_open::tests::staging(self);
        self.bound_validate(staged, true)?;
        let mut journals = Vec::new();
        for name in [
            "cluster-state.sqlite3-journal",
            "cluster-log.sqlite3-journal",
        ] {
            if !self.entries.iter().any(|v| v == name) && self.plan.state.join(name).exists() {
                journals.push(Guard::open(&self.plan.state.join(name), false, true)?);
            }
        }
        // Allocate before changing the original baseline; only field moves follow.
        let mut entries = self.entries.clone();
        entries.extend(
            staged
                .iter()
                .chain(&journals)
                .map(|g| g.path.file_name().unwrap().to_str().unwrap().to_owned()),
        );
        self.guards.reserve(staged.len() + journals.len());
        self.guards.append(staged);
        self.guards.append(&mut journals);
        self.entries = entries;
        Ok(())
    }
    pub(super) fn has_stores(&self) -> bool {
        self.entries.iter().any(|n| n == "cluster-state.sqlite3")
    }
    pub(super) fn read(&self, name: &str, max: u64) -> Result<Vec<u8>> {
        let path = self.plan.credentials.join(name);
        let g = self
            .guards
            .iter()
            .find(|g| g.path == path)
            .ok_or(Refusal::Identity)?;
        g.recheck()?;
        let bytes = g.read_at(max)?;
        g.recheck()?;
        Ok(bytes)
    }
}

#[cfg(test)]
pub(in crate::cluster) mod tests {
    use super::*;
    thread_local! {
        static CAPTURE_FAIL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static CAPTURE_EFFECT: std::cell::RefCell<Vec<(String, Vec<u8>)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    pub(in crate::cluster) struct CaptureFault;
    impl CaptureFault {
        pub(in crate::cluster) fn new() -> Self {
            CAPTURE_FAIL.with(|s| {
                assert!(!s.replace(true));
            });
            Self
        }
    }
    impl Drop for CaptureFault {
        fn drop(&mut self) {
            CAPTURE_FAIL.with(|s| s.set(false));
        }
    }
    pub(in crate::cluster) fn capture_effect() -> Vec<(String, Vec<u8>)> {
        CAPTURE_EFFECT.with(|s| s.borrow().clone())
    }
    pub(super) fn capture_stage(count: usize, owner: &Owner) -> Result<()> {
        if count == 2 && CAPTURE_FAIL.with(|s| s.get()) {
            let mut rows = fs::read_dir(&owner.plan.state)
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    (
                        e.file_name().into_string().unwrap(),
                        if e.metadata().unwrap().len() == 0 {
                            Vec::new()
                        } else {
                            fs::read(e.path()).unwrap()
                        },
                    )
                })
                .collect::<Vec<_>>();
            rows.sort();
            CAPTURE_EFFECT.with(|s| *s.borrow_mut() = rows);
            Err(Refusal::Clock)
        } else {
            Ok(())
        }
    }
    #[test]
    fn inspection_requires_complete_original_layout() {
        let l = Layout::new();
        let owner = Owner::acquire(l.plan("create-founder")).unwrap();
        let before = l.snapshot();
        assert_eq!(owner.inspect_validate(), Err(Refusal::Layout));
        assert_eq!(l.snapshot(), before);
    }
    pub(in crate::cluster) fn guards(owner: &Owner) -> Vec<(PathBuf, u64, u64)> {
        owner
            .guards
            .iter()
            .map(|g| {
                g.recheck().unwrap();
                (g.path.clone(), g.id.0, g.id.1)
            })
            .collect()
    }
    pub(in crate::cluster) fn baseline(owner: &Owner) -> (usize, Vec<String>) {
        (owner.guards.len(), owner.entries.clone())
    }
    pub(in crate::cluster) fn objects(dir: &Path) -> Vec<(String, u64, u64, bool, u64)> {
        let mut entries = fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                let f = options(false).open(e.path()).unwrap();
                let id = object(&f).unwrap();
                let m = f.metadata().unwrap();
                (
                    e.file_name().into_string().unwrap(),
                    id.0,
                    id.1,
                    m.is_file(),
                    m.len(),
                )
            })
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }
    pub(in crate::cluster) struct Layout {
        pub root: tempfile::TempDir,
    }
    impl Layout {
        pub(in crate::cluster) fn new() -> Self {
            let root = crate::service::application::tests::private_temp();
            for name in ["state", "credentials", "workspace"] {
                fs::create_dir(root.path().join(name)).unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(root.path().join(name), fs::Permissions::from_mode(0o700))
                        .unwrap();
                }
            }
            let l = Self { root };
            l.write("config", b"{}");
            l
        }
        pub(in crate::cluster) fn write(&self, name: &str, bytes: &[u8]) {
            let path = self.root.path().join(name);
            fs::write(&path, bytes).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        pub(in crate::cluster) fn plan(&self, mode: &str) -> Plan {
            let r = self.root.path();
            Plan::parse(&serde_json::to_vec(&serde_json::json!({"mode":mode,"config":r.join("config"),"state":r.join("state"),"credentials":r.join("credentials"),"workspace":r.join("workspace"),"peer":"127.0.0.1:7001","enrollment":"127.0.0.1:7002"})).unwrap()).unwrap()
        }
        pub(in crate::cluster) fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
            let mut all = Vec::new();
            for dir in ["state", "credentials"] {
                for entry in fs::read_dir(self.root.path().join(dir)).unwrap() {
                    let e = entry.unwrap();
                    let bytes = if e.metadata().unwrap().len() == 0 {
                        Vec::new()
                    } else {
                        fs::read(e.path()).unwrap()
                    };
                    all.push((format!("{dir}/{}", e.file_name().to_str().unwrap()), bytes));
                }
            }
            all.sort();
            all
        }
    }
    #[test]
    fn retained_reads_have_independent_offsets() {
        let l = Layout::new();
        // Inventory-only fixture; these are not claimed as valid SQLite stores.
        for name in [
            "cluster-owner.lock",
            "cluster-state.lock",
            "cluster-log.lock",
            "cluster-state.sqlite3",
            "cluster-log.sqlite3",
        ] {
            l.write(&format!("state/{name}"), b"");
        }
        l.write("credentials/identity.json", b"receipt");
        l.write("credentials/ca.der", b"root");
        l.write("credentials/member-key", b"sealed");
        let owner = Owner::acquire(l.plan("recover-joiner")).unwrap();
        assert_eq!(owner.read("identity.json", 16).unwrap(), b"receipt");
        assert_eq!(owner.read("identity.json", 16).unwrap(), b"receipt");
        assert!(owner.read("identity.json", 2).is_err());
    }
    #[test]
    fn strict_plan_and_endpoints() {
        let l = Layout::new();
        let p = l.plan("create-founder");
        assert_eq!(p.mode, Mode::CreateFounder);
        for s in [
            "0.0.0.0:1",
            "127.0.0.1:0",
            "localhost:3",
            "169.254.1.2:4",
            "[::ffff:127.0.0.1]:5",
            "[fe80::1%3]:6",
        ] {
            assert!(endpoint(s).is_err(), "{s}");
        }
        assert!(Plan::parse(&vec![b' '; 16385]).is_err());
        assert!(Plan::parse(br#"{"mode":"auto"}"#).is_err());
    }
    #[test]
    fn retained_owner_duplicate_and_occupied_fresh() {
        let l = Layout::new();
        let o = Owner::acquire(l.plan("join")).unwrap();
        let before = l.snapshot();
        assert!(Owner::acquire(l.plan("join")).is_err());
        assert_eq!(l.snapshot(), before);
        let path = l.root.path().join("state/cluster-owner.lock");
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        assert!(f.try_lock().is_err());
        drop(o);
        assert!(f.try_lock().is_ok());
    }
    #[test]
    fn overlap_alias_and_object_replacement() {
        let l = Layout::new();
        let mut p = l.plan("join");
        p.credentials = p.state.clone();
        assert!(Owner::acquire(p).is_err());
        #[cfg(windows)]
        {
            let l = Layout::new();
            let mut p = l.plan("join");
            fs::create_dir(p.state.join("nested")).unwrap();
            p.credentials = PathBuf::from(p.state.join("nested").to_str().unwrap().to_uppercase());
            assert!(Owner::acquire(p).is_err());
            let l = Layout::new();
            fs::hard_link(
                l.root.path().join("config"),
                l.root.path().join("state/cluster-owner.lock"),
            )
            .unwrap();
            assert!(Owner::acquire(l.plan("join")).is_err());
        }
        let l = Layout::new();
        let o = Owner::acquire(l.plan("join")).unwrap();
        l.write("state/unexpected", b"x");
        assert_eq!(o.recheck(), Err(Refusal::Layout));
        // Handles prevent replacement on Windows; Unix detects changed object IDs.
        let path = l.root.path().join("config");
        let renamed = fs::rename(&path, l.root.path().join("old-config"));
        #[cfg(windows)]
        assert!(renamed.is_err());
        #[cfg(unix)]
        {
            renamed.unwrap();
            l.write("config", b"{}");
            assert_eq!(o.recheck(), Err(Refusal::Layout));
        }
    }
    #[cfg(unix)]
    #[test]
    fn reparse_and_hardlink_refuse() {
        use std::os::unix::fs::symlink;
        let l = Layout::new();
        symlink(
            l.root.path().join("config"),
            l.root.path().join("state/cluster-owner.lock"),
        )
        .unwrap();
        assert!(Owner::acquire(l.plan("join")).is_err());
        let l = Layout::new();
        fs::hard_link(
            l.root.path().join("config"),
            l.root.path().join("state/cluster-owner.lock"),
        )
        .unwrap();
        assert!(Owner::acquire(l.plan("join")).is_err());
    }
}
