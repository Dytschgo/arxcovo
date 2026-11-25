//! Offline Windows 11 TEST recipe. Qualification inputs come from private
//! owner configuration and inspection of already-held media. Native issuance
//! and the durable lifecycle are outside this module.

use super::{
    AssetPin, RecipeError,
    lifecycle::{Boot, Failure, Pair, Readiness, TemplateRuntime, clean},
    media::{Image, MAX_GUEST},
};
use crate::vm::{
    BuildSlots, Id, PairSlots,
    guest::GUEST_READY_BUDGET,
    runtime::{
        InstallerBudget, InstallerFailureReason, InstallerPhase, InstallerRecipe, NativeRefusal,
    },
};
use arx_guest::protocol::ScriptStatus;
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    io::{Read, Write},
    time::Duration,
};
use tokio::time::Instant;

pub(super) const ID: &str = "windows11-x64-test-v1";

/// Catalog identity stays fixed as exact media observations change the digest.
/// Use a domain separate from owner-configured Ubuntu digest-derived IDs.
pub(super) fn catalog_id() -> Id {
    let digest = Sha256::new()
        .chain_update(b"arxcovo.recipe.catalog.v1\0")
        .chain_update(ID.as_bytes())
        .finalize();
    digest[..16].try_into().expect("SHA-256 prefix")
}
const MARKER: &[u8] = b"arxcovo-windows-test-seed-v1\n";
const UNATTEND: &str = "urn:schemas-microsoft-com:unattend";
const WCM: &str = "http://schemas.microsoft.com/WMIConfig/2002/State";
const TOPOLOGY: &[u8] = b"q35/WHPX;uefi;virtio-blk-pci;virtio-serial:org.arxcovo.agent.0;\
    virtio-net-pci:user,restrict=on,ipv6=off;os:builtin-ahci;seed+drivers:separate-fixed-ahci-ports";

// Each directory contains one selected w11/amd64 package, not a whole tree.
pub(super) const DRIVERS: [(&str, [&str; 3]); 3] = [
    (
        r"viostor\w11\amd64",
        ["viostor.inf", "viostor.cat", "viostor.sys"],
    ),
    (
        r"vioserial\w11\amd64",
        ["vioser.inf", "vioser.cat", "vioser.sys"],
    ),
    (
        r"NetKVM\w11\amd64",
        ["netkvm.inf", "netkvm.cat", "netkvm.sys"],
    ),
];

const MAX_ISO: u64 = 32 * 1024 * 1024 * 1024;
const MAX_DRIVERS: u64 = 2 * 1024 * 1024 * 1024;
const MAX_DRIVER_FILE: u64 = 32 * 1024 * 1024;
const MAX_RECEIPT: u64 = 256 * 1024;
const MAX_FIRMWARE: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "owner Windows image inspection is qualification work"
    )
)]
pub(in crate::vm) enum InstallImage {
    Wim,
    Esd,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::vm) struct Edition<'a> {
    pub index: u32,
    pub name: &'a str,
    pub edition_id: &'a str,
    pub build: &'a str,
    pub language: &'a str,
    pub architecture: &'a str,
    pub container: InstallImage,
}

pub(in crate::vm) struct WindowsPins<'a> {
    pub iso: AssetPin,
    pub edition: Edition<'a>,
    pub driver_medium: AssetPin,
    // INF, CAT, SYS in DRIVERS order. The owner receipt binds release,
    // versions, catalog trust, hardware IDs and the vioserial port ABI.
    pub driver_files: [[AssetPin; 3]; 3],
    pub driver_receipt: AssetPin,
    pub winpe_driver_letter: u8,
    pub guest: AssetPin,
    pub provisioner: AssetPin,
    pub firmware_code: AssetPin,
    pub firmware_vars: AssetPin,
    pub qemu_receipt: AssetPin,
}

pub(in crate::vm) struct WindowsAssets<'a> {
    pub iso: &'a mut dyn Read,
    pub edition: Edition<'a>,
    pub driver_medium: &'a mut dyn Read,
    // The caller extracts these exact, unique paths from the held medium.
    pub driver_files: [[&'a mut dyn Read; 3]; 3],
    pub driver_receipt: &'a [u8],
    pub firmware_code: &'a mut dyn Read,
    pub firmware_vars: &'a mut dyn Read,
    pub qemu_receipt: &'a [u8],
}

#[derive(Default)]
pub(in crate::vm) struct WindowsRecipe<'a> {
    // There are no built-in owner ISO, edition or driver pins.
    pub pins: Option<WindowsPins<'a>>,
}

pub(in crate::vm) struct PreparedWindows {
    pub digest: [u8; 32],
    pub image: InstalledImage,
}

pub(in crate::vm) struct InstalledImage {
    version: String,
    revision: Option<u32>,
    edition: String,
}

impl InstalledImage {
    fn from_edition(edition: Edition<'_>) -> Option<Self> {
        let mut parts = edition.build.split('.').map(|part| {
            let number = part.parse::<u32>().ok()?;
            (number.to_string() == part).then_some(number)
        });
        let (major, minor, build) = (parts.next()??, parts.next()??, parts.next()??);
        let revision = parts.next().map_or(Some(None), |part| part.map(Some))?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            version: format!("{major}.{minor}.{build}"),
            revision,
            edition: edition.edition_id.to_owned(),
        })
    }
}

impl WindowsRecipe<'_> {
    /// Validate every asset before writing. The digest binds the versioned
    /// recipe and verified metadata, and is returned only after a full write.
    pub(in crate::vm) fn write_test_seed(
        &self,
        assets: WindowsAssets<'_>,
        guest: &[u8],
        out: impl Write,
    ) -> Result<PreparedWindows, RecipeError> {
        let pins = self.pins.as_ref().ok_or(RecipeError::Unqualified)?;
        if !valid_edition(pins.edition)
            || pins.edition != assets.edition
            || !matches!(pins.winpe_driver_letter, b'D'..=b'W' | b'Y'..=b'Z')
        {
            return Err(RecipeError::Unqualified);
        }
        let installed =
            InstalledImage::from_edition(pins.edition).ok_or(RecipeError::Unqualified)?;
        verify(pins.guest, MAX_GUEST as u64, guest)?;
        verify(pins.provisioner, 32 * 1024, PROVISIONER.as_bytes())?;
        verify(pins.driver_receipt, MAX_RECEIPT, assets.driver_receipt)?;
        verify(pins.qemu_receipt, MAX_RECEIPT, assets.qemu_receipt)?;
        for (pins, files) in pins.driver_files.into_iter().zip(assets.driver_files) {
            for (pin, file) in pins.into_iter().zip(files) {
                verify(pin, MAX_DRIVER_FILE, file)?;
            }
        }
        for (pin, limit, asset) in [
            (pins.firmware_code, MAX_FIRMWARE, assets.firmware_code),
            (pins.firmware_vars, MAX_FIRMWARE, assets.firmware_vars),
            (pins.driver_medium, MAX_DRIVERS, assets.driver_medium),
            (pins.iso, MAX_ISO, assets.iso),
        ] {
            verify(pin, limit, asset)?;
        }
        let (xml, manifest) = seed_files(pins)?;
        let image = Image::windows([
            guest,
            manifest.as_bytes(),
            MARKER,
            PROVISIONER.as_bytes(),
            xml.as_bytes(),
        ])
        .ok_or(RecipeError::MediaLimit)?;
        image
            .write(out)
            .map_err(|err| RecipeError::Write(err.kind()))?;
        Ok(PreparedWindows {
            digest: recipe_digest(pins, &xml),
            image: installed,
        })
    }
}

fn valid_edition(edition: Edition<'_>) -> bool {
    edition.index != 0
        && edition.architecture == "amd64"
        && edition.language == "en-US"
        && edition
            .edition_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric())
        && [edition.name, edition.edition_id, edition.build]
            .into_iter()
            .all(|value| !value.is_empty() && value.len() <= 128 && value.chars().all(xml_char))
}

fn verify(pin: AssetPin, max: u64, input: impl Read) -> Result<(), RecipeError> {
    if !(1..=max).contains(&pin.length) {
        return Err(RecipeError::Unqualified);
    }
    let mut digest = Sha256::new();
    // Sha256 implements Write. The extra byte detects surplus without
    // draining an oversized stream. io::copy handles interrupted reads.
    let count = std::io::copy(&mut input.take(pin.length + 1), &mut digest)
        .map_err(|_| RecipeError::Unqualified)?;
    if count != pin.length || digest.finalize()[..] != pin.sha256 {
        return Err(RecipeError::Unqualified);
    }
    Ok(())
}

fn recipe_digest(pins: &WindowsPins<'_>, xml: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    for text in [
        ID.as_bytes(),
        TOPOLOGY,
        MARKER,
        DETACHED_HEALTH.as_bytes(),
        GENERALIZED_OOBE.as_bytes(),
        xml.as_bytes(),
        pins.edition.name.as_bytes(),
        pins.edition.edition_id.as_bytes(),
        pins.edition.build.as_bytes(),
        pins.edition.language.as_bytes(),
        pins.edition.architecture.as_bytes(),
    ] {
        digest.update((text.len() as u64).to_le_bytes());
        digest.update(text);
    }
    digest.update(match pins.edition.container {
        InstallImage::Wim => [0],
        InstallImage::Esd => [1],
    });
    digest.update(arx_guest::protocol::PROTOCOL_VERSION.to_le_bytes());
    for pin in [
        pins.iso,
        pins.driver_medium,
        pins.driver_receipt,
        pins.guest,
        pins.provisioner,
        pins.firmware_code,
        pins.firmware_vars,
        pins.qemu_receipt,
    ]
    .into_iter()
    .chain(pins.driver_files.into_iter().flatten())
    {
        digest.update(pin.length.to_le_bytes());
        digest.update(pin.sha256);
    }
    digest.finalize().into()
}

fn xml_char(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

// Markup, names and namespace prefixes are fixed recipe literals. Only
// values pass through these two writer entry points.
fn escape_xml(out: &mut String, value: &str, attribute: bool) -> Result<(), RecipeError> {
    for ch in value.chars() {
        let escaped = match ch {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' if attribute => "&quot;",
            '\'' if attribute => "&apos;",
            '\r' => "&#13;",
            '\n' if attribute => "&#10;",
            '\t' if attribute => "&#9;",
            ch if xml_char(ch) => {
                out.push(ch);
                continue;
            }
            _ => return Err(RecipeError::Unqualified),
        };
        out.push_str(escaped);
    }
    Ok(())
}

#[derive(Default)]
struct Xml(String);

impl Xml {
    fn attr(&mut self, key: &str, value: &str) -> Result<(), RecipeError> {
        write!(self.0, " {key}=\"").unwrap();
        escape_xml(&mut self.0, value, true)?;
        self.0.push('"');
        Ok(())
    }

    fn value(&mut self, key: &str, value: &str) -> Result<(), RecipeError> {
        write!(self.0, "<{key}>").unwrap();
        escape_xml(&mut self.0, value, false)?;
        write!(self.0, "</{key}>").unwrap();
        Ok(())
    }

    fn component(&mut self, name: &str) -> Result<(), RecipeError> {
        self.0.push_str("<component");
        self.attr("name", name)?;
        self.0.push_str(r#" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">"#);
        Ok(())
    }

    fn command(
        &mut self,
        order: usize,
        command: &str,
        specialize: bool,
    ) -> Result<(), RecipeError> {
        if specialize && command.len() > 259 {
            return Err(RecipeError::MediaLimit);
        }
        self.0
            .push_str(r#"<RunSynchronousCommand wcm:action="add">"#);
        self.value("Order", &order.to_string())?;
        self.value("Path", command)?;
        if specialize {
            self.0.push_str("<WillReboot>Never</WillReboot>");
        }
        self.0.push_str("</RunSynchronousCommand>");
        Ok(())
    }
}

fn seed_files(pins: &WindowsPins<'_>) -> Result<(String, String), RecipeError> {
    let script_hash = hex::encode_upper(pins.provisioner.sha256);
    let manifest = format!(
        "arx-windows-seed-v1\n{}\n{}\n{script_hash}\n",
        pins.guest.length,
        hex::encode_upper(pins.guest.sha256),
    );
    let manifest_hash = hex::encode_upper(Sha256::digest(&manifest));
    let mut xml = Xml::default();
    xml.0
        .push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<unattend");
    xml.attr("xmlns", UNATTEND)?;
    xml.attr("xmlns:wcm", WCM)?;
    xml.0.push_str(r#"><settings pass="windowsPE">"#);
    xml.component("Microsoft-Windows-International-Core-WinPE")?;
    xml.0
        .push_str("<SetupUILanguage><UILanguage>en-US</UILanguage></SetupUILanguage>");
    xml.0.push_str(LOCALE);
    xml.0.push_str("</component>");
    xml.component("Microsoft-Windows-PnpCustomizationsWinPE")?;
    xml.0.push_str("<DriverPaths>");
    let letter = char::from(pins.winpe_driver_letter);
    for (index, (dir, _)) in DRIVERS.iter().enumerate() {
        xml.0.push_str(r#"<PathAndCredentials wcm:action="add""#);
        xml.attr("wcm:keyValue", &(index + 1).to_string())?;
        xml.0.push('>');
        xml.value("Path", &format!(r"{letter}:\{dir}"))?;
        xml.0.push_str("</PathAndCredentials>");
    }
    xml.0.push_str("</DriverPaths></component>");
    xml.component("Microsoft-Windows-Setup")?;
    xml.0.push_str(DISK_LAYOUT);
    xml.0.push_str(
        "<ImageInstall><OSImage><InstallFrom><MetaData wcm:action=\"add\"><Key>/IMAGE/INDEX</Key>",
    );
    xml.value("Value", &pins.edition.index.to_string())?;
    xml.0.push_str("</MetaData></InstallFrom><InstallTo><DiskID>0</DiskID><PartitionID>3</PartitionID></InstallTo><WillShowUI>Never</WillShowUI></OSImage></ImageInstall>");
    xml.0.push_str("<UserData><AcceptEula>true</AcceptEula></UserData><DynamicUpdate><Enable>false</Enable><WillShowUI>Never</WillShowUI></DynamicUpdate><RunSynchronous>");
    let mut guard = String::from(r"cmd.exe /d /c ");
    for (dir, names) in DRIVERS {
        for name in names {
            write!(guard, r"if not exist {letter}:\{dir}\{name} exit /b 1 & ").unwrap();
        }
    }
    guard.push_str("exit /b 0");
    xml.command(1, &guard, false)?;
    for (order, bypass) in ["BypassTPMCheck", "BypassSecureBootCheck"]
        .into_iter()
        .enumerate()
    {
        xml.command(
            order + 2,
            &format!(r"reg.exe add HKLM\SYSTEM\Setup\LabConfig /v {bypass} /t REG_DWORD /d 1 /f"),
            false,
        )?;
    }
    xml.0
        .push_str(r#"</RunSynchronous></component></settings><settings pass="specialize">"#);
    xml.component("Microsoft-Windows-Deployment")?;
    xml.0.push_str("<RunSynchronous>");
    for (order, body) in [
        String::from(r"$d=@([IO.DriveInfo]::GetDrives()|?{$_.DriveType -eq 5 -and (Test-Path ($_.Name+'arxseed.id'))});if($d.Count-ne 1){exit 1};[IO.File]::WriteAllText($PSHOME+'\..\..\arxseed.loc',$d[0].Name)"),
        format!(r"$p=[IO.File]::ReadAllText($PSHOME+'\..\..\arxseed.loc')+'arxsetup.ps1';if((gi $p).Length-ne {}){{exit 1}};cp $p ($PSHOME+'\..\..\arxsetup.ps1')", pins.provisioner.length),
        format!(r"[IO.File]::WriteAllText($PSHOME+'\..\..\arxseed.pin','{manifest_hash}')"),
        format!(r"$p=$PSHOME+'\..\..\arxsetup.ps1';if((Get-FileHash $p).Hash-ne '{script_hash}'){{exit 1}};& $p install"),
    ].into_iter().enumerate() {
        xml.command(order + 1, &format!(r#"powershell.exe -nop -noni -ep Bypass -c "$ErrorActionPreference='Stop';{body}""#), true)?;
    }
    xml.0
        .push_str(r#"</RunSynchronous></component></settings><settings pass="oobeSystem">"#);
    xml.component("Microsoft-Windows-International-Core")?;
    xml.0.push_str(LOCALE);
    xml.0.push_str("</component>");
    xml.component("Microsoft-Windows-Shell-Setup")?;
    xml.0.push_str(OOBE);
    xml.0.push_str("</component></settings></unattend>\n");
    Ok((xml.0, manifest))
}

const LOCALE: &str = "<InputLocale>en-US</InputLocale><SystemLocale>en-US</SystemLocale>\
    <UILanguage>en-US</UILanguage><UserLocale>en-US</UserLocale>";

const DISK_LAYOUT: &str = r#"<DiskConfiguration>
<Disk wcm:action="add"><DiskID>0</DiskID><WillWipeDisk>true</WillWipeDisk>
<CreatePartitions>
<CreatePartition wcm:action="add"><Order>1</Order><Type>EFI</Type><Size>260</Size></CreatePartition>
<CreatePartition wcm:action="add"><Order>2</Order><Type>MSR</Type><Size>16</Size></CreatePartition>
<CreatePartition wcm:action="add"><Order>3</Order><Type>Primary</Type><Extend>true</Extend></CreatePartition>
</CreatePartitions><ModifyPartitions>
<ModifyPartition wcm:action="add"><Order>1</Order><PartitionID>1</PartitionID><Format>FAT32</Format></ModifyPartition>
<ModifyPartition wcm:action="add"><Order>2</Order><PartitionID>3</PartitionID><Format>NTFS</Format></ModifyPartition>
</ModifyPartitions></Disk><WillShowUI>Never</WillShowUI>
</DiskConfiguration>"#;

const OOBE: &str = r#"<OOBE><HideEULAPage>true</HideEULAPage><HideOnlineAccountScreens>true</HideOnlineAccountScreens>
<HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE><ProtectYourPC>3</ProtectYourPC></OOBE>
<UserAccounts><LocalAccounts><LocalAccount wcm:action="add"><Name>ArxBootstrap</Name><Group>Users</Group>
</LocalAccount></LocalAccounts></UserAccounts>"#;

pub(super) const PROVISIONER: &str = r###"param([Parameter(Mandatory=$true)][ValidateSet('install','finish')][string]$Phase)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$system = Join-Path $env:SystemRoot 'System32'
$root = Join-Path $env:ProgramFiles 'Arxcovo'
$guest = Join-Path $root 'arx-guest.exe'
$work = Join-Path $root 'work'
$state = Join-Path $root 'setup.phase'
$script = Join-Path $system 'arxsetup.ps1'
$location = Join-Path $system 'arxseed.loc'
$manifestPin = Join-Path $system 'arxseed.pin'
$task = 'ArxcovoTemplateFinalize'
$bootstrap = 'ArxBootstrap'
$setupKey = 'HKLM:\SYSTEM\Setup'
$sc = Join-Path $system 'sc.exe'
$shell = Join-Path $system 'WindowsPowerShell\v1.0\powershell.exe'

function Invoke-FixedTool($Program, [string[]]$Arguments) {
    & $Program @Arguments *> $null
    if ($LASTEXITCODE -ne 0) { throw 'tool' }
}

function Assert-PlainPath([string]$Path) {
    $current = $Path
    while ($current) {
        $entry = Get-Item -LiteralPath $current -Force -ErrorAction SilentlyContinue
        if ($entry -and ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'reparse'
        }
        $current = [IO.Path]::GetDirectoryName($current)
    }
}

function Set-DirectorySecurity([string]$Path, [string]$Descriptor) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetSecurityDescriptorSddlForm($Descriptor)
    Set-Acl -LiteralPath $Path -AclObject $acl
}

function Open-GuestRegistryKey([string]$Path) {
    [Microsoft.Win32.Registry]::LocalMachine.CreateSubKey($Path)
}

function Set-VerifiedRegistryValue([string]$Path, [string]$Name, [Microsoft.Win32.RegistryValueKind]$Kind, $Value) {
    $key = Open-GuestRegistryKey $Path
    try {
        $key.SetValue($Name, $Value, $Kind)
        $key.Flush()
        if ($key.GetValueKind($Name) -ne $Kind) { throw 'registry kind' }
        $actual = $key.GetValue($Name)
        if ($Kind -eq 'Binary') {
            if ([Convert]::ToBase64String($actual) -cne [Convert]::ToBase64String($Value)) {
                throw 'registry value'
            }
        } elseif ($actual -cne $Value) {
            throw 'registry value'
        }
    } finally { $key.Close() }
}

function Set-GuestPowerShellLogging {
    $policy = 'SOFTWARE\Policies\Microsoft\Windows\PowerShell'
    Set-VerifiedRegistryValue "$policy\ScriptBlockLogging" EnableScriptBlockLogging DWord 0
    Set-VerifiedRegistryValue "$policy\ScriptBlockLogging" EnableScriptBlockInvocationLogging DWord 0
    Set-VerifiedRegistryValue "$policy\ModuleLogging" EnableModuleLogging DWord 0
    Set-VerifiedRegistryValue "$policy\Transcription" EnableTranscripting DWord 0
}

function Get-GuestServiceSid {
    $sid = ([Security.Principal.NTAccount]::new('NT SERVICE','arx-guest')).Translate(
        [Security.Principal.SecurityIdentifier]).Value
    if ($sid -cnotmatch '^S-1-5-80(-[0-9]+){5}$') { throw 'service' }
    $sid
}

function Set-PortSecurity([string]$ServiceSid) {
    # This is the dedicated raw PDO class from WdfPdoInitAssignRawDevice,
    # distinct from the enumerated port interface class used by arx-guest.
    $class = 'SYSTEM\CurrentControlSet\Control\Class\{6FDE7547-1B65-48AE-B628-80BE62016026}\Properties'
    $descriptor = [Security.AccessControl.RawSecurityDescriptor]::new("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;$ServiceSid)")
    $bytes = [byte[]]::new($descriptor.BinaryLength)
    $descriptor.GetBinaryForm($bytes, 0)
    Set-VerifiedRegistryValue $class Security Binary $bytes
}

function Install-ServiceFromSeed {
    $source = [IO.File]::ReadAllText($location)
    if ($source -cnotmatch '^[D-Z]:\\$' -or [IO.DriveInfo]::new($source).DriveType -ne 'CDRom') {
        throw 'seed'
    }
    $marker = Join-Path $source 'arxseed.id'
    $manifest = Join-Path $source 'arxpins.txt'
    $binary = Join-Path $source 'arxguest.exe'
    if ((Get-Item -LiteralPath $marker).Length -ne 29 -or
        [IO.File]::ReadAllText($marker) -cne ("arxcovo-windows-test-seed-v1" + [char]10) -or
        (Get-Item -LiteralPath $manifest).Length -notin (1..1024) -or
        (Get-FileHash -LiteralPath $manifest).Hash -cne [IO.File]::ReadAllText($manifestPin)) {
        throw 'seed'
    }
    $pins = [IO.File]::ReadAllLines($manifest)
    if ($pins.Count -ne 4 -or $pins[0] -cne 'arx-windows-seed-v1' -or
        $pins[1] -cnotmatch '^[1-9][0-9]{0,7}$' -or [long]$pins[1] -gt 32MB -or
        (Get-Item -LiteralPath $binary).Length -ne [long]$pins[1] -or
        $pins[2] -cnotmatch '^[A-F0-9]{64}$' -or
        (Get-FileHash -LiteralPath $binary).Hash -cne $pins[2] -or
        (Get-FileHash -LiteralPath $script).Hash -cne $pins[3]) {
        throw 'pins'
    }
    Assert-PlainPath $root
    New-Item -Path $root -ItemType Directory | Out-Null
    Set-DirectorySecurity $root 'O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FRFX;;;LS)'
    Copy-Item -LiteralPath $binary -Destination $guest
    if ((Get-FileHash -LiteralPath $guest).Hash -cne $pins[2]) { throw 'copy' }
    # sc.exe receives the escaped inner quotes as part of the SCM image path.
    $image = '\"' + $guest + '\" serve-service'
    Invoke-FixedTool $sc @('create','arx-guest','binPath=',$image,'type=','own','start=','demand',
        'obj=','NT AUTHORITY\LocalService')
    Invoke-FixedTool $sc @('sdset','arx-guest','O:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;CCRC;;;LS)')
    Invoke-FixedTool $sc @('sidtype','arx-guest','unrestricted')
    Invoke-FixedTool $sc @('privs','arx-guest','SeChangeNotifyPrivilege/SeIncreaseQuotaPrivilege')
    $sid = Get-GuestServiceSid
    New-Item -Path $work -ItemType Directory | Out-Null
    Set-DirectorySecurity $work "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;;FRFX;;;LS)(A;;0x44;;;$sid)"
    Invoke-FixedTool (Join-Path $system 'icacls.exe') @($work,'/setintegritylevel','L')

    Remove-Item -LiteralPath $location,$manifestPin -Force
    $action = New-ScheduledTaskAction -Execute $shell -Argument (
        '-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $script + '" finish')
    $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 55) -MultipleInstances IgnoreNew
    $principal = New-ScheduledTaskPrincipal -UserId 'S-1-5-18' -LogonType ServiceAccount
    $trigger = New-ScheduledTaskTrigger -AtStartup
    Register-ScheduledTask -TaskName $task -Action $action -Settings $settings -Trigger $trigger -Principal $principal | Out-Null
    Start-ScheduledTask -TaskName $task
}

function Clear-Bootstrap {
    $account = Get-LocalUser -Name $bootstrap
    Disable-LocalUser -InputObject $account
    $profile = @(Get-CimInstance -ClassName Win32_UserProfile -Filter "SID='$($account.SID.Value)'")
    if ($profile.Count -gt 1 -or ($profile.Count -eq 1 -and ($profile[0].Loaded -or $profile[0].Special))) {
        throw 'profile'
    }
    foreach ($entry in $profile) {
        Assert-PlainPath $entry.LocalPath
        $entry | Remove-CimInstance
        if (Test-Path -LiteralPath $entry.LocalPath) { throw 'profile' }
    }
    Remove-LocalUser -InputObject $account
    if (Get-CimInstance -ClassName Win32_UserProfile -Filter "SID='$($account.SID.Value)'") {
        throw 'profile'
    }
    Assert-NoBootstrap
}

function Assert-NoBootstrap {
    if (Get-LocalUser -Name $bootstrap,'defaultuser0' -ErrorAction SilentlyContinue) { throw 'account' }
    if (Test-Path -LiteralPath (Join-Path $env:SystemDrive "Users\$bootstrap")) { throw 'profile' }
}

function Clear-SetupCaches {
    $panther = Join-Path $env:SystemRoot 'Panther'
    $sysprep = Join-Path $system 'Sysprep'
    $disk = $env:SystemDrive + '\'
    $stageRoot = Join-Path $disk '$WINDOWS.~BT\Sources'
    $staging = Join-Path $stageRoot 'Panther'
    $directories = @($disk,$panther,(Join-Path $panther 'Unattend'),$sysprep,$stageRoot,$staging)
    $answerFiles = @(
        foreach ($dir in $directories) {
            foreach ($name in 'unattend.xml','autounattend.xml') { Join-Path $dir $name }
        }
    )
    $override = Get-ItemPropertyValue -LiteralPath $setupKey -Name UnattendFile -ErrorAction SilentlyContinue
    if ($override) {
        if ($override -notin $answerFiles) { throw 'override' }
        Remove-ItemProperty -LiteralPath $setupKey -Name UnattendFile
    }
    foreach ($file in $answerFiles) {
        Assert-PlainPath $file
        if (Test-Path -LiteralPath $file) {
            $entry = Get-Item -LiteralPath $file -Force
            if ($entry.PSIsContainer -or $entry.Length -gt 256KB) { throw 'cache' }
            # A known answer-file location can contain a foreign file.
            if (-not ([IO.File]::ReadAllText($file).Contains('ArxBootstrap'))) { throw 'cache' }
            Remove-Item -LiteralPath $file -Force
            if (Test-Path -LiteralPath $file) { throw 'cache' }
        }
    }
    if (Get-ItemPropertyValue -LiteralPath $setupKey -Name UnattendFile -ErrorAction SilentlyContinue) {
        throw 'override'
    }
    $left = 4096
    $pending = [Collections.Generic.Stack[string]]::new()
    foreach ($dir in $panther,$sysprep,$staging) {
        Assert-PlainPath $dir
        if ([IO.Directory]::Exists($dir)) { $pending.Push($dir) }
    }
    while ($pending.Count) {
        foreach ($item in [IO.Directory]::EnumerateFileSystemEntries($pending.Pop())) {
            $left--
            if ($left -lt 0) { throw 'inventory' }
            $attributes = [IO.File]::GetAttributes($item)
            if ($attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'inventory' }
            if ($attributes -band [IO.FileAttributes]::Directory) { $pending.Push($item) }
            if ([IO.Path]::GetFileName($item) -match 'unattend.*\.xml') { throw 'cache' }
        }
    }
    $winlogon = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
    if ((Get-ItemPropertyValue -LiteralPath $winlogon -Name AutoAdminLogon -ErrorAction SilentlyContinue) -eq '1' -or
        $null -ne (Get-ItemPropertyValue -LiteralPath $winlogon -Name DefaultPassword -ErrorAction SilentlyContinue)) {
        throw 'autologon'
    }
    foreach ($path in $location,$manifestPin) {
        if (Test-Path -LiteralPath $path) { throw 'staging' }
    }
}

function Test-InstallerMediaAttached {
    [IO.DriveInfo]::GetDrives().DriveType -contains 'CDRom'
}

function Write-SetupPhase([string]$Value) {
    $bytes = [Text.Encoding]::ASCII.GetBytes($Value)
    $file = [IO.File]::Open($state, 'Create', 'Write', 'None')
    try {
        $file.Write($bytes, 0, $bytes.Length)
        $file.Flush($true)
    } finally { $file.Dispose() }
}

function Set-TestDefaultRoute {
    $nics = @(Get-CimInstance Win32_NetworkAdapter -Filter "ServiceName='netkvm'" |
        Where-Object { $_.InterfaceIndex -gt 0 })
    if ($nics.Count -ne 1) { throw 'test nic' }
    $index = $nics[0].InterfaceIndex
    $prefix = '0.0.0.0/0'
    $gateway = '10.0.2.2'
    $saved = @(Get-NetRoute -InterfaceIndex $index -DestinationPrefix $prefix -PolicyStore PersistentStore -ErrorAction SilentlyContinue)
    if (@($saved | Where-Object NextHop -ne $gateway).Count) { throw 'test route' }
    if (!$saved.Count) {
        New-NetRoute -InterfaceIndex $index -DestinationPrefix $prefix -NextHop $gateway -RouteMetric 4096 | Out-Null
    }
}

function Finish-Setup {
    $imageState = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup\State'
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($true) {
        $installing = Get-ItemPropertyValue -LiteralPath $setupKey -Name SystemSetupInProgress
        $oobe = Get-ItemPropertyValue -LiteralPath $setupKey -Name OOBEInProgress -ErrorAction SilentlyContinue
        if ((Get-ItemPropertyValue -LiteralPath $imageState -Name ImageState) -ceq 'IMAGE_STATE_COMPLETE' -and
            $installing -eq 0 -and ($null -eq $oobe -or $oobe -eq 0)) { break }
        if ($timer.Elapsed.TotalMinutes -ge 45) { throw 'setup timeout' }
        Start-Sleep -Seconds 2
    }
    $boot = (Get-CimInstance -ClassName Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().Ticks
    Assert-PlainPath $state
    if (-not (Test-Path -LiteralPath $state)) {
        Clear-Bootstrap
        Clear-SetupCaches
        Set-PortSecurity (Get-GuestServiceSid)
        Write-SetupPhase "1:$boot"
        Invoke-FixedTool (Join-Path $system 'shutdown.exe') @('/s','/t','0')
        return
    }
    if (Test-InstallerMediaAttached) { throw 'media attached' }
    if ((Get-Item -LiteralPath $state).Length -gt 32) { throw 'stage' }
    $previous = [regex]::Match([IO.File]::ReadAllText($state), '^([12]):([0-9]{1,19})$')
    if (-not $previous.Success -or [long]$previous.Groups[2].Value -ge $boot) {
        throw 'stage'
    }
    $stage = $previous.Groups[1].Value
    Assert-NoBootstrap
    Clear-SetupCaches
    if ($stage -eq '1') {
        Set-TestDefaultRoute
        Set-GuestPowerShellLogging
        Write-SetupPhase "2:$boot"
        # SCM checks the auto-start configuration. Start explicitly on this
        # boot after persisting the phase and verifying every policy value.
        Invoke-FixedTool $sc @('config','arx-guest','start=','auto')
        Invoke-FixedTool $sc @('start','arx-guest')
        return
    }
    Unregister-ScheduledTask -TaskName $task -Confirm:$false
    if (Get-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue) { throw 'task' }
    Remove-Item -LiteralPath $state,$script -Force
    if ((Test-Path -LiteralPath $state) -or (Test-Path -LiteralPath $script)) { throw 'staging' }
    Invoke-FixedTool (Join-Path $system 'Sysprep\Sysprep.exe') @('/generalize','/oobe','/shutdown','/quiet')
}

try {
    if ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value -cne 'S-1-5-18') { throw 'context' }
    if ($Phase -eq 'install') { Install-ServiceFromSeed } else { Finish-Setup }
    exit 0
} catch {
    # No raw command output, exception details, guest identity or paths.
    exit 1
}
"###;

// The caller supplies elapsed monotonic time from before the first launch.
// Once a stage fails, the candidate cannot advance or be retried in place.
pub(super) const BUILD_LIMIT: Duration = Duration::from_secs(3 * 60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Install,
    DetachedReadiness,
    Generalize,
    AttestGeneralization,
    DetachedVerification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StageFailure {
    Timeout,
    UnexpectedStage,
    Failed,
    #[cfg(test)]
    Residue,
}

#[derive(Clone, Copy)]
pub(super) enum StageEvidence {
    Clean,
    #[cfg(test)]
    Residue,
    Failed,
}

pub(super) struct BuildStages(Result<Option<Stage>, StageFailure>);

impl Default for BuildStages {
    fn default() -> Self {
        Self(Ok(Some(Stage::Install)))
    }
}

impl BuildStages {
    pub(super) fn observe(
        &mut self,
        elapsed: Duration,
        stage: Stage,
        evidence: StageEvidence,
    ) -> Result<(), StageFailure> {
        self.0 = (|| {
            let expected = self.0?;
            if elapsed >= BUILD_LIMIT {
                return Err(StageFailure::Timeout);
            }
            if expected != Some(stage) {
                return Err(StageFailure::UnexpectedStage);
            }
            match evidence {
                #[cfg(test)]
                StageEvidence::Residue => return Err(StageFailure::Residue),
                StageEvidence::Failed => return Err(StageFailure::Failed),
                StageEvidence::Clean => {}
            }
            Ok(match stage {
                Stage::Install => Some(Stage::DetachedReadiness),
                Stage::DetachedReadiness => Some(Stage::Generalize),
                Stage::Generalize => Some(Stage::AttestGeneralization),
                Stage::AttestGeneralization => Some(Stage::DetachedVerification),
                Stage::DetachedVerification => None,
            })
        })();
        self.0.map(|_| ())
    }

    pub(super) fn can_seal(&self) -> bool {
        self.0 == Ok(None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InstalledTarget {
    Readiness(PairSlots),
    Verification(PairSlots),
}

impl InstalledTarget {
    fn probe(self, image: &InstalledImage) -> String {
        image.probe(matches!(self, Self::Readiness(_)))
    }
}

impl InstalledImage {
    pub(in crate::vm) fn probe(&self, validation: bool) -> String {
        let validation = if validation { "$true" } else { "$false" };
        let revision = self
            .revision
            .map_or_else(|| "$null".to_owned(), |number| number.to_string());
        // These substitutions are validated ASCII numeric fields and a SKU ID.
        format!(
            "$validation = {validation}\n$expectedVersion = '{}'\n$expectedEdition = '{}'\n$expectedRevision = {revision}\n{DETACHED_HEALTH}",
            self.version, self.edition,
        )
    }
}

const GENERALIZED_OOBE: &str = "IMAGE_STATE_GENERALIZE_RESEAL_TO_OOBE";

pub(super) struct OfflineSetupState {
    pub registry: String,
    pub state_ini: String,
}

impl OfflineSetupState {
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(super) fn from_released_pair(registry: bool, state_ini: bool) -> Self {
        let observed = |generalized| if generalized { GENERALIZED_OOBE } else { "" }.to_owned();
        Self {
            registry: observed(registry),
            state_ini: observed(state_ini),
        }
    }

    fn is_generalized(&self) -> bool {
        // A clean QMP stop proves the release, while the two Setup witnesses
        // establish successful generalization on the released disk itself.
        self.registry == GENERALIZED_OOBE && self.state_ini == GENERALIZED_OOBE
    }
}

// The restricted guest returns only exit status. The same probe checks both
// detached boots, with a fixed phase-specific expectation for finalizer state.
const DETACHED_HEALTH: &str = r#"$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
try {
    $setup = Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\Setup'
    $image = Get-ItemPropertyValue -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup\State' -Name ImageState
    if ($image -cne 'IMAGE_STATE_COMPLETE' -or $setup.SystemSetupInProgress -ne 0 -or
        ($setup.PSObject.Properties['OOBEInProgress'] -and $setup.OOBEInProgress -ne 0) -or
        $setup.PSObject.Properties['UnattendFile']) { exit 1 }
    $os = Get-CimInstance -ClassName Win32_OperatingSystem
    $sku = Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    if ($os.Version -cne $expectedVersion -or $sku.EditionID -cne $expectedEdition -or
        ($null -ne $expectedRevision -and [uint32]$sku.UBR -ne $expectedRevision)) { exit 1 }
    if (@(Get-CimInstance -ClassName Win32_CDROMDrive).Count -ne 0) { exit 1 }
    $service = Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Services\arx-guest'
    if ($service.Start -ne 2 -or $service.ObjectName -ine 'NT AUTHORITY\LocalService') { exit 1 }
    foreach ($name in @('viostor','vioser','netkvm')) {
        $devices = @(Get-CimInstance -ClassName Win32_PnPEntity -Filter "Service = '$name'")
        if ($devices.Count -ne 1 -or $devices[0].ConfigManagerErrorCode -ne 0) { exit 1 }
    }
    $nic = @(Get-NetAdapter | Where-Object Status -eq 'Up')
    if ($nic.Count -ne 1) { exit 1 }
    $routes = @(Get-NetRoute -InterfaceIndex $nic[0].ifIndex -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0')
    if (!$routes.Count -or @($routes | Where-Object NextHop -ne '10.0.2.2').Count) { exit 1 }
    if (@(Get-LocalUser | Where-Object { $_.Name -in @('ArxBootstrap','defaultuser0') }).Count -ne 0) { exit 1 }
    $programs = Get-ItemPropertyValue -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion' -Name ProgramFilesDir
    if (-not [IO.Path]::IsPathRooted($programs)) { exit 1 }
    $phase = Join-Path $programs 'Arxcovo\setup.phase'
    $system = Join-Path $env:SystemRoot 'System32'
    $disk = [IO.Path]::GetPathRoot($env:SystemRoot)
    if ((Test-Path -LiteralPath (Join-Path $system 'arxseed.loc')) -or
        (Test-Path -LiteralPath (Join-Path $system 'arxseed.pin')) -or
        (Test-Path -LiteralPath (Join-Path $disk 'Users\ArxBootstrap')) -or
        (Test-Path -LiteralPath (Join-Path $disk 'Users\defaultuser0'))) { exit 1 }
    $panther = Join-Path $env:SystemRoot 'Panther'
    $sysprep = Join-Path $system 'Sysprep'
    $sources = Join-Path $disk '$WINDOWS.~BT\Sources'
    $staging = Join-Path $sources 'Panther'
    foreach ($dir in $disk,$panther,(Join-Path $panther 'Unattend'),$sysprep,$sources,$staging) {
        foreach ($name in 'unattend.xml','autounattend.xml') {
            if (Test-Path -LiteralPath (Join-Path $dir $name)) { exit 1 }
        }
    }
    $winlogon = Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
    if (($winlogon.PSObject.Properties['AutoAdminLogon'] -and $winlogon.AutoAdminLogon -eq '1') -or
        $winlogon.PSObject.Properties['DefaultPassword']) { exit 1 }
    $remaining = 4096
    $pending = [Collections.Generic.Stack[string]]::new()
    foreach ($dir in $panther,$sysprep,$staging) {
        if (Test-Path -LiteralPath $dir) { $pending.Push($dir) }
    }
    while ($pending.Count) {
        $dir = $pending.Pop()
        if ((Get-Item -LiteralPath $dir -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { exit 1 }
        Get-ChildItem -LiteralPath $dir -Force | ForEach-Object {
            if (--$remaining -lt 0 -or ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
                $_.Name -match 'unattend.*\.xml') { throw 'setup inventory' }
            if ($_.PSIsContainer) { $pending.Push($_.FullName) }
        }
    }
    if ($validation) {
        $boot = $os.LastBootUpTime.ToUniversalTime().Ticks
        $marker = Get-Item -LiteralPath $phase -Force
        if ($marker.Length -notin (3..32) -or ($marker.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
            (Get-Content -LiteralPath $phase -Raw) -cne "2:$boot") { exit 1 }
    } elseif ((Test-Path -LiteralPath $phase) -or
              (Test-Path -LiteralPath (Join-Path $system 'arxsetup.ps1')) -or
              @(Get-ScheduledTask | Where-Object { $_.TaskName -eq 'ArxcovoTemplateFinalize' }).Count -ne 0) {
        exit 1
    }
    exit 0
} catch { exit 1 }
"#;

pub(super) struct WindowsBuild<'a> {
    started_at: Instant,
    budget: InstallerBudget,
    stages: BuildStages,
    boots: Vec<Boot>,
    image: &'a InstalledImage,
}

impl<'a> WindowsBuild<'a> {
    pub(super) fn install(
        runtime: &mut dyn TemplateRuntime,
        slots: BuildSlots,
        image: &'a InstalledImage,
    ) -> Result<Self, Failure> {
        let budget = InstallerBudget::start(InstallerRecipe::Windows);
        let mut build = Self {
            started_at: budget.clip(BUILD_LIMIT) - BUILD_LIMIT,
            budget,
            stages: BuildStages::default(),
            boots: Vec::with_capacity(4),
            image,
        };
        build.qmp(runtime, slots, InstallerPhase::WindowsSetup, Stage::Install)?;
        build.installed(
            runtime,
            InstalledTarget::Readiness(slots.pair),
            Stage::DetachedReadiness,
        )?;
        build.qmp(
            runtime,
            slots,
            InstallerPhase::WindowsFinalize,
            Stage::Generalize,
        )?;
        Ok(build)
    }

    pub(super) fn attest_generalization(
        &mut self,
        runtime: &mut dyn TemplateRuntime,
        slots: PairSlots,
        files: Pair,
    ) -> Result<(), Failure> {
        let deadline = self.deadline(BUILD_LIMIT)?;
        let observed = runtime
            .windows_offline_setup_state(slots, files, deadline)
            .and_then(|state| {
                if state.is_generalized() {
                    Ok(())
                } else {
                    Err(Failure::GeneralizationUnverified)
                }
            });
        self.advance(Stage::AttestGeneralization, observed)
    }

    pub(super) fn verify(
        &mut self,
        runtime: &mut dyn TemplateRuntime,
        slots: PairSlots,
    ) -> Result<(), Failure> {
        self.installed(
            runtime,
            InstalledTarget::Verification(slots),
            Stage::DetachedVerification,
        )
    }

    pub(super) fn check_seal(&self) -> Result<(), Failure> {
        self.deadline(BUILD_LIMIT)?;
        if self.stages.can_seal() {
            Ok(())
        } else {
            Err(Failure::Native(NativeRefusal::InstallerPhaseMismatch))
        }
    }

    fn qmp(
        &mut self,
        runtime: &mut dyn TemplateRuntime,
        slots: BuildSlots,
        phase: InstallerPhase,
        stage: Stage,
    ) -> Result<(), Failure> {
        let plan = self
            .budget
            .stage(phase, BUILD_LIMIT)
            .ok_or(Failure::Native(NativeRefusal::InstallerPhaseMismatch))?;
        if Instant::now() >= plan.deadline() {
            return Err(Failure::Native(NativeRefusal::InstallerDeadlineExpired));
        }
        let observed = runtime
            .windows_installer(slots, plan)
            .and_then(|(boot, observed)| {
                if observed != phase {
                    return Err(Failure::Native(NativeRefusal::InstallerPhaseMismatch));
                }
                self.fresh(boot)
            })
            .map_err(|error| match error {
                Failure::Installer {
                    report,
                    observed_at,
                } if report.reason
                    == InstallerFailureReason::Native(NativeRefusal::InstallerDeadlineExpired)
                    || (report.reason == InstallerFailureReason::Timeout
                        && observed_at >= plan.deadline()) =>
                {
                    Failure::InstallerDeadline(report)
                }
                error => error,
            });
        self.advance(stage, observed)
    }

    fn installed(
        &mut self,
        runtime: &mut dyn TemplateRuntime,
        target: InstalledTarget,
        stage: Stage,
    ) -> Result<(), Failure> {
        let ready_by = self.deadline(GUEST_READY_BUDGET)?;
        let (boot, ready) = runtime.windows_installed(target, ready_by)?;
        let checked = (|| {
            if Instant::now() >= ready_by {
                return Err(Failure::Native(NativeRefusal::InstallerDeadlineExpired));
            }
            self.fresh(boot)?;
            if ready != Readiness::ScriptChannel {
                return Err(Failure::Readiness(ready));
            }
            let deadline = self.deadline(Duration::from_secs(30))?;
            let health = runtime.windows_health(boot, &target.probe(self.image), deadline);
            if Instant::now() >= deadline {
                return Err(Failure::Native(NativeRefusal::InstallerDeadlineExpired));
            }
            if health? != (ScriptStatus::Exited, Some(0)) {
                return Err(Failure::Health);
            }
            Ok(())
        })();
        let stopped = clean(runtime.windows_stop(boot, self.budget.clip(BUILD_LIMIT)));
        self.advance(stage, stopped.and(checked))
    }

    fn advance(&mut self, stage: Stage, observed: Result<(), Failure>) -> Result<(), Failure> {
        let evidence = if observed.is_ok() {
            StageEvidence::Clean
        } else {
            StageEvidence::Failed
        };
        let advanced = self
            .stages
            .observe(self.started_at.elapsed(), stage, evidence);
        observed?;
        advanced.map_err(|reason| {
            Failure::Native(match reason {
                StageFailure::Timeout => NativeRefusal::InstallerDeadlineExpired,
                _ => NativeRefusal::InstallerPhaseMismatch,
            })
        })
    }

    fn fresh(&mut self, boot: Boot) -> Result<(), Failure> {
        if self.boots.contains(&boot) {
            return Err(Failure::ReusedBoot);
        }
        self.boots.push(boot);
        Ok(())
    }

    fn deadline(&self, limit: Duration) -> Result<Instant, Failure> {
        let deadline = self.budget.clip(limit);
        if Instant::now() >= deadline {
            return Err(Failure::Native(NativeRefusal::InstallerDeadlineExpired));
        }
        Ok(deadline)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::{
        io::Cursor,
        process::{Command, Stdio},
    };

    const GUEST: &[u8] = b"synthetic windows guest";
    const ISO: &[u8] = b"synthetic windows iso";
    const MEDIUM: &[u8] = b"synthetic virtio medium";
    const DRIVER: &[u8] = b"synthetic driver member";
    const CODE: &[u8] = b"synthetic firmware code";
    const VARS: &[u8] = b"synthetic firmware vars";
    const DRIVER_RECEIPT: &[u8] = b"synthetic vetted driver provenance";
    const QEMU_RECEIPT: &[u8] = b"synthetic vetted qemu provenance";
    const EDITION: Edition<'static> = Edition {
        index: 6,
        name: "Fixture Windows 11",
        edition_id: "Fixture",
        build: "10.0.26100",
        language: "en-US",
        architecture: "amd64",
        container: InstallImage::Wim,
    };

    fn pin(bytes: &[u8]) -> AssetPin {
        AssetPin {
            length: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
        }
    }

    pub(in crate::vm::template) fn fixture() -> WindowsPins<'static> {
        WindowsPins {
            iso: pin(ISO),
            edition: EDITION,
            driver_medium: pin(MEDIUM),
            driver_files: [[pin(DRIVER); 3]; 3],
            driver_receipt: pin(DRIVER_RECEIPT),
            winpe_driver_letter: b'F',
            guest: pin(GUEST),
            provisioner: pin(PROVISIONER.as_bytes()),
            firmware_code: pin(CODE),
            firmware_vars: pin(VARS),
            qemu_receipt: pin(QEMU_RECEIPT),
        }
    }

    fn write(
        recipe: WindowsRecipe<'_>,
        change: impl FnOnce(&mut WindowsAssets<'_>),
        out: &mut Vec<u8>,
    ) -> Result<[u8; 32], RecipeError> {
        with_assets(|mut assets, guest| {
            change(&mut assets);
            recipe
                .write_test_seed(assets, guest, out)
                .map(|prepared| prepared.digest)
        })
    }

    pub(in crate::vm::template) fn with_assets<T>(
        use_assets: impl FnOnce(WindowsAssets<'_>, &[u8]) -> T,
    ) -> T {
        let mut files = [const { Cursor::new(DRIVER) }; 9];
        let [a, b, c, d, e, f, g, h, i] = &mut files;
        let assets = WindowsAssets {
            iso: &mut Cursor::new(ISO),
            edition: EDITION,
            driver_medium: &mut Cursor::new(MEDIUM),
            driver_files: [[a, b, c], [d, e, f], [g, h, i]],
            driver_receipt: DRIVER_RECEIPT,
            firmware_code: &mut Cursor::new(CODE),
            firmware_vars: &mut Cursor::new(VARS),
            qemu_receipt: QEMU_RECEIPT,
        };
        use_assets(assets, GUEST)
    }

    #[test]
    fn missing_or_wrong_windows_pins_refuse_before_seed_output() {
        let mut out = Vec::new();
        for (pins, update) in [
            (
                None,
                (|_: &mut WindowsAssets<'_>| {}) as fn(&mut WindowsAssets<'_>),
            ),
            (Some(fixture()), |a| a.edition.index += 1),
            (Some(fixture()), |a| a.edition.name = "unexpected edition"),
            (Some(fixture()), |a| a.edition.container = InstallImage::Esd),
            (Some(fixture()), |a| {
                a.driver_receipt = b"wrong trust evidence"
            }),
        ] {
            let error = write(WindowsRecipe { pins }, update, &mut out).unwrap_err();
            assert_eq!(error.to_string(), "recipe_unqualified");
            assert!(out.is_empty());
        }
        for alter in [
            (|p: &mut WindowsPins<'_>| p.iso.sha256[0] ^= 1) as fn(&mut WindowsPins<'_>),
            |p| p.iso.length -= 1,
            |p| p.driver_medium.sha256[0] ^= 1,
            |p| p.driver_files[2][1].sha256[0] ^= 1,
            |p| p.driver_files[1][2].length = MAX_DRIVER_FILE + 1,
            |p| p.firmware_vars.length = 0,
            |p| p.provisioner.sha256[0] ^= 1,
            |p| p.guest.sha256[0] ^= 1,
            |p| p.winpe_driver_letter = b'X',
        ] {
            let mut pins = fixture();
            alter(&mut pins);
            let error = write(WindowsRecipe { pins: Some(pins) }, |_| {}, &mut out).unwrap_err();
            assert_eq!(error.to_string(), "recipe_unqualified");
            assert!(out.is_empty());
        }
    }

    #[test]
    fn deadline_failure_residue_and_out_of_order_evidence_block_seal() {
        use Stage::*;
        let sequence = [
            Install,
            DetachedReadiness,
            Generalize,
            AttestGeneralization,
            DetachedVerification,
        ];
        for (broken, elapsed, stage, evidence, reason) in [
            (
                Install,
                BUILD_LIMIT,
                Install,
                StageEvidence::Clean,
                StageFailure::Timeout,
            ),
            (
                Generalize,
                Duration::ZERO,
                Generalize,
                StageEvidence::Failed,
                StageFailure::Failed,
            ),
            (
                DetachedVerification,
                Duration::ZERO,
                DetachedVerification,
                StageEvidence::Residue,
                StageFailure::Residue,
            ),
            (
                DetachedReadiness,
                Duration::ZERO,
                Generalize,
                StageEvidence::Clean,
                StageFailure::UnexpectedStage,
            ),
            (
                DetachedVerification,
                BUILD_LIMIT,
                DetachedVerification,
                StageEvidence::Clean,
                StageFailure::Timeout,
            ),
        ] {
            let mut build = BuildStages::default();
            for previous in sequence.into_iter().take_while(|stage| *stage != broken) {
                build
                    .observe(Duration::ZERO, previous, StageEvidence::Clean)
                    .unwrap();
            }
            assert_eq!(build.observe(elapsed, stage, evidence), Err(reason));
            assert!(!build.can_seal());
            assert_eq!(
                build.observe(Duration::ZERO, broken, StageEvidence::Clean),
                Err(reason)
            );
        }
        let mut build = BuildStages::default();
        for stage in sequence {
            assert!(!build.can_seal());
            build
                .observe(Duration::ZERO, stage, StageEvidence::Clean)
                .unwrap();
        }
        assert!(build.can_seal());
    }

    #[test]
    fn offline_setup_state_requires_both_generalization_witnesses() {
        for (registry, state_ini, verified) in [
            (GENERALIZED_OOBE, GENERALIZED_OOBE, true),
            (GENERALIZED_OOBE, "IMAGE_STATE_COMPLETE", false),
            ("IMAGE_STATE_COMPLETE", GENERALIZED_OOBE, false),
            ("IMAGE_STATE_UNDEPLOYABLE", "", false),
        ] {
            let state = OfflineSetupState {
                registry: registry.to_owned(),
                state_ini: state_ini.to_owned(),
            };
            assert_eq!(state.is_generalized(), verified);
        }
    }

    #[test]
    fn unrenderable_installed_image_pins_refuse_before_seed_output() {
        for edition in [
            Edition {
                build: "26100",
                ..EDITION
            },
            Edition {
                build: "10.0.26100.1.1",
                ..EDITION
            },
            Edition {
                build: "10.0.026100",
                ..EDITION
            },
            Edition {
                edition_id: "Fixture';exit 0",
                ..EDITION
            },
        ] {
            let mut output = Vec::new();
            let recipe = WindowsRecipe {
                pins: Some(WindowsPins {
                    edition,
                    ..fixture()
                }),
            };
            assert!(matches!(
                write(recipe, |assets| assets.edition = edition, &mut output),
                Err(RecipeError::Unqualified)
            ));
            assert!(output.is_empty());
        }
    }

    #[test]
    fn windows_seed_is_deterministic_and_independently_readable() {
        let mut iso = Vec::new();
        let mut again = Vec::new();
        let recipe = || WindowsRecipe {
            pins: Some(fixture()),
        };
        let digest = write(recipe(), |_| {}, &mut iso).unwrap();
        assert_eq!(digest, write(recipe(), |_| {}, &mut again).unwrap());
        assert_eq!(iso, again);
        assert_eq!(
            hex::encode(Sha256::digest(&iso)),
            "7577adbf9adb821162f9a0b533fee4da76b3d14cbbba167d09d6434cb619b61b"
        );
        assert!(
            iso.windows(b"Set-TestDefaultRoute".len())
                .any(|window| window == b"Set-TestDefaultRoute")
        );

        let adversarial = "&<>\"' é\t\u{1f642}\r\n";
        let mut xml = Xml("<test".to_owned());
        xml.attr("field", adversarial).unwrap();
        xml.0.push('>');
        xml.value("value", adversarial).unwrap();
        xml.0.push_str("</test>");
        for invalid in ["\u{1}", "\u{b}", "\u{fffe}"] {
            assert!(Xml::default().value("value", invalid).is_err());
            assert!(Xml::default().attr("field", invalid).is_err());
        }

        // CI already provides Python. ElementTree is independent of this
        // writer and verifies expanded names and real ISO read-back bytes.
        let mut check = Command::new("python")
            .args(["-I", "-B", "-c", CHECK_SEED, &xml.0])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Python 3 is needed for independent seed verification");
        check.stdin.take().unwrap().write_all(&iso).unwrap();
        let result = check.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    // Hang guard for a spawned PowerShell process, not a speed budget.
    #[cfg(windows)]
    const POWERSHELL_FIXTURE_HANG_GUARD: Duration = Duration::from_secs(120);

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_finalizer_and_health_probes_gate_publication() {
        use crate::{
            service::application::tests::private_temp,
            vm::template::lifecycle::tests::protect_fixture,
        };
        use tokio::io::AsyncWriteExt;

        let fixture = private_temp();
        protect_fixture(fixture.path(), true);
        let image = InstalledImage::from_edition(Edition {
            build: "10.0.26100.42",
            ..EDITION
        })
        .unwrap();
        // Only role names are changed by the pair slots. The probe can be
        // rendered by stage without any runtime path or identity.
        for (name, validation) in [("validation.ps1", true), ("verification.ps1", false)] {
            let path = fixture.path().join(name);
            std::fs::write(&path, image.probe(validation)).unwrap();
            protect_fixture(&path, false);
        }
        let mut check = tokio::process::Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "& ([ScriptBlock]::Create([Console]::In.ReadToEnd()))",
            ])
            .env("ARX_FIXTURE_ROOT", fixture.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let result = tokio::time::timeout(POWERSHELL_FIXTURE_HANG_GUARD, async move {
            let mut input = check.stdin.take().unwrap();
            let fixture =
                format!("$provisioner = @'\n{PROVISIONER}'@\n{CHECK_PROVISIONER}\n{CHECK_HEALTH}");
            input.write_all(fixture.as_bytes()).await.unwrap();
            drop(input);
            check.wait_with_output().await.unwrap()
        })
        .await
        .expect("PowerShell fixture exceeded its test budget");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[cfg(windows)]
    const CHECK_PROVISIONER: &str = r#"
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput($provisioner, [ref]$tokens, [ref]$errors)
if ($errors) { throw 'invalid provisioner syntax' }
# Load only the tested function definitions. All guest operations callable
# from these functions are stubbed or restricted to the test temp directory.
foreach ($name in 'Set-VerifiedRegistryValue','Set-GuestPowerShellLogging','Set-PortSecurity',
                  'Write-SetupPhase','Set-TestDefaultRoute','Finish-Setup') {
    $definition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $name
    }, $false)
    if (-not $definition) { throw 'missing provisioner function' }
    . ([ScriptBlock]::Create($definition.Extent.Text))
}

$script:system = $env:ARX_FIXTURE_ROOT
$script:sc = Join-Path $system 'sc.exe'
$script:state = Join-Path $system 'setup.phase'
$script:script = Join-Path $system 'arxsetup.ps1'
$script:setupKey = 'HKLM:\SYSTEM\Setup'
$script:task = 'ArxcovoTemplateFinalize'
$script:taskPresent = $true
$script:mediaAttached = $false
$script:cleanupFails = $false
$script:registryFault = ''
$script:boot = [DateTime]::new(2026, 1, 1, 0, 0, 0, [DateTimeKind]::Utc)
$script:registry = @{}
$script:tools = [Collections.Generic.List[string]]::new()
$script:expectedPolicy = @(
    'SOFTWARE\Policies\Microsoft\Windows\PowerShell\ScriptBlockLogging\EnableScriptBlockLogging',
    'SOFTWARE\Policies\Microsoft\Windows\PowerShell\ScriptBlockLogging\EnableScriptBlockInvocationLogging',
    'SOFTWARE\Policies\Microsoft\Windows\PowerShell\ModuleLogging\EnableModuleLogging',
    'SOFTWARE\Policies\Microsoft\Windows\PowerShell\Transcription\EnableTranscripting'
)
$script:serviceSid = 'S-1-5-80-1-2-3-4-5'
$script:savedRoute = $false
$script:routeWrites = 0
[IO.File]::WriteAllText($script, 'fixture')

function Expect($Condition, $Description) {
    if (-not $Condition) { throw $Description }
}
function Expect-Failure([string]$Message) {
    try { Finish-Setup } catch {
        Expect ($_.Exception.Message -ceq $Message) "unexpected failure: $($_.Exception.Message)"
        return
    }
    throw 'expected a failure'
}
function Open-GuestRegistryKey($Path) {
    $key = [pscustomobject]@{ Path = $Path }
    $key | Add-Member ScriptMethod SetValue {
        param($Name, $Value, $Kind)
        $script:registry["$($this.Path)\$Name"] = @{ Value = $Value; Kind = $Kind }
    }
    $key | Add-Member ScriptMethod GetValueKind {
        param($Name)
        if ($Name -ceq 'EnableTranscripting' -and $script:registryFault -ceq 'kind') { return 'String' }
        $script:registry["$($this.Path)\$Name"].Kind
    }
    $key | Add-Member ScriptMethod GetValue {
        param($Name)
        if ($Name -ceq 'EnableTranscripting' -and $script:registryFault -ceq 'value') { return 1 }
        ,$script:registry["$($this.Path)\$Name"].Value
    }
    $key | Add-Member ScriptMethod Flush {}
    $key | Add-Member ScriptMethod Close {}
    $key
}
function Get-GuestServiceSid { $script:serviceSid }
function Get-ItemPropertyValue {
    param($LiteralPath, $Name, $ErrorAction)
    if ($LiteralPath -ceq $script:setupKey -and $Name -cin 'SystemSetupInProgress','OOBEInProgress') {
        return 0
    }
    if ($LiteralPath -ceq 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup\State' -and
        $Name -ceq 'ImageState') { return 'IMAGE_STATE_COMPLETE' }
    throw 'unexpected Setup read'
}
function Get-CimInstance($ClassName, $Filter) {
    if ($ClassName -ceq 'Win32_NetworkAdapter' -and $Filter -ceq "ServiceName='netkvm'") {
        return [pscustomobject]@{ InterfaceIndex = 12 }
    }
    Expect ($ClassName -ceq 'Win32_OperatingSystem') 'wrong boot witness'
    @{ LastBootUpTime = $script:boot }
}
function Get-NetRoute($InterfaceIndex, $DestinationPrefix, $PolicyStore, $ErrorAction) {
    Expect ($InterfaceIndex -eq 12 -and $DestinationPrefix -ceq '0.0.0.0/0' -and
        $PolicyStore -ceq 'PersistentStore') 'wrong persistent route query'
    if ($script:savedRoute) { [pscustomobject]@{ NextHop = '10.0.2.2' } }
}
function New-NetRoute($InterfaceIndex, $DestinationPrefix, $NextHop, $RouteMetric) {
    Expect ($InterfaceIndex -eq 12 -and $DestinationPrefix -ceq '0.0.0.0/0' -and
        $NextHop -ceq '10.0.2.2' -and $RouteMetric -eq 4096) 'wrong new route'
    $script:savedRoute = $true
    $script:routeWrites++
}
function Assert-PlainPath($Path) { Expect ($Path -ceq $script:state) 'unexpected phase path' }
function Clear-Bootstrap {}
function Assert-NoBootstrap {}
function Clear-SetupCaches {
    if ($script:cleanupFails) { throw 'cache fixture' }
}
function Test-InstallerMediaAttached { $script:mediaAttached }
function Unregister-ScheduledTask {
    param($TaskName, [switch]$Confirm)
    Expect ($TaskName -ceq $script:task -and -not $Confirm) 'unexpected task removal'
    $script:taskPresent = $false
}
function Get-ScheduledTask($TaskName, $ErrorAction) {
    Expect ($TaskName -ceq $script:task) 'unexpected task read'
    if ($script:taskPresent) { $TaskName }
}
function Invoke-FixedTool($Program, [string[]]$Arguments) {
    $step = $Arguments -join ' '
    switch -CaseSensitive ($step) {
        '/s /t 0' {
            Expect ($Program -ceq (Join-Path $script:system 'shutdown.exe')) 'wrong shutdown executable'
            Expect ([IO.File]::ReadAllText($script:state) -ceq "1:$($script:boot.Ticks)") 'shutdown before phase 1'
        }
        { $_ -cin 'config arx-guest start= auto','start arx-guest' } {
            Expect ($Program -ceq $script:sc) 'wrong SCM executable'
            Expect ([IO.File]::ReadAllText($script:state) -ceq "2:$($script:boot.Ticks)") 'SCM before phase 2'
            foreach ($policy in $script:expectedPolicy) {
                $entry = $script:registry[$policy]
                Expect ($entry -and $entry.Kind -eq 'DWord' -and $entry.Value -ceq 0) 'SCM before policy'
            }
        }
        '/generalize /oobe /shutdown /quiet' {
            Expect ($Program -ceq (Join-Path $script:system 'Sysprep\Sysprep.exe')) 'wrong Sysprep executable'
            Expect (-not $script:taskPresent) 'Sysprep before task removal'
            Expect (-not ([IO.File]::Exists($script:state) -or [IO.File]::Exists($script:script))) 'Sysprep before file cleanup'
        }
        default { throw 'unexpected native command' }
    }
    $script:tools.Add($step)
}

$script:cleanupFails = $true
Expect-Failure 'cache fixture'
Expect ($tools.Count -eq 0 -and -not [IO.File]::Exists($state)) 'initial cleanup failure advanced phase'
$script:cleanupFails = $false
Finish-Setup
Expect ($tools.Count -eq 1 -and $registry.Count -eq 1) 'initial phase touched SCM or unexpected registry keys'
$entry = $registry['SYSTEM\CurrentControlSet\Control\Class\{6FDE7547-1B65-48AE-B628-80BE62016026}\Properties\Security']
Expect ($entry.Kind -eq 'Binary') 'raw port class security is not binary'
$descriptor = [Security.AccessControl.RawSecurityDescriptor]::new($entry.Value, 0)
$acl = $descriptor.DiscretionaryAcl
Expect ($descriptor.ControlFlags -band [Security.AccessControl.ControlFlags]::DiscretionaryAclProtected) 'port ACL inherits'
Expect ($acl.Count -eq 3 -and @($acl | Where-Object AceType -ne AccessAllowed).Count -eq 0) 'unexpected port ACL'
Expect ($acl[0].SecurityIdentifier.Value -ceq 'S-1-5-18' -and $acl[0].AccessMask -eq 0x10000000) 'missing SYSTEM grant'
Expect ($acl[1].SecurityIdentifier.Value -ceq 'S-1-5-32-544' -and $acl[1].AccessMask -eq 0x10000000) 'missing Administrators grant'
Expect ($acl[2].SecurityIdentifier.Value -ceq $serviceSid -and $acl[2].AccessMask -eq (-1073741824)) 'wrong service SID grant'
Expect-Failure 'stage'

$script:boot = $boot.AddSeconds(1)
$script:mediaAttached = $true
Expect-Failure 'media attached'
$script:mediaAttached = $false
foreach ($fault in 'kind','value') {
    $script:registryFault = $fault
    Expect-Failure "registry $fault"
    Expect ($tools.Count -eq 1 -and [IO.File]::ReadAllText($state).StartsWith('1:')) 'unverified policy advanced phase'
}
$script:registryFault = ''
Finish-Setup
Expect (($tools -join '|') -ceq '/s /t 0|config arx-guest start= auto|start arx-guest') 'unexpected startup sequence'
Expect ($routeWrites -eq 1 -and $savedRoute) 'route missing or rewritten'
Expect ($registry.Count -eq 5) 'unexpected registry keys'
Expect-Failure 'stage'

$script:boot = $boot.AddSeconds(1)
$script:cleanupFails = $true
Expect-Failure 'cache fixture'
Expect ($tools.Count -eq 3 -and $taskPresent -and [IO.File]::Exists($state)) 'cleanup failure advanced to Sysprep'
$script:cleanupFails = $false
Finish-Setup
Expect ($tools.Count -eq 4 -and $tools[3] -ceq '/generalize /oobe /shutdown /quiet') 'Sysprep command missing'
"#;

    #[cfg(windows)]
    const CHECK_HEALTH: &str = r#"
# Invoke the exact rendered .ps1 files. Each invocation exits back here
# with the status the guest would receive. All guest providers are mocked.
& {
    $env:SystemRoot = Join-Path $env:ARX_FIXTURE_ROOT 'guest-windows'
    $programs = Join-Path $env:ARX_FIXTURE_ROOT 'guest-programs'
    $phase = Join-Path $programs 'Arxcovo\setup.phase'
    $panther = Join-Path $env:SystemRoot 'Panther'
    $fixtureBoot = [DateTime]::new(2026, 1, 1, 0, 0, 0, [DateTimeKind]::Utc)
    $disk = [IO.Path]::GetPathRoot($env:SystemRoot)
    $verification = Join-Path $env:ARX_FIXTURE_ROOT 'verification.ps1'
    $validation = Join-Path $env:ARX_FIXTURE_ROOT 'validation.ps1'
    function Get-ItemProperty([string]$LiteralPath) {
        switch -Exact ($LiteralPath) {
            'HKLM:\SYSTEM\Setup' {
                $state = @{ SystemSetupInProgress = 0; OOBEInProgress = 0 }
                if ($case -eq 'answer override') { $state.UnattendFile = '' }
                return [pscustomobject]$state
            }
            'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' {
                $edition = if ($case -eq 'wrong edition') { 'AnotherSku' } else { 'Fixture' }
                $revision = if ($case -eq 'wrong revision') { 43 } else { 42 }
                return [pscustomobject]@{ EditionID = $edition; UBR = $revision }
            }
            'HKLM:\SYSTEM\CurrentControlSet\Services\arx-guest' {
                return [pscustomobject]@{ Start = 2; ObjectName = 'NT AUTHORITY\LocalService' }
            }
            'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon' {
                return [pscustomobject]@{}
            }
            default { throw 'unexpected registry read' }
        }
    }
    function Get-ItemPropertyValue([string]$LiteralPath, [string]$Name) {
        switch -Exact ("$LiteralPath|$Name") {
            'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup\State|ImageState' { 'IMAGE_STATE_COMPLETE' }
            'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion|ProgramFilesDir' { $programs }
            default { throw 'unexpected registry value read' }
        }
    }
    function Get-CimInstance([string]$ClassName, [string]$Filter) {
        switch -Exact ($ClassName) {
            'Win32_OperatingSystem' {
                $version = if ($case -eq 'wrong build') { '10.0.27000' } else { '10.0.26100' }
                return [pscustomobject]@{ Version = $version; LastBootUpTime = $fixtureBoot }
            }
            'Win32_CDROMDrive' {}
            'Win32_PnPEntity' {
                if ($Filter -cnotmatch "^Service = '(viostor|vioser|netkvm)'$") { throw 'unexpected driver' }
                return [pscustomobject]@{ ConfigManagerErrorCode = 0 }
            }
            default { throw 'unexpected CIM query' }
        }
    }
    function Get-LocalUser {
        if ($case -eq 'setup account') { return [pscustomobject]@{ Name = 'defaultuser0' } }
        if ($case -eq 'bootstrap account') { return [pscustomobject]@{ Name = 'ArxBootstrap' } }
    }
    function Get-NetAdapter { [pscustomobject]@{ Status = 'Up'; ifIndex = 12 } }
    function Get-NetRoute($InterfaceIndex, $AddressFamily, $DestinationPrefix) {
        if ($InterfaceIndex -ne 12 -or $AddressFamily -cne 'IPv4' -or $DestinationPrefix -cne '0.0.0.0/0') {
            throw 'unexpected route query'
        }
        if ($case -ne 'missing route') {
            $nextHop = if ($case -eq 'wrong gateway') { '10.0.2.1' } else { '10.0.2.2' }
            [pscustomobject]@{ NextHop = $nextHop }
        }
    }
    function Test-Path([string]$LiteralPath) {
        if ($LiteralPath -eq $phase) { return $case -eq 'finalizer marker' }
        if ($LiteralPath -eq (Join-Path $disk 'autounattend.xml')) { return $case -eq 'root answer' }
        if ($LiteralPath -eq (Join-Path $disk '$WINDOWS.~BT\Sources\Panther\unattend.xml')) { return $case -eq 'staging answer' }
        if ($LiteralPath -eq (Join-Path $disk 'Users\defaultuser0')) { return $case -eq 'setup profile' }
        if ($LiteralPath -eq $panther) { return $case -eq 'nested answer' }
        return $false
    }
    function Get-Item([string]$LiteralPath, [switch]$Force) {
        if ($LiteralPath -eq $phase) { return [pscustomobject]@{ Length = 20; Attributes = 0 } }
        if ($LiteralPath -eq $panther) { return [pscustomobject]@{ Attributes = 0 } }
        throw 'unexpected item read'
    }
    function Get-Content([string]$LiteralPath, [switch]$Raw) {
        if ($LiteralPath -ne $phase -or -not $Raw) { throw 'unexpected marker read' }
        if ($case -eq 'stale readiness') { return '2:0' }
        return "2:$($fixtureBoot.Ticks)"
    }
    function Get-ChildItem([string]$LiteralPath, [switch]$Force) {
        if ($LiteralPath -ne $panther -or -not $Force) { throw 'unexpected inventory' }
        return [pscustomobject]@{ Name = 'Unattend-copy.xml'; Attributes = 0; PSIsContainer = $false }
    }
    function Get-ScheduledTask {}
    function Check-Probe([string]$case, [string]$file, [int]$expected) {
        & $file
        if ($LASTEXITCODE -ne $expected) { throw "probe '$case' returned $LASTEXITCODE, expected $expected" }
    }
    Check-Probe 'clean readiness' $validation 0
    Check-Probe 'clean verification' $verification 0
    Check-Probe 'stale readiness' $validation 1
    foreach ($scenario in 'wrong build','wrong edition','wrong revision','answer override','setup account',
        'bootstrap account','root answer','staging answer','nested answer','setup profile','finalizer marker',
        'missing route','wrong gateway') {
        Check-Probe $scenario $verification 1
    }
}
exit 0
"#;

    const CHECK_SEED: &str = r#"
import hashlib
import sys
import xml.etree.ElementTree as ET

iso = sys.stdin.buffer.read()
sector = 2048
def both(raw):
    half = len(raw)//2
    little = int.from_bytes(raw[:half], 'little')
    assert little == int.from_bytes(raw[half:], 'big')
    return little

def contents(kind):
    desc = iso[(15+kind)*sector:(16+kind)*sector]
    assert desc[:7] == bytes([kind])+b'CD001\x01'
    assert both(desc[80:88])*sector == len(iso)
    label = desc[40:72].decode('ascii' if kind == 1 else 'utf-16-be')
    assert label.rstrip() == 'ARXSETUP'
    if kind == 2:
        assert desc[88:91] == b'%/E'
    root = both(desc[158:166])
    assert both(desc[166:174]) == sector
    tree = iso[root*sector:(root+1)*sector]
    files, intervals = {}, []
    step = 0
    while tree and tree[0]:
        n = tree[0]
        rec, tree = tree[:n], tree[n:]
        assert len(rec) == n and n >= 34
        raw = rec[33:33+rec[32]]
        step += 1
        if step <= 2:
            assert raw == bytes([step-1]) and rec[25] == 2
            continue
        assert rec[25] == 0
        key = raw.decode('ascii' if kind == 1 else 'utf-16-be')
        assert key.endswith(';1') and key not in files
        offset, size = both(rec[2:10])*sector, both(rec[10:18])
        end = offset + (size+sector-1)//sector*sector
        assert size > 0 and 25*sector <= offset < end <= len(iso)
        assert all(end <= lo or offset >= hi for lo, hi in intervals)
        assert not any(iso[offset+size:end])
        intervals.append((offset,end))
        files[key] = iso[offset:offset+size]
    assert not any(tree)
    return files

primary, joliet = contents(1), contents(2)
assert list(primary) == ['ARXGUEST.EXE;1','ARXPINS.TXT;1','ARXSEED.ID;1','ARXSETUP.PS1;1','AUTOUNAT.XML;1']
assert list(joliet) == ['arxguest.exe;1','arxpins.txt;1','arxseed.id;1','arxsetup.ps1;1','Autounattend.xml;1']
assert list(primary.values()) == list(joliet.values())
guest, manifest, marker, script, answer = joliet.values()
sha = lambda b: hashlib.sha256(b).hexdigest().upper()
assert guest == b'synthetic windows guest'
assert marker == b'arxcovo-windows-test-seed-v1\n'
assert manifest.decode('ascii').splitlines() == ['arx-windows-seed-v1',str(len(guest)),sha(guest),sha(script)]
n = '{urn:schemas-microsoft-com:unattend}'
w = '{http://schemas.microsoft.com/WMIConfig/2002/State}'
root = ET.fromstring(answer)
assert root.tag == n+'unattend'
assert [s.attrib['pass'] for s in root] == ['windowsPE','specialize','oobeSystem']
pe, specialize, oobe = [list(s) for s in root]
assert [[c.attrib['name'] for c in stage] for stage in [pe,specialize,oobe]] == [
    ['Microsoft-Windows-International-Core-WinPE','Microsoft-Windows-PnpCustomizationsWinPE','Microsoft-Windows-Setup'],
    ['Microsoft-Windows-Deployment'],
    ['Microsoft-Windows-International-Core','Microsoft-Windows-Shell-Setup']]
assert all(c.attrib['processorArchitecture'] == 'amd64' for c in pe+specialize+oobe)
get = lambda e, name: e.findtext('.//'+n+name)
drivers = pe[1].find(n+'DriverPaths')
assert [d.attrib for d in drivers] == [{w+'action':'add',w+'keyValue':str(i)} for i in range(1,4)]
assert [get(d,'Path') for d in drivers] == [r'F:\viostor\w11\amd64',r'F:\vioserial\w11\amd64',r'F:\NetKVM\w11\amd64']
assert get(pe[2],'Key') == '/IMAGE/INDEX' and get(pe[2],'Value') == '6'
assert [x.text for x in pe[2].iter(n+'Type')] == ['EFI','MSR','Primary']
assert [x.text for x in pe[2].iter(n+'Format')] == ['FAT32','NTFS']
assert get(pe[2],'DiskID') == '0' and get(pe[2],'Enable') == 'false'
setup_paths = [get(c,'Path') for c in pe[2].find(n+'RunSynchronous')]
assert setup_paths[1:] == [
    r'reg.exe add HKLM\SYSTEM\Setup\LabConfig /v BypassTPMCheck /t REG_DWORD /d 1 /f',
    r'reg.exe add HKLM\SYSTEM\Setup\LabConfig /v BypassSecureBootCheck /t REG_DWORD /d 1 /f']
assert answer.count(b'LabConfig') == 2
for name in ['viostor', 'vioser', 'netkvm']:
    for suffix in ['.inf','.cat','.sys']:
        assert name+suffix in setup_paths[0]
hooks = [get(c,'Path') for c in specialize[0].find(n+'RunSynchronous')]
assert len(hooks) == 4 and all(len(hook) <= 259 for hook in hooks)
assert sha(manifest) in hooks[-2] and sha(script) in hooks[-1]
assert hooks[-1].endswith(';& $p install"')
assert b'[Microsoft.Win32.Registry]::LocalMachine.CreateSubKey($Path)' in script
assert rb"$policy = 'SOFTWARE\Policies\Microsoft\Windows\PowerShell'" in script
assert [line.strip() for line in script.splitlines() if b'Set-VerifiedRegistryValue "$policy' in line] == [
    rb'Set-VerifiedRegistryValue "$policy\ScriptBlockLogging" EnableScriptBlockLogging DWord 0',
    rb'Set-VerifiedRegistryValue "$policy\ScriptBlockLogging" EnableScriptBlockInvocationLogging DWord 0',
    rb'Set-VerifiedRegistryValue "$policy\ModuleLogging" EnableModuleLogging DWord 0',
    rb'Set-VerifiedRegistryValue "$policy\Transcription" EnableTranscripting DWord 0']
assert rb"SYSTEM\CurrentControlSet\Control\Class\{6FDE7547-1B65-48AE-B628-80BE62016026}\Properties" in script
assert b'Set-VerifiedRegistryValue $class Security Binary $bytes' in script
assert get(oobe[1],'HideOnlineAccountScreens') == 'true'
assert get(oobe[1],'HideWirelessSetupInOOBE') == 'true'
assert get(oobe[1],'ProtectYourPC') == '3'
assert get(oobe[1],'Name') == 'ArxBootstrap' and get(oobe[1],'Group') == 'Users'
assert not {x.tag.split('}')[-1] for x in root.iter()} & {
    'ProductKey','Password','Credentials','AutoLogon','FirstLogonCommands',
    'JoinDomain','SkipMachineOOBE','SkipUserOOBE','HideLocalAccountScreen'}
assert all(not v.startswith('\\\\') for v in root.itertext())
assert b'FirstLogon' not in script and b'Write-Host' not in script
assert script.index(b'Clear-Bootstrap\n        Clear-SetupCaches') < script.index(b"'/generalize','/oobe','/shutdown'")
extra = ET.fromstring(sys.argv[1])
assert extra.attrib == {'field':"&<>\"' é\t\U0001f642\r\n"}
assert extra.findtext('value') == extra.attrib['field']
"#;
}
