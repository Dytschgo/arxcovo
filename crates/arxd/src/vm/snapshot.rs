//! Bounded pair preparation under offline custody. The controller owns
//! admission and publication. Native custody issuance is closed.
use super::{
    DiskClaim, Id, PendingOperation,
    paths::{Artifact, ArtifactPin, ArtifactRole, FileIdentity, LocalPath},
    qemu_args::{SnapshotDiskRole, SnapshotInfo},
    qemu_img::{self, CLUSTER, MAX_INFO},
    runtime::{StopDisposition, snapshot_deadline},
};
use sha2::{Digest, Sha256};
use std::time::Instant;

pub const POINTS_PER_VM: u32 = 4;
pub const MANUAL_STATIC_POINTS: u32 = 3;
pub const POINTS_PER_HOST: u32 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    Bounds,
    Limit,
    Disk,
    ReleaseUnverified,
    DirtyCapture,
    Compatibility,
    Identity,
    Pin,
    VarsPair,
    Header,
    Inspection,
    Custody,
    Io,
    Cancelled,
    Deadline,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotName(String);

impl SnapshotName {
    pub fn parse(name: &str) -> Result<Self, Refusal> {
        if !(1..=48).contains(&name.len())
            || !name
                .split('-')
                .all(|word| !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            return Err(Refusal::Bounds);
        }
        Ok(Self(name.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Golden {
    pub object: Id,
    pub path: LocalPath,
    pub identity: FileIdentity,
    pub pin: ArtifactPin,
}

#[derive(Clone, Eq, PartialEq)]
pub enum Lineage {
    Test { template: Id, golden: Golden },
    Static { origin: Id },
}

#[derive(Clone, Eq, PartialEq)]
pub struct Compatibility {
    pub host: Id,
    pub vm: Id,
    pub lineage: Lineage,
    pub recipe: [u8; 32],
    pub firmware: ArtifactPin,
    pub virtual_disk_bytes: u64,
    pub vars_bytes: u64,
}

#[derive(Clone)]
pub struct Leaf {
    pub key: Id,
    pub path: LocalPath,
    pub identity: FileIdentity,
    pub pin: ArtifactPin,
}

#[derive(Clone)]
pub struct StoredPair {
    pub compatibility: Compatibility,
    pub generation: Id,
    pub disk: Leaf,
    pub vars: Leaf,
}

/// Only evidence for the exact active generation and its last writer qualifies.
/// A sealed initial generation can report CleanRelease with the same proof.
pub struct ActiveRelease {
    pub generation: Id,
    pub writer_epoch: Id,
    pub disposition: StopDisposition,
    pub endpoints_released: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Action {
    Capture { snapshot: Id },
    Restore { snapshot: Id },
}

#[derive(Clone, Copy)]
pub struct Destination {
    pub generation: Id,
    pub disk_key: Id,
    pub vars_key: Id,
}

impl Destination {
    /// Generate independent keys. The eventual store must also reject reuse
    /// against durable intents and committed/quarantined objects.
    pub fn generate() -> Result<Self, getrandom::Error> {
        fn key() -> Result<Id, getrandom::Error> {
            let mut bytes = [0; 16];
            getrandom::fill(&mut bytes)?;
            Ok(bytes)
        }
        Ok(Self {
            generation: key()?,
            disk_key: key()?,
            vars_key: key()?,
        })
    }
}

pub struct Intent {
    pub activity_id: i64,
    pub action: Action,
    pub active: StoredPair,
    pub active_writer_epoch: Id,
    pub release: ActiveRelease,
    /// The caller's ready sealed record for this snapshot ID, if restoring.
    pub selected: Option<(Id, StoredPair)>,
    pub destination: Destination,
}

/// Daemon-selected bounds and transactionally observed counts. Counts
/// include unresolved and protected points. Headroom is checked separately.
pub struct Limits {
    pub vm_points: u32,
    pub host_points: u32,
    pub max_file_bytes: u64,
    pub max_virtual_bytes: u64,
    /// Additional writable qcow2 space for a restored active generation.
    pub restore_growth_bytes: u64,
    pub staging_bytes: u64,
    pub metadata_bytes: u64,
}

pub struct CopyPlan {
    intent: Intent,
    volume: u64,
    destination_paths: [LocalPath; 2],
    max_file_bytes: u64,
    claim: DiskClaim,
    coexistence_bytes: u64,
}

impl CopyPlan {
    pub fn new(
        intent: Intent,
        root: &LocalPath,
        volume: u64,
        limits: Limits,
    ) -> Result<Self, Refusal> {
        let active = &intent.active;
        let compatibility = &active.compatibility;
        let is_static = matches!(compatibility.lineage, Lineage::Static { .. });
        let capture = matches!(intent.action, Action::Capture { .. });
        let snapshot = match intent.action {
            Action::Capture { snapshot } | Action::Restore { snapshot } => snapshot,
        };
        if intent.activity_id <= 0 || snapshot == [0; 16] || root.is_drive_root() {
            return Err(Refusal::Bounds);
        }
        if intent.release.generation != active.generation
            || intent.release.writer_epoch != intent.active_writer_epoch
            || !intent.release.endpoints_released
            || intent.release.disposition == StopDisposition::ChargedQuarantine
        {
            return Err(Refusal::ReleaseUnverified);
        }
        if capture && intent.release.disposition != StopDisposition::CleanRelease {
            return Err(Refusal::DirtyCapture);
        }
        let point_limit = if is_static && capture {
            MANUAL_STATIC_POINTS
        } else {
            POINTS_PER_VM
        };
        let new_point = u32::from(capture || is_static);
        if limits.host_points < limits.vm_points
            || limits.vm_points.saturating_add(new_point) > point_limit
            || limits.host_points.saturating_add(new_point) > POINTS_PER_HOST
        {
            return Err(Refusal::Limit);
        }
        let selected = match (&intent.selected, capture) {
            (None, true) => None,
            (Some((id, pair)), false)
                if *id == snapshot && &pair.compatibility == compatibility =>
            {
                Some(pair)
            }
            _ => return Err(Refusal::Compatibility),
        };
        if compatibility.firmware.size == 0
            || compatibility.vars_bytes == 0
            || compatibility.virtual_disk_bytes == 0
            || compatibility.virtual_disk_bytes > limits.max_virtual_bytes
        {
            return Err(Refusal::Bounds);
        }
        let dest = intent.destination;
        let dir = root
            .join(&hex::encode(dest.generation))
            .map_err(|_| Refusal::Bounds)?;
        let disk_path = dir
            .join(&format!("disk-{}", hex::encode(dest.disk_key)))
            .map_err(|_| Refusal::Bounds)?;
        let vars_path = dir
            .join(&format!("vars-{}", hex::encode(dest.vars_key)))
            .map_err(|_| Refusal::Bounds)?;
        let mut keys = vec![snapshot, dest.generation, dest.disk_key, dest.vars_key];
        let mut paths = vec![&disk_path, &vars_path];
        let mut ids = Vec::new();
        for pair in std::iter::once(active).chain(selected) {
            if pair.vars.pin.size != compatibility.vars_bytes {
                return Err(Refusal::VarsPair);
            }
            if pair.disk.pin.size < CLUSTER as u64
                || pair.disk.pin.size > limits.max_file_bytes
                || pair.vars.pin.size > limits.max_file_bytes
            {
                return Err(Refusal::Bounds);
            }
            keys.extend([pair.generation, pair.disk.key, pair.vars.key]);
            paths.extend([&pair.disk.path, &pair.vars.path]);
            ids.extend([pair.disk.identity, pair.vars.identity]);
        }
        if let Lineage::Test { golden, .. } = &compatibility.lineage {
            if golden.pin.size < CLUSTER as u64 || golden.pin.size > limits.max_file_bytes {
                return Err(Refusal::Bounds);
            }
            keys.push(golden.object);
            paths.push(&golden.path);
            ids.push(golden.identity);
        }
        if keys
            .iter()
            .enumerate()
            .any(|(i, key)| *key == [0; 16] || keys[..i].contains(key))
            || paths.iter().enumerate().any(|(i, path)| {
                !root.contains(path) || paths[..i].iter().any(|prior| path.overlaps(prior))
            })
            || paths[2..].iter().any(|path| dir.contains(path))
            || ids
                .iter()
                .enumerate()
                .any(|(i, id)| id.volume != volume || id.file == [0; 16] || ids[..i].contains(id))
        {
            return Err(Refusal::Identity);
        }
        let source = selected.unwrap_or(active);
        let pair_bytes = |pair: &StoredPair| pair.disk.pin.size.checked_add(pair.vars.pin.size);
        let active_bytes = pair_bytes(active).ok_or(Refusal::Disk)?;
        let copy_bytes = pair_bytes(source).ok_or(Refusal::Disk)?;
        if !capture && limits.restore_growth_bytes == 0 {
            return Err(Refusal::Disk);
        }
        let claim = DiskClaim {
            images: copy_bytes
                .checked_add(if capture {
                    0
                } else {
                    limits.restore_growth_bytes
                })
                .ok_or(Refusal::Disk)?,
            staging: limits.staging_bytes,
            metadata: limits.metadata_bytes,
        };
        let coexistence_bytes = claim
            .total()
            .and_then(|total| total.checked_add(active_bytes))
            .and_then(|total| total.checked_add(if capture { 0 } else { copy_bytes }))
            .ok_or(Refusal::Disk)?;
        Ok(Self {
            intent,
            volume,
            destination_paths: [disk_path, vars_path],
            max_file_bytes: limits.max_file_bytes,
            claim,
            coexistence_bytes,
        })
    }

    pub fn intent(&self) -> &Intent {
        &self.intent
    }

    pub fn volume(&self) -> u64 {
        self.volume
    }

    pub fn destination_paths(&self) -> &[LocalPath; 2] {
        &self.destination_paths
    }

    /// Incremental reservation for VmPolicy::disk. Active and selected
    /// source claims remain independently charged. Golden charge is shared.
    pub fn additional_claim(&self) -> DiskClaim {
        self.claim
    }

    pub fn coexistence_bytes(&self) -> u64 {
        self.coexistence_bytes
    }

    pub fn artifact_role(&self, side: Side, file: FileRole) -> ArtifactRole {
        use ArtifactRole::*;
        match (side, file) {
            (Side::Staging, FileRole::Disk) => SnapshotStagingDisk,
            (Side::Staging, FileRole::Vars) => SnapshotStagingVars,
            (Side::Source, FileRole::Disk) if self.intent.selected.is_some() => SnapshotDisk,
            (Side::Source, FileRole::Vars) if self.intent.selected.is_some() => SnapshotVars,
            (_, FileRole::Disk) if self.golden().is_some() => GuestDisk,
            (_, FileRole::Vars) if self.golden().is_some() => Vars,
            (_, FileRole::Disk) => StaticDisk,
            (_, FileRole::Vars) => StaticVars,
        }
    }

    fn source(&self) -> &StoredPair {
        self.intent
            .selected
            .as_ref()
            .map_or(&self.intent.active, |(_, pair)| pair)
    }

    fn golden(&self) -> Option<&Golden> {
        match &self.intent.active.compatibility.lineage {
            Lineage::Test { golden, .. } => Some(golden),
            Lineage::Static { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    Active,
    Source,
    Staging,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileRole {
    Disk,
    Vars,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub identity: FileIdentity,
    pub size: u64,
}

/// The adapter retains handles from exclusive creation/open through
/// settlement or quarantine. Native implementations must reject reparse
/// points and hard links and check the by-name open against the held handle.
/// Calls must be deadline-bounded. A timeout is not proof of release.
pub trait CopyCustody {
    /// Recheck exact live worker/lease ownership and pinned volume capacity
    /// (caller-available and total free space, claims, and reserve).
    fn checkpoint(
        &mut self,
        plan: &CopyPlan,
        worker: &PendingOperation,
        deadline: Instant,
    ) -> Result<(), Refusal>;

    fn observe(&mut self, side: Side, deadline: Instant) -> Result<[Observation; 2], Refusal>;

    /// Read one exact range from the retained object or fail.
    fn read(
        &mut self,
        side: Side,
        file: FileRole,
        offset: u64,
        bytes: &mut [u8],
        deadline: Instant,
    ) -> Result<(), Refusal>;

    /// Write one exact range to the never-activated object or fail.
    fn write_staging(
        &mut self,
        file: FileRole,
        offset: u64,
        bytes: &[u8],
        deadline: Instant,
    ) -> Result<(), Refusal>;

    /// Verify retained, sealed golden identity and pin from immutable
    /// custody and return its first cluster. No unbounded rehash here.
    fn golden_header(
        &mut self,
        golden: &Golden,
        header: &mut [u8],
        deadline: Instant,
    ) -> Result<(), Refusal>;

    /// Execute only the sealed read-only helper. Enforce the output bound
    /// and require exit success, Job-empty, joined workers and custody
    /// revalidation before returning an output length.
    fn inspect(
        &mut self,
        info: &SnapshotInfo<'_>,
        output: &mut [u8],
        deadline: Instant,
    ) -> Result<usize, Refusal>;

    /// Flush both files and prove durable creation of their directory and
    /// objects. On failure they remain charged under the existing intent.
    fn flush_staging(&mut self, deadline: Instant) -> Result<(), Refusal>;
}

struct CopyRun<'a> {
    plan: &'a CopyPlan,
    worker: &'a PendingOperation,
    deadline: Instant,
    cancelled: &'a dyn Fn() -> bool,
    now: &'a dyn Fn() -> Instant,
}

impl CopyRun<'_> {
    fn current(&self) -> Result<(), Refusal> {
        if (self.cancelled)() {
            return Err(Refusal::Cancelled);
        }
        if (self.now)() >= self.deadline {
            return Err(Refusal::Deadline);
        }
        Ok(())
    }

    fn checkpoint(&self, custody: &mut dyn CopyCustody) -> Result<(), Refusal> {
        self.current()?;
        custody.checkpoint(self.plan, self.worker, self.deadline)?;
        self.current()
    }

    fn check_objects(
        &self,
        custody: &mut dyn CopyCustody,
        side: Side,
        expected: [Observation; 2],
    ) -> Result<(), Refusal> {
        self.checkpoint(custody)?;
        let observed = custody.observe(side, self.deadline)?;
        if observed.map(|file| file.identity) != expected.map(|file| file.identity) {
            return Err(Refusal::Identity);
        }
        if observed[0].size != expected[0].size {
            return Err(Refusal::Pin);
        }
        if observed[1].size != expected[1].size {
            return Err(Refusal::VarsPair);
        }
        self.current()
    }

    fn digest(
        &self,
        custody: &mut dyn CopyCustody,
        phase: Stream,
        file: FileRole,
        pin: ArtifactPin,
        buffer: &mut [u8],
    ) -> Result<(), Refusal> {
        let mut hash = Sha256::new();
        let mut offset = 0;
        while offset < pin.size {
            self.checkpoint(custody)?;
            let take = (pin.size - offset).min(buffer.len() as u64) as usize;
            let bytes = &mut buffer[..take];
            custody.read(phase.side(), file, offset, bytes, self.deadline)?;
            hash.update(&*bytes);
            if phase == Stream::Copy {
                self.checkpoint(custody)?;
                custody.write_staging(file, offset, bytes, self.deadline)?;
            }
            offset += take as u64;
        }
        self.current()?;
        if hash.finalize()[..] != pin.sha256 {
            return Err(match file {
                FileRole::Disk => Refusal::Pin,
                FileRole::Vars => Refusal::VarsPair,
            });
        }
        Ok(())
    }

    fn inspect(
        &self,
        custody: &mut dyn CopyCustody,
        side: Side,
        disk: &Artifact,
        role: SnapshotDiskRole,
        header: &mut [u8],
    ) -> Result<(), Refusal> {
        let golden = self.plan.golden();
        let mut backing_header = vec![0; if golden.is_some() { CLUSTER } else { 0 }];
        let backing = golden
            .map(|golden| {
                self.checkpoint(custody)?;
                custody.golden_header(golden, &mut backing_header, self.deadline)?;
                artifact(
                    ArtifactRole::Golden,
                    &golden.path,
                    golden.identity,
                    golden.pin,
                )
            })
            .transpose()?;
        self.checkpoint(custody)?;
        custody.read(side, FileRole::Disk, 0, header, self.deadline)?;
        let size = self.plan.source().compatibility.virtual_disk_bytes;
        let info = SnapshotInfo::checked(
            disk,
            role,
            header,
            backing.as_ref().map(|a| (a, backing_header.as_slice())),
            size,
        )
        .map_err(|_| Refusal::Header)?;
        self.checkpoint(custody)?;
        let mut output = vec![0; MAX_INFO];
        let len = custody.inspect(&info, &mut output, self.deadline)?;
        self.current()?;
        qemu_img::parse_info(
            output.get(..len).ok_or(Refusal::Inspection)?,
            info.path(),
            size,
            self.plan.max_file_bytes,
            golden.map(|golden| &golden.path),
        )
        .map_err(|_| Refusal::Inspection)?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stream {
    VerifySource,
    Copy,
    VerifyStaging,
}

impl Stream {
    fn side(self) -> Side {
        match self {
            Self::VerifySource | Self::Copy => Side::Source,
            Self::VerifyStaging => Side::Staging,
        }
    }
}

fn artifact(
    role: ArtifactRole,
    path: &LocalPath,
    identity: FileIdentity,
    pin: ArtifactPin,
) -> Result<Artifact, Refusal> {
    Artifact::new(role, path.clone())
        .map(|artifact| artifact.with_identity(identity).with_pin(pin))
        .map_err(|_| Refusal::Bounds)
}

fn observations(pair: &StoredPair) -> [Observation; 2] {
    [&pair.disk, &pair.vars].map(|leaf| Observation {
        identity: leaf.identity,
        size: leaf.pin.size,
    })
}

/// A sealed result for one live worker. The custody borrow continues across
/// the caller's eventual settlement transaction.
pub struct VerifiedCopy<'a> {
    run: CopyRun<'a>,
    custody: &'a mut dyn CopyCustody,
    pair: StoredPair,
}

impl VerifiedCopy<'_> {
    pub fn pair(&self) -> &StoredPair {
        &self.pair
    }

    /// Revalidate immediately before settlement. The transaction must still
    /// compare the durable operation, active and selected snapshot records.
    pub fn recheck(&mut self) -> Result<(), Refusal> {
        self.run.check_objects(
            self.custody,
            Side::Active,
            observations(&self.run.plan.intent.active),
        )?;
        self.run.check_objects(
            self.custody,
            Side::Source,
            observations(self.run.plan.source()),
        )?;
        self.run
            .check_objects(self.custody, Side::Staging, observations(&self.pair))
    }
}

/// Verify and copy after both staging identities were recorded by the owning
/// worker. Reads at most 4x the source pair length plus up to four header
/// clusters. Writes at most one pair with no file-size-dependent allocation.
/// On refusal the operation retains its slots and claims for reconciliation.
pub fn copy_pair<'a>(
    plan: &'a CopyPlan,
    worker: &'a PendingOperation,
    custody: &'a mut dyn CopyCustody,
    staging: [FileIdentity; 2],
    cancelled: &'a dyn Fn() -> bool,
) -> Result<VerifiedCopy<'a>, Refusal> {
    copy_with_clock(plan, worker, custody, staging, cancelled, &Instant::now)
}

fn copy_with_clock<'a>(
    plan: &'a CopyPlan,
    worker: &'a PendingOperation,
    custody: &'a mut dyn CopyCustody,
    staging: [FileIdentity; 2],
    cancelled: &'a dyn Fn() -> bool,
    now: &'a dyn Fn() -> Instant,
) -> Result<VerifiedCopy<'a>, Refusal> {
    if plan.intent.activity_id != worker.activity_id() {
        return Err(Refusal::Custody);
    }
    let run = CopyRun {
        plan,
        worker,
        deadline: snapshot_deadline(worker, now()),
        cancelled,
        now,
    };
    let source = plan.source();
    let active = &plan.intent.active;
    let source_objects = observations(source);
    let active_objects = observations(active);
    if staging[0] == staging[1]
        || staging.iter().any(|id| {
            id.volume != plan.volume
                || id.file == [0; 16]
                || active_objects
                    .iter()
                    .chain(&source_objects)
                    .any(|existing| &existing.identity == id)
                || plan.golden().is_some_and(|golden| &golden.identity == id)
        })
    {
        return Err(Refusal::Identity);
    }
    run.check_objects(custody, Side::Active, active_objects)?;
    run.check_objects(custody, Side::Source, source_objects)?;
    run.check_objects(
        custody,
        Side::Staging,
        staging.map(|identity| Observation { identity, size: 0 }),
    )?;
    let mut buffer = vec![0; CLUSTER];
    let position = match plan.intent.action {
        Action::Restore { .. } => SnapshotDiskRole::Backup,
        Action::Capture { .. } => SnapshotDiskRole::Active,
    };
    let source_disk = artifact(
        plan.artifact_role(Side::Source, FileRole::Disk),
        &source.disk.path,
        source.disk.identity,
        source.disk.pin,
    )?;
    let leaves = [
        (FileRole::Disk, source.disk.pin),
        (FileRole::Vars, source.vars.pin),
    ];
    for (file, pin) in leaves {
        run.digest(custody, Stream::VerifySource, file, pin, &mut buffer)?;
    }
    run.inspect(custody, Side::Source, &source_disk, position, &mut buffer)?;
    run.check_objects(custody, Side::Source, source_objects)?;
    for (file, pin) in leaves {
        run.digest(custody, Stream::Copy, file, pin, &mut buffer)?;
    }
    run.checkpoint(custody)?;
    custody.flush_staging(run.deadline)?;
    for phase in [Stream::VerifySource, Stream::VerifyStaging] {
        for (file, pin) in leaves {
            run.digest(custody, phase, file, pin, &mut buffer)?;
        }
    }
    let dest = plan.intent.destination;
    let pair = StoredPair {
        compatibility: source.compatibility.clone(),
        generation: dest.generation,
        disk: Leaf {
            key: dest.disk_key,
            path: plan.destination_paths[0].clone(),
            identity: staging[0],
            pin: source.disk.pin,
        },
        vars: Leaf {
            key: dest.vars_key,
            path: plan.destination_paths[1].clone(),
            identity: staging[1],
            pin: source.vars.pin,
        },
    };
    let disk = artifact(
        plan.artifact_role(Side::Staging, FileRole::Disk),
        &pair.disk.path,
        pair.disk.identity,
        pair.disk.pin,
    )?;
    run.check_objects(custody, Side::Staging, observations(&pair))?;
    run.inspect(
        custody,
        Side::Staging,
        &disk,
        SnapshotDiskRole::Staging,
        &mut buffer,
    )?;
    let mut verified = VerifiedCopy { run, custody, pair };
    verified.recheck()?;
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::runtime::SNAPSHOT_COPY_MAXIMUM;
    use serde_json::json;
    use std::{cell::Cell, sync::Arc, time::Duration};

    const VOLUME: u64 = 42;
    const VIRTUAL: u64 = 4 * CLUSTER as u64;
    const VARS: &[u8] = b"paired VARS";
    const POINT: Id = [21; 16];

    fn key(n: u8) -> Id {
        [n; 16]
    }

    fn id(n: u8) -> FileIdentity {
        FileIdentity {
            volume: VOLUME,
            file: key(n),
        }
    }

    fn root() -> LocalPath {
        LocalPath::parse(r"D:\managed").unwrap()
    }

    fn header(backing: Option<&LocalPath>) -> Vec<u8> {
        let mut bytes = vec![0; CLUSTER];
        bytes[..4].copy_from_slice(b"QFI\xfb");
        bytes[4..8].copy_from_slice(&3_u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&16_u32.to_be_bytes());
        bytes[24..32].copy_from_slice(&VIRTUAL.to_be_bytes());
        bytes[96..100].copy_from_slice(&4_u32.to_be_bytes());
        bytes[100..104].copy_from_slice(&104_u32.to_be_bytes());
        if let Some(backing) = backing {
            let path = backing.as_str().as_bytes();
            bytes[8..16].copy_from_slice(&128_u64.to_be_bytes());
            bytes[16..20].copy_from_slice(&(path.len() as u32).to_be_bytes());
            bytes[104..108].copy_from_slice(&0xe279_2aca_u32.to_be_bytes());
            bytes[108..112].copy_from_slice(&5_u32.to_be_bytes());
            bytes[112..117].copy_from_slice(b"qcow2");
            bytes[128..128 + path.len()].copy_from_slice(path);
        }
        bytes
    }

    fn pin(bytes: &[u8]) -> ArtifactPin {
        ArtifactPin {
            size: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
        }
    }

    fn limits() -> Limits {
        Limits {
            vm_points: 1,
            host_points: 1,
            max_file_bytes: 8 * CLUSTER as u64,
            max_virtual_bytes: VIRTUAL,
            restore_growth_bytes: 3 * CLUSTER as u64,
            staging_bytes: 1024,
            metadata_bytes: 4096,
        }
    }

    fn stored(compatibility: &Compatibility, n: u8, disk: &[u8]) -> StoredPair {
        let dir = root().join(&format!("generation-{n}")).unwrap();
        StoredPair {
            compatibility: compatibility.clone(),
            generation: key(n),
            disk: Leaf {
                key: key(n + 1),
                path: dir.join("disk").unwrap(),
                identity: id(n + 1),
                pin: pin(disk),
            },
            vars: Leaf {
                key: key(n + 2),
                path: dir.join("vars").unwrap(),
                identity: id(n + 2),
                pin: pin(VARS),
            },
        }
    }

    fn intent(static_vm: bool, restore: bool) -> (Intent, Vec<u8>) {
        let golden_path = root().join("golden").unwrap();
        let compatibility = Compatibility {
            host: key(1),
            vm: key(2),
            lineage: if static_vm {
                Lineage::Static { origin: key(3) }
            } else {
                Lineage::Test {
                    template: key(3),
                    golden: Golden {
                        object: key(4),
                        path: golden_path.clone(),
                        identity: id(4),
                        pin: pin(&header(None)),
                    },
                }
            },
            recipe: [5; 32],
            firmware: pin(b"firmware"),
            virtual_disk_bytes: VIRTUAL,
            vars_bytes: VARS.len() as u64,
        };
        let mut disk = header((!static_vm).then_some(&golden_path));
        disk.extend_from_slice(b"partial last cluster");
        let active = stored(&compatibility, 6, &disk);
        let intent = Intent {
            activity_id: 1,
            action: if restore {
                Action::Restore { snapshot: POINT }
            } else {
                Action::Capture { snapshot: POINT }
            },
            active_writer_epoch: key(9),
            release: ActiveRelease {
                generation: active.generation,
                writer_epoch: key(9),
                disposition: if restore {
                    StopDisposition::DirtyRelease
                } else {
                    StopDisposition::CleanRelease
                },
                endpoints_released: true,
            },
            active,
            selected: restore.then(|| (POINT, stored(&compatibility, 10, &disk))),
            destination: Destination {
                generation: key(15),
                disk_key: key(16),
                vars_key: key(17),
            },
        };
        (intent, disk)
    }

    fn build(intent: Intent, limits: Limits) -> Result<CopyPlan, Refusal> {
        CopyPlan::new(intent, &root(), VOLUME, limits)
    }

    #[test]
    fn plan_enforces_names_counts_lengths_and_offline_evidence() {
        assert_eq!(
            SnapshotName::parse("Before-1").unwrap().as_str(),
            "before-1"
        );
        for invalid in ["", "é", "-a", "a--b", "a-", "a b", "a_b", &"a".repeat(49)] {
            assert_eq!(SnapshotName::parse(invalid), Err(Refusal::Bounds));
        }
        assert!(SnapshotName::parse(&"x".repeat(48)).is_ok());
        for (static_vm, restore, vm_points, host_points, expected) in [
            (false, false, 3, 15, None),
            (false, false, 4, 15, Some(Refusal::Limit)),
            (false, true, 4, 16, None),
            (true, false, 2, 15, None),
            (true, false, 3, 13, Some(Refusal::Limit)),
            (true, true, 3, 15, None),
            (true, true, 4, 15, Some(Refusal::Limit)),
            (true, true, 3, 16, Some(Refusal::Limit)),
        ] {
            let limits = Limits {
                vm_points,
                host_points,
                ..limits()
            };
            assert_eq!(build(intent(static_vm, restore).0, limits).err(), expected);
        }
        let (mut request, _) = intent(true, false);
        request.release.disposition = StopDisposition::DirtyRelease;
        assert_eq!(build(request, limits()).err(), Some(Refusal::DirtyCapture));
        for alter in [
            (|release: &mut ActiveRelease| release.generation = key(99)) as fn(&mut ActiveRelease),
            |release| release.writer_epoch = key(99),
            |release| release.endpoints_released = false,
            |release| release.disposition = StopDisposition::ChargedQuarantine,
        ] {
            let (mut request, _) = intent(true, true);
            alter(&mut request.release);
            assert_eq!(
                build(request, limits()).err(),
                Some(Refusal::ReleaseUnverified)
            );
        }
        for limits in [
            Limits {
                max_file_bytes: CLUSTER as u64,
                ..limits()
            },
            Limits {
                max_virtual_bytes: VIRTUAL - 1,
                ..limits()
            },
        ] {
            assert_eq!(
                build(intent(true, false).0, limits).err(),
                Some(Refusal::Bounds)
            );
        }
        let (mut request, _) = intent(false, false);
        request.destination.vars_key = request.destination.disk_key;
        assert_eq!(build(request, limits()).err(), Some(Refusal::Identity));
        let (mut request, _) = intent(true, false);
        request.active.vars.path = LocalPath::parse(r"d:\MANAGED\generation-6\DISK").unwrap();
        assert_eq!(build(request, limits()).err(), Some(Refusal::Identity));
    }

    #[test]
    fn restore_compares_host_vm_lineage_firmware_and_pair_geometry() {
        let variants: [fn(&mut Compatibility); 7] = [
            |c| c.host = key(99),
            |c| c.vm = key(99),
            |c| c.lineage = Lineage::Static { origin: key(99) },
            |c| c.recipe[0] ^= 1,
            |c| c.firmware.sha256[0] ^= 1,
            |c| c.virtual_disk_bytes -= 1,
            |c| c.vars_bytes -= 1,
        ];
        for alter in variants {
            let (mut request, _) = intent(true, true);
            alter(&mut request.selected.as_mut().unwrap().1.compatibility);
            assert_eq!(build(request, limits()).err(), Some(Refusal::Compatibility));
        }
        let (mut request, _) = intent(false, true);
        let selected = &mut request.selected.as_mut().unwrap().1;
        let Lineage::Test { golden, .. } = &mut selected.compatibility.lineage else {
            unreachable!()
        };
        golden.pin.sha256[0] ^= 1;
        assert_eq!(build(request, limits()).err(), Some(Refusal::Compatibility));
        let (mut request, _) = intent(true, true);
        request.selected.as_mut().unwrap().0 = key(99);
        assert_eq!(build(request, limits()).err(), Some(Refusal::Compatibility));
        let (mut request, _) = intent(true, true);
        request.selected.as_mut().unwrap().1.vars.pin.size -= 1;
        assert_eq!(build(request, limits()).err(), Some(Refusal::VarsPair));
    }

    #[test]
    fn peak_reserves_logical_pairs_and_never_credits_displaced_storage() {
        for restore in [false, true] {
            let (mut request, disk) = intent(true, restore);
            let old = disk.len() as u64 + VARS.len() as u64 + u64::from(restore) * 19;
            if restore {
                request.active.disk.pin.size += 19;
            }
            let limits = limits();
            let scratch = limits.staging_bytes + limits.metadata_bytes;
            let growth = u64::from(restore) * limits.restore_growth_bytes;
            let copied = disk.len() as u64 + VARS.len() as u64;
            let plan = build(request, limits).unwrap_or_else(|e| panic!("{e:?}"));
            assert_eq!(
                plan.additional_claim().total(),
                Some(copied + growth + scratch)
            );
            assert_eq!(
                plan.coexistence_bytes(),
                old + copied + growth + scratch + u64::from(restore) * copied
            );
        }
        let overflow = Limits {
            metadata_bytes: u64::MAX,
            ..limits()
        };
        assert_eq!(
            build(intent(true, false).0, overflow).err(),
            Some(Refusal::Disk)
        );
        let no_growth = Limits {
            restore_growth_bytes: 0,
            ..limits()
        };
        assert_eq!(
            build(intent(true, true).0, no_growth).err(),
            Some(Refusal::Disk)
        );
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Fault {
        None,
        Cancel,
        Expire,
        Capacity,
        SourceIdentity,
        LateSourceIdentity,
        LateStagingIdentity,
        SourcePin,
        LateSourcePin,
        StagingVars,
        StagingHeader,
        GoldenHeader,
        GoldenPin,
        WrongBacking,
        WrongVirtualSize,
        WrongFormat,
        OversizeInfo,
        HelperRelease,
        Flush,
    }

    /// Memory-only held-object model. The fixture does not create paths,
    /// change ambient ACLs or execute a subprocess on any platform.
    struct Fake<'a> {
        plan: &'a CopyPlan,
        binding: Arc<()>,
        fault: Fault,
        now: &'a Cell<Instant>,
        cancelled: &'a Cell<bool>,
        source: [Vec<u8>; 2],
        staging: [Vec<u8>; 2],
        inspected: Vec<(ArtifactRole, bool)>,
        bytes_read: usize,
        bytes_written: usize,
        flushed: bool,
    }

    const STAGING_FILES: [FileIdentity; 2] = [
        FileIdentity {
            volume: VOLUME,
            file: [31; 16],
        },
        FileIdentity {
            volume: VOLUME,
            file: [32; 16],
        },
    ];

    impl CopyCustody for Fake<'_> {
        fn checkpoint(
            &mut self,
            plan: &CopyPlan,
            worker: &PendingOperation,
            deadline: Instant,
        ) -> Result<(), Refusal> {
            assert!(std::ptr::eq(plan, self.plan));
            assert!(deadline <= worker.deadline());
            if !Arc::ptr_eq(&self.binding, &worker.binding) {
                return Err(Refusal::Custody);
            }
            if self.fault == Fault::Capacity && self.bytes_written > 0 {
                return Err(Refusal::Disk);
            }
            Ok(())
        }

        fn observe(&mut self, side: Side, _: Instant) -> Result<[Observation; 2], Refusal> {
            let mut objects = match side {
                Side::Active => observations(&self.plan.intent.active),
                Side::Source => observations(self.plan.source()),
                Side::Staging => [0, 1].map(|i| Observation {
                    identity: STAGING_FILES[i],
                    size: self.staging[i].len() as u64,
                }),
            };
            let replaced = match (side, self.fault) {
                (Side::Source, Fault::SourceIdentity) => true,
                (Side::Source, Fault::LateSourceIdentity)
                | (Side::Staging, Fault::LateStagingIdentity) => self.flushed,
                _ => false,
            };
            if replaced {
                objects[0].identity = id(98);
            }
            if side == Side::Staging && self.fault == Fault::StagingHeader && self.flushed {
                self.staging[0][0] ^= 1;
            }
            Ok(objects)
        }

        fn read(
            &mut self,
            side: Side,
            file: FileRole,
            offset: u64,
            buffer: &mut [u8],
            _: Instant,
        ) -> Result<(), Refusal> {
            assert!(buffer.len() <= CLUSTER);
            let i = usize::from(file == FileRole::Vars);
            let bytes = match side {
                Side::Source => &self.source[i],
                Side::Staging => &self.staging[i],
                Side::Active => panic!("restore attempted to copy the displaced pair"),
            };
            let offset = offset as usize;
            let end = offset + buffer.len();
            buffer.copy_from_slice(bytes.get(offset..end).ok_or(Refusal::Io)?);
            self.bytes_read += buffer.len();
            Ok(())
        }

        fn write_staging(
            &mut self,
            file: FileRole,
            offset: u64,
            bytes: &[u8],
            deadline: Instant,
        ) -> Result<(), Refusal> {
            assert!(bytes.len() <= CLUSTER);
            let leaf = &mut self.staging[usize::from(file == FileRole::Vars)];
            assert_eq!(leaf.len() as u64, offset);
            leaf.extend_from_slice(bytes);
            self.bytes_written += bytes.len();
            match self.fault {
                Fault::Cancel => self.cancelled.set(true),
                Fault::Expire => self.now.set(deadline),
                _ => {}
            }
            Ok(())
        }

        fn golden_header(
            &mut self,
            golden: &Golden,
            buffer: &mut [u8],
            _: Instant,
        ) -> Result<(), Refusal> {
            let mut bytes = header(None);
            if self.fault == Fault::GoldenPin {
                return Err(Refusal::Pin);
            }
            assert_eq!(golden.pin, pin(&bytes));
            if self.fault == Fault::GoldenHeader {
                bytes[63] = 1;
            }
            buffer.copy_from_slice(&bytes);
            self.bytes_read += buffer.len();
            Ok(())
        }

        fn inspect(
            &mut self,
            info: &SnapshotInfo<'_>,
            output: &mut [u8],
            _: Instant,
        ) -> Result<usize, Refusal> {
            assert_eq!(output.len(), MAX_INFO);
            let path = info.path();
            assert_eq!(
                info.args(),
                ["info", "--output=json", "-f", "qcow2", path.as_str()]
            );
            let source = self.inspected.is_empty();
            let side = if source { Side::Source } else { Side::Staging };
            let disk_role = self.plan.artifact_role(side, FileRole::Disk);
            assert_eq!(info.disk().path_for(disk_role), Ok(path));
            let golden = info
                .golden()
                .map(|a| a.path_for(ArtifactRole::Golden).unwrap());
            assert_eq!(golden, self.plan.golden().map(|g| &g.path));
            self.inspected.push((disk_role, golden.is_some()));
            let mut value = json!({
                "filename": path.as_str(),
                "format": "qcow2",
                "virtual-size": VIRTUAL,
                "actual-size": CLUSTER,
                "dirty-flag": false,
            });
            if let Some(golden) = golden {
                value["backing-filename"] = json!(golden.as_str());
                value["full-backing-filename"] = json!(golden.as_str());
                value["backing-filename-format"] = json!("qcow2");
            }
            if !source {
                match self.fault {
                    Fault::WrongBacking => value["backing-filename"] = json!("golden"),
                    Fault::WrongVirtualSize => value["virtual-size"] = json!(VIRTUAL + 1),
                    Fault::WrongFormat => value["format"] = json!("raw"),
                    Fault::OversizeInfo => return Ok(output.len() + 1),
                    Fault::HelperRelease => return Err(Refusal::Custody),
                    _ => {}
                }
            }
            let bytes = serde_json::to_vec(&value).unwrap();
            output[..bytes.len()].copy_from_slice(&bytes);
            Ok(bytes.len())
        }

        fn flush_staging(&mut self, _: Instant) -> Result<(), Refusal> {
            if self.fault == Fault::Flush {
                return Err(Refusal::Io);
            }
            self.flushed = true;
            match self.fault {
                Fault::LateSourcePin => *self.source[0].last_mut().unwrap() ^= 1,
                Fault::StagingVars => self.staging[1][0] ^= 1,
                _ => {}
            }
            Ok(())
        }
    }

    fn worker(now: Instant, after: Duration) -> PendingOperation {
        PendingOperation {
            activity_id: 1,
            deadline: now + after,
            binding: Arc::new(()),
        }
    }

    fn fake<'a>(
        plan: &'a CopyPlan,
        worker: &PendingOperation,
        disk: Vec<u8>,
        fault: Fault,
        now: &'a Cell<Instant>,
        cancelled: &'a Cell<bool>,
    ) -> Fake<'a> {
        let mut fake = Fake {
            plan,
            binding: worker.binding.clone(),
            fault,
            now,
            cancelled,
            source: [disk, VARS.to_vec()],
            staging: [vec![], vec![]],
            inspected: vec![],
            bytes_read: 0,
            bytes_written: 0,
            flushed: false,
        };
        if fault == Fault::SourcePin {
            *fake.source[0].last_mut().unwrap() ^= 1;
        }
        fake
    }

    #[test]
    fn complete_pair_copy_seals_both_roles_with_bounded_io() {
        for (static_vm, restore, expected_role) in [
            (true, false, ArtifactRole::StaticDisk),
            (false, false, ArtifactRole::GuestDisk),
            (true, true, ArtifactRole::SnapshotDisk),
            (false, true, ArtifactRole::SnapshotDisk),
        ] {
            let (request, disk) = intent(static_vm, restore);
            let plan = build(request, limits()).unwrap_or_else(|e| panic!("{e:?}"));
            let now = Cell::new(Instant::now());
            let cancelled = Cell::new(false);
            let worker = worker(now.get(), Duration::from_secs(20));
            let mut fake = fake(&plan, &worker, disk.clone(), Fault::None, &now, &cancelled);
            let clock = || now.get();
            let mut sealed =
                copy_with_clock(&plan, &worker, &mut fake, STAGING_FILES, &|| false, &clock)
                    .unwrap_or_else(|e| panic!("{e:?}"));
            assert_eq!(sealed.pair().generation, plan.intent.destination.generation);
            assert_eq!(sealed.pair().disk.pin, pin(&disk));
            assert_eq!(sealed.pair().vars.pin, pin(VARS));
            assert_eq!(sealed.pair().disk.identity, STAGING_FILES[0]);
            assert_eq!(sealed.pair().vars.identity, STAGING_FILES[1]);
            sealed.recheck().unwrap();
            assert_eq!(fake.staging, [disk.clone(), VARS.to_vec()]);
            assert!(fake.flushed);
            assert_eq!(
                fake.inspected,
                [
                    (expected_role, !static_vm),
                    (ArtifactRole::SnapshotStagingDisk, !static_vm)
                ]
            );
            assert_eq!(fake.bytes_written, disk.len() + VARS.len());
            assert_eq!(
                fake.bytes_read,
                4 * fake.bytes_written + if static_vm { 2 } else { 4 } * CLUSTER
            );
        }
    }

    #[test]
    fn refuses_each_custody_pin_header_pair_and_helper_mismatch() {
        for (fault, expected, writes) in [
            (Fault::SourceIdentity, Refusal::Identity, false),
            (Fault::LateSourceIdentity, Refusal::Identity, true),
            (Fault::LateStagingIdentity, Refusal::Identity, true),
            (Fault::SourcePin, Refusal::Pin, false),
            (Fault::LateSourcePin, Refusal::Pin, true),
            (Fault::StagingVars, Refusal::VarsPair, true),
            (Fault::StagingHeader, Refusal::Header, true),
            (Fault::GoldenHeader, Refusal::Header, false),
            (Fault::GoldenPin, Refusal::Pin, false),
            (Fault::WrongBacking, Refusal::Inspection, true),
            (Fault::WrongVirtualSize, Refusal::Inspection, true),
            (Fault::WrongFormat, Refusal::Inspection, true),
            (Fault::OversizeInfo, Refusal::Inspection, true),
            (Fault::HelperRelease, Refusal::Custody, true),
            (Fault::Flush, Refusal::Io, true),
        ] {
            let (request, disk) = intent(false, false);
            let plan = build(request, limits()).unwrap_or_else(|e| panic!("{e:?}"));
            let now = Cell::new(Instant::now());
            let cancelled = Cell::new(false);
            let worker = worker(now.get(), Duration::from_secs(20));
            let mut fake = fake(&plan, &worker, disk, fault, &now, &cancelled);
            let result = copy_pair(&plan, &worker, &mut fake, STAGING_FILES, &|| false);
            assert_eq!(result.err(), Some(expected));
            assert_eq!(fake.bytes_written > 0, writes);
        }
    }

    #[test]
    fn invalid_source_header_cannot_reach_inspector_even_with_matching_pin() {
        for (offset, mask) in [(63, 1), (79, 4), (128, 0x20)] {
            let (mut request, mut disk) = intent(false, false);
            disk[offset] ^= mask;
            request.active.disk.pin = pin(&disk);
            let plan = build(request, limits()).unwrap_or_else(|e| panic!("{e:?}"));
            let now = Cell::new(Instant::now());
            let cancelled = Cell::new(false);
            let worker = worker(now.get(), Duration::from_secs(20));
            let mut fake = fake(&plan, &worker, disk, Fault::None, &now, &cancelled);
            assert_eq!(
                copy_pair(&plan, &worker, &mut fake, STAGING_FILES, &|| false).err(),
                Some(Refusal::Header)
            );
            assert!(fake.inspected.is_empty());
            assert_eq!(fake.bytes_written, 0);
        }
    }

    #[test]
    fn interrupted_worker_leaves_partial_staging_and_cannot_seal() {
        for (fault, expected) in [
            (Fault::Cancel, Refusal::Cancelled),
            (Fault::Expire, Refusal::Deadline),
            (Fault::Capacity, Refusal::Disk),
        ] {
            let (request, disk) = intent(true, false);
            let plan = build(request, limits()).unwrap_or_else(|e| panic!("{e:?}"));
            let now = Cell::new(Instant::now());
            let cancelled = Cell::new(false);
            let worker = worker(now.get(), Duration::from_secs(3600));
            let deadline = now.get() + SNAPSHOT_COPY_MAXIMUM;
            assert_eq!(snapshot_deadline(&worker, now.get()), deadline);
            let mut fake = fake(&plan, &worker, disk, fault, &now, &cancelled);
            assert_eq!(
                copy_with_clock(
                    &plan,
                    &worker,
                    &mut fake,
                    STAGING_FILES,
                    &|| cancelled.get(),
                    &|| now.get()
                )
                .err(),
                Some(expected)
            );
            if fault == Fault::Expire {
                assert_eq!(now.get(), deadline);
            }
            assert_eq!(fake.bytes_written, CLUSTER);
            assert!(fake.staging[1].is_empty());
            assert!(!fake.flushed);
        }
    }

    #[test]
    fn worker_binding_and_deadline_fence_initial_and_settlement_phases() {
        let (request, disk) = intent(true, true);
        let plan = build(request, limits()).unwrap_or_else(|e| panic!("{e:?}"));
        let now = Cell::new(Instant::now());
        let cancelled = Cell::new(false);
        let valid = worker(now.get(), Duration::from_secs(3));
        let stale = worker(now.get(), Duration::from_secs(3));
        let mut fake = fake(&plan, &valid, disk, Fault::None, &now, &cancelled);
        assert_eq!(
            copy_pair(&plan, &stale, &mut fake, STAGING_FILES, &|| false).err(),
            Some(Refusal::Custody)
        );
        assert_eq!(
            copy_pair(
                &plan,
                &valid,
                &mut fake,
                [plan.intent.active.disk.identity, STAGING_FILES[1]],
                &|| false
            )
            .err(),
            Some(Refusal::Identity)
        );
        assert_eq!(fake.bytes_written, 0);
        let read_clock = || now.get();
        let mut sealed = copy_with_clock(
            &plan,
            &valid,
            &mut fake,
            STAGING_FILES,
            &|| false,
            &read_clock,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        now.set(valid.deadline());
        assert_eq!(sealed.recheck(), Err(Refusal::Deadline));
    }
}
