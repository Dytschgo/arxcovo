//! Private create-once custody primitives. No startup or membership authority.
#[cfg(any(test, target_os = "linux"))]
use super::ca::ClusterCa;
use super::csr::ProvenCsr;
use crate::cluster::enrollment::{ClusterId, MAX_TIME_MS, NodeId};
use rcgen::{CertificateParams, KeyPair, PublicKeyData};
mod codec;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Unqualified,
    Layout,
    Ownership,
    Context,
    Corrupt,
    KeyMismatch,
    Clock,
    CaExpiring,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Ca = 1,
    Member = 2,
}
impl Kind {
    fn bounds(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Self::Ca => 10..=8200,
            Self::Member => 5..=4100,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Context {
    kind: Kind,
    install: [u8; 16],
    cluster: ClusterId,
    root: [u8; 32],
    spki: [u8; 32],
}
fn checked_time(now: Option<u64>) -> Result<u64, Error> {
    now.filter(|n| *n <= MAX_TIME_MS).ok_or(Error::Clock)
}
pub(super) fn parse_key(bytes: &[u8]) -> Result<KeyPair, Error> {
    // Require exactly one DER SEQUENCE; ring performs the actual PKCS#8 parsing.
    use x509_parser::der_parser::der::parse_der;
    let (rest, _) = parse_der(bytes).map_err(|_| Error::Corrupt)?;
    if !rest.is_empty() {
        return Err(Error::Corrupt);
    }
    KeyPair::from_pkcs8_der_and_sign_algo(&bytes.into(), &rcgen::PKCS_ECDSA_P256_SHA256)
        .map_err(|_| Error::Corrupt)
}
fn digest(key: &KeyPair) -> [u8; 32] {
    super::ca::fingerprint_of(&key.subject_public_key_info())
}
struct MemberKey {
    key: KeyPair,
    context: Context,
}
impl MemberKey {
    fn from_payload(context: Context, bytes: &[u8]) -> Result<Self, Error> {
        if context.kind != Kind::Member {
            return Err(Error::Context);
        }
        let (key, _) = codec::parts(Kind::Member, bytes)?;
        let key = parse_key(key)?;
        if digest(&key) != context.spki {
            return Err(Error::KeyMismatch);
        }
        Ok(Self { key, context })
    }
    fn csr(&self) -> Result<Vec<u8>, Error> {
        let csr = CertificateParams::default()
            .serialize_request(&self.key)
            .map_err(|_| Error::Unavailable)?
            .der()
            .to_vec();
        ProvenCsr::parse(&csr).map_err(|_| Error::Corrupt)?;
        Ok(csr)
    }
    fn check_leaf(
        &self,
        node: NodeId,
        root: &[u8],
        leaf: &[u8],
        now: Option<u64>,
    ) -> Result<(), Error> {
        use rustls::{
            pki_types::{CertificateDer, UnixTime},
            server::danger::ClientCertVerifier,
        };
        use std::{sync::Arc, time::Duration};
        let now = checked_time(now)?;
        if super::ca::fingerprint_of(root) != self.context.root {
            return Err(Error::Context);
        }
        let context = self.context;
        let verifier = super::verify::PeerVerifier::new(
            CertificateDer::from(root.to_vec()),
            context.cluster,
            Arc::new(move |cluster, actual_node, key| {
                if cluster == context.cluster
                    && actual_node == node
                    && key.as_bytes() == &context.spki
                {
                    Ok(())
                } else {
                    Err(super::verify::VerifyRefusal::KeyMismatch)
                }
            }),
        )
        .map_err(|_| Error::Corrupt)?;
        verifier
            .verify_client_cert(
                &CertificateDer::from(leaf),
                &[],
                UnixTime::since_unix_epoch(Duration::from_millis(now)),
            )
            .map_err(|_| Error::KeyMismatch)?;
        Ok(())
    }
}

// Native composition has no qualified backend in this slice.
fn native() -> Result<(), Error> {
    Err(Error::Unqualified)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        fs::{self, File},
        io::{Read, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        },
        path::{Component, Path, PathBuf},
    };
    struct Root {
        path: PathBuf,
        dir: File,
    }
    fn same(a: &fs::Metadata, b: &fs::Metadata) -> bool {
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    fn privacy(m: &fs::Metadata, directory: bool) -> Result<(), Error> {
        // SAFETY: geteuid takes no pointers.
        if m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o077 != 0
            || (if directory {
                !m.is_dir()
            } else {
                !m.is_file() || m.nlink() != 1
            })
        {
            return Err(Error::Ownership);
        }
        Ok(())
    }
    impl Root {
        fn open(path: &Path) -> Result<Self, Error> {
            if !path.is_absolute()
                || path.components().count() > 64
                || path.as_os_str().len() > 4096
            {
                return Err(Error::Layout);
            }
            let mut dir = File::open("/").map_err(|_| Error::Layout)?;
            for component in path.components().skip(1) {
                let Component::Normal(name) = component else {
                    return Err(Error::Layout);
                };
                use std::os::unix::ffi::OsStrExt;
                let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| Error::Layout)?;
                // SAFETY: live directory descriptor, valid NUL-terminated name; returned fd owned here.
                let fd = unsafe {
                    libc::openat(
                        dir.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if fd < 0 {
                    return Err(Error::Layout);
                }
                // SAFETY: successful openat transferred a new descriptor.
                dir = unsafe { File::from_raw_fd(fd) };
                let m = dir.metadata().map_err(|_| Error::Layout)?;
                // Ancestors may be root owned; writable shared ancestors require sticky protection.
                if (m.uid() != 0 && m.uid() != unsafe { libc::geteuid() })
                    || (m.mode() & 0o022 != 0 && !(m.uid() == 0 && m.mode() & 0o1000 != 0))
                {
                    return Err(Error::Ownership);
                }
            }
            privacy(&dir.metadata().map_err(|_| Error::Layout)?, true)?;
            Ok(Self {
                path: path.to_owned(),
                dir,
            })
        }
        fn recheck(&self) -> Result<(), Error> {
            let current = Self::open(&self.path)?;
            if !same(
                &self.dir.metadata().map_err(|_| Error::Layout)?,
                &current.dir.metadata().map_err(|_| Error::Layout)?,
            ) {
                return Err(Error::Ownership);
            }
            Ok(())
        }
        fn file(&self, name: &str, create: bool) -> Result<File, Error> {
            self.recheck()?;
            let name = std::ffi::CString::new(name).map_err(|_| Error::Layout)?;
            let flags = libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | libc::O_NONBLOCK
                | if create {
                    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL
                } else {
                    libc::O_RDONLY
                };
            // SAFETY: checked retained directory, fixed basename, owned result.
            let fd = unsafe { libc::openat(self.dir.as_raw_fd(), name.as_ptr(), flags, 0o600) };
            if fd < 0 {
                return Err(Error::Layout);
            }
            // SAFETY: fd is newly owned from successful openat.
            let file = unsafe { File::from_raw_fd(fd) };
            privacy(&file.metadata().map_err(|_| Error::Layout)?, false)?;
            Ok(file)
        }
        fn read(&self, name: &str, max: usize) -> Result<Vec<u8>, Error> {
            let mut f = self.file(name, false)?;
            let before = f.metadata().map_err(|_| Error::Layout)?;
            if before.len() > max as u64 {
                return Err(Error::Corrupt);
            }
            let mut bytes = Vec::new();
            (&mut f)
                .take(max as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Unavailable)?;
            let current = self.file(name, false)?;
            if bytes.len() > max || !same(&before, &current.metadata().map_err(|_| Error::Layout)?)
            {
                return Err(Error::Ownership);
            }
            Ok(bytes)
        }
        fn entries(&self, max: usize) -> Result<Vec<String>, Error> {
            self.recheck()?;
            let mut names = Vec::new();
            for e in fs::read_dir(&self.path)
                .map_err(|_| Error::Layout)?
                .take(max + 1)
            {
                if names.len() == max {
                    return Err(Error::Layout);
                }
                let name = e
                    .map_err(|_| Error::Layout)?
                    .file_name()
                    .into_string()
                    .map_err(|_| Error::Layout)?;
                names.push(name);
            }
            self.recheck()?;
            names.sort();
            Ok(names)
        }
    }
    pub(super) struct Custody {
        wrap: Root,
        store: Root,
        lock: File,
        installation: [u8; 16],
        key: [u8; 32],
        failure: Option<Error>,
        #[cfg(test)]
        pub(super) fail_at: Option<usize>,
        #[cfg(test)]
        pub(super) effects: usize,
    }
    impl Drop for Custody {
        fn drop(&mut self) {
            self.key.fill(0);
        }
    }
    #[cfg(test)]
    pub(super) type InstallationHook = Box<dyn FnMut(&mut Custody, bool, [u8; 16], [u8; 32])>;
    #[cfg(test)]
    thread_local! {
        pub(super) static INSTALLATION_HOOK: std::cell::RefCell<Option<InstallationHook>> =
            const { std::cell::RefCell::new(None) };
    }
    impl Custody {
        fn roots(wrap: &Path, store: &Path) -> Result<(Root, Root), Error> {
            let a = Root::open(wrap)?;
            let b = Root::open(store)?;
            if wrap.starts_with(store)
                || store.starts_with(wrap)
                || same(
                    &a.dir.metadata().map_err(|_| Error::Layout)?,
                    &b.dir.metadata().map_err(|_| Error::Layout)?,
                )
            {
                return Err(Error::Layout);
            }
            Ok((a, b))
        }
        fn acquire(store: &Root, create: bool) -> Result<File, Error> {
            if create {
                drop(store.file("custody-lock", true)?);
                store.dir.sync_all().map_err(|_| Error::Unavailable)?;
            }
            let lock = store.file("custody-lock", false)?;
            codec::lock(&store.read("custody-lock", 0)?)?;
            // SAFETY: lock descriptor remains owned for Custody lifetime; nonblocking exclusive OS lock.
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(Error::Ownership);
            }
            Ok(lock)
        }
        fn owner(
            wrap: Root,
            store: Root,
            lock: File,
            installation: [u8; 16],
            key: [u8; 32],
        ) -> Self {
            Self {
                wrap,
                store,
                lock,
                installation,
                key,
                failure: None,
                #[cfg(test)]
                fail_at: None,
                #[cfg(test)]
                effects: 0,
            }
        }
        pub(super) fn create_fresh_installation(wrap: &Path, store: &Path) -> Result<Self, Error> {
            let (wrap, store) = Self::roots(wrap, store)?;
            if !wrap.entries(2)?.is_empty() || !store.entries(3)?.is_empty() {
                return Err(Error::Layout);
            }
            let lock = Self::acquire(&store, true)?;
            let mut id = [0; 16];
            let mut key = [0; 32];
            getrandom::fill(&mut id).map_err(|_| Error::Unavailable)?;
            getrandom::fill(&mut key).map_err(|_| Error::Unavailable)?;
            let mut owner = Self::owner(wrap, store, lock, id, key);
            #[cfg(test)]
            INSTALLATION_HOOK.with_borrow_mut(|hook| {
                if let Some(hook) = hook {
                    hook(&mut owner, false, id, key);
                }
            });
            let result = (|| {
                owner.write(true, "install-context", &codec::install(id))?;
                let secret = codec::Secret(key.to_vec());
                owner.write(true, "seal-key", &secret.0)
            })();
            if let Err(e) = result {
                owner.failure = Some(e);
                #[cfg(test)]
                INSTALLATION_HOOK.with_borrow_mut(|hook| {
                    if let Some(hook) = hook {
                        hook(&mut owner, true, id, key);
                    }
                });
                return Err(e);
            }
            Ok(owner)
        }
        pub(super) fn open_existing(
            wrap: &Path,
            store: &Path,
            expected: [u8; 16],
        ) -> Result<Self, Error> {
            let (wrap, store) = Self::roots(wrap, store)?;
            let lock = Self::acquire(&store, false)?;
            let id = codec::installation(&wrap.read("install-context", 25)?)?;
            if id != expected {
                return Err(Error::Context);
            }
            let secret = codec::Secret(wrap.read("seal-key", 32)?);
            let key = codec::wrapping(&secret.0)?;
            let owner = Self::owner(wrap, store, lock, id, key);
            owner.inventory()?;
            Ok(owner)
        }
        fn inventory(&self) -> Result<Vec<String>, Error> {
            self.wrap.recheck()?;
            self.store.recheck()?;
            if self.wrap.entries(2)? != ["install-context", "seal-key"] {
                return Err(Error::Layout);
            }
            if codec::installation(&self.wrap.read("install-context", 25)?)? != self.installation
                || codec::wrapping(&self.wrap.read("seal-key", 32)?)? != self.key
            {
                return Err(Error::Context);
            }
            let current = self.store.file("custody-lock", false)?;
            if !same(
                &self.lock.metadata().map_err(|_| Error::Layout)?,
                &current.metadata().map_err(|_| Error::Layout)?,
            ) {
                return Err(Error::Ownership);
            }
            codec::lock(&self.store.read("custody-lock", 0)?)?;
            let entries = self.store.entries(3)?;
            for name in &entries {
                match name.as_str() {
                    "custody-lock" => (),
                    "ca-key" | "member-key" => {
                        let bytes = self.store.read(name, 16384)?;
                        let (ctx, _) = codec::frame(&bytes)?;
                        if ctx.install != self.installation
                            || (name == "ca-key") != (ctx.kind == Kind::Ca)
                        {
                            return Err(Error::Context);
                        }
                        let payload = codec::open(&self.key, ctx, &bytes)?;
                        match ctx.kind {
                            Kind::Member => {
                                MemberKey::from_payload(ctx, &payload.0)?;
                            }
                            Kind::Ca => {
                                let (key, der) = codec::parts(Kind::Ca, &payload.0)?;
                                let (_, cert) = x509_parser::parse_x509_certificate(der)
                                    .map_err(|_| Error::Corrupt)?;
                                // This checks internal integrity only. Actual issuer release below
                                // independently requires the caller's fresh clock observation.
                                let earliest = cert
                                    .validity()
                                    .not_before
                                    .timestamp()
                                    .checked_add(60)
                                    .and_then(|s| u64::try_from(s.max(0)).ok())
                                    .and_then(|s| s.checked_mul(1000))
                                    .ok_or(Error::Clock)?;
                                ClusterCa::load(key, der, ctx.root, ctx.spki, earliest)?;
                            }
                        }
                    }
                    _ => return Err(Error::Layout),
                }
            }
            Ok(entries)
        }
        fn common_context(&self, cluster: ClusterId, root: [u8; 32]) -> Result<(), Error> {
            for name in self.inventory()? {
                if name == "custody-lock" {
                    continue;
                }
                let (ctx, _) = codec::frame(&self.store.read(&name, 16384)?)?;
                if ctx.cluster != cluster || ctx.root != root {
                    return Err(Error::Context);
                }
            }
            Ok(())
        }
        fn seam(&mut self) -> Result<(), Error> {
            #[cfg(test)]
            {
                self.effects += 1;
                if self.fail_at == Some(self.effects) {
                    return Err(Error::Unavailable);
                }
            }
            Ok(())
        }
        fn write(&mut self, wrap: bool, name: &str, bytes: &[u8]) -> Result<(), Error> {
            self.seam()?;
            let root = if wrap { &self.wrap } else { &self.store };
            let mut f = root.file(name, true)?;
            self.seam()?;
            // Split the real write so tests can leave an actual truncated artifact.
            let mid = bytes.len() / 2;
            f.write_all(&bytes[..mid]).map_err(|_| Error::Unavailable)?;
            self.seam()?;
            f.write_all(&bytes[mid..]).map_err(|_| Error::Unavailable)?;
            self.seam()?;
            f.sync_all().map_err(|_| Error::Unavailable)?;
            self.seam()?;
            let root = if wrap { &self.wrap } else { &self.store };
            root.dir.sync_all().map_err(|_| Error::Unavailable)?;
            root.recheck()?;
            let current = root.file(name, false)?;
            if !same(
                &f.metadata().map_err(|_| Error::Layout)?,
                &current.metadata().map_err(|_| Error::Layout)?,
            ) {
                return Err(Error::Ownership);
            }
            Ok(())
        }
        fn create<T>(
            &mut self,
            operation: impl FnOnce(&mut Self) -> Result<T, Error>,
        ) -> Result<T, Error> {
            if let Some(e) = self.failure {
                return Err(e);
            }
            self.failure = Some(Error::Unavailable); // spent before any effect
            match operation(self) {
                Ok(value) => {
                    self.failure = None;
                    Ok(value)
                }
                Err(e) => {
                    self.failure = Some(e);
                    Err(e)
                }
            }
        }
        pub(super) fn create_founder_ca(
            &mut self,
            cluster: ClusterId,
            now: Option<u64>,
        ) -> Result<ClusterCa, Error> {
            self.create(|s| {
                if s.inventory()? != ["custody-lock"] {
                    return Err(Error::Layout);
                }
                let (ca, key) = ClusterCa::generate(checked_time(now)?)?;
                let key = codec::Secret(key);
                let ctx = Context {
                    kind: Kind::Ca,
                    install: s.installation,
                    cluster,
                    root: ca.fingerprint(),
                    spki: ca.spki_digest()?,
                };
                let payload = codec::Secret(codec::payload(Kind::Ca, &key.0, ca.cert_der())?);
                let bytes = codec::seal(&s.key, ctx, &payload.0)?;
                s.write(false, "ca-key", &bytes)?;
                Ok(ca)
            })
        }
        pub(super) fn load_ca(
            &self,
            expected: Context,
            now: Option<u64>,
        ) -> Result<ClusterCa, Error> {
            self.common_context(expected.cluster, expected.root)?;
            if expected.install != self.installation || expected.kind != Kind::Ca {
                return Err(Error::Context);
            }
            let bytes = self.store.read("ca-key", 16384)?;
            let payload = codec::open(&self.key, expected, &bytes)?;
            let (key, cert) = codec::parts(Kind::Ca, &payload.0)?;
            ClusterCa::load(key, cert, expected.root, expected.spki, checked_time(now)?)
        }
        pub(super) fn create_member(
            &mut self,
            cluster: ClusterId,
            root: [u8; 32],
            learner: bool,
        ) -> Result<(MemberKey, Vec<u8>), Error> {
            self.create(|s| {
                let entries = s.inventory()?;
                s.common_context(cluster, root)?;
                if entries.iter().any(|n| n == "member-key")
                    || (learner && entries.iter().any(|n| n == "ca-key"))
                {
                    return Err(Error::Layout);
                }
                let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
                    .map_err(|_| Error::Unavailable)?;
                let context = Context {
                    kind: Kind::Member,
                    install: s.installation,
                    cluster,
                    root,
                    spki: digest(&key),
                };
                let payload =
                    codec::Secret(codec::payload(Kind::Member, &key.serialize_der(), &[])?);
                let sealed = codec::seal(&s.key, context, &payload.0)?;
                s.write(false, "member-key", &sealed)?;
                let owner = MemberKey { key, context };
                let csr = owner.csr()?;
                Ok((owner, csr))
            })
        }
        pub(super) fn load_member(
            &self,
            expected: Context,
            learner: bool,
        ) -> Result<MemberKey, Error> {
            let entries = self.inventory()?;
            self.common_context(expected.cluster, expected.root)?;
            if expected.install != self.installation
                || expected.kind != Kind::Member
                || (learner && entries.iter().any(|n| n == "ca-key"))
            {
                return Err(Error::Context);
            }
            let bytes = self.store.read("member-key", 16384)?;
            MemberKey::from_payload(expected, &codec::open(&self.key, expected, &bytes)?.0)
        }
    }
}
