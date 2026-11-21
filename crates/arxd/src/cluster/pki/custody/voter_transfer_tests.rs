use super::*;

#[test]
fn transfer_package_is_bound_to_exporter_challenge_and_claim() {
    let key = [1; 32];
    let payload = codec::Secret(vec![2; 48]);
    let claim = [3; 192];
    let package = seal_channel(&key, &claim, payload).unwrap();
    assert_eq!(
        open_channel(&key, &claim, package.clone()).unwrap().0,
        [2; 48]
    );
    assert!(open_channel(&[9; 32], &claim, package.clone()).is_err());
    for bit in [0, 15, 47, 55, 87, 95, 127, 135, 143, 151, 159, 191] {
        let mut other = claim;
        other[bit] ^= 1;
        assert!(open_channel(&key, &other, package.clone()).is_err());
    }
    let mut corrupt = package;
    corrupt[31] ^= 1;
    assert!(open_channel(&key, &claim, corrupt).is_err());
}

#[test]
fn custody_channel_rejects_truncation_and_empty_payload() {
    let key = [1; 32];
    let claim = [3; 192];
    let package = seal_channel(&key, &claim, codec::Secret(vec![4; 48])).unwrap();
    for len in 0..package.len() {
        assert!(open_channel(&key, &claim, package[..len].to_vec()).is_err());
    }
}

#[cfg(target_os = "linux")]
mod installation {
    use super::super::{disk, *};
    use crate::cluster::{
        pki::custody::{FounderCustody, LearnerCustody},
        startup_plan::{Owner, tests::Layout},
    };
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    const NOW: u64 = 1_000_000;
    const CLUSTER: [u8; 16] = [1; 16];
    const CHANNEL: [u8; 32] = [3; 32];
    const CONTEXT: [u8; 192] = [4; 192];

    fn private(layout: &Layout) -> [PathBuf; 2] {
        let paths = ["wrap", "staging"].map(|name| layout.root.path().join(name));
        for path in &paths {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        paths
    }

    struct Fixture {
        _issuer: Layout,
        receiver: Layout,
        owner: Owner,
        learner: LearnerCustody,
        root: Vec<u8>,
        sealed: Vec<u8>,
    }

    impl Fixture {
        fn new() -> Self {
            let issuer = Layout::new();
            let owner = Owner::acquire(issuer.plan("create-founder")).unwrap();
            let [wrap, staging] = private(&issuer);
            let mut founder = FounderCustody::create(&owner, &wrap, &staging).unwrap();
            let (original, _) = founder.seal(CLUSTER, NOW).unwrap();
            let root = original.cert_der().to_vec();
            let sealed = founder
                .seal_for_voter(CLUSTER, &root, NOW, &CHANNEL, &CONTEXT)
                .unwrap();
            let receiver = Layout::new();
            let owner = Owner::acquire(receiver.plan("join")).unwrap();
            let [wrap, staging] = private(&receiver);
            let mut learner = LearnerCustody::create(&owner, &wrap, &staging).unwrap();
            learner.seal(CLUSTER, original.fingerprint()).unwrap();
            Self {
                _issuer: issuer,
                receiver,
                owner,
                learner,
                root,
                sealed,
            }
        }

        fn receive(&mut self, now: u64, key: &[u8; 32]) -> Result<(), Error> {
            self.learner.receive_voter_ca(
                &self.owner,
                CLUSTER,
                &self.root,
                now,
                (key, &CONTEXT, self.sealed.clone()),
            )
        }

        fn load(&self) -> Result<ClusterCa, Error> {
            self.learner.load_voter_ca(CLUSTER, &self.root, NOW)
        }
    }

    #[test]
    fn generated_ca_seals_and_loads_only_after_checked_durable_completion() {
        let mut f = Fixture::new();
        assert!(f.load().is_err());
        f.receive(NOW, &CHANNEL).unwrap();
        assert!(f.load().unwrap().cert_der() == f.root);
        assert!(f.learner.member().is_ok());
        let path = f.receiver.root.path().join("voter-custody/ca-key");
        let saved = fs::read(&path).unwrap();
        assert!(saved != f.sealed);
        assert!(f.receive(NOW, &CHANNEL).is_err());
        assert!(fs::read(&path).unwrap() == saved);
        let mut changed = saved;
        changed[140] ^= 1;
        fs::write(path, changed).unwrap();
        assert!(f.load().is_err());
    }

    #[test]
    fn wrong_tls_context_key_pair_and_checked_time_do_not_create_custody() {
        let mut f = Fixture::new();
        assert!(f.receive(NOW, &[7; 32]).is_err());
        let (other, key) = ClusterCa::generate(NOW).unwrap();
        let key = codec::Secret(key);
        assert!(other.cert_der() != f.root);
        let swapped = codec::Secret(codec::payload(Kind::Ca, &key.0, &f.root).unwrap());
        let swapped = seal_channel(&CHANNEL, &CONTEXT, swapped).unwrap();
        let original = std::mem::replace(&mut f.sealed, swapped);
        assert_eq!(f.receive(NOW, &CHANNEL), Err(Error::KeyMismatch));
        f.sealed = original;
        assert_eq!(f.receive(u64::MAX, &CHANNEL), Err(Error::Clock));
        assert!(!f.receiver.root.path().join("voter-custody").exists());
        assert!(f.load().is_err());
    }

    #[test]
    fn interrupted_writes_and_unconfirmed_readback_spend_the_reservation() {
        for step in [disk::Fault::Write, disk::Fault::Sync, disk::Fault::Readback] {
            let mut f = Fixture::new();
            f.learner.voter.fault = Some(step);
            assert_eq!(f.receive(NOW, &CHANNEL), Err(Error::Unavailable));
            assert!(
                f.receiver
                    .root
                    .path()
                    .join("voter-custody/reservation")
                    .exists()
            );
            assert!(f.load().is_err());
            f.learner.voter.fault = None;
            assert!(f.receive(NOW, &CHANNEL).is_err());
            assert!(f.learner.member().is_ok());
        }
    }
}
