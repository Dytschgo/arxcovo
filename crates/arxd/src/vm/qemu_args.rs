use super::{
    paths::{Artifact, ArtifactRole, InvalidPath, LocalPath},
    qemu_img::{CLUSTER, InvalidImage, inspect_header},
    runtime::OfflineLease,
};
use serde_json::{Value, json};

pub const QEMU_VERSION: &str = "11.1.3";
pub const MACHINE: &str = "pc-q35-11.1";
pub const GUEST_PORT: &str = "org.arxcovo.agent.0";
pub const NETWORK: &str = "user,id=arxnet0,restrict=on,ipv6=off";

pub struct SerialName(String);

impl SerialName {
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce)?;
        Ok(Self(format!("arxcovo-guest-{}", hex::encode(nonce))))
    }

    pub fn basename(&self) -> &str {
        &self.0
    }
}

pub enum BootPurpose<'a> {
    Installed,
    Installer(&'a Artifact),
}

pub struct BootRecipe<'a> {
    pub image: &'a Artifact,
    pub data: &'a Artifact,
    pub firmware: &'a Artifact,
    pub vars: &'a Artifact,
    pub disk: &'a Artifact,
    pub backing: Option<&'a Artifact>,
    pub serial: &'a SerialName,
    pub boot: BootPurpose<'a>,
    pub ram_mib: u32,
    pub vcpus: u32,
}

/// What a sealed plan does. Only the validated builders in this module choose it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanKind {
    /// The fixed QEMU recipe.
    Boot,
    /// Read-only qemu-img inspection of the immutable golden image.
    Inspect,
    /// qemu-img inspection of a writable image, bound to an offline lease.
    InspectWritable,
    /// Mutating qemu-img operation, bound to an offline lease.
    Mutate,
}

enum Binding<'a> {
    Boot,
    Inspect,
    Offline {
        lease: &'a OfflineLease,
        mutate: bool,
    },
}

/// A sealed program image and argv. The fields are private. The only
/// constructors are this module's validated builders: `BootRecipe::plan`,
/// `golden_info`, `offline_info` and `create`. Each one checks roles, header
/// and size before it returns a plan, so no crate-internal caller can mint an
/// unvalidated plan or edit the argv the native launcher consumes.
pub struct CommandPlan<'a> {
    image: &'a LocalPath,
    args: Vec<String>,
    binding: Binding<'a>,
}

impl<'a> CommandPlan<'a> {
    pub fn image(&self) -> &'a LocalPath {
        self.image
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn kind(&self) -> PlanKind {
        match self.binding {
            Binding::Boot => PlanKind::Boot,
            Binding::Inspect => PlanKind::Inspect,
            Binding::Offline { mutate: false, .. } => PlanKind::InspectWritable,
            Binding::Offline { mutate: true, .. } => PlanKind::Mutate,
        }
    }

    /// Present exactly for writable-image plans; the launcher keeps it
    /// borrowed for the whole helper launch.
    pub fn offline_lease(&self) -> Option<&'a OfflineLease> {
        match self.binding {
            Binding::Offline { lease, .. } => Some(lease),
            Binding::Boot | Binding::Inspect => None,
        }
    }
}

fn role(artifact: &Artifact, role: ArtifactRole) -> Result<&LocalPath, InvalidImage> {
    artifact.path_for(role).map_err(|InvalidPath| InvalidImage)
}

fn info_args(disk: &LocalPath) -> Vec<String> {
    ["info", "--output=json", "-f", "qcow2", disk.as_str()]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Validated read-only `qemu-img info` of the immutable golden image.
pub(super) fn golden_info<'a>(
    image: &'a Artifact,
    golden: &Artifact,
    header: &[u8],
) -> Result<CommandPlan<'a>, InvalidImage> {
    let (image, args) = golden_info_args(image, golden, header)?;
    Ok(CommandPlan {
        image,
        args,
        binding: Binding::Inspect,
    })
}

fn golden_info_args<'a>(
    image: &'a Artifact,
    golden: &Artifact,
    header: &[u8],
) -> Result<(&'a LocalPath, Vec<String>), InvalidImage> {
    inspect_header(header, None)?;
    Ok((
        role(image, ArtifactRole::QemuImg)?,
        info_args(role(golden, ArtifactRole::Golden)?),
    ))
}

/// Validated `qemu-img info` of a writable guest disk. Holds the lease.
pub(super) fn offline_info<'a>(
    lease: &'a OfflineLease,
    image: &'a Artifact,
    disk: &Artifact,
    header: &[u8],
    backing: Option<&Artifact>,
) -> Result<CommandPlan<'a>, InvalidImage> {
    let (image, args) = offline_info_args(image, disk, header, backing)?;
    Ok(CommandPlan {
        image,
        args,
        binding: Binding::Offline {
            lease,
            mutate: false,
        },
    })
}

fn offline_info_args<'a>(
    image: &'a Artifact,
    disk: &Artifact,
    header: &[u8],
    backing: Option<&Artifact>,
) -> Result<(&'a LocalPath, Vec<String>), InvalidImage> {
    let backing = backing
        .map(|artifact| role(artifact, ArtifactRole::Golden))
        .transpose()?;
    inspect_header(header, backing)?;
    Ok((
        role(image, ArtifactRole::QemuImg)?,
        info_args(role(disk, ArtifactRole::GuestDisk)?),
    ))
}

/// Validated `qemu-img create` of a staging image. Holds the lease.
pub(super) fn create<'a>(
    lease: &'a OfflineLease,
    image: &'a Artifact,
    staging: &Artifact,
    backing: Option<&Artifact>,
    size: u64,
    ceiling: u64,
) -> Result<CommandPlan<'a>, InvalidImage> {
    let (image, args) = create_args(image, staging, backing, size, ceiling)?;
    Ok(CommandPlan {
        image,
        args,
        binding: Binding::Offline {
            lease,
            mutate: true,
        },
    })
}

fn create_args<'a>(
    image: &'a Artifact,
    staging: &Artifact,
    backing: Option<&Artifact>,
    size: u64,
    ceiling: u64,
) -> Result<(&'a LocalPath, Vec<String>), InvalidImage> {
    let cluster = CLUSTER as u64;
    let peak = size
        .div_ceil(cluster)
        .checked_mul(cluster + 32)
        .and_then(|n| n.checked_add(4 * cluster));
    if size == 0 || peak.is_none_or(|peak| peak > ceiling) {
        return Err(InvalidImage);
    }
    let mut args = [
        "create",
        "-f",
        "qcow2",
        "-o",
        "compat=1.1,cluster_size=65536,lazy_refcounts=off",
    ]
    .map(str::to_owned)
    .to_vec();
    if let Some(backing) = backing {
        args.extend([
            "-F".into(),
            "qcow2".into(),
            "-b".into(),
            role(backing, ArtifactRole::Golden)?.as_str().into(),
        ]);
    }
    args.extend([
        role(staging, ArtifactRole::Staging)?.as_str().into(),
        size.to_string(),
    ]);
    Ok((role(image, ArtifactRole::QemuImg)?, args))
}

impl BootRecipe<'_> {
    pub fn plan(&self) -> Result<CommandPlan<'_>, InvalidPath> {
        use ArtifactRole::*;
        if !(1..=4).contains(&self.vcpus) || !(256..=8192).contains(&self.ram_mib) {
            return Err(InvalidPath);
        }
        let image = self.image.path_for(Qemu)?;
        let mut args: Vec<String> = [
            "-no-user-config",
            "-nodefaults",
            "-display",
            "none",
            "-serial",
            "none",
            "-monitor",
            "none",
            "-accel",
            "whpx",
            "-S",
            "-chardev",
            "stdio,id=arxqmp,signal=off",
            "-mon",
            "chardev=arxqmp,mode=control",
            "-netdev",
            NETWORK,
            "-device",
            "virtio-net-pci,netdev=arxnet0",
            "-device",
            "virtio-serial-pci,id=arxserial",
            "-device",
            "virtio-blk-pci,drive=guestdisk,bootindex=2",
            "-L",
            self.data.path_for(Data)?.as_str(),
            "-machine",
            "pc-q35-11.1,pflash0=code,pflash1=vars",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        args.extend([
            "-smp".into(),
            self.vcpus.to_string(),
            "-m".into(),
            self.ram_mib.to_string(),
            "-chardev".into(),
            format!("pipe,id=arxguest,path={}", self.serial.basename()),
            "-device".into(),
            format!("virtserialport,bus=arxserial.0,chardev=arxguest,name={GUEST_PORT}"),
        ]);
        block(
            &mut args,
            "raw",
            "code",
            self.firmware.path_for(FirmwareCode)?,
            true,
            None,
        );
        block(
            &mut args,
            "raw",
            "vars",
            self.vars.path_for(Vars)?,
            false,
            None,
        );
        if let Some(backing) = self.backing {
            block(
                &mut args,
                "qcow2",
                "golden",
                backing.path_for(Golden)?,
                true,
                Some(Value::Null),
            );
        }
        block(
            &mut args,
            "qcow2",
            "guestdisk",
            self.disk.path_for(GuestDisk)?,
            false,
            Some(self.backing.map_or(Value::Null, |_| json!("golden"))),
        );
        if let BootPurpose::Installer(media) = self.boot {
            block(
                &mut args,
                "raw",
                "installer",
                media.path_for(InstallMedia)?,
                true,
                None,
            );
            args.extend([
                "-device".into(),
                "ide-cd,drive=installer,bus=ide.0,bootindex=1".into(),
            ]);
        }
        Ok(CommandPlan {
            image,
            args,
            binding: Binding::Boot,
        })
    }
}

fn block(
    args: &mut Vec<String>,
    driver: &str,
    node: &str,
    path: &LocalPath,
    read_only: bool,
    backing: Option<Value>,
) {
    let mut spec = json!({
        "driver": driver, "node-name": node, "read-only": read_only,
        "file": { "driver": "file", "filename": path.as_str(), "read-only": read_only },
    });
    if let Some(backing) = backing {
        spec["backing"] = backing;
    }
    args.extend(["-blockdev".into(), spec.to_string()]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(role: ArtifactRole, path: &str) -> Artifact {
        Artifact::new(role, LocalPath::parse(path).unwrap()).unwrap()
    }

    fn header(backing: Option<&str>) -> Vec<u8> {
        let mut header = vec![0; CLUSTER];
        header[..4].copy_from_slice(b"QFI\xfb");
        for (at, val) in [(4, 3u32), (20, 16), (96, 4), (100, 104)] {
            header[at..at + 4].copy_from_slice(&val.to_be_bytes());
        }
        header[24..32].copy_from_slice(&1_048_576u64.to_be_bytes());
        if let Some(backing) = backing {
            let at = 512;
            header[8..16].copy_from_slice(&(at as u64).to_be_bytes());
            header[16..20].copy_from_slice(&(backing.len() as u32).to_be_bytes());
            header[at..at + backing.len()].copy_from_slice(backing.as_bytes());
            // Backing-format extension "qcow2", then the end marker.
            header[104..108].copy_from_slice(&0xe279_2acau32.to_be_bytes());
            header[108..112].copy_from_slice(&5u32.to_be_bytes());
            header[112..117].copy_from_slice(b"qcow2");
        }
        header
    }

    /// The lease is uninhabited in this slice, so the leased constructors are
    /// exercised through the same private validation they call first.
    #[test]
    fn every_constructor_validates_role_header_and_size_before_a_plan() {
        let tool = artifact(ArtifactRole::QemuImg, "E:/bundle/qemu-img.exe");
        let golden = artifact(ArtifactRole::Golden, "D:/vm/golden.qcow2");
        let disk = artifact(ArtifactRole::GuestDisk, "D:/vm/disk.qcow2");
        let staging = artifact(ArtifactRole::Staging, "D:/vm/staging.qcow2");
        let plain = header(None);
        let backed = header(Some(r"D:\vm\golden.qcow2"));
        let mut corrupt = plain.clone();
        corrupt[79] = 4; // external data file feature

        // golden_info: role and header checks are inside the constructor.
        assert!(golden_info(&tool, &golden, &plain).is_ok());
        assert!(golden_info(&golden, &golden, &plain).is_err(), "tool role");
        assert!(golden_info(&tool, &disk, &plain).is_err(), "golden role");
        assert!(golden_info(&tool, &golden, &corrupt).is_err(), "header");
        assert!(
            golden_info(&tool, &golden, &backed).is_err(),
            "backed golden"
        );

        // offline_info validation.
        assert!(offline_info_args(&tool, &disk, &backed, Some(&golden)).is_ok());
        assert!(offline_info_args(&tool, &disk, &plain, None).is_ok());
        assert!(
            offline_info_args(&tool, &golden, &plain, None).is_err(),
            "disk role"
        );
        assert!(
            offline_info_args(&disk, &disk, &plain, None).is_err(),
            "tool role"
        );
        assert!(
            offline_info_args(&tool, &disk, &corrupt, None).is_err(),
            "header"
        );
        assert!(
            offline_info_args(&tool, &disk, &plain, Some(&golden)).is_err(),
            "missing backing"
        );
        assert!(
            offline_info_args(&tool, &disk, &backed, Some(&disk)).is_err(),
            "backing role"
        );

        // create validation.
        let ceiling = 1 << 30;
        assert!(create_args(&tool, &staging, Some(&golden), 1 << 20, ceiling).is_ok());
        assert!(
            create_args(&tool, &staging, None, 0, ceiling).is_err(),
            "zero size"
        );
        assert!(
            create_args(&tool, &staging, None, 1 << 30, ceiling).is_err(),
            "peak above ceiling"
        );
        assert!(
            create_args(&tool, &staging, None, u64::MAX, u64::MAX).is_err(),
            "overflow"
        );
        assert!(
            create_args(&tool, &disk, None, 1 << 20, ceiling).is_err(),
            "staging role"
        );
        assert!(
            create_args(&staging, &staging, None, 1 << 20, ceiling).is_err(),
            "tool role"
        );
        assert!(
            create_args(&tool, &staging, Some(&disk), 1 << 20, ceiling).is_err(),
            "backing role"
        );
    }
}
