use super::paths::{Artifact, ArtifactRole, InvalidPath, LocalPath};
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

pub struct CommandPlan<'a> {
    pub image: &'a LocalPath,
    pub args: Vec<String>,
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
        Ok(CommandPlan { image, args })
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
