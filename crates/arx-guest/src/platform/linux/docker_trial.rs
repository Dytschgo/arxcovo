use super::*;
use arx_guest::docker_trial::{self, BootMode, Health};

const MODE_FILE: &str = "/sys/firmware/qemu_fw_cfg/by_name/opt/arxcovo/compose-trial/raw";
const SYSTEMCTL: &str = "/usr/bin/systemctl";
const OPENSSL: &str = "/usr/bin/openssl";
const UNITS: [&str; 4] = [
    "containerd.service",
    "docker.socket",
    "docker.service",
    "nginx.service",
];
pub(super) fn mode() -> io::Result<Option<BootMode>> {
    let file = match File::open(MODE_FILE) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let value = bounded_read(file, 32)?;
    match value.strip_suffix('\0').unwrap_or(&value) {
        docker_trial::DETACHED => Ok(Some(BootMode::Detached)),
        docker_trial::ENDPOINT => Ok(Some(BootMode::Endpoint)),
        _ => Err(invalid()),
    }
}

#[derive(Default)]
pub(super) struct LinuxBootstrap {
    versions: Option<[String; 4]>,
}

fn health() -> io::Result<Health> {
    let absent = |path| match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Ok(_) => Ok(false),
        Err(error) => Err(error),
    };
    let private_empty = |path| {
        let dir = walk_owned_dirs(path, ROOT_UID)?;
        verify_private_dir(dir.as_fd(), ROOT_UID)?;
        let metadata = File::from(dir.try_clone()?).metadata()?;
        if metadata.gid() != 0 || metadata.mode() & 0o777 != 0o700 {
            return Err(invalid());
        }
        // Read via the held descriptor after checking every component.
        let empty = fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))?
            .next()
            .is_none();
        Ok::<_, io::Error>((dir, empty))
    };
    let (secrets, secrets_empty) = private_empty(compose::SECRET_ROOT)?;
    let (_, backups_empty) = private_empty("/var/lib/arx-compose/backup-staging")?;
    let swaps = bounded_read(File::open("/proc/swaps")?, 4096)?;
    let mountinfo = bounded_read(File::open("/proc/self/mountinfo")?, 128 << 10)?;
    Ok(Health {
        recipe_layout: effective_uid() == ROOT_UID && compose_finalized()? && backups_empty,
        media_and_residue_absent: absent("/dev/disk/by-label/ARXCOMPOSE")?
            && absent("/var/lib/arxcovo/compose-install")?
            && absent("/run/arx-compose-install")?
            && absent("/run/docker.sock")?,
        units_masked: UNITS.iter().all(|unit| {
            matches!(
                call(SYSTEMCTL, &["is-enabled", unit]),
                RunResult::Exited { code: 1, stdout } if stdout == b"masked\n"
            )
        }),
        units_inactive: UNITS.iter().all(|unit| {
            matches!(
                call(SYSTEMCTL, &["is-active", unit]),
                RunResult::Exited { code: 3, stdout } if stdout == b"inactive\n"
            )
        }),
        swap_disabled: swaps.lines().count() == 1 && swaps.starts_with("Filename"),
        secret_tmpfs: secrets_empty
            && is_tmpfs(secrets.as_fd())?
            && docker_trial::secret_mount_matches(&mountinfo),
    })
}

fn call(program: &'static str, args: &[&str]) -> RunResult {
    if verify_engine(program).is_err() {
        return RunResult::LaunchFailed;
    }
    DockerRunner {
        program,
        cwd: compose::ENGINE_CWD,
    }
    .run(&Invocation {
        argv: args.iter().map(|arg| (*arg).to_owned()).collect(),
        timeout: Duration::from_secs(15),
        capture: true,
    })
}

fn output(program: &'static str, args: &[&str]) -> Option<String> {
    match call(program, args) {
        RunResult::Exited { code: 0, stdout } => String::from_utf8(stdout).ok(),
        _ => None,
    }
}

impl docker_trial::Bootstrap for LinuxBootstrap {
    fn health(&mut self) -> Option<[u8; 32]> {
        if !health().ok()?.ready() {
            return None;
        }
        let mut versions = Vec::new();
        for name in docker_trial::PACKAGES {
            let state = output(
                "/usr/bin/dpkg-query",
                &["-W", "-f=${Status}:${Version}\\n", name],
            )?;
            let version = state
                .strip_prefix("install ok installed:")?
                .strip_suffix('\n')?;
            if version.is_empty()
                || version.len() > 128
                || !version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".:+~-".contains(&b))
            {
                return None;
            }
            versions.push(version.to_owned());
        }
        let versions: [String; 4] = versions.try_into().ok()?;
        let digest = docker_trial::package_digest(versions.each_ref().map(String::as_str));
        self.versions = Some(versions);
        Some(digest)
    }

    fn provision(&mut self, challenge: docker_trial::Challenge) -> Option<Vec<u8>> {
        let [_, server, client, plugin] = self.versions.take()?;
        // The endpoint is an independent disposable copy. Only this copy
        // gets these unit-mask changes. nginx stays masked for origin setup.
        let units = &UNITS[..3];
        let systemctl = |verb| {
            let mut args = vec![verb];
            args.extend_from_slice(units);
            output(SYSTEMCTL, &args).is_some()
        };
        if !systemctl("unmask") || !systemctl("start") {
            return None;
        }
        let expected = format!("{} {}\n", upstream(&client), upstream(&server));
        let plugin = format!("v{}\n", upstream(&plugin));
        let ready = docker(&[
            "version",
            "--format",
            "{{.Client.Version}} {{.Server.Version}}",
        ])
        .as_deref()
            == Some(expected.as_str())
            && docker(&["compose", "version", "--short"])
                .is_some_and(|value| value == plugin || value == plugin[1..])
            && docker(&["info", "--format", "{{.DockerRootDir}}"]).as_deref()
                == Some("/var/lib/docker\n")
            && walk_owned_dirs("/var/lib/containerd", ROOT_UID).is_ok()
            && verify_local_volume();
        if !ready {
            return None;
        }
        start_origin(challenge).ok()
    }
}

// The endpoint copy uses a dedicated runtime directory. The private key
// stays in the guest and disappears with /run on shutdown.
fn start_origin(challenge: docker_trial::Challenge) -> io::Result<Vec<u8>> {
    const ROOT: &str = "/run/arx-docker-trial-origin";
    const KEY: &str = "/run/arx-docker-trial-origin/leaf.key";
    const CERT: &str = "/run/arx-docker-trial-origin/leaf.pem";
    const CONFIG: &str = "/run/arx-docker-trial-origin/nginx.conf";
    let run = walk_owned_dirs("/run", ROOT_UID)?;
    if !is_tmpfs(run.as_fd())? {
        return Err(invalid());
    }
    // SAFETY: run is a held directory and the fixed component is NUL-terminated.
    if unsafe { libc::mkdirat(run.as_raw_fd(), c"arx-docker-trial-origin".as_ptr(), 0o700) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let dir = private_subdir(run.as_fd(), "arx-docker-trial-origin", ROOT_UID, false)?;
    let name = challenge.origin_name();
    output(
        OPENSSL,
        &[
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-noenc",
            "-days",
            "1",
            "-keyout",
            KEY,
            "-out",
            CERT,
            "-subj",
            &format!("/CN={name}"),
            "-addext",
            &format!("subjectAltName=DNS:{name}"),
            "-addext",
            "keyUsage=critical,digitalSignature",
            "-addext",
            "extendedKeyUsage=serverAuth",
            "-addext",
            "basicConstraints=critical,CA:FALSE",
            "-config",
            "/dev/null",
        ],
    )
    .ok_or_else(invalid)?;
    let key = File::from(open_at(dir.as_fd(), "leaf.key", libc::O_RDONLY, 0)?);
    let key_meta = key.metadata()?;
    if !key_meta.is_file()
        || key_meta.uid() != ROOT_UID
        || key_meta.nlink() != 1
        || key_meta.mode() & 0o777 != 0o600
    {
        return Err(invalid());
    }
    let config = origin_config(challenge, ROOT, KEY, CERT);
    replace_at(dir.as_fd(), "nginx.conf", config.as_bytes(), 0o600, None)?;
    output("/usr/sbin/nginx", &["-p", ROOT, "-c", CONFIG, "-t"]).ok_or_else(invalid)?;
    // nginx detaches its master after successful setup. Use an invocation
    // without capture so its inherited stdio does not hold the runner open.
    let launch = DockerRunner {
        program: "/usr/sbin/nginx",
        cwd: compose::ENGINE_CWD,
    }
    .run(&Invocation {
        argv: ["-p", ROOT, "-c", CONFIG].map(str::to_owned).into(),
        timeout: Duration::from_secs(15),
        capture: false,
    });
    if !matches!(launch, RunResult::Exited { code: 0, .. }) {
        return Err(invalid());
    }
    match call(OPENSSL, &["x509", "-in", CERT, "-outform", "DER"]) {
        RunResult::Exited { code: 0, stdout }
            if (1..=docker_trial::MAX_ORIGIN_CERT).contains(&stdout.len()) =>
        {
            Ok(stdout)
        }
        _ => Err(invalid()),
    }
}

fn origin_config(challenge: docker_trial::Challenge, root: &str, key: &str, cert: &str) -> String {
    let name = challenge.origin_name();
    let body = challenge.origin_health();
    let path = docker_trial::ORIGIN_HEALTH_PATH;
    format!(
        "user www-data;\npid {root}/nginx.pid;\nerror_log /dev/null;\nevents {{}}\nhttp {{\n\
         access_log off;\nserver {{\nlisten 10.0.2.15:443 ssl;\nserver_name {name};\n\
         ssl_certificate {cert};\nssl_certificate_key {key};\nssl_protocols TLSv1.2 TLSv1.3;\n\
         location = {path} {{ return 200 '{body}'; }}\nlocation / {{ return 404; }}\n}}\n}}\n"
    )
}

#[cfg(test)]
#[test]
fn origin_config_serves_only_the_boot_witness_at_the_forwarded_guest_address() {
    let challenge = docker_trial::Challenge {
        generation: 42,
        nonce: [0xaa; 16],
    };
    let config = origin_config(challenge, "/run/trial", "/run/trial/key", "/run/trial/cert");
    assert!(config.contains("listen 10.0.2.15:443 ssl;"));
    assert!(config.contains(&format!(
        "location = /arx-health {{ return 200 '{}'; }}",
        challenge.origin_health()
    )));
    assert!(config.contains("location / { return 404; }"));
    assert!(config.contains("ssl_certificate_key /run/trial/key;"));
}

fn docker(args: &[&str]) -> Option<String> {
    let mut argv = vec![
        "--config",
        compose::DOCKER_CONFIG,
        "--host",
        "unix:///run/docker.sock",
    ];
    argv.extend_from_slice(args);
    output(compose::DOCKER, &argv)
}

fn verify_local_volume() -> bool {
    const NAME: &str = "arx-qualification-probe";
    const DATA: &str = "/var/lib/docker/volumes/arx-qualification-probe/_data";
    // Only a fresh copy qualifies. The collector retires the entire copy.
    docker(&["volume", "ls", "--format", "{{.Name}}"]).as_deref() == Some("")
        && docker(&["volume", "create", "--driver", "local", NAME]).as_deref()
            == Some("arx-qualification-probe\n")
        && docker(&[
            "volume",
            "inspect",
            "--format",
            "{{.Driver}} {{.Mountpoint}}",
            NAME,
        ])
        .as_deref()
            == Some("local /var/lib/docker/volumes/arx-qualification-probe/_data\n")
        && walk_owned_dirs(DATA, ROOT_UID).is_ok()
}

fn upstream(version: &str) -> &str {
    let version = version
        .split_once(':')
        .map_or(version, |(_, version)| version);
    version
        .split_once('-')
        .map_or(version, |(version, _)| version)
}
