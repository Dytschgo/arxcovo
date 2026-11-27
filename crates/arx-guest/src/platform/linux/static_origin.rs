//! Linux implementation of the bounded static first-boot provisioning.
use super::*;
use arx_guest::static_origin::{self, Request, health_body, server_name};

const MODE_FILE: &str = "/sys/firmware/qemu_fw_cfg/by_name/opt/arxcovo/static-origin/raw";
const OPENSSL: &str = "/usr/bin/openssl";
const STATE: &str = "/etc/arxcovo";

pub(super) fn prepare(port: &File, compose: bool) -> io::Result<()> {
    let source = match File::open(MODE_FILE) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let value = bounded_read(source, 49)?;
    let value = value.strip_suffix('\0').unwrap_or(&value);
    let (generation, dns) = match value.split_once(':') {
        Some((generation, dns)) if compose => {
            (generation, Some(dns.parse().map_err(|_| invalid())?))
        }
        None if !compose => (value, None),
        _ => return Err(invalid()),
    };
    let generation = parse_generation(generation)?;
    let dir = walk_owned_dirs(STATE, ROOT_UID)?;
    let marker = bounded_read(
        File::from(open_at(dir.as_fd(), "web-finalized", libc::O_RDONLY, 0)?),
        64,
    )?;
    if marker != "ubuntu-noble-amd64-web-v1\n" {
        return Err(invalid());
    }
    static_origin::serve(
        port,
        port,
        generation,
        |Request::StaticOrigin { nonce, .. }| provision(dir.as_fd(), generation, nonce, dns),
    )
}

fn parse_generation(value: &str) -> io::Result<[u8; 16]> {
    let mut id = [0; 16];
    if value.len() != 32
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid());
    }
    for (index, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| invalid())?;
    }
    (id != [0; 16]).then_some(id).ok_or_else(invalid)
}

fn call(program: &'static str, args: &[&str]) -> io::Result<()> {
    match run(program, args, true)? {
        RunResult::Exited { code: 0, .. } => Ok(()),
        _ => Err(invalid()),
    }
}

fn run(program: &'static str, args: &[&str], capture: bool) -> io::Result<RunResult> {
    verify_engine(program)?;
    Ok(DockerRunner { program, cwd: "/" }.run(&Invocation {
        argv: args.iter().map(|arg| (*arg).to_owned()).collect(),
        timeout: Duration::from_secs(15),
        capture,
    }))
}

fn der(path: &str) -> io::Result<Vec<u8>> {
    match run(OPENSSL, &["x509", "-in", path, "-outform", "DER"], true)? {
        RunResult::Exited { code: 0, stdout }
            if (1..=static_origin::MAX_CERT).contains(&stdout.len()) =>
        {
            Ok(stdout)
        }
        _ => Err(invalid()),
    }
}

fn private_file(dir: BorrowedFd<'_>, name: &str, bound: u64) -> io::Result<File> {
    let file = File::from(open_at(dir, name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?);
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != ROOT_UID
        || meta.gid() != 0
        || meta.nlink() != 1
        || meta.mode() & 0o022 != 0
        || !(1..=bound).contains(&meta.len())
    {
        return Err(invalid());
    }
    Ok(file)
}

fn ensure_ca(dir: BorrowedFd<'_>, name: &str) -> io::Result<OwnedFd> {
    const STAGING: &str = "origin-ca-stage";
    const STAGING_DIR: &str = "/etc/arxcovo/origin-ca-stage";
    match private_subdir(dir, "origin-ca", ROOT_UID, false) {
        Ok(ca) => return Ok(ca),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let staging = private_subdir(dir, STAGING, ROOT_UID, true)?;
    // Recover the exclusively named, private staging slot after a partial
    // first boot. The committed CA directory is never replaced.
    for name in ["ca.key", "ca.pem"] {
        let leaf = component(name)?;
        if unsafe { libc::unlinkat(staging.as_raw_fd(), leaf.as_ptr(), 0) } != 0
            && io::Error::last_os_error().kind() != io::ErrorKind::NotFound
        {
            return Err(io::Error::last_os_error());
        }
    }
    call(
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
            "3650",
            "-keyout",
            &format!("{STAGING_DIR}/ca.key"),
            "-out",
            &format!("{STAGING_DIR}/ca.pem"),
            "-subj",
            &format!("/CN={name}"),
            "-addext",
            "basicConstraints=critical,CA:TRUE,pathlen:0",
            "-addext",
            "keyUsage=critical,keyCertSign",
            "-config",
            "/dev/null",
        ],
    )?;
    for name in ["ca.key", "ca.pem"] {
        private_file(staging.as_fd(), name, 4096)?.sync_all()?;
    }
    File::from(staging).sync_all()?;
    if unsafe {
        libc::renameat2(
            dir.as_raw_fd(),
            c"origin-ca-stage".as_ptr(),
            dir.as_raw_fd(),
            c"origin-ca".as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    File::from(dir.try_clone_to_owned()?).sync_all()?;
    private_subdir(dir, "origin-ca", ROOT_UID, false)
}

fn provision(
    dir: BorrowedFd<'_>,
    generation: [u8; 16],
    nonce: [u8; 16],
    dns: Option<std::net::Ipv4Addr>,
) -> io::Result<[Vec<u8>; 2]> {
    const CA: &str = "/etc/arxcovo/origin-ca/ca.pem";
    const CA_KEY: &str = "/etc/arxcovo/origin-ca/ca.key";
    const KEY: &str = "/run/arx-static-origin/leaf.key";
    const CERT: &str = "/run/arx-static-origin/leaf.pem";
    const CSR: &str = "/run/arx-static-origin/leaf.csr";
    const EXT: &str = "/run/arx-static-origin/leaf.ext";
    let name = server_name(generation);
    let body = health_body(nonce);
    let ca = ensure_ca(dir, &name)?;
    let ca_key = private_file(ca.as_fd(), "ca.key", 4096)?;
    if ca_key.metadata()?.mode() & 0o777 != 0o600 {
        return Err(invalid());
    }
    private_file(ca.as_fd(), "ca.pem", 4096)?;
    let run = walk_owned_dirs("/run", ROOT_UID)?;
    if !is_tmpfs(run.as_fd())? {
        return Err(invalid());
    }
    let run = private_subdir(run.as_fd(), "arx-static-origin", ROOT_UID, true)?;
    call(
        OPENSSL,
        &[
            "req",
            "-new",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-noenc",
            "-keyout",
            KEY,
            "-out",
            CSR,
            "-subj",
            &format!("/CN={name}"),
            "-config",
            "/dev/null",
        ],
    )?;
    let extensions = format!(
        "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:{name}\n"
    );
    replace_at(run.as_fd(), "leaf.ext", extensions.as_bytes(), 0o600, None)?;
    call(
        OPENSSL,
        &[
            "x509",
            "-req",
            "-in",
            CSR,
            "-CA",
            CA,
            "-CAkey",
            CA_KEY,
            "-set_serial",
            &format!("0x{body}"),
            "-days",
            "7",
            "-extfile",
            EXT,
            "-out",
            CERT,
        ],
    )?;
    // The routing adapter uses these names on every restart. Keep the
    // persistent copy's TLS leaf and key separate from the generation CA.
    replace_at(
        dir,
        "origin.key",
        bounded_read(private_file(run.as_fd(), "leaf.key", 4096)?, 4096)?.as_bytes(),
        0o600,
        None,
    )?;
    replace_at(
        dir,
        "origin.crt",
        bounded_read(private_file(run.as_fd(), "leaf.pem", 4096)?, 4096)?.as_bytes(),
        0o600,
        None,
    )?;
    configure_network(dns)?;
    if dns.is_some() {
        // The base image masks these services. Do not enable them across
        // boots. Only this checked static provisioning path starts them.
        let units = ["containerd.service", "docker.socket", "docker.service"];
        let mut unmask = vec!["unmask"];
        unmask.extend(units);
        call("/usr/bin/systemctl", &unmask)?;
        let mut start = vec!["start"];
        start.extend(units);
        call("/usr/bin/systemctl", &start)?;
    }
    let config = config(&name, &body, dns.is_some());
    replace_at(dir, "nginx.conf", config.as_bytes(), 0o600, None)?;
    let nginx = compose::routing::NGINX_BIN;
    let path = compose::routing::NGINX_CONFIG_PATH;
    call(nginx, &["-t", "-c", path])?;
    let launch = DockerRunner {
        program: nginx,
        cwd: "/",
    }
    .run(&Invocation {
        argv: ["-c", path].map(str::to_owned).into(),
        timeout: Duration::from_secs(15),
        capture: false,
    });
    if !matches!(launch, RunResult::Exited { code: 0, .. }) {
        return Err(invalid());
    }
    Ok([der(CA)?, der(CERT)?])
}

fn configure_network(dns: Option<std::net::Ipv4Addr>) -> io::Result<()> {
    const IP: &str = "/usr/sbin/ip";
    let mut nic = None;
    let mut entries = fs::read_dir("/sys/class/net")?;
    for entry in entries.by_ref().take(32) {
        let entry = entry?;
        let name = entry.file_name().into_string().map_err(|_| invalid())?;
        if name.is_empty()
            || name.len() > 15
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(invalid());
        }
        let address = bounded_read(File::open(entry.path().join("address"))?, 32)?;
        if address.trim_end_matches('\n') == "52:54:00:12:34:56" && nic.replace(name).is_some() {
            return Err(invalid());
        }
    }
    if entries.next().is_some() {
        return Err(invalid());
    }
    let nic = nic.ok_or_else(invalid)?;
    call(IP, &["link", "set", "dev", &nic, "up"])?;
    call(IP, &["-4", "addr", "replace", "10.0.2.15/24", "dev", &nic])?;
    if let Some(dns) = dns {
        call(
            IP,
            &[
                "-4", "route", "replace", "default", "via", "10.0.2.2", "dev", &nic,
            ],
        )?;
        call("/usr/bin/resolvectl", &["dns", &nic, &dns.to_string()])?;
        call("/usr/bin/resolvectl", &["default-route", &nic, "yes"])?;
    }
    Ok(())
}

fn config(name: &str, body: &str, compose: bool) -> String {
    let site = if compose {
        "return 404;"
    } else {
        "root /srv/arxcovo/site; try_files $uri $uri/ =404;"
    };
    format!(
        "user www-data;\nerror_log /dev/null;\nevents {{}}\nhttp {{\ninclude /etc/nginx/mime.types;\naccess_log off;\nserver {{\nlisten 10.0.2.15:443 ssl;\nserver_name {name};\nssl_certificate /etc/arxcovo/origin.crt;\nssl_certificate_key /etc/arxcovo/origin.key;\nssl_protocols TLSv1.2 TLSv1.3;\nlocation = /arx-health {{ return 200 '{body}'; }}\nlocation / {{ {site} }}\n}}\n}}\n"
    )
}

#[cfg(test)]
#[test]
fn config_has_the_static_loopback_forward_target_and_boot_witness() {
    let name = server_name([2; 16]);
    let body = health_body([3; 16]);
    let compose = config(&name, &body, true);
    assert!(compose.contains("listen 10.0.2.15:443 ssl;"));
    assert!(compose.contains(&format!(
        "location = /arx-health {{ return 200 '{body}'; }}"
    )));
    assert!(compose.contains("location / { return 404; }"));
    assert!(config(&name, &body, false).contains("root /srv/arxcovo/site;"));
    assert_eq!(parse_generation(&"02".repeat(16)).unwrap(), [2; 16]);
    assert!(parse_generation(&"00".repeat(16)).is_err());
    assert!(parse_generation(&"FF".repeat(16)).is_err());
}
