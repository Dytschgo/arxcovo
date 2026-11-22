use super::{
    paths::{Artifact, ArtifactRole, InvalidPath, LocalPath},
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

/// What a sealed plan does. Only the typed builders in this module choose it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanKind {
    /// The fixed QEMU recipe.
    Boot,
    /// Read-only qemu-img inspection of a trusted artifact.
    Inspect,
    /// Mutating qemu-img operation, bound to a borrowed offline lease.
    Mutate,
}

enum Binding<'a> {
    Boot,
    Inspect,
    Mutate(&'a OfflineLease),
}

/// A sealed program image and argv. The fields are private and the only
/// constructors are this module's operation-specific builders, so no caller
/// can construct, extend or edit the argv the native launcher consumes.
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
            Binding::Mutate(_) => PlanKind::Mutate,
        }
    }

    /// Present exactly for mutating plans; the launcher keeps it borrowed.
    pub fn offline_lease(&self) -> Option<&'a OfflineLease> {
        match self.binding {
            Binding::Mutate(lease) => Some(lease),
            Binding::Boot | Binding::Inspect => None,
        }
    }
}

/// Fixed read-only `qemu-img info` of an already role-checked image.
pub(super) fn qemu_img_info<'a>(image: &'a LocalPath, disk: &LocalPath) -> CommandPlan<'a> {
    CommandPlan {
        image,
        args: ["info", "--output=json", "-f", "qcow2", disk.as_str()]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        binding: Binding::Inspect,
    }
}

/// Fixed `qemu-img create` on a staging role. Requires the offline lease.
pub(super) fn qemu_img_create<'a>(
    lease: &'a OfflineLease,
    image: &'a LocalPath,
    staging: &LocalPath,
    backing: Option<&LocalPath>,
    size: u64,
) -> CommandPlan<'a> {
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
            backing.as_str().into(),
        ]);
    }
    args.extend([staging.as_str().into(), size.to_string()]);
    CommandPlan {
        image,
        args,
        binding: Binding::Mutate(lease),
    }
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
