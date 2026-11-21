//! Only these custody operations handle transfer plaintext.
use super::*;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};

fn cipher(key: &[u8; 32]) -> Result<LessSafeKey, Error> {
    UnboundKey::new(&aead::CHACHA20_POLY1305, key)
        .map(LessSafeKey::new)
        .map_err(|_| Error::Unavailable)
}

fn seal_channel(
    key: &[u8; 32],
    context: &[u8],
    mut plaintext: codec::Secret,
) -> Result<Vec<u8>, Error> {
    // TLS exports a distinct key for the one receiver challenge and package.
    let tag = cipher(key)?
        .seal_in_place_separate_tag(
            Nonce::assume_unique_for_key([0; 12]),
            Aad::from(context),
            &mut plaintext.0,
        )
        .map_err(|_| Error::Unavailable)?;
    let mut sealed = std::mem::take(&mut plaintext.0);
    sealed.extend_from_slice(tag.as_ref());
    Ok(sealed)
}

fn open_channel(key: &[u8; 32], context: &[u8], sealed: Vec<u8>) -> Result<codec::Secret, Error> {
    let mut plaintext = codec::Secret(sealed);
    let len = cipher(key)?
        .open_in_place(
            Nonce::assume_unique_for_key([0; 12]),
            Aad::from(context),
            &mut plaintext.0,
        )
        .map_err(|_| Error::Corrupt)?
        .len();
    plaintext.0.truncate(len);
    Ok(plaintext)
}

#[cfg(test)]
#[path = "voter_transfer_tests.rs"]
mod tests;

#[cfg(target_os = "linux")]
pub(super) mod disk {
    use super::super::linux::{Custody, FounderCustody, FounderLayout, LearnerCustody, Root, same};
    use super::*;
    use crate::cluster::startup_plan::Owner;
    use std::{fs::File, io::Write, os::fd::AsRawFd};

    const DIRECTORY: &str = "voter-custody";
    const RESERVATION: &str = "reservation";
    const ENVELOPE: &str = "ca-key";
    const FILE_MAX: usize = 16 * 1024;

    #[derive(Default)]
    pub(in crate::cluster::pki::custody) struct Installation {
        attempted: bool,
        #[cfg(test)]
        pub(in crate::cluster::pki::custody) fault: Option<Fault>,
        complete: Option<Complete>,
    }

    #[cfg(test)]
    #[derive(Clone, Copy, Eq, PartialEq)]
    pub(in crate::cluster::pki::custody) enum Fault {
        Write,
        Sync,
        Readback,
    }

    #[cfg(test)]
    impl Installation {
        fn check(&self, step: Fault) -> Result<(), Error> {
            if self.fault == Some(step) {
                Err(Error::Unavailable)
            } else {
                Ok(())
            }
        }
    }

    struct Complete {
        root: Root,
        reservation: File,
        envelope: File,
        context: Context,
        bytes: Vec<u8>,
    }

    fn ca_context(
        custody: &Custody,
        cluster: ClusterId,
        root: [u8; 32],
        der: &[u8],
    ) -> Result<Context, Error> {
        if der.is_empty()
            || der.len() > 4096
            || super::super::super::ca::fingerprint_of(der) != root
        {
            return Err(Error::Context);
        }
        let (remaining, certificate) =
            x509_parser::parse_x509_certificate(der).map_err(|_| Error::Corrupt)?;
        if !remaining.is_empty() {
            return Err(Error::Corrupt);
        }
        Ok(Context {
            kind: Kind::Ca,
            install: custody.installation,
            cluster,
            root,
            spki: super::super::super::ca::fingerprint_of(certificate.public_key().raw),
        })
    }

    fn load(
        payload: &codec::Secret,
        context: Context,
        original: &[u8],
        now: u64,
    ) -> Result<ClusterCa, Error> {
        let (key, der) = codec::parts(Kind::Ca, &payload.0)?;
        if der != original {
            return Err(Error::Context);
        }
        ClusterCa::load(key, der, context.root, context.spki, now)
    }

    impl FounderCustody {
        pub(in crate::cluster) fn seal_for_voter(
            &self,
            cluster: ClusterId,
            original: &[u8],
            now: u64,
            key: &[u8; 32],
            transcript: &[u8],
        ) -> Result<Vec<u8>, Error> {
            let custody = &self.0;
            let context = ca_context(
                custody,
                cluster,
                super::super::super::ca::fingerprint_of(original),
                original,
            )?;
            custody.common_context(context.cluster, context.root)?;
            if custody.inventory()? != ["ca-key", "custody-lock", "member-key"] {
                return Err(Error::Layout);
            }
            let bytes = custody.store.read("ca-key", FILE_MAX)?;
            let plaintext = codec::open(&custody.key, context, &bytes)?;
            load(&plaintext, context, original, now)?;
            seal_channel(key, transcript, plaintext)
        }
    }

    impl LearnerCustody {
        pub(in crate::cluster) fn receive_voter_ca(
            &mut self,
            owner: &Owner,
            cluster: ClusterId,
            original: &[u8],
            now: u64,
            channel: (&[u8; 32], &[u8], Vec<u8>),
        ) -> Result<(), Error> {
            if self.voter.attempted {
                return Err(Error::Ownership);
            }
            self.member()?;
            let custody = &self.custody;
            let context = ca_context(
                custody,
                cluster,
                super::super::super::ca::fingerprint_of(original),
                original,
            )?;
            custody.common_context(cluster, context.root)?;
            let (key, transcript, sealed) = channel;
            let plaintext = open_channel(key, transcript, sealed)?;
            load(&plaintext, context, original, now)?;
            self.voter.attempted = true;
            let bytes = seal_installation(&custody.key, context, plaintext)?;
            let root = create_root(custody, owner)?;
            let reservation = root.file(RESERVATION, true)?;
            reservation.sync_all().map_err(|_| Error::Unavailable)?;
            root.dir.sync_all().map_err(|_| Error::Unavailable)?;
            let mut envelope = root.file(ENVELOPE, true)?;
            #[cfg(test)]
            if self.voter.fault == Some(Fault::Write) {
                envelope
                    .write_all(&bytes[..bytes.len() / 2])
                    .map_err(|_| Error::Unavailable)?;
                self.voter.check(Fault::Write)?;
            }
            envelope.write_all(&bytes).map_err(|_| Error::Unavailable)?;
            #[cfg(test)]
            self.voter.check(Fault::Sync)?;
            envelope.sync_all().map_err(|_| Error::Unavailable)?;
            root.dir.sync_all().map_err(|_| Error::Unavailable)?;
            let complete = Complete {
                root,
                reservation,
                envelope,
                context,
                bytes,
            };
            #[cfg(test)]
            self.voter.check(Fault::Readback)?;
            complete.load(custody, context, original, now)?;
            self.voter.complete = Some(complete);
            Ok(())
        }

        pub(in crate::cluster) fn load_voter_ca(
            &self,
            cluster: ClusterId,
            original: &[u8],
            now: u64,
        ) -> Result<ClusterCa, Error> {
            self.member()?;
            let custody = &self.custody;
            let context = ca_context(
                custody,
                cluster,
                super::super::super::ca::fingerprint_of(original),
                original,
            )?;
            custody.common_context(cluster, context.root)?;
            self.voter
                .complete
                .as_ref()
                .ok_or(Error::Ownership)?
                .load(custody, context, original, now)
        }
    }

    fn create_root(custody: &Custody, owner: &Owner) -> Result<Root, Error> {
        let path = custody.store.path.parent().ok_or(Error::Layout)?;
        let voter_path = path.join(DIRECTORY);
        for other in [
            &owner.plan.credentials,
            &owner.plan.state,
            &owner.plan.workspace,
            &owner.plan.config,
            &custody.wrap.path,
            &custody.store.path,
        ] {
            if voter_path.starts_with(other) || other.starts_with(&voter_path) {
                return Err(Error::Layout);
            }
        }
        let layout = FounderLayout::checked(owner)?;
        let parent = Root::open_checked(path, |m, _| layout.check(m, false))?;
        // SAFETY: checked live owner directory and a fixed NUL-terminated basename.
        if unsafe { libc::mkdirat(parent.dir.as_raw_fd(), c"voter-custody".as_ptr(), 0o700) } != 0 {
            return Err(Error::Layout);
        }
        parent.dir.sync_all().map_err(|_| Error::Unavailable)?;
        parent.recheck()?;
        Root::open_checked(&voter_path, |m, leaf| layout.check(m, leaf))
    }

    fn seal_installation(
        key: &[u8; 32],
        context: Context,
        mut plaintext: codec::Secret,
    ) -> Result<Vec<u8>, Error> {
        let mut nonce = [0; 12];
        getrandom::fill(&mut nonce).map_err(|_| Error::Unavailable)?;
        let mut bytes = codec::header(context, nonce, plaintext.0.len())?;
        let tag = cipher(key)?
            .seal_in_place_separate_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(&bytes),
                &mut plaintext.0,
            )
            .map_err(|_| Error::Unavailable)?;
        bytes.extend_from_slice(&plaintext.0);
        bytes.extend_from_slice(tag.as_ref());
        Ok(bytes)
    }

    impl Complete {
        fn load(
            &self,
            custody: &Custody,
            context: Context,
            original: &[u8],
            now: u64,
        ) -> Result<ClusterCa, Error> {
            if self.context != context || self.root.entries(2)? != [ENVELOPE, RESERVATION] {
                return Err(Error::Context);
            }
            for (name, pinned) in [(RESERVATION, &self.reservation), (ENVELOPE, &self.envelope)] {
                let current = self.root.file(name, false)?;
                if !same(
                    &current.metadata().map_err(|_| Error::Layout)?,
                    &pinned.metadata().map_err(|_| Error::Layout)?,
                ) {
                    return Err(Error::Ownership);
                }
            }
            codec::lock(&self.root.read(RESERVATION, 0)?)?;
            let ciphertext = self.root.read(ENVELOPE, FILE_MAX)?;
            if ciphertext != self.bytes {
                return Err(Error::Ownership);
            }
            load(
                &codec::open(&custody.key, context, &ciphertext)?,
                context,
                original,
                now,
            )
        }
    }
}
