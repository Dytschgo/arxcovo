use super::{
    paths::{Artifact, ArtifactPin, ArtifactRole, FileIdentity, InvalidPath, LocalPath},
    qemu_img::{CLUSTER, InvalidImage, inspect_header},
    runtime::{InstallerPhase, OfflineLease},
    static_web::OriginPortLease,
};
use serde_json::{Value, json};

pub const QEMU_VERSION: &str = "11.1.3";
pub const MACHINE: &str = "pc-q35-11.1";
pub const GUEST_PORT: &str = "org.arxcovo.agent.0";
/// The fixed QEMU device id of the guest port; `VSERPORT_CHANGE` names it.
pub const GUEST_PORT_ID: &str = "arxguestport";
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

/// The patched backend's netdev option that names the custodied policy.
/// Stock QEMU does not know it (ADR 0064).
pub const PUBLIC_EGRESS_OPTION: &str = "arx-public-egress";
/// The fixed public-egress user network before its policy and forward.
pub const PUBLIC_EGRESS_NETWORK: &str =
    "user,id=arxnet0,restrict=off,ipv4=on,ipv6=off,net=10.0.2.0/24,host=10.0.2.2,dns=10.0.2.3";

/// Closed network policy of an installed static boot. Each variant is a
/// separate sealed recipe; neither is a change to the other.
#[derive(Clone, Copy, Debug)]
pub enum StaticNetwork<'a> {
    /// The fixed restricted user network plus one IPv4 loopback forward to
    /// guest TCP 443.
    RestrictedOrigin,
    /// ADR 0064: the patched backend's public-only egress policy, named by
    /// its read-only custodied artifact, plus the same single loopback
    /// forward. Requires the separately pinned `PublicEgressQemu` image.
    PublicEgress(&'a Artifact),
}

#[derive(Clone, Copy)]
pub enum BootPurpose<'a> {
    Installed,
    InstalledStaticWeb(&'a OriginPortLease, StaticNetwork<'a>),
    UbuntuInstall {
        iso: &'a Artifact,
        cidata: &'a Artifact,
        kernel: &'a Artifact,
        initrd: &'a Artifact,
    },
    WindowsSetup {
        iso: &'a Artifact,
        answer: &'a Artifact,
        drivers: &'a Artifact,
    },
    WindowsFinalize,
}

impl BootPurpose<'_> {
    fn phase(self) -> Option<InstallerPhase> {
        match self {
            Self::Installed | Self::InstalledStaticWeb(..) => None,
            Self::UbuntuInstall { .. } => Some(InstallerPhase::UbuntuInstall),
            Self::WindowsSetup { .. } => Some(InstallerPhase::WindowsSetup),
            Self::WindowsFinalize => Some(InstallerPhase::WindowsFinalize),
        }
    }
}

pub struct BootRecipe<'a> {
    pub image: &'a Artifact,
    pub data: &'a Artifact,
    pub firmware: &'a Artifact,
    pub vars: &'a Artifact,
    pub disk: &'a Artifact,
    /// First cluster of `disk`, read through its held custody pin.
    pub disk_header: &'a [u8],
    pub backing: Option<&'a Artifact>,
    /// First cluster of `backing`, present exactly when `backing` is.
    pub backing_header: Option<&'a [u8]>,
    pub serial: &'a SerialName,
    pub boot: BootPurpose<'a>,
    pub ram_mib: u32,
    pub vcpus: u32,
}

/// What a sealed plan does. Only the validated builders in this module choose it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanKind {
    /// The fixed QEMU recipe for an installed disk.
    Boot,
    /// A standalone static VM with the one reserved IPv4 origin forward.
    StaticWebBoot,
    /// A static VM on the patched public-only egress backend (ADR 0064).
    PublicEgressBoot,
    /// A QMP-only installer or finalization recipe.
    InstallerBoot(InstallerPhase),
    /// Read-only qemu-img inspection of the immutable golden image.
    Inspect,
    /// qemu-img inspection of a writable image, bound to an offline lease.
    InspectWritable,
    /// Mutating qemu-img operation, bound to an offline lease.
    Mutate,
}

/// Expected custody for one artifact in a sealed installer plan.
pub struct InstallerArtifact<'a> {
    pub path: &'a LocalPath,
    pub identity: FileIdentity,
    pub pin: Option<ArtifactPin>,
    pub role: ArtifactRole,
}

enum Binding<'a> {
    Boot,
    StaticWeb(&'a OriginPortLease),
    PublicEgress {
        lease: &'a OriginPortLease,
        policy: &'a Artifact,
        image: &'a Artifact,
    },
    Installer(InstallerPhase, Vec<InstallerArtifact<'a>>),
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
            Binding::StaticWeb(_) => PlanKind::StaticWebBoot,
            Binding::PublicEgress { .. } => PlanKind::PublicEgressBoot,
            Binding::Installer(phase, _) => PlanKind::InstallerBoot(phase),
            Binding::Inspect => PlanKind::Inspect,
            Binding::Offline { mutate: false, .. } => PlanKind::InspectWritable,
            Binding::Offline { mutate: true, .. } => PlanKind::Mutate,
        }
    }

    pub fn static_origin_lease(&self) -> Option<&OriginPortLease> {
        match &self.binding {
            Binding::StaticWeb(lease) | Binding::PublicEgress { lease, .. } => Some(lease),
            _ => None,
        }
    }

    /// The custodied policy artifact named in a public-egress plan's argv.
    pub fn public_egress_policy(&self) -> Option<&'a Artifact> {
        match self.binding {
            Binding::PublicEgress { policy, .. } => Some(policy),
            _ => None,
        }
    }

    /// The held patched executable a public-egress plan launches.
    pub fn public_egress_image(&self) -> Option<&'a Artifact> {
        match self.binding {
            Binding::PublicEgress { image, .. } => Some(image),
            _ => None,
        }
    }

    /// Present exactly for writable-image plans; the launcher keeps it
    /// borrowed for the whole helper launch.
    pub fn offline_lease(&self) -> Option<&'a OfflineLease> {
        match self.binding {
            Binding::Offline { lease, .. } => Some(lease),
            Binding::Boot
            | Binding::StaticWeb(_)
            | Binding::PublicEgress { .. }
            | Binding::Installer(..)
            | Binding::Inspect => None,
        }
    }

    pub fn installer_artifacts(&self) -> &[InstallerArtifact<'a>] {
        match &self.binding {
            Binding::Installer(_, artifacts) => artifacts,
            _ => &[],
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

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum SnapshotDiskRole {
    Active,
    Backup,
    Staging,
}

/// Fixed inspection request shared by the fixture inspector and the closed
/// native constructor. This alone has no execution authority.
pub struct SnapshotInfo<'a> {
    disk: &'a Artifact,
    golden: Option<&'a Artifact>,
    path: &'a LocalPath,
    args: Vec<String>,
}

impl<'a> SnapshotInfo<'a> {
    pub(super) fn checked(
        disk: &'a Artifact,
        position: SnapshotDiskRole,
        header: &[u8],
        golden: Option<(&'a Artifact, &[u8])>,
        size: u64,
    ) -> Result<Self, InvalidImage> {
        let backing = if let Some((artifact, header)) = golden {
            if inspect_header(header, None)? != size {
                return Err(InvalidImage);
            }
            Some(role(artifact, ArtifactRole::Golden)?)
        } else {
            None
        };
        let expected_role = match position {
            SnapshotDiskRole::Active if backing.is_some() => ArtifactRole::GuestDisk,
            SnapshotDiskRole::Active => ArtifactRole::StaticDisk,
            SnapshotDiskRole::Backup => ArtifactRole::SnapshotDisk,
            SnapshotDiskRole::Staging => ArtifactRole::SnapshotStagingDisk,
        };
        let path = role(disk, expected_role)?;
        if inspect_header(header, backing)? != size
            || backing.is_some_and(|backing| path.overlaps(backing))
        {
            return Err(InvalidImage);
        }
        Ok(Self {
            disk,
            golden: golden.map(|(artifact, _)| artifact),
            path,
            args: info_args(path),
        })
    }

    pub fn disk(&self) -> &'a Artifact {
        self.disk
    }

    pub fn path(&self) -> &'a LocalPath {
        self.path
    }

    pub fn golden(&self) -> Option<&'a Artifact> {
        self.golden
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }
}

pub(super) fn offline_snapshot_info<'a>(
    lease: &'a OfflineLease,
    image: &'a Artifact,
    info: &SnapshotInfo<'a>,
) -> Result<CommandPlan<'a>, InvalidImage> {
    Ok(CommandPlan {
        image: role(image, ArtifactRole::QemuImg)?,
        args: info.args.clone(),
        binding: Binding::Offline {
            lease,
            mutate: false,
        },
    })
}

/// Validated `qemu-img create` of a staging image. Holds the lease. A backed
/// create checks the golden's first cluster: qemu-img opens the declared
/// backing with traversal enabled, so it must itself be unbacked.
pub(super) fn create<'a>(
    lease: &'a OfflineLease,
    image: &'a Artifact,
    staging: &Artifact,
    backing: Option<(&Artifact, &[u8])>,
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
    backing: Option<(&Artifact, &[u8])>,
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
    if let Some((golden, header)) = backing {
        inspect_header(header, None)?;
        args.extend([
            "-F".into(),
            "qcow2".into(),
            "-b".into(),
            role(golden, ArtifactRole::Golden)?.as_str().into(),
        ]);
    }
    args.extend([
        role(staging, ArtifactRole::Staging)?.as_str().into(),
        size.to_string(),
    ]);
    Ok((role(image, ArtifactRole::QemuImg)?, args))
}

impl BootRecipe<'_> {
    /// Refuses with `InvalidPath` on any role, size or qcow2 metadata
    /// violation. The disk may reference only the approved golden, the golden
    /// must be unbacked, and neither may name an external data file.
    pub fn plan(&self) -> Result<CommandPlan<'_>, InvalidPath> {
        use ArtifactRole::*;
        if !(1..=4).contains(&self.vcpus) || !(256..=8192).contains(&self.ram_mib) {
            return Err(InvalidPath);
        }
        let phase = self.boot.phase();
        let static_origin = match self.boot {
            BootPurpose::InstalledStaticWeb(lease, network) if lease.verify() => {
                Some((lease, network))
            }
            BootPurpose::InstalledStaticWeb(..) => return Err(InvalidPath),
            _ => None,
        };
        let (disk_role, vars_role) = match (static_origin, phase) {
            (Some(_), _) => (StaticDisk, StaticVars),
            (_, Some(InstallerPhase::WindowsSetup)) => (TestInstallDisk, WindowsSetupVars),
            (_, Some(_)) => (TestInstallDisk, Vars),
            _ => (GuestDisk, Vars),
        };
        if (phase.is_some() || static_origin.is_some()) && self.backing.is_some() {
            return Err(InvalidPath);
        }
        let (network, image_role, nic) = match static_origin {
            None => (NETWORK.to_owned(), Qemu, "virtio-net-pci,netdev=arxnet0"),
            Some((lease, StaticNetwork::RestrictedOrigin)) => (
                format!(
                    "{NETWORK},hostfwd=tcp:127.0.0.1:{}-10.0.2.15:443",
                    lease.port()
                ),
                Qemu,
                "virtio-net-pci,netdev=arxnet0,mac=52:54:00:12:34:56",
            ),
            Some((lease, StaticNetwork::PublicEgress(policy))) => (
                format!(
                    "{PUBLIC_EGRESS_NETWORK},{PUBLIC_EGRESS_OPTION}={},hostfwd=tcp:127.0.0.1:{}-10.0.2.15:443",
                    // QEMU keyval doubles a literal comma. `LocalPath` admits
                    // none today; escaping keeps the contract if that changes.
                    custodied_policy(policy)?.as_str().replace(',', ",,"),
                    lease.port()
                ),
                PublicEgressQemu,
                "virtio-net-pci,netdev=arxnet0",
            ),
        };
        if image_role == PublicEgressQemu {
            held_executable(self.image)?;
        }
        let approved = match (self.backing, self.backing_header) {
            (Some(golden), Some(header)) => {
                inspect_header(header, None).map_err(|InvalidImage| InvalidPath)?;
                Some(golden.path_for(Golden)?)
            }
            (None, None) => None,
            _ => return Err(InvalidPath),
        };
        inspect_header(self.disk_header, approved).map_err(|InvalidImage| InvalidPath)?;
        let image = self.image.path_for(image_role)?;
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
            &network,
            "-device",
            nic,
            "-device",
            "virtio-serial-pci,id=arxserial",
            "-device",
            if phase == Some(InstallerPhase::WindowsSetup) {
                "virtio-blk-pci,drive=guestdisk"
            } else {
                "virtio-blk-pci,drive=guestdisk,bootindex=2"
            },
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
            if phase.is_some() {
                "null,id=arxguest".into()
            } else {
                format!("pipe,id=arxguest,path={}", self.serial.basename())
            },
            "-device".into(),
            format!(
                "virtserialport,bus=arxserial.0,chardev=arxguest,id={GUEST_PORT_ID},name={GUEST_PORT}"
            ),
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
            self.vars.path_for(vars_role)?,
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
            self.disk.path_for(disk_role)?,
            false,
            Some(self.backing.map_or(Value::Null, |_| json!("golden"))),
        );
        let binding = match phase {
            None => match static_origin {
                None => Binding::Boot,
                Some((lease, StaticNetwork::RestrictedOrigin)) => Binding::StaticWeb(lease),
                Some((lease, StaticNetwork::PublicEgress(policy))) => Binding::PublicEgress {
                    lease,
                    policy,
                    image: self.image,
                },
            },
            Some(phase) => {
                let mut artifacts = Vec::new();
                for (artifact, role) in [
                    (self.image, Qemu),
                    (self.data, Data),
                    (self.firmware, FirmwareCode),
                    (self.vars, vars_role),
                    (self.disk, disk_role),
                ] {
                    bind(&mut artifacts, artifact, role, role == WindowsSetupVars)?;
                }
                self.installer_args(&mut args, &mut artifacts)?;
                Binding::Installer(phase, artifacts)
            }
        };
        Ok(CommandPlan {
            image,
            args,
            binding,
        })
    }

    fn installer_args<'a>(
        &'a self,
        args: &mut Vec<String>,
        artifacts: &mut Vec<InstallerArtifact<'a>>,
    ) -> Result<(), InvalidPath> {
        use ArtifactRole::*;
        match self.boot {
            BootPurpose::UbuntuInstall {
                iso,
                cidata,
                kernel,
                initrd,
            } => {
                let iso = bind(artifacts, iso, UbuntuInstallIso, true)?;
                let cidata = bind(artifacts, cidata, UbuntuCidata, true)?;
                let kernel = bind(artifacts, kernel, UbuntuKernel, true)?;
                let initrd = bind(artifacts, initrd, UbuntuInitrd, true)?;
                cd(args, "osiso", iso, "ide.0");
                cd(args, "cidata", cidata, "ide.1");
                args.extend([
                    "-kernel".into(),
                    kernel.as_str().into(),
                    "-initrd".into(),
                    initrd.as_str().into(),
                    "-append".into(),
                    "autoinstall ds=nocloud ---".into(),
                ]);
            }
            BootPurpose::WindowsSetup {
                iso,
                answer,
                drivers,
            } => {
                let iso = bind(artifacts, iso, WindowsInstallIso, true)?;
                let answer = bind(artifacts, answer, WindowsAnswerSeed, true)?;
                let drivers = bind(artifacts, drivers, WindowsDriverMedium, true)?;
                // The qualified private VARS selects ide.0 through BootNext.
                // Keep firmware's persistent disk order free of CD overrides.
                cd(args, "osiso", iso, "ide.0");
                args.extend([
                    "-device".into(),
                    "ich9-ahci,id=arxmedia,bus=pcie.0,addr=0x1e".into(),
                ]);
                cd(args, "answer", answer, "arxmedia.0");
                cd(args, "drivers", drivers, "arxmedia.1");
            }
            BootPurpose::WindowsFinalize
            | BootPurpose::Installed
            | BootPurpose::InstalledStaticWeb(..) => {}
        }
        Ok(())
    }
}

/// The patched executable must name a held object: identity and a bounded pin.
fn held_executable(image: &Artifact) -> Result<(), InvalidPath> {
    let pinned = image
        .pin()
        .is_some_and(|pin| (1..=super::public_egress::MAX_EXECUTABLE_BYTES).contains(&pin.size));
    if image.identity().is_none() || !pinned {
        return Err(InvalidPath);
    }
    Ok(())
}

/// The policy must be a custodied, pinned, bounded read-only artifact.
fn custodied_policy(policy: &Artifact) -> Result<&LocalPath, InvalidPath> {
    let path = policy.path_for(ArtifactRole::PublicEgressPolicy)?;
    let pinned = policy
        .pin()
        .is_some_and(|pin| (1..=super::public_egress::MAX_POLICY_BYTES).contains(&pin.size));
    if policy.identity().is_none() || !pinned {
        return Err(InvalidPath);
    }
    Ok(path)
}

fn bind<'a>(
    artifacts: &mut Vec<InstallerArtifact<'a>>,
    artifact: &'a Artifact,
    role: ArtifactRole,
    pinned: bool,
) -> Result<&'a LocalPath, InvalidPath> {
    let path = artifact.path_for(role)?;
    let identity = artifact.identity().ok_or(InvalidPath)?;
    let pin = artifact.pin();
    if (pinned && pin.is_none())
        || pin.is_some_and(|pin| pin.size == 0)
        || artifacts.iter().any(|other| {
            other.identity == identity || (other.path.contains(path) && path.contains(other.path))
        })
    {
        return Err(InvalidPath);
    }
    artifacts.push(InstallerArtifact {
        path,
        identity,
        pin,
        role,
    });
    Ok(path)
}

fn cd(args: &mut Vec<String>, node: &str, path: &LocalPath, bus: &str) {
    block(args, "raw", node, path, true, None);
    args.extend(["-device".into(), format!("ide-cd,drive={node},bus={bus}")]);
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

    /// An 11.1.x `QCowHeader` (112 bytes, `block/qcow2.h`) with the optional
    /// backing-format extension, the end marker and the backing name.
    fn header(backing: Option<&str>) -> Vec<u8> {
        let mut header = vec![0; CLUSTER];
        header[..4].copy_from_slice(b"QFI\xfb");
        for (at, val) in [(4, 3u32), (20, 16), (96, 4), (100, 112)] {
            header[at..at + 4].copy_from_slice(&val.to_be_bytes());
        }
        header[24..32].copy_from_slice(&1_048_576u64.to_be_bytes());
        if let Some(backing) = backing {
            let at = 512;
            header[8..16].copy_from_slice(&(at as u64).to_be_bytes());
            header[16..20].copy_from_slice(&(backing.len() as u32).to_be_bytes());
            header[at..at + backing.len()].copy_from_slice(backing.as_bytes());
            // Backing-format extension "qcow2", then the end marker.
            header[112..116].copy_from_slice(&0xe279_2acau32.to_be_bytes());
            header[116..120].copy_from_slice(&5u32.to_be_bytes());
            header[120..125].copy_from_slice(b"qcow2");
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
        let unbacked = Some((&golden, plain.as_slice()));
        assert!(create_args(&tool, &staging, unbacked, 1 << 20, ceiling).is_ok());
        assert!(
            create_args(
                &tool,
                &staging,
                Some((&golden, backed.as_slice())),
                1 << 20,
                ceiling
            )
            .is_err(),
            "golden with its own backing"
        );
        assert!(
            create_args(
                &tool,
                &staging,
                Some((&golden, corrupt.as_slice())),
                1 << 20,
                ceiling
            )
            .is_err(),
            "golden with an external data file"
        );
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
            create_args(
                &tool,
                &staging,
                Some((&disk, plain.as_slice())),
                1 << 20,
                ceiling
            )
            .is_err(),
            "backing role"
        );
    }
}
