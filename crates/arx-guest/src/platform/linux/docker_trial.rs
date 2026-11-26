use super::*;
use arx_guest::docker_trial::{self, BootMode, Health};

const MODE_FILE: &str = "/sys/firmware/qemu_fw_cfg/by_name/opt/arxcovo/compose-trial/raw";
const SYSTEMCTL: &str = "/usr/bin/systemctl";
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

    fn provision(&mut self) -> bool {
        let Some([_, server, client, plugin]) = self.versions.take() else {
            return false;
        };
        // The endpoint is an independent disposable copy. Only this copy
        // gets these unit-mask changes. nginx stays masked for origin setup.
        let units = &UNITS[..3];
        let systemctl = |verb| {
            let mut args = vec![verb];
            args.extend_from_slice(units);
            output(SYSTEMCTL, &args).is_some()
        };
        if !systemctl("unmask") || !systemctl("start") {
            return false;
        }
        let expected = format!("{} {}\n", upstream(&client), upstream(&server));
        let plugin = format!("v{}\n", upstream(&plugin));
        docker(&[
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
            && verify_local_volume()
    }
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
