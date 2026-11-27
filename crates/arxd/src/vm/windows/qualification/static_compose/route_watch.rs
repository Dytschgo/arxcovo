//! Read-only IPv4 route measurement, independent of the dossier and QEMU.
//! QEMU's reviewed backend also checks the route at the socket boundary.

use super::*;
use crate::vm::public_egress::FROZEN_DENY;
use std::{
    collections::BTreeSet,
    ffi::c_void,
    net::Ipv4Addr,
    ptr::{NonNull, addr_of, null_mut},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::HANDLE,
    NetworkManagement::{
        IpHelper::*,
        Ndis::{IfOperStatusUp, NET_LUID_LH},
    },
    Networking::WinSock::{AF_INET, IpDadStatePreferred, SOCKADDR_INET},
};

const NETWORK: NativeRefusal = NativeRefusal::NetworkFenceUnverified;
const MAX_TABLE_ROWS: usize = 4096;
const MAX_TARGETS: usize = 32;

type Interface = (u32, u64);

struct WatchContext {
    policy: PublicEgressPolicy,
    changed: AtomicBool,
    interfaces: OnceLock<BTreeSet<Interface>>,
}

impl WatchContext {
    fn on_uplink(&self, interface: Interface) -> bool {
        let uplink = self.policy.uplink();
        interface.0 == uplink.interface_index || interface.1 == uplink.interface_luid
    }
}

struct Notification {
    handle: usize,
    context: Arc<WatchContext>,
}

impl Notification {
    fn arm(
        context: &Arc<WatchContext>,
        register: impl FnOnce(*const c_void, *mut HANDLE) -> u32,
    ) -> Result<Self, NativeRefusal> {
        let mut handle = null_mut();
        if register(Arc::as_ptr(context).cast(), &mut handle) != 0 || handle.is_null() {
            return Err(NETWORK);
        }
        Ok(Self {
            handle: handle as usize,
            context: context.clone(),
        })
    }
}

impl Drop for Notification {
    fn drop(&mut self) {
        // The registration's Arc keeps its callback context live until
        // deregistration. Retain that reference if cancellation fails.
        if unsafe { CancelMibChangeNotify2(self.handle as HANDLE) } != 0 {
            std::mem::forget(self.context.clone());
        }
    }
}

pub(in crate::vm::windows::qualification) struct RouteWatch {
    _notifications: [Notification; 3],
    context: Arc<WatchContext>,
}

impl RouteWatch {
    pub(super) fn arm(policy: &PublicEgressPolicy) -> Result<Self, NativeRefusal> {
        let context = Arc::new(WatchContext {
            policy: policy.clone(),
            changed: AtomicBool::new(false),
            interfaces: OnceLock::new(),
        });
        // Context belongs to the returned registrations and remains alive
        // during registration. The callbacks only mark it invalid.
        let route = Notification::arm(&context, |context, handle| unsafe {
            NotifyRouteChange2(AF_INET, Some(route_changed), context, false, handle)
        })?;
        let address = Notification::arm(&context, |context, handle| unsafe {
            NotifyUnicastIpAddressChange(AF_INET, Some(address_changed), context, false, handle)
        })?;
        let interface = Notification::arm(&context, |context, handle| unsafe {
            NotifyIpInterfaceChange(AF_INET, Some(interface_changed), context, false, handle)
        })?;
        Ok(Self {
            _notifications: [route, address, interface],
            context,
        })
    }

    pub(super) fn require_current(&self) -> Result<(), NativeRefusal> {
        (!self.context.changed.load(Ordering::Acquire))
            .then_some(())
            .ok_or(NETWORK)
    }

    pub(super) fn require_baseline(
        &self,
        policy: &PublicEgressPolicy,
        public_targets: &[Ipv4Addr],
        expected: Hash,
    ) -> Result<(), NativeRefusal> {
        let (_, observed) = self.observe(policy, public_targets)?;
        if observed != expected {
            self.context.changed.store(true, Ordering::Release);
        }
        self.require_current()
    }

    pub(super) fn observe(
        &self,
        policy: &PublicEgressPolicy,
        public_targets: &[Ipv4Addr],
    ) -> Result<(RouteObservation, Hash), NativeRefusal> {
        sample_while_current(&self.context.changed, || {
            if policy != &self.context.policy {
                return Err(NETWORK);
            }
            let (route, fingerprint, interfaces) = topology(policy, public_targets)?;
            if self.context.interfaces.get_or_init(|| interfaces.clone()) != &interfaces {
                return Err(NETWORK);
            }
            Ok((route, fingerprint))
        })
    }
}

fn sample_while_current<T>(
    changed: &AtomicBool,
    sample: impl FnOnce() -> Result<T, NativeRefusal>,
) -> Result<T, NativeRefusal> {
    if changed.load(Ordering::Acquire) {
        return Err(NETWORK);
    }
    let observed = sample();
    if observed.is_err() {
        changed.store(true, Ordering::Release);
    }
    if changed.load(Ordering::Acquire) {
        return Err(NETWORK);
    }
    observed
}

// SAFETY: context must be the live Arc pointer passed to registration.
unsafe fn mark_changed(context: *const c_void, relevant: impl FnOnce(&WatchContext) -> bool) {
    let context = unsafe { &*context.cast::<WatchContext>() };
    if relevant(context) {
        context.changed.store(true, Ordering::Release);
    }
}

unsafe extern "system" fn route_changed(
    context: *const c_void,
    row: *const MIB_IPFORWARD_ROW2,
    _: MIB_NOTIFICATION_TYPE,
) {
    // Windows owns the row for the duration of the callback.
    unsafe {
        mark_changed(context, |context| {
            row.as_ref()
                .is_none_or(|row| route_relevant(&context.policy, row).unwrap_or(true))
        });
    }
}

unsafe extern "system" fn address_changed(
    context: *const c_void,
    row: *const MIB_UNICASTIPADDRESS_ROW,
    _: MIB_NOTIFICATION_TYPE,
) {
    unsafe {
        mark_changed(context, |context| {
            row.as_ref()
                .is_none_or(|row| address_relevant(&context.policy, row).unwrap_or(true))
        });
    }
}

unsafe extern "system" fn interface_changed(
    context: *const c_void,
    row: *const MIB_IPINTERFACE_ROW,
    _: MIB_NOTIFICATION_TYPE,
) {
    unsafe {
        mark_changed(context, |context| {
            row.as_ref().is_none_or(|row| {
                let key = (row.InterfaceIndex, row.InterfaceLuid.Value);
                context.on_uplink(key)
                    || context
                        .interfaces
                        .get()
                        .is_none_or(|set| set.contains(&key))
            })
        });
    }
}

struct Table<T>(NonNull<T>);

impl<T> Table<T> {
    fn read(load: impl FnOnce(*mut *mut T) -> u32) -> Result<Self, NativeRefusal> {
        let mut table = null_mut();
        if load(&mut table) != 0 {
            return Err(NETWORK);
        }
        NonNull::new(table).map(Self).ok_or(NETWORK)
    }

    // The row pointer and count come from the same API-allocated table.
    unsafe fn rows<R>(&self, first: *const R, len: u32) -> Result<&[R], NativeRefusal> {
        let len = len as usize;
        if len == 0 || len > MAX_TABLE_ROWS {
            return Err(NETWORK);
        }
        Ok(unsafe { std::slice::from_raw_parts(first, len) })
    }
}

impl<T> Drop for Table<T> {
    fn drop(&mut self) {
        unsafe { FreeMibTable(self.0.as_ptr().cast()) };
    }
}

fn ipv4(addr: SOCKADDR_INET) -> Result<Ipv4Addr, NativeRefusal> {
    // The tag selects the union member, and s_addr is in network byte order.
    if unsafe { addr.si_family } != AF_INET {
        return Err(NETWORK);
    }
    Ok(unsafe { addr.Ipv4.sin_addr.S_un.S_addr }
        .to_ne_bytes()
        .into())
}

fn best_route(source: Ipv4Addr, target: Ipv4Addr) -> Result<RouteObservation, NativeRefusal> {
    let mut route = MIB_IPFORWARDROW::default();
    let mut luid = NET_LUID_LH::default();
    // Both outputs point to initialized storage. Match the address byte
    // order used by the separately reviewed QEMU/libslirp backend.
    if unsafe {
        GetBestRoute(
            u32::from_ne_bytes(target.octets()),
            u32::from_ne_bytes(source.octets()),
            &mut route,
        ) != 0
            || ConvertInterfaceIndexToLuid(route.dwForwardIfIndex, &mut luid) != 0
    } {
        return Err(NETWORK);
    }
    Ok(RouteObservation {
        interface_index: route.dwForwardIfIndex,
        interface_luid: unsafe { luid.Value },
        source,
        gateway: route.dwForwardNextHop.to_ne_bytes().into(),
        default_route: route.dwForwardDest == 0 && route.dwForwardMask == 0,
    })
}

fn covers_prefix(
    address: Ipv4Addr,
    prefix: u8,
    includes: impl FnOnce(Ipv4Addr, Ipv4Addr) -> bool,
) -> bool {
    if !(1..=32).contains(&prefix) {
        return false;
    }
    let host_mask = u32::MAX.checked_shr(u32::from(prefix)).unwrap_or(0);
    let address = u32::from(address);
    includes((address & !host_mask).into(), (address | host_mask).into())
}

fn deny_covers_prefix(policy: &PublicEgressPolicy, address: Ipv4Addr, prefix: u8) -> bool {
    covers_prefix(address, prefix, |first, last| {
        FROZEN_DENY
            .iter()
            .chain(policy.deny())
            .any(|denied| denied.contains(first) && denied.contains(last))
    })
}

fn unrelated_prefix(policy: &PublicEgressPolicy, address: Ipv4Addr, prefix: u8) -> bool {
    covers_prefix(address, prefix, |first, last| {
        FROZEN_DENY
            .iter()
            .any(|denied| denied.contains(first) && denied.contains(last))
            && !policy
                .deny()
                .iter()
                .any(|site| site.contains(first) || (first..=last).contains(&site.network()))
    })
}

fn address_relevant(
    policy: &PublicEgressPolicy,
    row: &MIB_UNICASTIPADDRESS_ROW,
) -> Result<bool, NativeRefusal> {
    let uplink = policy.uplink();
    let ip = ipv4(row.Address)?;
    Ok(row.InterfaceIndex == uplink.interface_index
        || unsafe { row.InterfaceLuid.Value } == uplink.interface_luid
        || !unrelated_prefix(policy, ip, row.OnLinkPrefixLength))
}

fn route_relevant(
    policy: &PublicEgressPolicy,
    row: &MIB_IPFORWARD_ROW2,
) -> Result<bool, NativeRefusal> {
    let uplink = policy.uplink();
    let ip = ipv4(row.DestinationPrefix.Prefix)?;
    let gateway = ipv4(row.NextHop)?;
    // Always include defaults (prefix 0), uplink changes and public-numbered
    // routes, even on secondary adapters. Only immutable denies can make a
    // non-uplink private route irrelevant to the public egress policy.
    Ok(row.InterfaceIndex == uplink.interface_index
        || unsafe { row.InterfaceLuid.Value } == uplink.interface_luid
        || !unrelated_prefix(policy, ip, row.DestinationPrefix.PrefixLength)
        || (!gateway.is_unspecified() && !unrelated_prefix(policy, gateway, 32)))
}

fn topology(
    policy: &PublicEgressPolicy,
    public_targets: &[Ipv4Addr],
) -> Result<(RouteObservation, Hash, BTreeSet<Interface>), NativeRefusal> {
    if public_targets.is_empty()
        || public_targets.len() > MAX_TARGETS
        || public_targets.iter().any(|&ip| policy.denies(ip))
    {
        return Err(NETWORK);
    }
    let uplink = policy.uplink();
    let route = best_route(uplink.source, Ipv4Addr::UNSPECIFIED)?;
    policy.verify_route(Some(&route))?;
    for &target in public_targets {
        policy.verify_route(Some(&best_route(uplink.source, target)?))?;
    }

    let addresses = Table::read(|out| unsafe { GetUnicastIpAddressTable(AF_INET, out) })?;
    let routes = Table::read(|out| unsafe { GetIpForwardTable2(AF_INET, out) })?;
    let interfaces = Table::read(|out| unsafe { GetIpInterfaceTable(AF_INET, out) })?;
    let links = Table::read(|out| unsafe { GetIfTable2(out) })?;
    let (addresses, routes, interfaces, links) = unsafe {
        let a = addresses.0.as_ptr();
        let r = routes.0.as_ptr();
        let i = interfaces.0.as_ptr();
        let l = links.0.as_ptr();
        (
            addresses.rows(
                addr_of!((*a).Table).cast::<MIB_UNICASTIPADDRESS_ROW>(),
                (*a).NumEntries,
            )?,
            routes.rows(
                addr_of!((*r).Table).cast::<MIB_IPFORWARD_ROW2>(),
                (*r).NumEntries,
            )?,
            interfaces.rows(
                addr_of!((*i).Table).cast::<MIB_IPINTERFACE_ROW>(),
                (*i).NumEntries,
            )?,
            links.rows(addr_of!((*l).Table).cast::<MIB_IF_ROW2>(), (*l).NumEntries)?,
        )
    };
    let mut relevant = BTreeSet::new();
    let mut address_projection = Vec::with_capacity(addresses.len());
    for row in addresses {
        if !address_relevant(policy, row)? {
            continue;
        }
        relevant.insert((row.InterfaceIndex, unsafe { row.InterfaceLuid.Value }));
        let ip = ipv4(row.Address)?;
        if !deny_covers_prefix(policy, ip, row.OnLinkPrefixLength) {
            return Err(NETWORK);
        }
        address_projection.push((
            row.InterfaceIndex,
            unsafe { row.InterfaceLuid.Value },
            ip,
            row.OnLinkPrefixLength,
            row.DadState,
            row.SkipAsSource,
        ));
    }
    if !address_projection
        .iter()
        .any(|&(index, luid, ip, _, state, skip)| {
            (index, luid, ip) == (uplink.interface_index, uplink.interface_luid, uplink.source)
                && state == IpDadStatePreferred
                && !skip
        })
    {
        return Err(NETWORK);
    }
    let mut route_projection = Vec::with_capacity(routes.len());
    for row in routes {
        if !route_relevant(policy, row)? {
            continue;
        }
        relevant.insert((row.InterfaceIndex, unsafe { row.InterfaceLuid.Value }));
        let address = ipv4(row.DestinationPrefix.Prefix)?;
        let prefix = row.DestinationPrefix.PrefixLength;
        let next_hop = ipv4(row.NextHop)?;
        if (prefix == 0 && !address.is_unspecified())
            || (prefix != 0 && !deny_covers_prefix(policy, address, prefix))
            || (!next_hop.is_unspecified() && !policy.denies(next_hop))
        {
            return Err(NETWORK);
        }
        route_projection.push((
            row.InterfaceIndex,
            unsafe { row.InterfaceLuid.Value },
            address,
            prefix,
            next_hop,
            row.Metric,
            row.Loopback,
        ));
    }
    let mut interface_projection: Vec<_> = interfaces
        .iter()
        .filter(|row| relevant.contains(&(row.InterfaceIndex, unsafe { row.InterfaceLuid.Value })))
        .map(|row| {
            (
                row.InterfaceIndex,
                unsafe { row.InterfaceLuid.Value },
                row.Metric,
                row.Connected,
                row.WeakHostSend,
                row.WeakHostReceive,
                row.DisableDefaultRoutes,
            )
        })
        .collect();
    let mut link_projection: Vec<_> = links
        .iter()
        .filter(|row| relevant.contains(&(row.InterfaceIndex, unsafe { row.InterfaceLuid.Value })))
        .map(|row| {
            (
                row.InterfaceIndex,
                unsafe { row.InterfaceLuid.Value },
                row.OperStatus,
                row.AdminStatus,
                row.MediaConnectState,
            )
        })
        .collect();
    if !link_projection.iter().any(|&(index, luid, status, _, _)| {
        (index, luid) == (uplink.interface_index, uplink.interface_luid) && status == IfOperStatusUp
    }) {
        return Err(NETWORK);
    }

    // Avoid volatile lifetimes/counters and machine labels. The enrollment
    // pins this projection separately from the reviewed site deny inventory.
    address_projection.sort_unstable();
    route_projection.sort_unstable();
    interface_projection.sort_unstable();
    link_projection.sort_unstable();
    let hash = projection(
        "arx-docker-ipv4-topology-v2",
        (
            address_projection,
            route_projection,
            interface_projection,
            link_projection,
        ),
    )
    .map_err(|_| NETWORK)?;
    Ok((route, hash, relevant))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> PublicEgressPolicy {
        let body = "version=1\ninterface_index=7\ninterface_luid=8\nsource=10.8.0.4\ngateway=10.8.0.1\ndeny=11.0.0.4/32\ndeny=192.168.2.0/24\n";
        let bytes = format!(
            "{body}fingerprint={}\n",
            hex::encode(digest(body.as_bytes()))
        );
        PublicEgressPolicy::parse(bytes.as_bytes()).unwrap()
    }

    #[test]
    fn local_prefix_coverage_requires_the_entire_network() {
        let policy = policy();
        let ip = "11.0.0.4".parse().unwrap();
        assert!(deny_covers_prefix(&policy, ip, 32));
        assert!(!deny_covers_prefix(&policy, ip, 24));
        assert!(!deny_covers_prefix(&policy, ip, 0));
        assert!(deny_covers_prefix(&policy, policy.uplink().source, 24));
        assert!(!deny_covers_prefix(&policy, policy.uplink().source, 7));
    }

    #[test]
    fn relevant_notifications_include_enrolled_lans_and_alternate_defaults() {
        use windows_sys::Win32::Networking::WinSock::SOCKADDR_IN;
        let sockaddr = |octets| {
            let mut addr = SOCKADDR_IN {
                sin_family: AF_INET,
                ..Default::default()
            };
            addr.sin_addr.S_un.S_addr = u32::from_ne_bytes(octets);
            SOCKADDR_INET { Ipv4: addr }
        };
        let context = WatchContext {
            policy: policy(),
            changed: AtomicBool::new(false),
            interfaces: OnceLock::from(BTreeSet::from([(7, 8)])),
        };
        let pointer = (&raw const context).cast();
        let mut route = MIB_IPFORWARD_ROW2 {
            InterfaceIndex: 9,
            ..Default::default()
        };
        route.DestinationPrefix.Prefix = sockaddr([192, 168, 50, 0]);
        route.DestinationPrefix.PrefixLength = 24;
        route.NextHop = sockaddr([0; 4]);
        let interface = MIB_IPINTERFACE_ROW {
            InterfaceIndex: 9,
            ..Default::default()
        };
        unsafe {
            route_changed(pointer, &route, 0);
            interface_changed(pointer, &interface, 0);
        }
        assert!(!context.changed.load(Ordering::Acquire));

        for (destination, prefix) in [([0; 4], 0), ([192, 168, 2, 0], 24), ([11, 0, 0, 0], 24)] {
            route.DestinationPrefix.Prefix = sockaddr(destination);
            route.DestinationPrefix.PrefixLength = prefix;
            unsafe { route_changed(pointer, &route, 0) };
            assert!(context.changed.swap(false, Ordering::AcqRel));
        }
        route.DestinationPrefix.Prefix = sockaddr([192, 168, 50, 0]);
        route.InterfaceIndex = 7;
        unsafe { route_changed(pointer, &route, 0) };
        assert!(context.changed.load(Ordering::Acquire));
    }

    #[test]
    fn observation_failure_and_notifications_permanently_retire_the_sample() {
        let context = WatchContext {
            policy: policy(),
            changed: AtomicBool::new(false),
            interfaces: OnceLock::new(),
        };
        let changed = &context.changed;
        assert_eq!(sample_while_current(changed, || Ok(1)), Ok(1));
        let result = sample_while_current(changed, || {
            // The observer owns the callback context for this exchange.
            unsafe { route_changed((&raw const context).cast(), std::ptr::null(), 0) };
            Ok(2)
        });
        assert_eq!(result, Err(NETWORK));
        assert_eq!(
            sample_while_current::<()>(changed, || panic!("retired sample retried")),
            Err(NETWORK)
        );

        let changed = AtomicBool::new(false);
        assert_eq!(
            sample_while_current::<()>(&changed, || Err(NETWORK)),
            Err(NETWORK)
        );
        assert_eq!(
            sample_while_current::<()>(&changed, || panic!("unknown topology retried")),
            Err(NETWORK)
        );
    }
}
