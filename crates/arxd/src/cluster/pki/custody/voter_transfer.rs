//! Only these custody operations handle transfer plaintext.
use super::*;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};

/// Own every temporary transfer buffer, including the TLS exporter output.
pub(in crate::cluster) struct Wipe<T: AsMut<[u8]>>(pub(in crate::cluster) T);

impl<T: AsMut<[u8]>> Drop for Wipe<T> {
    fn drop(&mut self) {
        for byte in self.0.as_mut() {
            // SAFETY: Each byte is exclusively borrowed from the owned live buffer.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

/// A spent installation only becomes usable if the fresh proof succeeds after readback.
#[derive(Default)]
pub(in crate::cluster) enum Completion<T> {
    #[default]
    Fresh,
    Pending,
    Refused,
    Confirmed(T),
}

impl<T> Completion<T> {
    pub(in crate::cluster) fn available(&self) -> Result<(), Error> {
        if matches!(self, Self::Fresh) {
            Ok(())
        } else {
            Err(Error::Ownership)
        }
    }

    pub(in crate::cluster) fn spend(&mut self) -> Result<(), Error> {
        self.available()?;
        *self = Self::Pending;
        Ok(())
    }

    pub(in crate::cluster) fn confirm(
        &mut self,
        durable: T,
        eligible: impl FnOnce() -> Result<(), Error>,
    ) -> Result<(), Error> {
        if !matches!(self, Self::Pending) {
            return Err(Error::Ownership);
        }
        *self = Self::Refused;
        eligible()?;
        *self = Self::Confirmed(durable);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn prove<P>(&mut self, prove: impl FnOnce() -> Result<P, Error>) -> Result<P, Error> {
        prove().inspect_err(|_| *self = Self::Refused)
    }

    pub(in crate::cluster) fn get(&self) -> Result<&T, Error> {
        match self {
            Self::Confirmed(value) => Ok(value),
            _ => Err(Error::Ownership),
        }
    }
}

fn cipher(key: &[u8; 32]) -> Result<LessSafeKey, Error> {
    UnboundKey::new(&aead::CHACHA20_POLY1305, key)
        .map(LessSafeKey::new)
        .map_err(|_| Error::Unavailable)
}

fn seal_channel(
    key: &[u8; 32],
    context: &[u8],
    mut plaintext: Wipe<Vec<u8>>,
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

fn open_channel(key: &[u8; 32], context: &[u8], sealed: Vec<u8>) -> Result<Wipe<Vec<u8>>, Error> {
    open(key, context, [0; 12], sealed)
}

fn open(
    key: &[u8; 32],
    context: &[u8],
    nonce: [u8; 12],
    sealed: Vec<u8>,
) -> Result<Wipe<Vec<u8>>, Error> {
    let mut plaintext = Wipe(sealed);
    let len = cipher(key)?
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(context),
            &mut plaintext.0,
        )
        .map_err(|_| Error::Corrupt)?
        .len();
    plaintext.0.truncate(len);
    Ok(plaintext)
}

#[cfg(test)]
pub(in crate::cluster) fn fixture_seal(
    key: &[u8; 32],
    transcript: &[u8],
    original: &[u8],
    private: &KeyPair,
) -> Result<Vec<u8>, Error> {
    seal_channel(
        key,
        transcript,
        Wipe(codec::payload(
            Kind::Ca,
            private.serialized_der(),
            original,
        )?),
    )
}

#[cfg(test)]
pub(in crate::cluster) fn fixture_authenticate(
    key: &[u8; 32],
    transcript: &[u8],
    package: Vec<u8>,
    original: &[u8],
) -> Result<(), Error> {
    let decrypted = open_channel(key, transcript, package)?;
    let (_, root) = codec::parts(Kind::Ca, &decrypted.0)?;
    if root != original {
        return Err(Error::Context);
    }
    Ok(())
}

#[cfg(test)]
#[path = "voter_transfer_tests.rs"]
mod tests;

#[cfg(all(test, target_os = "linux"))]
pub(in crate::cluster) use disk::ReceiveStage;

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
        #[cfg(test)]
        pub(in crate::cluster::pki::custody) fault: Option<Fault>,
        complete: Completion<Complete>,
        #[cfg(test)]
        observe: Option<Box<dyn Fn(ReceiveStage) + Send + Sync>>,
    }

    #[cfg(test)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(in crate::cluster) enum ReceiveStage {
        BeforeUnwrap,
        ChannelOpen,
        BeforeReadback,
        ReadbackOpen,
        Confirmed,
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

    #[cfg(test)]
    impl Installation {
        fn observe(&self, stage: ReceiveStage) {
            if let Some(observe) = &self.observe {
                observe(stage);
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

    fn open_installation(
        custody: &Custody,
        expected: Context,
        bytes: &[u8],
    ) -> Result<Wipe<Vec<u8>>, Error> {
        let (context, nonce) = codec::frame(bytes)?;
        if context != expected {
            return Err(Error::Context);
        }
        open(
            &custody.key,
            &bytes[..codec::HEADER],
            nonce,
            bytes[codec::HEADER..].to_vec(),
        )
    }

    // Validate the retained root and its key without constructing a throwaway
    // rcgen signer with an owned serialized copy of the private key.
    fn validate(payload: &Wipe<Vec<u8>>, original: &[u8], now: u64) -> Result<(), Error> {
        use ring::{
            rand::SystemRandom,
            signature::{self, KeyPair as _},
        };
        use x509_parser::prelude::*;

        checked_time(Some(now))?;
        let (key, der) = codec::parts(Kind::Ca, &payload.0)?;
        if der != original {
            return Err(Error::Context);
        }
        let (rest, cert) = X509Certificate::from_der(der).map_err(|_| Error::Corrupt)?;
        let constraints = cert
            .basic_constraints()
            .map_err(|_| Error::Corrupt)?
            .ok_or(Error::Corrupt)?;
        let usage = cert
            .key_usage()
            .map_err(|_| Error::Corrupt)?
            .ok_or(Error::Corrupt)?;
        let algorithm = &cert.public_key().algorithm;
        if !rest.is_empty()
            || cert.subject() != cert.issuer()
            || !constraints.value.ca
            || constraints.value.path_len_constraint != Some(0)
            || usage.value.flags != 0x60
            || cert.signature_algorithm.algorithm.to_id_string() != "1.2.840.10045.4.3.2"
            || cert.signature_algorithm.parameters.is_some()
            || cert.tbs_certificate.signature != cert.signature_algorithm
            || algorithm.algorithm.to_id_string() != "1.2.840.10045.2.1"
            || algorithm
                .parameters
                .as_ref()
                .and_then(|p| p.as_oid().ok())
                .is_none_or(|oid| oid.to_id_string() != "1.2.840.10045.3.1.7")
            || cert
                .subject_alternative_name()
                .map_err(|_| Error::Corrupt)?
                .is_some()
        {
            return Err(Error::Corrupt);
        }
        let public = cert.public_key().subject_public_key.data.as_ref();
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, public)
            .verify(cert.tbs_certificate.as_ref(), &cert.signature_value.data)
            .map_err(|_| Error::Corrupt)?;
        let (rest, _) = x509_parser::der_parser::der::parse_der(key).map_err(|_| Error::Corrupt)?;
        if !rest.is_empty() {
            return Err(Error::Corrupt);
        }
        let key = signature::EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            key,
            &SystemRandom::new(),
        )
        .map_err(|_| Error::Corrupt)?;
        if key.public_key().as_ref() != public {
            return Err(Error::KeyMismatch);
        }
        let leaf_start = i64::try_from(now / 1000).map_err(|_| Error::Clock)? - 60;
        let leaf_end = leaf_start + 30 * 86400;
        if leaf_start < cert.validity().not_before.timestamp()
            || leaf_end > cert.validity().not_after.timestamp()
        {
            return Err(Error::CaExpiring);
        }
        Ok(())
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
            if custody.checked_inventory()? != ["ca-key", "custody-lock", "member-key"] {
                return Err(Error::Layout);
            }
            let member = custody.store.read("member-key", FILE_MAX)?;
            let (member_context, _) = codec::frame(&member)?;
            if member_context.kind != Kind::Member
                || member_context.cluster != cluster
                || member_context.root != context.root
                || member_context.install != custody.installation
            {
                return Err(Error::Context);
            }
            let member = open_installation(custody, member_context, &member)?;
            codec::parts(Kind::Member, &member.0)?;
            drop(member);
            let bytes = custody.store.read("ca-key", FILE_MAX)?;
            let plaintext = open_installation(custody, context, &bytes)?;
            validate(&plaintext, original, now)?;
            seal_channel(key, transcript, plaintext)
        }
    }

    impl LearnerCustody {
        #[cfg(test)]
        pub(in crate::cluster) fn observe_voter_receive(
            &mut self,
            observe: impl Fn(ReceiveStage) + Send + Sync + 'static,
        ) {
            self.voter.observe = Some(Box::new(observe));
        }

        #[cfg(test)]
        pub(in crate::cluster) fn voter_custody_state(&self) -> (bool, bool) {
            (
                self.voter.complete.get().is_ok(),
                self.voter.complete.available().is_ok(),
            )
        }

        pub(in crate::cluster) fn receive_voter_ca<P>(
            &mut self,
            owner: &Owner,
            cluster: ClusterId,
            original: &[u8],
            now: u64,
            channel: (&[u8; 32], &[u8], Vec<u8>),
            (mut prove, installed): (impl FnMut() -> Result<P, Error>, impl FnOnce()),
        ) -> Result<(), Error> {
            self.voter.complete.available()?;
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
            #[cfg(test)]
            self.voter.observe(ReceiveStage::BeforeUnwrap);
            let bytes = {
                let _proof = self.voter.complete.prove(&mut prove)?;
                #[cfg(test)]
                self.voter.observe(ReceiveStage::ChannelOpen);
                let plaintext = open_channel(key, transcript, sealed)?;
                validate(&plaintext, original, now)?;
                self.voter.complete.spend()?;
                seal_installation(&custody.key, context, plaintext)?
            };
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
            let ciphertext = complete.read_ciphertext(context)?;
            #[cfg(test)]
            self.voter.observe(ReceiveStage::BeforeReadback);
            let _proof = self.voter.complete.prove(prove)?;
            #[cfg(test)]
            self.voter.observe(ReceiveStage::ReadbackOpen);
            let plaintext = open_installation(custody, context, &ciphertext)?;
            validate(&plaintext, original, now)?;
            self.voter.complete.confirm(complete, || Ok(()))?;
            installed();
            #[cfg(test)]
            self.voter.observe(ReceiveStage::Confirmed);
            Ok(())
        }

        pub(in crate::cluster) fn load_voter_ca<P>(
            &self,
            cluster: ClusterId,
            original: &[u8],
            now: u64,
            prove: impl FnOnce() -> Result<P, Error>,
        ) -> Result<(ClusterCa, P), Error> {
            self.member()?;
            let custody = &self.custody;
            let context = ca_context(
                custody,
                cluster,
                super::super::super::ca::fingerprint_of(original),
                original,
            )?;
            custody.common_context(cluster, context.root)?;
            let ciphertext = self.voter.complete.get()?.read_ciphertext(context)?;
            let proof = prove()?;
            let plaintext = open_installation(custody, context, &ciphertext)?;
            validate(&plaintext, original, now)?;
            let (key, der) = codec::parts(Kind::Ca, &plaintext.0)?;
            let ca = ClusterCa::load(key, der, context.root, context.spki, now)?;
            Ok((ca, proof))
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
        mut plaintext: Wipe<Vec<u8>>,
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
        fn read_ciphertext(&self, context: Context) -> Result<Vec<u8>, Error> {
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
            Ok(ciphertext)
        }
    }
}
