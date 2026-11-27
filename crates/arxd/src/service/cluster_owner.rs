//! Private Linux owner control. Lifecycle integration is deferred to 13d.
#![allow(dead_code)] // The daemon does not bind this endpoint in 13c.

use super::Shutdown;
use crate::cluster::state_store::LogPos;
use std::{
    ffi::CString,
    future::Future,
    io,
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    task::JoinSet,
    time::timeout,
};

pub(crate) const MAX_PASTE: usize = 8_800;
const MAGIC: &[u8; 4] = b"ARXO";
const HEADER: usize = 7;
const HANDSHAKE: Duration = Duration::from_secs(2);
pub(super) const REPLY_WAIT: Duration = Duration::from_secs(16);
pub(super) const PRIVATE_WORK_WAIT: Duration = REPLY_WAIT.saturating_sub(HANDSHAKE);
const MAX_CONNECTIONS: usize = 4;
const TIME_EVIDENCE: u8 = 5;
const TIME_REPLY: u8 = 16;
const MINT_RECEIPT: u8 = 6;
const OBSERVE_LEDGER: u8 = 7;
const MINT_RECEIPT_REPLY: u8 = 17;
const LEDGER_REPLY: u8 = 18;
pub(crate) const LEDGER_VERSION: u8 = 1;
const SLOT_LEN: usize = 56;
const SLOT_COUNT: usize = 4;
const LEDGER_REQUEST_LEN: usize = 2 + SLOT_COUNT * SLOT_LEN;
pub(crate) const LEDGER_REPLY_LEN: usize = 164;
pub(crate) const TIME_EVIDENCE_LEN: usize = 53;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Refused,
    Unknown,
}

/// One allocation of bounded sensitive bytes. No Debug or implicit conversions.
pub(crate) struct Paste(Box<[u8; MAX_PASTE]>, usize);

impl Paste {
    pub(crate) fn empty() -> Self {
        Self(Box::new([0; MAX_PASTE]), 0)
    }

    pub(crate) fn copy(bytes: &[u8]) -> Option<Self> {
        let mut paste = Self::empty();
        paste.extend(bytes).then_some(paste)
    }

    pub(super) fn extend(&mut self, bytes: &[u8]) -> bool {
        let end = self.1 + bytes.len();
        let Some(target) = self.0.get_mut(self.1..end) else {
            return false;
        };
        target.copy_from_slice(bytes);
        self.1 = end;
        true
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0[..self.1]
    }

    fn clear(&mut self) {
        wipe(self.0.as_mut_slice());
        self.1 = 0;
    }

    #[cfg(test)]
    pub(crate) async fn read_probe_pipe(
        mut pipe: impl tokio::io::AsyncRead + Unpin,
    ) -> io::Result<Self> {
        let mut paste = Self::empty();
        while paste.1 < MAX_PASTE {
            let read = pipe.read(&mut paste.0[paste.1..]).await?;
            if read == 0 {
                return Ok(paste);
            }
            paste.1 += read;
        }
        let mut trailing = InputHeader([0; HEADER]);
        if pipe.read(&mut trailing.0[..1]).await? != 0 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(paste)
    }
}

impl Drop for Paste {
    fn drop(&mut self) {
        self.clear();
    }
}

pub(super) fn wipe(bytes: &mut [u8]) {
    for byte in bytes {
        // SAFETY: each byte is uniquely borrowed and valid for the write.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
}

pub(crate) enum Request {
    Mint,
    Join(Paste),
    MintReceipt,
    ObserveLedger(Box<LedgerSlots>),
}

pub(crate) enum Reply {
    Minted(Paste),
    Joined,
    Refused,
    Unknown,
}

// Keep the owner CLI's original reply type closed over its two operations.
pub(crate) enum DispatchReply {
    Control(Reply),
    MintReceipt(MintReceipt),
    Ledger([u8; LEDGER_REPLY_LEN]),
}

impl From<Reply> for DispatchReply {
    fn from(reply: Reply) -> Self {
        Self::Control(reply)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MintReceipt {
    pub(crate) position: LogPos,
    pub(crate) digest: [u8; 32],
}

pub(crate) fn position_bytes(position: LogPos) -> [u8; 24] {
    let mut bytes = [0; 24];
    bytes[..8].copy_from_slice(&position.term.to_be_bytes());
    bytes[8..16].copy_from_slice(&position.leader.to_be_bytes());
    bytes[16..].copy_from_slice(&position.index.to_be_bytes());
    bytes
}

impl MintReceipt {
    fn encode(self) -> [u8; SLOT_LEN] {
        let mut bytes = [0; SLOT_LEN];
        bytes[..24].copy_from_slice(&position_bytes(self.position));
        bytes[24..].copy_from_slice(&self.digest);
        bytes
    }

    fn decode(bytes: &[u8; SLOT_LEN]) -> Self {
        let word = |start| u64::from_be_bytes(bytes[start..start + 8].try_into().unwrap());
        Self {
            position: LogPos {
                term: word(0),
                leader: word(8),
                index: word(16),
            },
            digest: bytes[24..].try_into().unwrap(),
        }
    }
}

pub(crate) struct LedgerSlots {
    slots: [MintReceipt; SLOT_COUNT],
    count: usize,
}

impl LedgerSlots {
    pub(crate) fn receipts(&self) -> &[MintReceipt] {
        &self.slots[..self.count]
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; LEDGER_REQUEST_LEN] = bytes.try_into().ok()?;
        let count = usize::from(bytes[1]);
        if bytes[0] != LEDGER_VERSION || !matches!(count, 2 | 4) {
            return None;
        }
        let slots = std::array::from_fn(|i| {
            let offset = 2 + i * SLOT_LEN;
            MintReceipt::decode(bytes[offset..offset + SLOT_LEN].try_into().unwrap())
        });
        for (i, slot) in slots[..count].iter().enumerate() {
            let index = slot.position.index;
            if !(1..=i64::MAX as u64).contains(&index)
                || index.checked_add(1).is_none()
                || (i > 0 && slots[i - 1].position.index >= index)
                || slots[..i].iter().any(|p| p.digest == slot.digest)
            {
                return None;
            }
        }
        if bytes[2 + count * SLOT_LEN..].iter().any(|&b| b != 0) {
            return None;
        }
        Some(Self { slots, count })
    }

    #[cfg(test)]
    pub(crate) fn for_test(receipts: &[MintReceipt]) -> Self {
        let mut bytes = [0; LEDGER_REQUEST_LEN];
        bytes[..2].copy_from_slice(&[LEDGER_VERSION, receipts.len().try_into().unwrap()]);
        for (slot, receipt) in bytes[2..].chunks_exact_mut(SLOT_LEN).zip(receipts) {
            slot.copy_from_slice(&receipt.encode());
        }
        Self::decode(&bytes).unwrap()
    }
}

/// Constructed only from a checked, kernel-authenticated connection.
pub(crate) struct Operator(u32);

impl Operator {
    pub(crate) fn uid(&self) -> u32 {
        self.0
    }

    /// Linux in-process campaigns only. Outside tests the checked connection
    /// remains the sole constructor.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn for_campaign(uid: u32) -> Self {
        Self(uid)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Roles {
    pub(crate) owner: u32,
    pub(crate) daemon: u32,
    pub(crate) agent: u32,
    pub(crate) build: u32,
    pub(crate) group: u32,
}

impl Roles {
    pub(super) fn qualify(self) -> Result<(), Failure> {
        let ids = [self.owner, self.daemon, self.agent, self.build];
        for (index, uid) in ids.iter().enumerate() {
            if *uid == 0 || *uid == u32::MAX || ids[..index].contains(uid) {
                return Err(Failure::Refused);
            }
        }
        if self.group == 0
            || self.group == u32::MAX
            || !groups(self.owner)?.contains(&self.group)
            || groups(self.agent)?.contains(&self.group)
            || groups(self.build)?.contains(&self.group)
        {
            return Err(Failure::Refused);
        }
        Ok(())
    }
}

/// Current primary and supplementary groups of a local account, for owner
/// publication's effective search-permission check.
pub(super) fn account_groups(uid: u32) -> Option<Vec<u32>> {
    groups(uid).ok()
}

fn groups(uid: u32) -> Result<Vec<libc::gid_t>, Failure> {
    let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buffer = [0u8; 16_384];
    // SAFETY: output pointers and backing storage remain valid until the lookup completes.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return Err(Failure::Refused);
    }
    // SAFETY: a successful lookup initialized the record in the caller's storage.
    let record = unsafe { record.assume_init() };
    let mut ids = [0; 128];
    let mut count = ids.len() as libc::c_int;
    // SAFETY: the password record's name remains backed by buffer and ids has count slots.
    if unsafe { libc::getgrouplist(record.pw_name, record.pw_gid, ids.as_mut_ptr(), &mut count) }
        < 0
    {
        return Err(Failure::Refused);
    }
    ids.get(..count as usize)
        .map(<[_]>::to_vec)
        .ok_or(Failure::Refused)
}

pub(super) fn address(path: &Path) -> bool {
    path.is_absolute()
        && path.as_os_str().len() <= 100
        && path
            .as_os_str()
            .as_bytes()
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'.'))
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str()
        && path.parent().is_some_and(|p| p != Path::new("/"))
}

/// Bounded startup evidence. It contains no path, identity, or peer input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BindError {
    Policy(&'static str),
    DirectoryMode(u32),
    Io {
        operation: &'static str,
        kind: io::ErrorKind,
        errno: Option<i32>,
    },
}

impl BindError {
    fn io(operation: &'static str) -> impl FnOnce(io::Error) -> Self {
        move |error| Self::Io {
            operation,
            kind: error.kind(),
            errno: error.raw_os_error(),
        }
    }
}

fn no_acl(path: &Path) -> Result<(), BindError> {
    let name =
        CString::new(path.as_os_str().as_bytes()).map_err(|_| BindError::Policy("pathname"))?;
    for attr in [c"system.posix_acl_access", c"system.posix_acl_default"] {
        // SAFETY: both strings are terminated. A null/zero buffer queries only the length.
        if unsafe { libc::lgetxattr(name.as_ptr(), attr.as_ptr(), std::ptr::null_mut(), 0) } >= 0 {
            return Err(BindError::Policy("extended ACL"));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENODATA) {
            return Err(BindError::io("read ACL")(error));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Object(u64, u64);

/// Read-only ADR 0046 owner-route qualification (owners, modes and POSIX
/// ACLs of every socket ancestor) for owner publication before any bind.
pub(super) fn route_qualified(path: &Path, owner: u32, daemon: u32, group: u32) -> bool {
    route(path, owner, daemon, Some(group)).is_ok()
}

fn route(
    path: &Path,
    owner: u32,
    daemon: u32,
    group: Option<u32>,
) -> Result<Vec<Object>, BindError> {
    if !address(path) {
        return Err(BindError::Policy("pathname"));
    }
    let parent = path.parent().ok_or(BindError::Policy("pathname"))?;
    let mut identities = Vec::new();
    for p in parent.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let m = std::fs::symlink_metadata(p).map_err(BindError::io("stat directory"))?;
        let mode = m.mode() & 0o7777;
        if !m.is_dir() || ![0, daemon, owner].contains(&m.uid()) {
            return Err(BindError::Policy("directory type or owner"));
        }
        if p == parent {
            if m.uid() != daemon || group.is_some_and(|g| g != m.gid()) {
                return Err(BindError::Policy("directory owner or group"));
            }
            if mode != 0o2710 {
                return Err(BindError::DirectoryMode(mode));
            }
        } else if mode & 0o022 != 0 && !(m.uid() == 0 && mode & 0o1000 != 0) {
            // A root-owned sticky ancestor cannot rename a trusted child as a foreign UID.
            return Err(BindError::Policy("writable ancestor"));
        }
        no_acl(p)?;
        identities.push(Object(m.dev(), m.ino()));
    }
    Ok(identities)
}

fn socket(path: &Path, daemon: u32) -> Result<Object, BindError> {
    let m = std::fs::symlink_metadata(path).map_err(BindError::io("stat socket"))?;
    let dir = std::fs::symlink_metadata(path.parent().ok_or(BindError::Policy("pathname"))?)
        .map_err(BindError::io("stat socket directory"))?;
    if !m.file_type().is_socket()
        || m.nlink() != 1
        || m.uid() != daemon
        || m.gid() != dir.gid()
        || m.mode() & 0o7777 != 0o660
    {
        return Err(BindError::Policy(
            "socket type, links, owner, group or mode",
        ));
    }
    no_acl(path)?;
    Ok(Object(m.dev(), m.ino()))
}

pub(crate) struct Server {
    listener: UnixListener,
    path: PathBuf,
    roles: Roles,
    route: Vec<Object>,
    object: Object,
}

impl Server {
    pub(crate) fn bind(path: PathBuf, roles: Roles) -> Result<Self, BindError> {
        roles
            .qualify()
            .map_err(|_| BindError::Policy("control roles"))?;
        // SAFETY: identity queries take no pointers.
        if unsafe { libc::geteuid() } != roles.daemon {
            return Err(BindError::Policy("daemon UID"));
        }
        let identities = route(&path, roles.owner, roles.daemon, Some(roles.group))?;
        // Socket creation is exclusive. A stale or swapped object is never unlinked here.
        let listener = UnixListener::bind(&path).map_err(BindError::io("bind socket"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660))
            .map_err(BindError::io("set socket mode"))?;
        let server = Self {
            listener,
            object: socket(&path, roles.daemon)?,
            path,
            roles,
            route: identities,
        };
        if !server.original() {
            return Err(BindError::Policy("route replaced during bind"));
        }
        Ok(server)
    }

    fn original(&self) -> bool {
        route(
            &self.path,
            self.roles.owner,
            self.roles.daemon,
            Some(self.roles.group),
        )
        .is_ok_and(|objects| objects == self.route)
            && socket(&self.path, self.roles.daemon).is_ok_and(|object| object == self.object)
    }

    #[cfg(test)]
    pub(crate) fn start<F, Work>(self, dispatch: F) -> Control
    where
        F: Fn(Operator, Request) -> Work + Clone + Send + 'static,
        Work: Future<Output = Reply> + Send,
    {
        self.start_with_stop(
            move |operator, request| {
                let work = dispatch(operator, request);
                async move { work.await.into() }
            },
            || [0; 29],
            || [0; TIME_EVIDENCE_LEN],
            Shutdown::default(),
        )
    }

    pub(crate) fn start_with_stop<F, Work, S, T>(
        self,
        dispatch: F,
        status: S,
        time_evidence: T,
        stop: Shutdown,
    ) -> Control
    where
        F: Fn(Operator, Request) -> Work + Clone + Send + 'static,
        Work: Future<Output = DispatchReply> + Send,
        S: Fn() -> [u8; 29] + Clone + Send + 'static,
        T: Fn() -> [u8; TIME_EVIDENCE_LEN] + Clone + Send + 'static,
    {
        let requested = stop.clone();
        let supervisor =
            tokio::spawn(
                async move { self.serve(requested, dispatch, status, time_evidence).await },
            );
        Control {
            stop,
            supervisor,
            outcome: None,
        }
    }

    async fn serve<F, Work, S, T>(
        &self,
        stop: Shutdown,
        dispatch: F,
        status: S,
        time_evidence: T,
    ) -> Result<(), Failure>
    where
        F: Fn(Operator, Request) -> Work + Clone + Send + 'static,
        Work: Future<Output = DispatchReply> + Send,
        S: Fn() -> [u8; 29] + Clone + Send + 'static,
        T: Fn() -> [u8; TIME_EVIDENCE_LEN] + Clone + Send + 'static,
    {
        let mut work = JoinSet::new();
        let mut outcome = Ok(());
        loop {
            tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                Some(joined) = work.join_next(), if !work.is_empty() => {
                    if joined.is_err() {
                        outcome = Err(Failure::Unknown);
                        break;
                    }
                }
                incoming = self.listener.accept() => {
                    let Ok((stream, _)) = incoming else {
                        outcome = Err(Failure::Refused);
                        break;
                    };
                    if work.len() == MAX_CONNECTIONS {
                        continue;
                    }
                    if !self.original() {
                        outcome = Err(Failure::Refused);
                        break;
                    }
                    if peer(&stream, self.roles.owner).is_err() {
                        continue;
                    }
                    let dispatch = dispatch.clone();
                    let stop = stop.clone();
                    let owner = self.roles.owner;
                    let status = status.clone();
                    let time_evidence = time_evidence.clone();
                    work.spawn(async move {
                        connection_with_status(stream, owner, stop, dispatch, status, time_evidence).await;
                    });
                }
            }
        }
        stop.cancel();
        while let Some(joined) = work.join_next().await {
            if joined.is_err() {
                outcome = Err(Failure::Unknown);
            }
        }
        outcome
    }
}

/// Dropping observation stops intake and detaches the supervisor, which keeps
/// ownership of admitted work. Close observation cannot abort that supervisor.
pub(crate) struct Control {
    stop: Shutdown,
    supervisor: tokio::task::JoinHandle<Result<(), Failure>>,
    outcome: Option<Result<(), Failure>>,
}

impl Control {
    pub(crate) fn stop(&self) {
        self.stop.cancel();
    }

    #[cfg(test)]
    pub(crate) fn observe_task(supervisor: tokio::task::JoinHandle<Result<(), Failure>>) -> Self {
        Self {
            stop: Shutdown::default(),
            supervisor,
            outcome: None,
        }
    }

    pub(crate) async fn close(&mut self, observation: Duration) -> Result<(), Failure> {
        self.stop();
        if let Some(outcome) = self.outcome {
            return outcome;
        }
        let outcome = timeout(observation, &mut self.supervisor)
            .await
            .map_err(|_| Failure::Unknown)?
            .unwrap_or(Err(Failure::Unknown));
        self.outcome = Some(outcome);
        outcome
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.original() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
async fn connection<F, Work>(stream: UnixStream, uid: u32, stop: Shutdown, dispatch: F)
where
    F: FnOnce(Operator, Request) -> Work,
    Work: Future<Output = Reply>,
{
    connection_with_status(
        stream,
        uid,
        stop,
        |operator, request| async move { dispatch(operator, request).await.into() },
        || [0; 29],
        || [0; TIME_EVIDENCE_LEN],
    )
    .await;
}

async fn connection_with_status<F, Work, S, T>(
    mut stream: UnixStream,
    uid: u32,
    stop: Shutdown,
    dispatch: F,
    status: S,
    time_evidence: T,
) where
    F: FnOnce(Operator, Request) -> Work,
    Work: Future<Output = DispatchReply>,
    S: FnOnce() -> [u8; 29],
    T: FnOnce() -> [u8; TIME_EVIDENCE_LEN],
{
    let request = tokio::select! {
        biased;
        _ = stop.cancelled() => return,
        request = timeout(HANDSHAKE, receive(&mut stream)) => request,
    };
    let reply = match request {
        Ok(Ok((4, body))) if body.bytes().is_empty() && !stop.is_cancelled() => {
            let _ = timeout(HANDSHAKE, send(&mut stream, 15, &status())).await;
            return;
        }
        Ok(Ok((TIME_EVIDENCE, body)))
            if body.bytes().is_empty() && !stop.is_cancelled() && peer(&stream, uid).is_ok() =>
        {
            let _ = timeout(HANDSHAKE, send(&mut stream, TIME_REPLY, &time_evidence())).await;
            return;
        }
        Ok(Ok((tag, body)))
            if !stop.is_cancelled()
                && (!matches!(tag, MINT_RECEIPT | OBSERVE_LEDGER)
                    || peer(&stream, uid).is_ok()) =>
        {
            match decode_request(tag, body) {
                Ok(request) => dispatch(Operator(uid), request).await,
                Err(_) => Reply::Refused.into(),
            }
        }
        _ => Reply::Refused.into(),
    };
    // A transport failure cannot resubmit any command or release a paste elsewhere.
    let _ = timeout(HANDSHAKE, async {
        match reply {
            DispatchReply::Control(reply) => send_reply(&mut stream, reply).await,
            DispatchReply::MintReceipt(receipt) => {
                let mut bytes = [LEDGER_VERSION; 1 + SLOT_LEN];
                bytes[1..].copy_from_slice(&receipt.encode());
                send(&mut stream, MINT_RECEIPT_REPLY, &bytes).await
            }
            DispatchReply::Ledger(bytes) => send(&mut stream, LEDGER_REPLY, &bytes).await,
        }
    })
    .await;
}

fn peer(stream: &UnixStream, expected: u32) -> Result<(), Failure> {
    // Tokio's Linux peer_cred uses SO_PEERCRED on the connected AF_UNIX socket.
    let credentials = stream.peer_cred().map_err(|_| Failure::Refused)?;
    if !peer_matches(credentials.uid(), credentials.pid(), expected) {
        return Err(Failure::Refused);
    }
    Ok(())
}

fn peer_matches(uid: u32, pid: Option<i32>, expected: u32) -> bool {
    uid == expected && pid.is_some_and(|pid| pid > 0)
}

pub(super) async fn connect(path: &Path, daemon: u32) -> Result<UnixStream, Failure> {
    // SAFETY: identity query takes no pointers.
    let owner = unsafe { libc::geteuid() };
    if owner == 0 || daemon == 0 || owner == daemon || daemon == u32::MAX {
        return Err(Failure::Refused);
    }
    let before = route(path, owner, daemon, None).map_err(|_| Failure::Refused)?;
    let object = socket(path, daemon).map_err(|_| Failure::Refused)?;
    let stream = timeout(HANDSHAKE, UnixStream::connect(path))
        .await
        .map_err(|_| Failure::Refused)?
        .map_err(|_| Failure::Refused)?;
    peer(&stream, daemon)?;
    if before != route(path, owner, daemon, None).map_err(|_| Failure::Refused)?
        || object != socket(path, daemon).map_err(|_| Failure::Refused)?
    {
        return Err(Failure::Refused);
    }
    // Tokio owns a close-on-exec descriptor. It has no relationship to standard streams.
    Ok(stream)
}

pub(super) async fn request(mut stream: UnixStream, request: Request) -> Result<Reply, Failure> {
    let mint = matches!(request, Request::Mint);
    let (tag, body) = match &request {
        Request::Mint => (1, &[][..]),
        Request::Join(paste) => (2, paste.bytes()),
        Request::MintReceipt | Request::ObserveLedger(_) => return Err(Failure::Refused),
    };
    timeout(HANDSHAKE, send(&mut stream, tag, body))
        .await
        .map_err(|_| Failure::Unknown)?
        .map_err(|_| Failure::Unknown)?;
    drop(request);
    let (tag, body) = timeout(REPLY_WAIT, receive(&mut stream))
        .await
        .map_err(|_| Failure::Unknown)?
        .map_err(|_| Failure::Unknown)?;
    match (mint, tag, body.bytes().is_empty()) {
        (true, 11, false) if paste_shape(body.bytes()) => Ok(Reply::Minted(body)),
        (false, 12, true) => Ok(Reply::Joined),
        (_, 13, true) => Ok(Reply::Refused),
        (_, 14, true) => Ok(Reply::Unknown),
        _ => Err(Failure::Unknown),
    }
}

#[cfg(test)]
async fn read_request(stream: &mut UnixStream) -> io::Result<Request> {
    let (tag, body) = receive(stream).await?;
    decode_request(tag, body)
}

fn decode_request(tag: u8, body: Paste) -> io::Result<Request> {
    match tag {
        1 if body.bytes().is_empty() => Ok(Request::Mint),
        2 if paste_shape(body.bytes()) => Ok(Request::Join(body)),
        MINT_RECEIPT if body.bytes().is_empty() => Ok(Request::MintReceipt),
        OBSERVE_LEDGER => LedgerSlots::decode(body.bytes())
            .map(Box::new)
            .map(Request::ObserveLedger)
            .ok_or_else(|| io::ErrorKind::InvalidData.into()),
        _ => Err(io::ErrorKind::InvalidData.into()),
    }
}

pub(super) fn paste_shape(bytes: &[u8]) -> bool {
    let Some(body) = bytes.strip_prefix(b"arxje1-") else {
        return false;
    };
    let size_offset = match body.get(..2) {
        Some(b"04") => 14,
        Some(b"06") => 38,
        _ => return false,
    };
    let mut root_size = [0; 2];
    let Some(length) = body.get(size_offset..size_offset + 4) else {
        return false;
    };
    // Check canonical frame size here. The existing transfer consumer owns
    // checksums, pins, the inner code and the locally qualified route.
    hex::decode_to_slice(length, &mut root_size).is_ok()
        && (1..=4096).contains(&u16::from_be_bytes(root_size))
        && body.len()
            == size_offset + 4 + 2 * (usize::from(u16::from_be_bytes(root_size)) + 270 + 4)
        && body
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}

async fn send_reply(stream: &mut UnixStream, reply: Reply) -> io::Result<()> {
    let (tag, body) = match &reply {
        Reply::Minted(paste) => (11, paste.bytes()),
        Reply::Joined => (12, &[][..]),
        Reply::Refused => (13, &[][..]),
        Reply::Unknown => (14, &[][..]),
    };
    send(stream, tag, body).await
}

async fn send(stream: &mut UnixStream, tag: u8, bytes: &[u8]) -> io::Result<()> {
    let len = u16::try_from(bytes.len()).map_err(|_| io::ErrorKind::InvalidData)?;
    let mut header = [0; HEADER];
    header[..4].copy_from_slice(MAGIC);
    header[4] = tag;
    header[5..].copy_from_slice(&len.to_be_bytes());
    stream.write_all(&header).await?;
    stream.write_all(bytes).await?;
    stream.shutdown().await
}

async fn receive(stream: &mut UnixStream) -> io::Result<(u8, Paste)> {
    let mut header = InputHeader([0; HEADER]);
    let mut paste = Paste::empty();
    // Invalid headers and trailing bytes may themselves contain sensitive input.
    stream.read_exact(&mut header.0).await?;
    let length = usize::from(u16::from_be_bytes([header.0[5], header.0[6]]));
    if &header.0[..4] != MAGIC || length > MAX_PASTE {
        return Err(io::ErrorKind::InvalidData.into());
    }
    stream.read_exact(&mut paste.0[..length]).await?;
    paste.1 = length;
    if stream.read(&mut header.0[..1]).await? != 0 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok((header.0[4], paste))
}

struct InputHeader([u8; HEADER]);

impl Drop for InputHeader {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[tokio::test]
async fn time_evidence_is_fixed_and_owner_only() {
    // SAFETY: getuid takes no arguments.
    let owner = unsafe { libc::geteuid() };
    for (tag, payload, uid, reply_tag, expected) in [
        (
            TIME_EVIDENCE,
            [].as_slice(),
            owner,
            TIME_REPLY,
            [9; 53].as_slice(),
        ),
        (TIME_EVIDENCE, [].as_slice(), owner ^ 1, 13, [].as_slice()),
        (TIME_EVIDENCE, [1].as_slice(), owner, 13, [].as_slice()),
        (4, [].as_slice(), owner, 15, [7; 29].as_slice()),
    ] {
        let (server, mut client) = UnixStream::pair().unwrap();
        send(&mut client, tag, payload).await.unwrap();
        connection_with_status(
            server,
            uid,
            Shutdown::default(),
            |_, _| async { panic!("observation reached enrollment dispatch") },
            || [7; 29],
            || {
                assert_eq!((tag, uid, payload), (TIME_EVIDENCE, owner, [].as_slice()));
                [9; TIME_EVIDENCE_LEN]
            },
        )
        .await;
        let (received, body) = receive(&mut client).await.unwrap();
        assert_eq!(received, reply_tag);
        assert_eq!(body.bytes(), expected);
    }
}
