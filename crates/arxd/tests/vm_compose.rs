use arxd::vm::{
    AssetPin, UbuntuRecipe,
    paths::ArtifactPin,
    ubuntu_compose::{Closure, OfflineVerifier, PackagePin, UbuntuComposeRecipe},
    ubuntu_web::{IsoPoolManifest, IsoPoolVerifier, PROFILE_PIN, PoolPackage, UbuntuWebRecipe},
};
use sha2::{Digest, Sha256};

struct Offline(bool);

impl IsoPoolVerifier for Offline {
    fn verify(&self, _: &UbuntuRecipe, _: &IsoPoolManifest<'_>) -> bool {
        self.0
    }
}

impl OfflineVerifier for Offline {
    fn verify(&self, _: [u8; 32], _: &Closure<'_>) -> bool {
        self.0
    }
}

fn asset(bytes: &[u8]) -> AssetPin {
    AssetPin {
        length: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    }
}

fn artifact(bytes: &[u8]) -> ArtifactPin {
    let pin = asset(bytes);
    ArtifactPin {
        size: pin.length,
        sha256: pin.sha256,
    }
}

#[test]
fn compose_offline_closure_attests_order_medium_and_install_profiles() {
    let guest = b"fake-arx-guest";
    let iso = AssetPin {
        length: 1,
        sha256: UbuntuRecipe::ISO_SHA256,
    };
    let base = UbuntuRecipe {
        iso: Some(iso),
        kernel: Some(asset(b"kernel")),
        initrd: Some(asset(b"initrd")),
        guest: Some(asset(guest)),
        service: Some(asset(UbuntuRecipe::SERVICE)),
        finalize: Some(asset(UbuntuRecipe::FINALIZE)),
        nocloud_schema: Some(UbuntuRecipe::NOCLOUD_PROFILE_PIN),
        firmware_code: Some(asset(b"code")),
        firmware_vars: Some(asset(b"vars")),
        qemu_receipt: Some(asset(b"receipt")),
    };
    let pool = IsoPoolManifest {
        iso,
        signed_metadata: artifact(b"ubuntu-index"),
        packages: &[PoolPackage {
            name: "nginx",
            version: "1.0",
            relative_path: "pool/nginx.deb",
            artifact: artifact(b"nginx"),
        }],
    };
    let web = UbuntuWebRecipe::qualify(
        &base,
        Some(PROFILE_PIN),
        &pool,
        Some(pool.canonical_pin().unwrap()),
        &Offline(true),
    )
    .unwrap();
    let medium = b"fake read-only Compose dependency ISO";
    let packages = [
        "containerd.io",
        "docker-ce-cli",
        "docker-ce",
        "docker-compose-plugin",
    ]
    .map(|name| PackagePin {
        name,
        version: "5:1.2.3-1~noble",
        archive: artifact(b"deb"),
    });
    let closure = Closure {
        docker_fingerprint: [1; 20],
        docker_signing_key: artifact(b"key"),
        signed_docker_metadata: artifact(b"docker-index"),
        signed_ubuntu_metadata: pool.signed_metadata,
        base_versions: artifact(b"dpkg-query receipt"),
        medium: artifact(medium),
        packages: &packages,
    };
    let web_digest = web.recipe_digest();
    let expected = closure.receipt(web_digest).unwrap();
    assert!(UbuntuComposeRecipe::qualify(&web, &closure, expected, &Offline(false)).is_err());
    let mut changed_order = packages;
    changed_order.swap(0, 1);
    let wrong_closure = Closure {
        packages: &changed_order,
        ..closure
    };
    assert!(UbuntuComposeRecipe::qualify(&web, &wrong_closure, expected, &Offline(true)).is_err());
    let recipe = UbuntuComposeRecipe::qualify(&web, &closure, expected, &Offline(true)).unwrap();
    assert_eq!(recipe.digest(), expected.sha256);
    recipe.verify_medium(medium.as_slice()).unwrap();
    let mut wrong = *medium;
    wrong[0] ^= 1;
    assert!(recipe.verify_medium(wrong.as_slice()).is_err());
    assert!(
        recipe
            .verify_medium(
                medium
                    .iter()
                    .copied()
                    .chain([0])
                    .collect::<Vec<_>>()
                    .as_slice()
            )
            .is_err()
    );

    let detached = recipe.detached_health();
    assert!(detached.contains("test ! -e /dev/disk/by-label/ARXCOMPOSE"));
    assert!(detached.contains("test ! -e /run/docker.sock"));
    assert!(detached.contains("verify_package '' 'docker-compose-plugin' '5:1.2.3-1~noble'"));
    assert!(detached.contains("/var/lib/arx-compose/backup-staging"));
    let seed = recipe.render_seed([1; 16], guest).unwrap();
    const SECTOR: usize = 2048;
    let primary = &seed[16 * SECTOR..17 * SECTOR];
    assert_eq!(&primary[..7], b"\x01CD001\x01");
    let sectors = u32::from_le_bytes(primary[80..84].try_into().unwrap()) as usize;
    assert_eq!(seed.len(), sectors * SECTOR);
    assert!(seed.len() <= 64 * 1024 * 1024);
    let has = |text: &str| {
        seed.windows(text.len())
            .any(|window| window == text.as_bytes())
    };
    assert!(has(
        "mount -t iso9660 -o ro,nodev,nosuid,noexec /dev/disk/by-label/ARXCOMPOSE"
    ));
    assert!(has(
        "systemctl --root=\"$target\" mask docker.service docker.socket containerd.service nginx.service"
    ));
    assert!(has("$target/var/lib/arx-compose/backup-staging"));
    assert!(!has("arxcovo-test"));
    assert!(!has("60-arxcovo-test.yaml"));
    let test_seed = base.render_test_seed([1; 16], guest).unwrap();
    assert!(
        test_seed
            .windows(b"arxcovo-test".len())
            .any(|window| window == b"arxcovo-test")
    );
}
