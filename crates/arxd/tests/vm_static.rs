use arxd::vm::{
    AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe,
    paths::ArtifactPin,
    static_web::{
        FullCopyReservation, InvalidTransition, NameError, PublishingPhase, StaticAgentAction,
        StaticEvent, StaticName, StaticRefusal, StaticState, WebHealth, authorize_agent,
    },
    ubuntu_web::{
        IsoPoolManifest, IsoPoolVerifier, OriginProvisioning, PROFILE_PIN, PoolPackage,
        UbuntuWebRecipe,
    },
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const BUILD: [u8; 16] = [0x5a; 16];
const GUEST: &[u8] = b"offline-test-guest\x00\xff";
const ISO: AssetPin = AssetPin {
    length: 19,
    sha256: UbuntuRecipe::ISO_SHA256,
};
const NGINX: PoolPackage<'_> = PoolPackage {
    name: "nginx",
    version: "1.24.0-2ubuntu7.1",
    relative_path: "pool/main/n/nginx/nginx_amd64.deb",
    artifact: ArtifactPin {
        size: 23,
        sha256: [0x41; 32],
    },
};
const COMMON: PoolPackage<'_> = PoolPackage {
    name: "nginx-common",
    version: "1.24.0-2ubuntu7.1",
    relative_path: "pool/main/n/nginx/nginx-common_all.deb",
    artifact: ArtifactPin {
        size: 47,
        sha256: [0xe2; 32],
    },
};

fn base_recipe() -> UbuntuRecipe {
    let pin = |bytes: &[u8]| AssetPin {
        length: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    };
    let asset = Some(pin(b"synthetic host asset"));
    UbuntuRecipe {
        iso: Some(ISO),
        kernel: asset,
        initrd: asset,
        guest: Some(pin(GUEST)),
        service: Some(pin(UbuntuRecipe::SERVICE)),
        finalize: Some(pin(UbuntuRecipe::FINALIZE)),
        nocloud_schema: Some(UbuntuRecipe::NOCLOUD_PROFILE_PIN),
        firmware_code: asset,
        firmware_vars: asset,
        qemu_receipt: asset,
    }
}

fn manifest<'a>(packages: &'a [PoolPackage<'a>]) -> IsoPoolManifest<'a> {
    IsoPoolManifest {
        iso: ISO,
        signed_metadata: ArtifactPin {
            size: 17,
            sha256: [0x83; 32],
        },
        packages,
    }
}

struct Inspection(bool);

impl IsoPoolVerifier for Inspection {
    fn verify(&self, base: &UbuntuRecipe, manifest: &IsoPoolManifest<'_>) -> bool {
        assert_eq!(base.iso.unwrap().sha256, ISO.sha256);
        assert!(!manifest.packages.is_empty());
        self.0
    }
}

fn qualify<'a>(base: &'a UbuntuRecipe, manifest: &'a IsoPoolManifest<'a>) -> UbuntuWebRecipe<'a> {
    UbuntuWebRecipe::qualify(
        base,
        Some(PROFILE_PIN),
        manifest,
        Some(manifest.canonical_pin().unwrap()),
        &Inspection(true),
    )
    .unwrap()
}

// Read independently through the Joliet descriptor, directory and big-endian
// extent fields. The recipe extends the generated primary namespace instead.
fn joliet_files(image: &[u8]) -> BTreeMap<String, &[u8]> {
    let extent = |record: &[u8]| {
        let number = |at| u32::from_be_bytes(record[at..at + 4].try_into().unwrap()) as usize;
        let start = number(6) * 2048;
        &image[start..start + number(14)]
    };
    let descriptor = &image[17 * 2048..18 * 2048];
    assert_eq!(&descriptor[..7], b"\x02CD001\x01");
    let mut directory = extent(&descriptor[156..]);
    let mut files = BTreeMap::new();
    while directory[0] != 0 {
        let (record, remaining) = directory.split_at(usize::from(directory[0]));
        let name = &record[33..33 + usize::from(record[32])];
        if record[25] == 0 {
            let name = String::from_utf16(
                &name
                    .chunks_exact(2)
                    .map(|pair| u16::from_be_bytes(pair.try_into().unwrap()))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            assert!(files.insert(name, extent(record)).is_none());
        }
        directory = remaining;
    }
    files
}

fn yaml_fields(text: &str) -> BTreeMap<String, &str> {
    let mut fields = BTreeMap::new();
    let mut parents = Vec::new();
    for line in text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let content = line.trim_start();
        let indent = line.len() - content.len();
        while parents.last().is_some_and(|&(depth, _)| depth >= indent) {
            parents.pop();
        }
        let (key, value) = content
            .strip_prefix("- ")
            .unwrap_or(content)
            .split_once(':')
            .unwrap();
        parents.push((indent, key));
        if !value.trim().is_empty() {
            let path = parents
                .iter()
                .map(|&(_, key)| key)
                .collect::<Vec<_>>()
                .join(".");
            assert!(fields.insert(path, value.trim()).is_none());
        }
    }
    fields
}

fn nginx_directives(text: &str) -> Vec<(String, &str)> {
    let mut blocks = Vec::new();
    let mut directives = Vec::new();
    for token in text.trim().split_inclusive(['{', '}', ';']) {
        let (head, tail) = token.trim().split_at(token.trim().len() - 1);
        match tail {
            "{" => blocks.push(head.trim()),
            "}" => {
                assert!(head.is_empty());
                blocks.pop().expect("open block");
            }
            ";" => directives.push((blocks.join("/"), head.trim())),
            _ => panic!("unterminated directive"),
        }
    }
    assert!(blocks.is_empty());
    directives
}

fn heredoc<'a>(script: &'a str, tag: &str) -> &'a str {
    let (_, contents) = script.split_once(&format!("<<'{tag}'\n")).unwrap();
    contents.split_once(&format!("\n{tag}\n")).unwrap().0
}

#[test]
fn web_seed_golden_preserves_offline_base_and_installs_exact_web_profile() {
    let base = base_recipe();
    let manifest = manifest(&[NGINX, COMMON]);
    let recipe = qualify(&base, &manifest);
    let seed = recipe.render_seed(BUILD, GUEST).unwrap();
    assert_eq!(
        seed,
        qualify(&base, &manifest).render_seed(BUILD, GUEST).unwrap()
    );
    assert_eq!(
        hex::encode(Sha256::digest(&seed)),
        "f558582ed7da8fa0a80f48294416c37f7a38d47e3e94dc7cd76edf5529378fa8",
    );
    assert_eq!(
        hex::encode(recipe.recipe_digest()),
        "e5b0463595eef89858ee44b5e6c96e470b71708523425340acbcc56d27b45b30",
    );
    let original = base.render_seed(BUILD, GUEST).unwrap();
    let original = joliet_files(&original);
    let mut files = joliet_files(&seed);
    let user = std::str::from_utf8(files.remove("user-data.;1").unwrap()).unwrap();
    assert_eq!(files.len(), 6);
    for (name, bytes) in files {
        assert_eq!(bytes, original[&name], "{name}");
    }
    let base_user = std::str::from_utf8(original["user-data.;1"]).unwrap();
    let commands = user
        .strip_prefix(base_user)
        .unwrap()
        .strip_prefix("    - |\n")
        .unwrap();
    let commands = commands
        .lines()
        .map(|line| line.strip_prefix("      ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let fields = yaml_fields(base_user.split_once("  late-commands:").unwrap().0);
    assert_eq!(
        fields["autoinstall.network.ethernets.disabled.activation-mode"],
        "'off'"
    );
    assert_eq!(
        fields["autoinstall.apt.mirror-selection.primary.uri"],
        "file:///cdrom"
    );
    assert_eq!(fields["autoinstall.apt.geoip"], "false");
    assert_eq!(fields["autoinstall.refresh-installer.update"], "false");
    assert_eq!(fields["autoinstall.shutdown"], "poweroff");

    let parts: Vec<_> = commands
        .lines()
        .filter(|line| line.starts_with("stage_deb '"))
        .collect();
    assert_eq!(parts.len(), 2);
    for (line, package) in parts.into_iter().zip([NGINX, COMMON]) {
        let args: Vec<_> = line.split('\'').skip(1).step_by(2).collect();
        assert_eq!(
            args,
            [
                package.relative_path,
                package.name,
                &package.artifact.size.to_string(),
                &hex::encode(package.artifact.sha256)
            ]
        );
    }
    let mask = commands.find("mask nginx.service apt-daily.timer").unwrap();
    let install = commands
        .find("apt-get -y --no-download --no-remove --no-install-recommends")
        .unwrap();
    assert!(commands.find("sha256sum --check --strict").unwrap() < mask && mask < install);
    assert!(commands.contains("Dir::Etc::sourcelist=/dev/null -o Dir::Etc::sourceparts=-"));
    assert!(commands.contains("install /var/lib/arxcovo/web-install/*.deb"));
    assert!(commands.contains("sites-enabled/default"));

    let config = heredoc(&commands, "ARX_NGINX");
    let directives = nginx_directives(config);
    let expected = [
        ("", "user www-data"),
        ("http", "include /etc/nginx/mime.types"),
        ("http", "default_type application/octet-stream"),
        ("http", "access_log off"),
        ("http/server", "listen 10.0.2.15:443 ssl"),
        ("http/server", "server_name _"),
        ("http/server", "ssl_protocols TLSv1.2 TLSv1.3"),
        ("http/server", "ssl_certificate /etc/arxcovo/origin.crt"),
        ("http/server", "ssl_certificate_key /etc/arxcovo/origin.key"),
        ("http/server", "root /srv/arxcovo/site"),
        ("http/server", "index index.html"),
        (
            "http/server/location = /arx-health",
            "default_type text/plain",
        ),
        ("http/server/location = /arx-health", r#"return 200 "ok\n""#),
        ("http/server/location /", "try_files $uri $uri/ =404"),
    ];
    assert!(
        directives
            .iter()
            .map(|(context, line)| (context.as_str(), *line))
            .eq(expected)
    );
    let network = heredoc(&commands, "ARX_NETWORK");
    let fields = yaml_fields(network);
    assert_eq!(
        fields,
        BTreeMap::from([
            ("network.version".into(), "2"),
            ("network.renderer".into(), "networkd"),
            (
                "network.ethernets.origin.match.macaddress".into(),
                "'52:54:00:12:34:56'"
            ),
            (
                "network.ethernets.origin.addresses".into(),
                "[10.0.2.15/24]"
            ),
            ("network.ethernets.origin.dhcp4".into(), "false"),
            ("network.ethernets.origin.dhcp6".into(), "false"),
            ("network.ethernets.origin.accept-ra".into(), "false"),
            ("network.ethernets.origin.link-local".into(), "[]"),
        ])
    );
    let sysctl = heredoc(&commands, "ARX_IPV6");
    assert_eq!(
        sysctl,
        "net.ipv6.conf.all.disable_ipv6=1\nnet.ipv6.conf.default.disable_ipv6=1"
    );
    let health = recipe.detached_health();
    for (path, contents) in [
        ("nginx.conf", config),
        ("static-network.yaml", network),
        ("static-ipv6.conf", sysctl),
    ] {
        let pin = hex::encode(Sha256::digest(format!("{contents}\n")));
        assert!(
            health
                .lines()
                .any(|line| line == format!("{pin}  /etc/arxcovo/{path}"))
        );
    }
    for line in [
        "absent /etc/arxcovo/origin.key",
        "absent /etc/arxcovo/origin.crt",
        "test ! -e /dev/sr0",
        "verify_package \"\" 'nginx' '1.24.0-2ubuntu7.1'",
        "verify_package \"\" 'nginx-common' '1.24.0-2ubuntu7.1'",
    ] {
        assert!(health.lines().any(|entry| entry == line), "{line}");
    }
}

#[test]
fn missing_bad_and_unverified_pins_refuse_before_writing() {
    let base = base_recipe();
    let good = manifest(&[NGINX, COMMON]);
    let pin = good.canonical_pin().unwrap();
    for (profile, receipt, verified) in [
        (None, Some(pin), true),
        (Some(PROFILE_PIN), None, true),
        (Some(PROFILE_PIN), Some(PROFILE_PIN), true),
        (Some(PROFILE_PIN), Some(pin), false),
    ] {
        let error = UbuntuWebRecipe::qualify(&base, profile, &good, receipt, &Inspection(verified))
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "recipe_unqualified");
    }
    for packages in [
        vec![],
        vec![COMMON],
        vec![COMMON, NGINX],
        vec![NGINX, NGINX],
        vec![
            NGINX,
            PoolPackage {
                relative_path: NGINX.relative_path,
                ..COMMON
            },
        ],
        vec![PoolPackage {
            relative_path: "pool/../nginx.deb",
            ..NGINX
        }],
        vec![PoolPackage {
            version: "1'; shutdown",
            ..NGINX
        }],
        vec![PoolPackage {
            artifact: ArtifactPin {
                size: u64::MAX,
                ..NGINX.artifact
            },
            ..NGINX
        }],
    ] {
        assert!(matches!(
            manifest(&packages).canonical_pin(),
            Err(RecipeError::Unqualified)
        ));
    }
    let modified = [
        PoolPackage {
            version: "1.24.0-2ubuntu7.2",
            ..NGINX
        },
        COMMON,
    ];
    let modified = manifest(&modified);
    assert!(matches!(
        UbuntuWebRecipe::qualify(
            &base,
            Some(PROFILE_PIN),
            &modified,
            Some(pin),
            &Inspection(true)
        ),
        Err(RecipeError::Unqualified)
    ));
    assert_ne!(
        qualify(&base, &modified).recipe_digest(),
        qualify(&base, &good).recipe_digest()
    );
    let mut out = Vec::new();
    assert!(matches!(
        qualify(&base, &good).write_seed(UbuntuAssets::default(), BUILD, GUEST, &mut out),
        Err(RecipeError::Unqualified)
    ));
    assert!(out.is_empty());
}

#[test]
fn origin_identity_is_per_generation_and_bounded() {
    let first = OriginProvisioning::new(BUILD);
    let second = OriginProvisioning::new([1; 16]);
    assert_eq!(
        first.server_name(),
        format!("arx-{}.invalid", "5a".repeat(16))
    );
    assert_ne!(first.server_name(), second.server_name());
    assert!(first.payload(b"", b"fixture").is_none());
    assert!(first.payload(&[1; 8193], b"fixture").is_none());
    assert!(first.payload(b"fixture", &[2; 4097]).is_none());
    let payload = first
        .payload(b"public fixture", b"private fixture")
        .unwrap();
    assert_eq!(payload.server_name(), first.server_name());
    assert_eq!(
        payload.into_private_parts(),
        (b"public fixture".as_slice(), b"private fixture".as_slice())
    );
}

#[test]
fn static_names_are_bounded_and_collide_across_case() {
    for invalid in [
        "",
        "-a",
        "a-",
        "a.b",
        "../tmp",
        "a\\b",
        "A B",
        "ümlaut",
        "a\n",
        &"a".repeat(64),
    ] {
        assert_eq!(StaticName::parse(invalid), Err(NameError::Invalid));
    }
    assert_eq!(StaticName::parse("0").unwrap().as_str(), "0");
    assert_eq!(
        StaticName::parse(&"a".repeat(63)).unwrap().as_str().len(),
        63
    );
    let used = BTreeSet::from([StaticName::parse("Arx-Web-1").unwrap()]);
    let duplicate = StaticName::parse("ARX-web-1").unwrap();
    assert_eq!(duplicate.as_str(), "arx-web-1");
    assert_eq!(
        duplicate.require_available(|key| used.contains(key)),
        Err(NameError::Unavailable)
    );
    assert_eq!(
        StaticName::parse("arx-WEB-2")
            .unwrap()
            .require_available(|key| used.contains(key)),
        Ok(())
    );
}

#[test]
fn static_state_requires_proven_settlement_and_recovers_to_stopped() {
    use StaticEvent as Event;
    use StaticState as State;
    let states = [
        State::Creating,
        State::Stopped,
        State::Running,
        State::Recovering,
        State::Failed,
    ];
    let events = [
        Event::Created,
        Event::Started,
        Event::Stopped,
        Event::NeedsRecovery,
        Event::RecoveredStopped,
        Event::CreationFailed,
    ];
    let transitions = [
        [
            Some(State::Stopped),
            None,
            None,
            Some(State::Recovering),
            None,
            Some(State::Failed),
        ],
        [
            None,
            Some(State::Running),
            None,
            Some(State::Recovering),
            None,
            None,
        ],
        [
            None,
            None,
            Some(State::Stopped),
            Some(State::Recovering),
            None,
            None,
        ],
        [
            None,
            None,
            None,
            Some(State::Recovering),
            Some(State::Stopped),
            None,
        ],
        [None; 6],
    ];
    for (state, expected) in states.into_iter().zip(transitions) {
        for (event, result) in events.into_iter().zip(expected) {
            assert_eq!(
                state.settle(event),
                result.ok_or(InvalidTransition),
                "{state:?} {event:?}"
            );
        }
    }
}

#[test]
fn agent_policy_has_a_closed_action_set_and_checks_owner_and_state() {
    use StaticAgentAction::{Get, RunScript, Start, Stop};
    let actions = [Get, Start, Stop, RunScript];
    // Exhaustive by design. Adding any operation, including a destructive
    // operation, requires changing this compile-time API shape assertion.
    let code = |action| match action {
        Get => 0,
        Start => 1,
        Stop => 2,
        RunScript => 3,
    };
    assert_eq!(actions.map(code), [0, 1, 2, 3]);
    use StaticState::{Creating, Failed, Recovering, Running, Stopped};
    for (state, allowed) in [
        (Creating, [true, false, false, false]),
        (Stopped, [true, true, false, false]),
        (Running, [true, false, true, true]),
        (Recovering, [true, false, false, false]),
        (Failed, [true, false, false, false]),
    ] {
        for (action, allowed) in actions.into_iter().zip(allowed) {
            assert_eq!(
                authorize_agent(Some(("owner", state)), "other", action),
                Err(StaticRefusal::NotFound)
            );
            assert_eq!(
                authorize_agent(None, "owner", action),
                Err(StaticRefusal::NotFound)
            );
            assert_eq!(
                authorize_agent(Some(("owner", state)), "owner", action),
                allowed.then_some(()).ok_or(StaticRefusal::InvalidState)
            );
        }
    }
    assert_eq!(WebHealth::default(), WebHealth::Unavailable);
    assert_eq!(PublishingPhase::default(), PublishingPhase::Unpublished);
}

#[test]
fn full_copy_claim_covers_growth_metadata_and_staging_without_overflow() {
    let request = FullCopyReservation {
        qcow2_bytes: 100,
        qcow2_growth_bytes: 250,
        vars_bytes: 2,
        staging_bytes: 19,
        metadata_bytes: 3,
    };
    let claim = request.checked_claim().unwrap();
    assert_eq!((claim.images, claim.metadata, claim.staging), (352, 3, 19));
    let fits = FullCopyReservation {
        qcow2_bytes: u64::MAX - 4,
        qcow2_growth_bytes: 1,
        vars_bytes: 1,
        staging_bytes: 1,
        metadata_bytes: 1,
    };
    assert!(fits.checked_claim().is_some());
    for invalid in [
        FullCopyReservation {
            qcow2_bytes: u64::MAX,
            ..fits
        },
        FullCopyReservation {
            vars_bytes: 4,
            ..fits
        },
        FullCopyReservation {
            metadata_bytes: 3,
            ..fits
        },
        FullCopyReservation {
            staging_bytes: 2,
            ..fits
        },
        FullCopyReservation {
            qcow2_bytes: 0,
            ..request
        },
        FullCopyReservation {
            vars_bytes: 0,
            ..request
        },
    ] {
        assert!(invalid.checked_claim().is_none());
    }
}
