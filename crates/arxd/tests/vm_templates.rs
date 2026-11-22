use arxd::vm::{AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    ops::Range,
};

const BLOCK: usize = 2048;
const GUEST: &[u8] = b"offline-test-guest\x00\xff";
const HOST_ASSET: &[u8] = b"synthetic host asset";
const ID: [u8; 16] = [0x5a; 16];

fn pin(bytes: &[u8]) -> AssetPin {
    AssetPin {
        length: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    }
}

fn fixture_recipe() -> UbuntuRecipe {
    UbuntuRecipe {
        iso: Some(AssetPin {
            length: HOST_ASSET.len() as u64, // Synthetic refusal fixture.
            sha256: UbuntuRecipe::ISO_SHA256,
        }),
        kernel: Some(pin(HOST_ASSET)),
        initrd: Some(pin(HOST_ASSET)),
        guest: Some(pin(GUEST)),
        service: Some(pin(UbuntuRecipe::SERVICE)),
        finalize: Some(pin(UbuntuRecipe::FINALIZE)),
        nocloud_schema: Some(pin(UbuntuRecipe::NOCLOUD_SCHEMA)),
        firmware_code: Some(pin(HOST_ASSET)),
        firmware_vars: Some(pin(HOST_ASSET)),
        qemu_receipt: Some(pin(HOST_ASSET)),
    }
}

fn fixture() -> Vec<u8> {
    fixture_recipe().render_seed(ID, GUEST).unwrap()
}

// Independent reader for this two-namespace, single-root ISO9660 profile.
// It discovers directories, path tables and files from on-media records.
type Parsed<T> = Result<T, &'static str>;

fn number16(bytes: &[u8]) -> Parsed<u16> {
    let le = u16::from_le_bytes(bytes[..2].try_into().unwrap());
    let be = u16::from_be_bytes(bytes[2..4].try_into().unwrap());
    (le == be).then_some(le).ok_or("16-bit endian split")
}

fn number32(bytes: &[u8]) -> Parsed<usize> {
    let le = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    let be = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
    (le == be)
        .then_some(le as usize)
        .ok_or("32-bit endian split")
}

#[derive(Clone)]
struct Record<'a> {
    name: &'a [u8],
    block: usize,
    size: usize,
    directory: bool,
}

fn record(bytes: &[u8]) -> Parsed<(Record<'_>, usize)> {
    let length = usize::from(*bytes.first().ok_or("record length")?);
    let data = bytes.get(..length).ok_or("record length")?;
    if length < 34 || data[1] != 0 || data[26..28] != [0, 0] || number16(&data[28..32])? != 1 {
        return Err("record fields");
    }
    let name_len = usize::from(data[32]);
    let name_end = 33 + name_len;
    if name_len == 0
        || name_end.next_multiple_of(2) != length
        || data[name_end..].iter().any(|&byte| byte != 0)
        || !matches!(data[25], 0 | 2)
    {
        return Err("record shape");
    }
    Ok((
        Record {
            name: &data[33..name_end],
            block: number32(&data[2..10])?,
            size: number32(&data[10..18])?,
            directory: data[25] == 2,
        },
        length,
    ))
}

fn padded_extent(block: usize, size: usize, limit: usize) -> Parsed<Range<usize>> {
    if size == 0 {
        return Err("empty extent");
    }
    let start = block.checked_mul(BLOCK).ok_or("extent bounds")?;
    let end = size
        .div_ceil(BLOCK)
        .checked_mul(BLOCK)
        .and_then(|size| start.checked_add(size))
        .filter(|end| *end <= limit)
        .ok_or("extent bounds")?;
    Ok(start..end)
}

fn claim(claimed: &mut BTreeSet<usize>, range: Range<usize>) -> Parsed<()> {
    for block in range.step_by(BLOCK) {
        if !claimed.insert(block) {
            return Err("extent overlap");
        }
    }
    Ok(())
}

fn joliet_text(bytes: &[u8]) -> Parsed<String> {
    if !bytes.len().is_multiple_of(2) {
        return Err("Joliet encoding");
    }
    String::from_utf16(
        &bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>(),
    )
    .map_err(|_| "Joliet encoding")
}

fn directory(
    iso: &[u8],
    descriptor: &[u8],
    joliet: bool,
    claimed: &mut BTreeSet<usize>,
) -> Parsed<BTreeMap<String, (usize, usize)>> {
    let label = if joliet {
        joliet_text(&descriptor[40..72])?
    } else {
        std::str::from_utf8(&descriptor[40..72])
            .map_err(|_| "label encoding")?
            .to_owned()
    };
    if label.trim_end_matches(' ') != "CIDATA" {
        return Err("volume label");
    }
    if number32(&descriptor[80..88])? != iso.len() / BLOCK
        || number16(&descriptor[120..124])? != 1
        || number16(&descriptor[124..128])? != 1
        || usize::from(number16(&descriptor[128..132])?) != BLOCK
        || number32(&descriptor[132..140])? != 10
        || descriptor[144..148] != [0; 4]
        || descriptor[152..156] != [0; 4]
        || descriptor[881] != 1
        || (joliet && descriptor[88..91] != *b"%/E")
    {
        return Err("volume geometry");
    }
    let (root, _) = record(&descriptor[156..190])?;
    if root.name != b"\0" || !root.directory || root.size != BLOCK {
        return Err("root record");
    }
    let root_range = padded_extent(root.block, root.size, iso.len())?;
    claim(claimed, root_range.clone())?;

    let little = u32::from_le_bytes(descriptor[140..144].try_into().unwrap()) as usize;
    let big = u32::from_be_bytes(descriptor[148..152].try_into().unwrap()) as usize;
    for (is_big, block) in [(false, little), (true, big)] {
        let range = padded_extent(block, 10, iso.len())?;
        claim(claimed, range.clone())?;
        let table = &iso[range];
        let root_address = table[2..6].try_into().unwrap();
        let root_address = if is_big {
            u32::from_be_bytes(root_address)
        } else {
            u32::from_le_bytes(root_address)
        };
        let parent = if is_big { [0, 1] } else { [1, 0] };
        if table[..2] != [1, 0]
            || root_address as usize != root.block
            || table[6..8] != parent
            || table[8..].iter().any(|&byte| byte != 0)
        {
            return Err("path table");
        }
    }

    let mut data = &iso[root_range];
    for identifier in [0, 1] {
        let (entry, length) = record(data)?;
        if entry.name != [identifier]
            || entry.block != root.block
            || entry.size != root.size
            || !entry.directory
        {
            return Err("root links");
        }
        data = &data[length..];
    }
    let mut entries = BTreeMap::new();
    while let Some(&first) = data.first() {
        if first == 0 {
            if data.iter().any(|&byte| byte != 0) {
                return Err("root padding");
            }
            break;
        }
        let (entry, length) = record(data)?;
        if entry.directory {
            return Err("extra directory");
        }
        let name = if joliet {
            joliet_text(entry.name)?
        } else {
            std::str::from_utf8(entry.name)
                .map_err(|_| "name encoding")?
                .to_owned()
        };
        let name = name.strip_suffix(";1").ok_or("file version")?;
        let name = name.strip_suffix('.').unwrap_or(name);
        if entries
            .insert(name.to_owned(), (entry.block, entry.size))
            .is_some()
        {
            return Err("duplicate file");
        }
        data = &data[length..];
    }
    Ok(entries)
}

fn read_iso(iso: &[u8]) -> Parsed<BTreeMap<String, &[u8]>> {
    if iso.len() > 64 * 1024 * 1024 || !iso.len().is_multiple_of(BLOCK) {
        return Err("image length");
    }
    let mut primary = None;
    let mut supplementary = None;
    let mut descriptor_end = None;
    for (index, block) in iso.chunks_exact(BLOCK).enumerate().skip(16) {
        if block[1..7] != *b"CD001\x01" {
            return Err("descriptor signature");
        }
        match block[0] {
            1 if primary.is_none() && supplementary.is_none() => primary = Some(block),
            2 if primary.is_some() && supplementary.is_none() => supplementary = Some(block),
            255 => {
                descriptor_end = Some((index + 1) * BLOCK);
                break;
            }
            _ => return Err("unexpected descriptor"),
        }
    }
    let mut claimed = BTreeSet::new();
    claim(&mut claimed, 0..descriptor_end.ok_or("descriptor end")?)?;
    let primary = directory(iso, primary.ok_or("primary")?, false, &mut claimed)?;
    let joliet = directory(iso, supplementary.ok_or("Joliet")?, true, &mut claimed)?;
    if primary.len() != 5 || joliet.len() != 5 {
        return Err("file count");
    }
    let mut file_bytes = BTreeMap::new();
    for (name, alias, limit) in [
        ("arx-guest", "ARXGUEST", 32 * 1024 * 1024),
        ("arx-guest.service", "ARXGUEST.SVC", 64 * 1024 * 1024),
        ("arx-finalize", "FINALIZE", 64 * 1024 * 1024),
        ("meta-data", "METADATA", 1024),
        ("user-data", "USERDATA", 16 * 1024),
    ] {
        let &(block, size) = joliet.get(name).ok_or("file name")?;
        if primary.get(alias) != Some(&(block, size)) || size > limit {
            return Err("alias or file length");
        }
        let range = padded_extent(block, size, iso.len())?;
        let start = range.start;
        claim(&mut claimed, range)?;
        file_bytes.insert(name.to_owned(), &iso[start..start + size]);
    }
    if claimed.len() != iso.len() / BLOCK {
        return Err("unclaimed extent");
    }
    for name in [
        "meta-data",
        "user-data",
        "arx-guest.service",
        "arx-finalize",
    ] {
        std::str::from_utf8(file_bytes[name]).map_err(|_| "file UTF-8")?;
    }
    Ok(file_bytes)
}

// Parse the small emitted YAML subset. Track nested keys by indentation,
// and fence off the one literal command from the mapping.
fn cloud_config(text: &str) -> (BTreeMap<String, &str>, Vec<&str>) {
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("#cloud-config"));
    let mut path = Vec::new();
    let mut scalars = BTreeMap::new();
    let mut commands = Vec::new();
    let mut keys = BTreeSet::new();
    while let Some(line) = lines.next() {
        let indent = line.len() - line.trim_start_matches(' ').len();
        assert_eq!(indent % 2, 0);
        path.truncate(indent / 2);
        assert_eq!(indent / 2, path.len());
        let content = line.trim_start();
        if content == "- |" {
            assert_eq!(path, ["autoinstall", "late-commands"]);
            for command in lines {
                commands.push(command.strip_prefix("      ").unwrap());
            }
            break;
        }
        if let Some(item) = content.strip_prefix("- ") {
            assert_eq!(path, ["autoinstall", "apt", "mirror-selection", "primary"]);
            assert!(keys.insert(format!("{}.[0]", path.join("."))));
            path.push("[0]");
            let (key, value) = item.split_once(": ").unwrap();
            assert!(
                scalars
                    .insert(format!("{}.{key}", path.join(".")), value)
                    .is_none()
            );
            continue;
        }
        let (key, value) = content.split_once(':').unwrap();
        path.push(key);
        let qualified = path.join(".");
        assert!(keys.insert(qualified.clone()));
        if !value.is_empty() {
            assert!(
                scalars
                    .insert(qualified, value.strip_prefix(' ').unwrap())
                    .is_none()
            );
            path.pop();
        }
    }
    (scalars, commands)
}

#[test]
fn golden_seed_is_deterministic() {
    let iso = fixture();
    assert_eq!(iso, fixture());
    assert_eq!(
        hex::encode(Sha256::digest(&iso)),
        "8ed71fef5e89400d9c76df0a479d23a4fcd4373bed586d6b944c38501f3f9175",
    );
    assert!(read_iso(&iso).is_ok());
    assert_ne!(iso, fixture_recipe().render_seed([0; 16], GUEST).unwrap());
}

fn check_hashes(files: &BTreeMap<String, &[u8]>, recipe: &UbuntuRecipe) -> Parsed<()> {
    for (name, expected) in [
        ("arx-guest", recipe.guest.unwrap()),
        ("arx-guest.service", recipe.service.unwrap()),
        ("arx-finalize", recipe.finalize.unwrap()),
    ] {
        let observed = pin(files[name]);
        if observed.length != expected.length || observed.sha256 != expected.sha256 {
            return Err("asset pin");
        }
    }
    Ok(())
}

#[test]
fn readback_contains_only_the_fixed_offline_profile() {
    let iso = fixture();
    let recipe = fixture_recipe();
    let files = read_iso(&iso).unwrap();
    check_hashes(&files, &recipe).unwrap();
    let text = |name| std::str::from_utf8(files[name]).unwrap();
    let meta: BTreeMap<_, _> = text("meta-data")
        .lines()
        .map(|line| line.split_once(": ").unwrap())
        .collect();
    assert_eq!(text("meta-data").lines().count(), 2);
    assert_eq!(
        meta,
        BTreeMap::from([
            (
                "instance-id",
                "arx-install-5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a"
            ),
            ("local-hostname", "arxcovo"),
        ])
    );
    let (fields, commands) = cloud_config(text("user-data"));
    let expected = [
        ("version", "1"),
        ("interactive-sections", "[]"),
        ("source.id", "ubuntu-server"),
        ("source.search_drivers", "false"),
        ("refresh-installer.update", "false"),
        ("drivers.install", "false"),
        ("ssh.install-server", "false"),
        ("ssh.allow-pw", "false"),
        ("ssh.authorized-keys", "[]"),
        ("identity.hostname", "arxcovo"),
        ("identity.username", "arx"),
        ("identity.password", "'!'"),
        ("storage.layout.name", "direct"),
        ("storage.layout.ptable", "gpt"),
        ("storage.layout.match.path", "/dev/vda"),
        ("network.version", "2"),
        ("network.renderer", "networkd"),
        ("network.ethernets.disabled.match.name", "'*'"),
        ("network.ethernets.disabled.dhcp4", "false"),
        ("network.ethernets.disabled.dhcp6", "false"),
        ("network.ethernets.disabled.accept-ra", "false"),
        ("network.ethernets.disabled.link-local", "[]"),
        ("network.ethernets.disabled.activation-mode", "'off'"),
        ("apt.geoip", "false"),
        ("apt.fallback", "offline-install"),
        ("apt.mirror-selection.primary.[0].uri", "file:///cdrom"),
        ("apt.mirror-selection.primary.[0].arches", "[amd64]"),
        ("updates", "security"),
        ("shutdown", "poweroff"),
    ];
    assert_eq!(
        fields,
        expected
            .into_iter()
            .map(|(key, value)| (format!("autoinstall.{key}"), value))
            .collect()
    );
    assert_eq!(commands[0], "set -eu");
    let command_index = |needle: &str| {
        commands
            .iter()
            .position(|line| line.contains(needle))
            .unwrap()
    };
    let mount = command_index("mount -t iso9660 -o ro,nosuid,nodev,noexec -L CIDATA");
    let verify = command_index("sha256sum --check --strict");
    let install = command_index(r#"install -m 0755 "$seed/arx-guest" "#);
    let enable = command_index("systemctl --root=/target enable arx-guest.service");
    let finalize =
        command_index("curtin in-target --target=/target -- /bin/sh /usr/local/sbin/arx-finalize");
    assert!(mount < verify && verify < install && install < enable && enable < finalize);
    for asset in ["arx-guest", "arx-guest.service", "arx-finalize"] {
        let hash = hex::encode(Sha256::digest(files[asset]));
        assert!(commands[verify - 1].contains(&format!("'{hash}' {asset}")));
    }
    let service = text("arx-guest.service");
    for line in [
        "User=root",
        "ExecStart=/usr/local/bin/arx-guest serve-virtio",
        "Delegate=pids",
        "DelegateSubgroup=arx-supervisor",
    ] {
        assert!(service.lines().any(|entry| entry == line));
    }
    assert!(service.contains("udevadm wait --timeout=10 /dev/virtio-ports/org.arxcovo.agent.0"));
    let finalize = text("arx-finalize");
    for action in [
        "exit !(root && bootstrap)",
        "install -m 0644 /dev/null /etc/cloud/cloud-init.disabled",
        "rm -rf /var/lib/cloud /var/log/installer /var/lib/subiquity /var/lib/arx-guest",
        "rm -rf /root/.ssh /home/arx/.ssh",
        "rm -f /etc/ssh/ssh_host_* /var/lib/systemd/random-seed",
        "rm -f /etc/machine-id /var/lib/dbus/machine-id",
        "install -m 0444 /dev/null /etc/machine-id",
        "ln -s /etc/machine-id /var/lib/dbus/machine-id",
        "rm -f /usr/local/sbin/arx-finalize",
    ] {
        assert!(finalize.contains(action));
    }
    assert!(
        finalize
            .ends_with("printf '%s\\n' ubuntu-noble-amd64-v1 > /etc/arxcovo/template-finalized\n")
    );
}

fn first_file(iso: &[u8], descriptor: usize) -> usize {
    let root = descriptor * BLOCK + 158;
    number32(&iso[root..root + 8]).unwrap() * BLOCK + 68
}

#[test]
fn independent_reader_rejects_malformed_images() {
    let clean = fixture();
    let primary = first_file(&clean, 16);
    let joliet = first_file(&clean, 17);

    let mut bad = clean.clone();
    bad[16 * BLOCK + 40] = b'X';
    assert_eq!(read_iso(&bad), Err("volume label"));

    bad = clean.clone();
    bad[primary + 6] ^= 1;
    assert_eq!(read_iso(&bad), Err("32-bit endian split"));

    // First Joliet entry is arx-finalize. Locate its primary alias.
    let service = primary + usize::from(clean[primary]);
    let finalize = service + usize::from(clean[service]);
    for (block, expected) in [(u32::MAX, "extent bounds"), (23, "extent overlap")] {
        bad = clean.clone();
        for offset in [finalize + 2, joliet + 2] {
            bad[offset..offset + 4].copy_from_slice(&block.to_le_bytes());
            bad[offset + 4..offset + 8].copy_from_slice(&block.to_be_bytes());
        }
        assert_eq!(read_iso(&bad), Err(expected));
    }

    let mut end = joliet;
    while clean[end] != 0 {
        end += usize::from(clean[end]);
    }
    let first = &clean[joliet..joliet + usize::from(clean[joliet])];
    bad = clean.clone();
    bad[end..end + first.len()].copy_from_slice(first);
    assert_eq!(read_iso(&bad), Err("duplicate file"));
    bad[end + 34] = b'x';
    assert_eq!(read_iso(&bad), Err("file count"));

    let files = read_iso(&clean).unwrap();
    let offset = |name: &str| files[name].as_ptr() as usize - clean.as_ptr() as usize;
    bad = clean.clone();
    bad[offset("user-data")] = 0xff;
    assert_eq!(read_iso(&bad), Err("file UTF-8"));
    bad = clean.clone();
    bad[offset("arx-guest")] ^= 1;
    assert_eq!(
        check_hashes(&read_iso(&bad).unwrap(), &fixture_recipe()),
        Err("asset pin"),
    );
}

fn write_fixture(recipe: &UbuntuRecipe, kernel: &mut dyn Read, out: &mut Vec<u8>) -> RecipeError {
    let [mut iso, mut initrd, mut code, mut vars, mut receipt] = [HOST_ASSET; 5];
    recipe
        .write_seed(
            UbuntuAssets {
                iso: Some(&mut iso),
                kernel: Some(kernel),
                initrd: Some(&mut initrd),
                firmware_code: Some(&mut code),
                firmware_vars: Some(&mut vars),
                qemu_receipt: Some(&mut receipt),
            },
            ID,
            GUEST,
            out,
        )
        .unwrap_err()
}

#[test]
fn unverified_pin_or_missing_reader_never_writes() {
    assert_eq!(UbuntuRecipe::ID, "ubuntu-noble-amd64-v1");
    assert_eq!(
        UbuntuRecipe::ISO_NAME,
        "ubuntu-24.04.3-live-server-amd64.iso"
    );
    assert_eq!(
        hex::encode(UbuntuRecipe::ISO_SHA256),
        "c3514bf0056180d09376462a7a1b4f213c1d6e8ea67fae5c25099c6fd3d8274b",
    );
    let mut out = Vec::new();
    let mut recipe = fixture_recipe();
    recipe.initrd = None;
    let mut must_not_read = NeverRead;
    let error = write_fixture(&recipe, &mut must_not_read, &mut out);
    assert_eq!(error.to_string(), "recipe_unqualified");
    assert!(out.is_empty());
    let error = fixture_recipe()
        .write_seed(UbuntuAssets::default(), ID, GUEST, &mut out)
        .unwrap_err();
    assert!(matches!(error, RecipeError::Unqualified));
    assert!(out.is_empty());
    recipe = fixture_recipe();
    recipe.iso = Some(pin(HOST_ASSET));
    let error = write_fixture(&recipe, &mut must_not_read, &mut out);
    assert!(matches!(error, RecipeError::Unqualified));
    assert!(out.is_empty());
}

struct NeverRead;

impl Read for NeverRead {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("unqualified recipe consumed an asset")
    }
}

#[test]
fn asset_digest_length_and_io_failure_refuse_before_output() {
    let mut out = Vec::new();
    let mut other = HOST_ASSET.to_vec();
    other[0] ^= 1;
    for mut input in [other.as_slice(), &HOST_ASSET[..HOST_ASSET.len() - 1]] {
        assert!(matches!(
            write_fixture(&fixture_recipe(), &mut input, &mut out),
            RecipeError::Unqualified,
        ));
        assert!(out.is_empty());
    }
    let mut unending = UnendingReader { count: 0 };
    assert!(matches!(
        write_fixture(&fixture_recipe(), &mut unending, &mut out),
        RecipeError::Unqualified,
    ));
    assert_eq!(unending.count, HOST_ASSET.len() + 1);
    assert!(out.is_empty());
    let mut broken = BrokenReader;
    assert!(matches!(
        write_fixture(&fixture_recipe(), &mut broken, &mut out),
        RecipeError::Unqualified,
    ));
    assert!(out.is_empty());
}

#[test]
fn final_iso_digest_refusal_does_not_write() {
    let [mut kernel, mut initrd, mut code, mut vars, mut receipt] = [HOST_ASSET; 5];
    let mut iso = io::Cursor::new(HOST_ASSET);
    let mut out = Vec::new();
    let error = fixture_recipe()
        .write_seed(
            UbuntuAssets {
                iso: Some(&mut iso),
                kernel: Some(&mut kernel),
                initrd: Some(&mut initrd),
                firmware_code: Some(&mut code),
                firmware_vars: Some(&mut vars),
                qemu_receipt: Some(&mut receipt),
            },
            ID,
            GUEST,
            &mut out,
        )
        .unwrap_err();
    assert!(matches!(error, RecipeError::Unqualified));
    assert_eq!(iso.position(), HOST_ASSET.len() as u64);
    assert!(out.is_empty());
}

struct BrokenReader;

struct UnendingReader {
    count: usize,
}

impl Read for UnendingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        buffer.fill(b'x');
        self.count += buffer.len();
        Ok(buffer.len())
    }
}

impl Read for BrokenReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("sentinel private path D:\\owner\\secrets"))
    }
}

#[test]
fn seed_refuses_oversize_and_bad_embedded_pins() {
    let mut recipe = fixture_recipe();
    let oversized = vec![0; 32 * 1024 * 1024 + 1];
    assert!(matches!(
        recipe.render_seed(ID, &oversized),
        Err(RecipeError::MediaLimit),
    ));
    for change in [
        |recipe: &mut UbuntuRecipe| recipe.guest = None,
        |recipe: &mut UbuntuRecipe| recipe.service = Some(pin(b"wrong unit")),
        |recipe: &mut UbuntuRecipe| recipe.finalize = Some(pin(b"wrong finalize")),
        |recipe: &mut UbuntuRecipe| recipe.nocloud_schema = Some(pin(b"wrong schema")),
    ] {
        recipe = fixture_recipe();
        change(&mut recipe);
        assert!(matches!(
            recipe.render_seed(ID, GUEST),
            Err(RecipeError::Unqualified),
        ));
    }
}

#[test]
fn host_data_and_provisioning_secrets_cannot_enter_seed() {
    let mut recipe = fixture_recipe();
    let sentinel = b"https://remote.invalid/my-provisioning-secret/D:\\private\\owner";
    recipe.qemu_receipt = Some(pin(sentinel));
    let iso = recipe.render_seed(ID, GUEST).unwrap();
    assert_eq!(iso, fixture());
    let text = String::from_utf8_lossy(&iso);
    for sentinel in [
        "remote.invalid",
        "my-provisioning-secret",
        "D:\\private\\owner",
        "http://",
        "https://",
        "ssh-rsa ",
        "ssh-ed25519 ",
        "BEGIN OPENSSH PRIVATE KEY",
        "ds=nocloud;",
        "seedfrom:",
        "addresses:",
        "nameservers:",
        "gateway4:",
        "proxy:",
        "$6$",
        "$y$",
    ] {
        assert!(!text.contains(sentinel), "{sentinel}");
    }
    assert_eq!(text.matches("password:").count(), 1);
    assert!(text.contains("password: '!'\n"));
}
