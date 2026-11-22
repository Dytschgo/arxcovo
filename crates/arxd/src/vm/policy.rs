use super::model::{DEFAULT_LEASE_SECONDS, MAX_UTC_MS, Refusal};

const GIB: u64 = 1024 * 1024 * 1024;
// Main DB and rollback journal (including journal headers and alignment).
const STATE_RESERVE: u64 = 9 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct VmPolicy {
    pub disk_cap: u64,
    pub free_floor: u64,
    pub ram_reserve: u64,
    pub max_guest_ram: u64,
    pub max_vcpus: u32,
}

impl Default for VmPolicy {
    fn default() -> Self {
        Self {
            disk_cap: 200 * GIB,
            free_floor: 50 * GIB,
            ram_reserve: 16 * GIB,
            max_guest_ram: 8 * GIB,
            max_vcpus: 4,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Capacity {
    pub free_bytes: u64,
    pub managed_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct HostObservation {
    /// Supply None for stale, ambiguous, or unqualified observations.
    pub owner_idle: Option<bool>,
    pub logical_cpus: Option<u32>,
    pub available_ram: Option<u64>,
    pub capacity: Option<Capacity>,
}

#[derive(Clone, Copy, Debug)]
pub struct Resources {
    pub vcpus: u32,
    pub guest_ram: u64,
    pub overhead_ram: u64,
}

impl VmPolicy {
    pub(crate) fn host(
        self,
        host: HostObservation,
        resources: Resources,
    ) -> Result<Capacity, Refusal> {
        let (Some(idle), Some(cpus), Some(ram), Some(capacity)) = (
            host.owner_idle,
            host.logical_cpus,
            host.available_ram,
            host.capacity,
        ) else {
            return Err(Refusal::UnknownHost);
        };
        if !idle {
            return Err(Refusal::OwnerActive);
        }
        if resources.vcpus == 0 || resources.vcpus > self.max_vcpus.min(cpus / 2) {
            return Err(Refusal::Cpu);
        }
        let needed_ram = resources
            .guest_ram
            .checked_add(resources.overhead_ram)
            .and_then(|n| n.checked_add(self.ram_reserve));
        if resources.guest_ram == 0
            || resources.guest_ram > self.max_guest_ram
            || needed_ram.is_none_or(|n| n > ram)
        {
            return Err(Refusal::Ram);
        }
        Ok(capacity)
    }

    pub(crate) fn disk(self, cap: Capacity, charged: u64, additional: u64) -> Result<(), Refusal> {
        // Count prior reservations in the free-space check too. They may not
        // have materialized yet. Double counting materialized claims is safe.
        let reserved = charged
            .checked_add(additional)
            .and_then(|n| n.checked_add(STATE_RESERVE))
            .ok_or(Refusal::Disk)?;
        // The observation does not identify which reservations have already
        // materialized. Budget both until an adapter can prove the overlap.
        let aggregate = cap.managed_bytes.checked_add(reserved);
        if aggregate.is_none_or(|n| n > self.disk_cap)
            || reserved
                .checked_add(self.free_floor)
                .is_none_or(|n| n > cap.free_bytes)
        {
            return Err(Refusal::Disk);
        }
        Ok(())
    }
}

pub(crate) fn lease(now: i64, seconds: Option<u64>) -> Result<i64, Refusal> {
    let seconds = seconds.unwrap_or(DEFAULT_LEASE_SECONDS);
    if !(60..=30 * 24 * 60 * 60).contains(&seconds) {
        return Err(Refusal::Invalid);
    }
    now.checked_add(seconds as i64 * 1000)
        .filter(|end| *end <= MAX_UTC_MS)
        .ok_or(Refusal::Invalid)
}
