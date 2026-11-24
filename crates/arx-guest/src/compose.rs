//! Guest half of Compose hosting on the static Ubuntu VM (ADR 0064, leg E2a).
//!
//! A guest that serves Compose accepts a pinned ComposeHello beside the legacy
//! Hello. Legacy frames keep their exact behavior. The host stages a strict
//! JSON project manifest in 16 KiB chunks, then deploys an exact revision. The
//! broker renders canonical Compose YAML into fixed root-owned directories and
//! drives Docker only through fixed invocations behind [`CommandRunner`].
//! Secrets arrive in a private request and are written to a dedicated tmpfs.
//!
//! The real Docker runner and Linux store live in the Linux platform module
//! and are **unverified on real hardware**. Tests use fakes.

mod codec;
mod engine;
mod layout;
mod manifest;
mod render;
mod service;
#[cfg(test)]
mod tests;
mod wire;

pub(crate) use codec::wipe;
pub use codec::{SecretText, base64_len, encode_base64, sha256_hex, wipe_bytes};
pub use engine::{
    APPLY_TIMEOUT, Clock, CommandRunner, DOCKER, ENGINE_CWD, ENGINE_ENV, HEALTH_ATTEMPTS,
    HEALTH_INTERVAL, Invocation, MAX_CAPTURE, PULL_TIMEOUT, QUERY_TIMEOUT, RunResult, STOP_TIMEOUT,
    compose_down, compose_ps, compose_recreate, compose_recreate_file, compose_stop, compose_up,
    container_inspect_mounts, container_rm, container_stop, image_inspect, image_pull,
    network_list, network_rm, parse_ps, volume_create, volume_label, volume_list,
    volume_mountpoint,
};
pub use layout::{
    DOCKER_CONFIG, DOCKER_VOLUME_ROOT, LAYOUT_VERSION, LOOPBACK_BASE, SECRET_ROOT, STATE_ROOT,
    compose_file, compose_project, loopback_port, network_name, revision_dir,
    revision_dir_relative, rotation_compose_file, secret_file, secret_instance, state_file,
    volume_name, volume_token,
};
pub use manifest::{
    EnvVar, Limits, MANIFEST_SCHEMA, MAX_ENV, MAX_MOUNTS, MAX_PROJECTS, MAX_SECRET_SLOTS,
    MAX_SERVICES, MAX_VOLUMES, ManifestRule, Mount, PLATFORM, Project, ProjectManifest,
    ServiceSpec, VolumeSpec, is_image_ref, is_project_id, is_slot_name,
};
pub use render::{PROJECT_LABEL, REVISION_LABEL, VOLUME_LABEL, render};
pub use service::{
    ComposeService, ComposeStore, MAX_JOURNAL, MAX_LOG, MAX_SECRET_BINDINGS, MAX_STAGED,
    MAX_STATE_BYTES, MAX_TOMBSTONES, SecretFileWitness, SecretMatch, StoreError,
};
pub(crate) use wire::is_compose_tag;
pub use wire::{
    COMPOSE_CAPABILITIES, COMPOSE_PROTOCOL_VERSION, ComposeRequest, ComposeResponse, Desired,
    FailureCode, HealthState, LogEntry, LogEvent, MAX_LOG_ENTRIES, MAX_LOG_PAGE, MAX_SECRET,
    Observation, OperationKind, OperationStage, OperationState, OperationView, ProjectView,
    RejectCode, STAGE_CHUNK, STAGE_TOTAL, ServiceState, ServiceView,
};

/// Multi-hostname guest nginx routing, ingress header trust, and deterministic
/// HTTP single-range (`Range` / `If-Range`) helpers (ADR 0064 / E3).
///
/// Real nginx process execution and guest socket ACLs are **unverified on real
/// hardware**; hosted CI verifies rendering, atomic validate/reload state
/// transitions, Host dispatch, header sanitization, and 206/416 semantics
/// against fakes.
pub mod routing {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Write as _;
    use std::net::Ipv4Addr;

    use super::layout::{LOOPBACK_BASE, loopback_port};
    use super::manifest::{MAX_PROJECTS, MAX_SERVICES, is_project_id};

    /// Maximum enrolled public hostnames per static VM.
    pub const MAX_ROUTES: usize = 8;
    /// Fixed guest nginx TLS bind address (ADR 0064 §4).
    pub const ORIGIN_BIND: &str = "10.0.2.15:443";
    /// Qualified slirp forward peer for host-forwarded ingress.
    pub const SLIRP_FORWARD_PEER: Ipv4Addr = Ipv4Addr::new(10, 0, 2, 2);
    /// Internal readiness path served on the reserved readiness Host.
    pub const READINESS_PATH: &str = "/arx-health";
    pub const ORIGIN_CERT_PATH: &str = "/etc/arxcovo/origin.crt";
    pub const ORIGIN_KEY_PATH: &str = "/etc/arxcovo/origin.key";
    pub const NGINX_BIN: &str = "/usr/sbin/nginx";
    pub const NGINX_CONFIG_PATH: &str = "/etc/arxcovo/nginx.conf";
    pub const NGINX_STAGING_PATH: &str = "/etc/arxcovo/nginx.conf.next";
    pub const LOOPBACK_MIN: u16 = LOOPBACK_BASE;
    pub const LOOPBACK_MAX: u16 = LOOPBACK_BASE + (MAX_PROJECTS as u16) * (MAX_SERVICES as u16) - 1;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RoutingError {
        InvalidReadinessHost,
        InvalidTrustedPeer,
        EmptyRoutes,
        TooManyRoutes,
        InvalidHostname,
        UnsortedOrDuplicateHostname,
        ZoneMismatch,
        ReadinessHostCollision,
        InvalidProject,
        InvalidService,
        SlotOutOfBounds,
        DuplicateRouteSlot,
        ProjectSlotConflict,
        PortCollision,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct RouteTarget {
        pub hostname: String,
        pub project_id: String,
        pub service: String,
        pub project_slot: u8,
        pub service_index: u8,
        pub route_slot: u8,
    }

    impl RouteTarget {
        /// A successful nginx signal alone cannot prove that workers loaded
        /// the new table. Each server exposes only its own binding witness.
        pub fn probe_path(&self) -> String {
            let binding = format!(
                "{}:{}:{}:{}:{}:{}",
                self.hostname,
                self.project_id,
                self.service,
                self.project_slot,
                self.service_index,
                self.route_slot
            );
            format!("/.arxcovo/route/{}", super::sha256_hex(binding.as_bytes()))
        }

        /// Deterministic bounded guest loopback port (`20000..=20031`).
        pub fn loopback_port(&self) -> u16 {
            loopback_port(self.project_slot, usize::from(self.service_index))
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RoutingTable {
        readiness_host: String,
        zone_name: Option<String>,
        trusted_peer: Ipv4Addr,
        routes: Vec<RouteTarget>,
    }

    impl RoutingTable {
        pub fn new(
            readiness_host: impl Into<String>,
            zone_name: Option<String>,
            trusted_peer: Ipv4Addr,
            routes: Vec<RouteTarget>,
        ) -> Result<Self, RoutingError> {
            let readiness_host = readiness_host.into();
            if !is_exact_hostname(&readiness_host) || !readiness_host.ends_with(".invalid") {
                return Err(RoutingError::InvalidReadinessHost);
            }
            if trusted_peer != SLIRP_FORWARD_PEER {
                return Err(RoutingError::InvalidTrustedPeer);
            }
            if routes.is_empty() {
                return Err(RoutingError::EmptyRoutes);
            }
            if routes.len() > MAX_ROUTES {
                return Err(RoutingError::TooManyRoutes);
            }
            if let Some(zone) = &zone_name
                && !is_exact_hostname(zone)
            {
                return Err(RoutingError::ZoneMismatch);
            }

            let mut route_slots = BTreeSet::new();
            let mut ports = BTreeSet::new();
            let mut project_slots: BTreeMap<&str, u8> = BTreeMap::new();
            let mut slot_projects: BTreeMap<u8, &str> = BTreeMap::new();

            for (index, route) in routes.iter().enumerate() {
                if !is_exact_hostname(&route.hostname) {
                    return Err(RoutingError::InvalidHostname);
                }
                if route.hostname == readiness_host {
                    return Err(RoutingError::ReadinessHostCollision);
                }
                if index > 0 && routes[index - 1].hostname >= route.hostname {
                    return Err(RoutingError::UnsortedOrDuplicateHostname);
                }
                if !is_project_id(&route.project_id) {
                    return Err(RoutingError::InvalidProject);
                }
                if !is_service_name(&route.service) {
                    return Err(RoutingError::InvalidService);
                }
                if usize::from(route.project_slot) >= MAX_PROJECTS
                    || usize::from(route.service_index) >= MAX_SERVICES
                    || usize::from(route.route_slot) >= MAX_ROUTES
                {
                    return Err(RoutingError::SlotOutOfBounds);
                }
                if !route_slots.insert(route.route_slot) {
                    return Err(RoutingError::DuplicateRouteSlot);
                }
                if let Some(&existing) = project_slots.get(route.project_id.as_str()) {
                    if existing != route.project_slot {
                        return Err(RoutingError::ProjectSlotConflict);
                    }
                } else {
                    project_slots.insert(route.project_id.as_str(), route.project_slot);
                }
                if let Some(&existing) = slot_projects.get(&route.project_slot) {
                    if existing != route.project_id.as_str() {
                        return Err(RoutingError::ProjectSlotConflict);
                    }
                } else {
                    slot_projects.insert(route.project_slot, route.project_id.as_str());
                }
                let port = route.loopback_port();
                if !(LOOPBACK_MIN..=LOOPBACK_MAX).contains(&port) || !ports.insert(port) {
                    return Err(RoutingError::PortCollision);
                }
            }

            let host_refs: Vec<&str> = routes.iter().map(|r| r.hostname.as_str()).collect();
            if !share_enrolled_zone(&host_refs, zone_name.as_deref()) {
                return Err(RoutingError::ZoneMismatch);
            }

            Ok(Self {
                readiness_host,
                zone_name,
                trusted_peer,
                routes,
            })
        }

        pub fn readiness_host(&self) -> &str {
            &self.readiness_host
        }

        pub fn zone_name(&self) -> Option<&str> {
            self.zone_name.as_deref()
        }

        pub fn trusted_peer(&self) -> Ipv4Addr {
            self.trusted_peer
        }

        pub fn routes(&self) -> &[RouteTarget] {
            &self.routes
        }

        /// Canonical nginx config for the static Compose VM:
        /// - binds `10.0.2.15:443 ssl`
        /// - gates direct origin access with `allow 10.0.2.2; deny all;`
        /// - rejects unknown `Host` with `return 444;` on `default_server`
        /// - reserves the internal readiness Host for `/arx-health`
        /// - dispatches each exact enrolled Host to its fixed `127.0.0.1:<port>`
        /// - accepts one canonical `CF-Connecting-IP`, replaces incoming
        ///   forwarding headers, suppresses access logs, preserves `Range` /
        ///   `If-Range` without buffering or rewriting, and bypasses cache.
        pub fn render_nginx_config(&self) -> String {
            let mut out = String::with_capacity(4096);
            out.push_str("# Generated by arx-guest from a validated routing table.\n");
            out.push_str("user www-data;\n");
            out.push_str("error_log /dev/null;\nevents {}\n");
            out.push_str("http {\n");
            out.push_str("  include /etc/nginx/mime.types;\n");
            out.push_str("  default_type application/octet-stream;\n");
            out.push_str("  server_tokens off;\n");
            out.push_str("  access_log off;\n");
            // realip canonicalizes the address, but it also understands address
            // lists and ports. Check the original field against IP-literal grammar
            // before forwarding the canonical value.
            let _ = writeln!(out, "  set_real_ip_from {};", self.trusted_peer);
            out.push_str("  real_ip_header CF-Connecting-IP;\n  real_ip_recursive off;\n");
            out.push_str("  map $http_cf_connecting_ip $arx_cf_ip {\n    default \"\";\n");
            out.push_str("    255.255.255.255 255.255.255.255;\n");
            let _ = writeln!(out, "    \"~^{}$\" $remote_addr;", ip_literal_pattern());
            out.push_str("  }\n");

            // Catch-all default server: reject unknown Hosts.
            let _ = writeln!(out, "  server {{");
            let _ = writeln!(out, "    listen {ORIGIN_BIND} ssl default_server;");
            let _ = writeln!(out, "    server_name _;");
            let _ = writeln!(out, "    allow {};", self.trusted_peer);
            let _ = writeln!(out, "    deny all;");
            let _ = writeln!(out, "    ssl_protocols TLSv1.2 TLSv1.3;");
            let _ = writeln!(out, "    ssl_certificate {ORIGIN_CERT_PATH};");
            let _ = writeln!(out, "    ssl_certificate_key {ORIGIN_KEY_PATH};");
            let _ = writeln!(out, "    return 444;");
            let _ = writeln!(out, "  }}");

            // Reserved internal readiness Host.
            let _ = writeln!(out, "  server {{");
            let _ = writeln!(out, "    listen {ORIGIN_BIND} ssl;");
            let _ = writeln!(out, "    server_name {};", self.readiness_host);
            write_host_guard(&mut out, self.trusted_peer, &self.readiness_host);
            let _ = writeln!(out, "    ssl_protocols TLSv1.2 TLSv1.3;");
            let _ = writeln!(out, "    ssl_certificate {ORIGIN_CERT_PATH};");
            let _ = writeln!(out, "    ssl_certificate_key {ORIGIN_KEY_PATH};");
            let _ = writeln!(out, "    location = {READINESS_PATH} {{");
            let _ = writeln!(out, "      default_type text/plain;");
            let _ = writeln!(out, "      add_header Cache-Control \"no-store\" always;");
            let _ = writeln!(out, "      return 200 \"ok\\n\";");
            let _ = writeln!(out, "    }}");
            let _ = writeln!(out, "    location / {{");
            let _ = writeln!(out, "      return 404;");
            let _ = writeln!(out, "    }}");
            let _ = writeln!(out, "  }}");

            // Enrolled exact-Host virtual servers in sorted order.
            for route in &self.routes {
                let port = route.loopback_port();
                let _ = writeln!(
                    out,
                    "  # slot {} -> {}:{} (127.0.0.1:{})",
                    route.route_slot, route.project_id, route.service, port
                );
                let _ = writeln!(out, "  server {{");
                let _ = writeln!(out, "    listen {ORIGIN_BIND} ssl;");
                let _ = writeln!(out, "    server_name {};", route.hostname);
                write_host_guard(&mut out, self.trusted_peer, &route.hostname);
                out.push_str("    if ($arx_cf_ip = \"\") { return 400; }\n");
                let _ = writeln!(out, "    ssl_protocols TLSv1.2 TLSv1.3;");
                let _ = writeln!(out, "    ssl_certificate {ORIGIN_CERT_PATH};");
                let _ = writeln!(out, "    ssl_certificate_key {ORIGIN_KEY_PATH};");
                let _ = writeln!(
                    out,
                    "    location = {} {{ return 204; }}",
                    route.probe_path()
                );
                out.push_str("    location ^~ /.arxcovo/ { return 404; }\n");
                for sensitive in ["/admin", "/events"] {
                    let _ = writeln!(out, "    location ^~ {sensitive} {{");
                    write_proxy_directives(&mut out, port, true);
                    let _ = writeln!(out, "    }}");
                }
                let _ = writeln!(out, "    location / {{");
                write_proxy_directives(&mut out, port, false);
                let _ = writeln!(out, "    }}");
                let _ = writeln!(out, "  }}");
            }

            out.push_str("}\n");
            out
        }
    }

    fn write_host_guard(out: &mut String, peer: Ipv4Addr, host: &str) {
        // Rewrite-phase checks also run for locations that return immediately.
        let _ = writeln!(
            out,
            "    if ($realip_remote_addr != {peer}) {{ return 403; }}"
        );
        let _ = writeln!(out, "    if ($http_host != {host}) {{ return 444; }}");
    }

    /// RFC 3986 IPv4address and IPv6address productions. The quoted PCRE uses
    /// [.] for the dotted tail so nginx's config parser need not unescape it.
    fn ip_literal_pattern() -> String {
        let octet = "(?:25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9]?[0-9])";
        let ipv4 = format!("(?:{octet}[.]){{3}}{octet}");
        let hex = "[0-9A-Fa-f]{1,4}";
        let low = format!("(?:{hex}:{hex}|{ipv4})");
        let mut forms = vec![ipv4, format!("(?:{hex}:){{6}}{low}")];
        // Compressed forms have zero to seven explicit groups on the left.
        for left_max in 0..=7 {
            let left = match left_max {
                0 => String::new(),
                1 => format!("(?:{hex})?"),
                n => format!("(?:(?:{hex}:){{0,{}}}{hex})?", n - 1),
            };
            let right = match left_max {
                7 => String::new(),
                6 => hex.to_string(),
                5 => low.clone(),
                n => format!("(?:{hex}:){{{}}}{low}", 5 - n),
            };
            forms.push(format!("{left}::{right}"));
        }
        format!("(?:{})", forms.join("|"))
    }

    fn write_proxy_directives(out: &mut String, port: u16, no_store: bool) {
        let _ = writeln!(out, "      proxy_pass http://127.0.0.1:{port};");
        let _ = writeln!(out, "      proxy_http_version 1.1;");
        let _ = writeln!(out, "      proxy_set_header Host $host;");
        let _ = writeln!(out, "      proxy_set_header CF-Connecting-IP \"\";");
        let _ = writeln!(out, "      proxy_set_header True-Client-IP \"\";");
        let _ = writeln!(out, "      proxy_set_header X-Forwarded-For $arx_cf_ip;");
        let _ = writeln!(out, "      proxy_set_header X-Real-IP $arx_cf_ip;");
        let _ = writeln!(out, "      proxy_set_header Forwarded \"\";");
        let _ = writeln!(out, "      proxy_set_header X-Forwarded-Host \"\";");
        let _ = writeln!(out, "      proxy_set_header X-Forwarded-Proto \"https\";");
        let _ = writeln!(out, "      proxy_set_header Range $http_range;");
        let _ = writeln!(out, "      proxy_set_header If-Range $http_if_range;");
        let _ = writeln!(out, "      proxy_buffering off;");
        let _ = writeln!(out, "      proxy_request_buffering off;");
        let _ = writeln!(out, "      proxy_force_ranges off;");
        let _ = writeln!(out, "      proxy_cache off;");
        let _ = writeln!(out, "      proxy_cache_bypass 1;");
        let _ = writeln!(out, "      proxy_no_cache 1;");
        if no_store {
            let _ = writeln!(out, "      add_header Cache-Control \"no-store\" always;");
        }
    }

    /// Exact lowercase DNS hostname without wildcards, trailing dot, port, or
    /// nginx metacharacters.
    pub fn is_exact_hostname(value: &str) -> bool {
        if value.len() > 253 || !value.contains('.') {
            return false;
        }
        if !value
            .rsplit('.')
            .next()
            .is_some_and(|tld| tld.as_bytes().first().is_some_and(u8::is_ascii_lowercase))
        {
            return false;
        }
        value.split('.').all(|label| {
            let bytes = label.as_bytes();
            !bytes.is_empty()
                && bytes.len() <= 63
                && bytes[0].is_ascii_alphanumeric()
                && bytes[bytes.len() - 1].is_ascii_alphanumeric()
                && bytes
                    .iter()
                    .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    }

    pub fn in_zone(host: &str, zone: &str) -> bool {
        host == zone
            || host
                .strip_suffix(zone)
                .is_some_and(|prefix| prefix.ends_with('.'))
    }

    /// True when every hostname belongs to `zone` (if given), or when multiple
    /// hostnames share a non-trivial DNS zone suffix (at least two labels).
    pub fn share_enrolled_zone(hosts: &[&str], zone: Option<&str>) -> bool {
        if hosts.is_empty() {
            return false;
        }
        if let Some(zone) = zone {
            return is_exact_hostname(zone) && hosts.iter().all(|host| in_zone(host, zone));
        }
        if hosts.len() == 1 {
            return is_exact_hostname(hosts[0]);
        }
        let split: Vec<Vec<&str>> = hosts
            .iter()
            .map(|h| h.rsplit('.').collect::<Vec<_>>())
            .collect();
        let min_labels = split.iter().map(Vec::len).min().unwrap_or(0);
        let mut common = 0;
        for idx in 0..min_labels {
            let label = split[0][idx];
            if split.iter().all(|parts| parts[idx] == label) {
                common += 1;
            } else {
                break;
            }
        }
        common >= 2
    }

    fn is_service_name(text: &str) -> bool {
        let bytes = text.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= 16
            && bytes[0].is_ascii_lowercase()
            && bytes[bytes.len() - 1] != b'-'
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
    }

    /// Fixed-argv nginx validation and reload commands (no shell).
    pub fn nginx_validate_argv() -> [&'static str; 4] {
        [NGINX_BIN, "-t", "-c", NGINX_STAGING_PATH]
    }

    pub fn nginx_reload_argv() -> [&'static str; 5] {
        [NGINX_BIN, "-s", "reload", "-c", NGINX_CONFIG_PATH]
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum NginxError {
        ReadActiveFailed,
        StageWriteFailed,
        ValidationFailed,
        ActivateFailed,
        ReloadFailed,
        RollbackFailed,
    }

    /// Guest-side atomic nginx config controller. Real Linux implementation is
    /// **unverified on real hardware**; tests supply a fake.
    pub trait NginxController {
        fn read_active(&self) -> Result<Option<String>, NginxError>;
        fn write_staging(&mut self, config: &str) -> Result<(), NginxError>;
        fn remove_staging(&mut self);
        fn run_fixed(&mut self, argv: &[&str]) -> bool;
        fn activate_staging(&mut self) -> Result<(), NginxError>;
        fn restore_active(&mut self, previous: Option<&str>) -> Result<(), NginxError>;
    }

    /// Generates, validates (`nginx -t`), atomically replaces, and gracefully
    /// reloads nginx config. On validation failure the previous active config
    /// stays untouched and reload is never invoked. On reload failure the prior
    /// active config is restored atomically.
    pub fn apply_nginx_config(
        table: &RoutingTable,
        controller: &mut dyn NginxController,
    ) -> Result<String, NginxError> {
        let rendered = table.render_nginx_config();
        let previous = controller.read_active()?;
        controller
            .write_staging(&rendered)
            .map_err(|_| NginxError::StageWriteFailed)?;
        if !controller.run_fixed(&nginx_validate_argv()) {
            controller.remove_staging();
            return Err(NginxError::ValidationFailed);
        }
        if controller.activate_staging().is_err() {
            controller.remove_staging();
            return Err(NginxError::ActivateFailed);
        }
        if !controller.run_fixed(&nginx_reload_argv()) {
            if controller.restore_active(previous.as_deref()).is_err() {
                return Err(NginxError::RollbackFailed);
            }
            return Err(NginxError::ReloadFailed);
        }
        Ok(rendered)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::net::IpAddr;

        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum HostDispatch<'a> {
            Readiness,
            Upstream(&'a RouteTarget),
            Rejected,
        }

        /// Strict Host header dispatch: rejects missing, malformed, uppercase,
        /// port-suffixed, multi-value, or unknown Host headers.
        pub fn dispatch_host<'a>(
            table: &'a RoutingTable,
            host_header: Option<&str>,
        ) -> HostDispatch<'a> {
            let Some(host) = host_header else {
                return HostDispatch::Rejected;
            };
            if !is_exact_hostname(host) {
                return HostDispatch::Rejected;
            }
            if host == table.readiness_host {
                return HostDispatch::Readiness;
            }
            match table.routes.iter().find(|route| route.hostname == host) {
                Some(route) => HostDispatch::Upstream(route),
                None => HostDispatch::Rejected,
            }
        }

        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct ForwardedHeaders {
            pub x_forwarded_for: String,
            pub x_real_ip: String,
            pub x_forwarded_proto: &'static str,
            pub forwarded: Option<String>,
            pub x_forwarded_host: Option<String>,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum HeaderTrustError {
            UntrustedPeer,
            MissingClientIp,
            InvalidClientIp,
        }

        /// Evaluates ingress header trust per ADR 0064 §4:
        /// - Trusts `CF-Connecting-IP` ONLY when the immediate peer equals the
        ///   qualified slirp forward peer (`10.0.2.2`).
        /// - Accepts exactly one syntactically valid IP address (no comma lists,
        ///   whitespace, or CRLF).
        /// - Replaces all incoming forwarding headers (`X-Forwarded-For`,
        ///   `X-Real-IP`, `Forwarded`, `X-Forwarded-Host`) rather than appending.
        pub fn resolve_client_ip(
            peer: IpAddr,
            trusted_peer: Ipv4Addr,
            cf_connecting_ip: Option<&str>,
        ) -> Result<ForwardedHeaders, HeaderTrustError> {
            if peer != IpAddr::V4(trusted_peer) {
                return Err(HeaderTrustError::UntrustedPeer);
            }
            let raw = cf_connecting_ip.ok_or(HeaderTrustError::MissingClientIp)?;
            if raw.is_empty() || raw.bytes().any(|b| b.is_ascii_whitespace() || b == b',') {
                return Err(HeaderTrustError::InvalidClientIp);
            }
            let parsed: IpAddr = raw.parse().map_err(|_| HeaderTrustError::InvalidClientIp)?;
            let canonical = parsed.to_string();
            Ok(ForwardedHeaders {
                x_forwarded_for: canonical.clone(),
                x_real_ip: canonical,
                x_forwarded_proto: "https",
                forwarded: None,
                x_forwarded_host: None,
            })
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum EffectiveClientIp {
            TrustedForwarded(IpAddr),
            SocketPeer(IpAddr),
        }

        /// Models upstream `TRUST_PROXY`: accepts `X-Forwarded-For` ONLY when
        /// `socket_peer` equals the measured dedicated nginx upstream peer and the
        /// header contains a single valid IP address. Untrusted peers always get
        /// socket-peer treatment.
        pub fn validate_upstream_peer(
            socket_peer: IpAddr,
            expected_nginx_peer: IpAddr,
            forwarded_for: Option<&str>,
        ) -> EffectiveClientIp {
            if socket_peer != expected_nginx_peer {
                return EffectiveClientIp::SocketPeer(socket_peer);
            }
            match forwarded_for
                .filter(|v| {
                    !v.is_empty() && !v.bytes().any(|b| b.is_ascii_whitespace() || b == b',')
                })
                .and_then(|v| v.parse::<IpAddr>().ok())
            {
                Some(ip) => EffectiveClientIp::TrustedForwarded(ip),
                None => EffectiveClientIp::SocketPeer(socket_peer),
            }
        }

        /// Operational log record that intentionally omits client/forwarded IPs,
        /// `Authorization`, `Cookie`, and request/response payloads.
        pub fn format_operational_event(route_slot: u8, method: &str, status: u16) -> String {
            let method = match method {
                "GET" | "HEAD" | "POST" | "PUT" | "DELETE" | "OPTIONS" => method,
                _ => "OTHER",
            };
            format!("route_slot={route_slot} method={method} status={status}")
        }

        /// Deterministic HTTP single-range response for `Range` / `If-Range`.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct RangeResponse {
            pub status: u16,
            pub accept_ranges: &'static str,
            pub content_range: Option<String>,
            pub content_length: usize,
            pub body: Vec<u8>,
        }

        /// RFC 9110 single byte-range evaluator:
        /// - Valid range (and matching `If-Range` if supplied): `206`,
        ///   `Accept-Ranges: bytes`, concrete `Content-Range: bytes start-end/len`,
        ///   exact `Content-Length`, and exact slice bytes.
        /// - Mismatched `If-Range`: `200` with full body.
        /// - Unsatisfiable offset (e.g. `start >= len`, `start > end`, empty body,
        ///   `bytes=-0`): `416` with `Content-Range: bytes */len`,
        ///   `Content-Length: 0`, empty body.
        /// - Multi-range or non-`bytes=` unit: falls back to `200` full body
        ///   without range rewriting.
        pub fn serve_byte_range(
            body: &[u8],
            etag: &str,
            range: Option<&str>,
            if_range: Option<&str>,
        ) -> RangeResponse {
            let full = || RangeResponse {
                status: 200,
                accept_ranges: "bytes",
                content_range: None,
                content_length: body.len(),
                body: body.to_vec(),
            };
            let Some(range_header) = range else {
                return full();
            };
            if let Some(if_range_val) = if_range
                && if_range_val.trim() != etag
            {
                return full();
            }
            let Some(spec) = range_header.trim().strip_prefix("bytes=") else {
                return full();
            };
            // Single-range only; multi-range commas are not rewritten.
            if spec.contains(',') {
                return full();
            }
            let Some((start_s, end_s)) = spec.split_once('-') else {
                return full();
            };
            let len = body.len();
            let unsatisfiable = || RangeResponse {
                status: 416,
                accept_ranges: "bytes",
                content_range: Some(format!("bytes */{len}")),
                content_length: 0,
                body: Vec::new(),
            };
            if len == 0 {
                return unsatisfiable();
            }
            let (start, end) = if start_s.is_empty() {
                // Suffix range: bytes=-<suffix>
                let Ok(suffix) = end_s.parse::<usize>() else {
                    return full();
                };
                if suffix == 0 {
                    return unsatisfiable();
                }
                let take = suffix.min(len);
                (len - take, len - 1)
            } else {
                let Ok(start) = start_s.parse::<usize>() else {
                    return full();
                };
                if start >= len {
                    return unsatisfiable();
                }
                let end = if end_s.is_empty() {
                    len - 1
                } else {
                    let Ok(parsed_end) = end_s.parse::<usize>() else {
                        return full();
                    };
                    if parsed_end < start {
                        return unsatisfiable();
                    }
                    parsed_end.min(len - 1)
                };
                (start, end)
            };
            let slice = &body[start..=end];
            RangeResponse {
                status: 206,
                accept_ranges: "bytes",
                content_range: Some(format!("bytes {start}-{end}/{len}")),
                content_length: slice.len(),
                body: slice.to_vec(),
            }
        }

        fn route(
            hostname: &str,
            project: &str,
            service: &str,
            project_slot: u8,
            service_index: u8,
            route_slot: u8,
        ) -> RouteTarget {
            RouteTarget {
                hostname: hostname.into(),
                project_id: project.into(),
                service: service.into(),
                project_slot,
                service_index,
                route_slot,
            }
        }

        fn two_host_table() -> RoutingTable {
            RoutingTable::new(
                "arx-0123456789abcdef0123456789abcdef.invalid",
                Some("example.invalid".into()),
                SLIRP_FORWARD_PEER,
                vec![
                    route("app.example.invalid", "calm-life", "web", 0, 0, 0),
                    route("beta.example.invalid", "probe-app", "api", 1, 0, 1),
                ],
            )
            .unwrap()
        }

        #[test]
        fn routing_table_enforces_list_bounds_order_zone_and_slot_uniqueness() {
            let ready = "arx-0123456789abcdef0123456789abcdef.invalid";
            // Empty list refused.
            assert_eq!(
                RoutingTable::new(ready, None, SLIRP_FORWARD_PEER, vec![]),
                Err(RoutingError::EmptyRoutes)
            );
            // 8 routes accepted; 9 refused.
            let eight: Vec<RouteTarget> = (0..8)
                .map(|i| {
                    route(
                        &format!("s{i}.example.invalid"),
                        &format!("proj-{i}"),
                        "web",
                        i,
                        0,
                        i,
                    )
                })
                .collect();
            let table = RoutingTable::new(
                ready,
                Some("example.invalid".into()),
                SLIRP_FORWARD_PEER,
                eight.clone(),
            )
            .unwrap();
            assert_eq!(table.routes().len(), 8);
            let mut nine = eight;
            nine.push(route("s8.example.invalid", "proj-0", "api", 0, 1, 0));
            assert_eq!(
                RoutingTable::new(
                    ready,
                    Some("example.invalid".into()),
                    SLIRP_FORWARD_PEER,
                    nine
                ),
                Err(RoutingError::TooManyRoutes)
            );
            // Unsorted or duplicate hostnames refused.
            assert_eq!(
                RoutingTable::new(
                    ready,
                    Some("example.invalid".into()),
                    SLIRP_FORWARD_PEER,
                    vec![
                        route("b.example.invalid", "p1", "web", 0, 0, 0),
                        route("a.example.invalid", "p2", "web", 1, 0, 1),
                    ],
                ),
                Err(RoutingError::UnsortedOrDuplicateHostname)
            );
            assert_eq!(
                RoutingTable::new(
                    ready,
                    Some("example.invalid".into()),
                    SLIRP_FORWARD_PEER,
                    vec![
                        route("a.example.invalid", "p1", "web", 0, 0, 0),
                        route("a.example.invalid", "p2", "web", 1, 0, 1),
                    ],
                ),
                Err(RoutingError::UnsortedOrDuplicateHostname)
            );
            // Cross-zone hostnames refused (with and without explicit zone).
            assert_eq!(
                RoutingTable::new(
                    ready,
                    Some("example.invalid".into()),
                    SLIRP_FORWARD_PEER,
                    vec![
                        route("a.example.invalid", "p1", "web", 0, 0, 0),
                        route("b.other.invalid", "p2", "web", 1, 0, 1),
                    ],
                ),
                Err(RoutingError::ZoneMismatch)
            );
            assert_eq!(
                RoutingTable::new(
                    ready,
                    None,
                    SLIRP_FORWARD_PEER,
                    vec![
                        route("a.foo.invalid", "p1", "web", 0, 0, 0),
                        route("b.bar.invalid", "p2", "web", 1, 0, 1),
                    ],
                ),
                Err(RoutingError::ZoneMismatch)
            );
            // Readiness Host collision refused.
            assert_eq!(
                RoutingTable::new(
                    ready,
                    None,
                    SLIRP_FORWARD_PEER,
                    vec![route(ready, "p1", "web", 0, 0, 0)],
                ),
                Err(RoutingError::ReadinessHostCollision)
            );
            // Untrusted forward peer refused.
            assert_eq!(
                RoutingTable::new(
                    ready,
                    None,
                    Ipv4Addr::LOCALHOST,
                    vec![route("a.example.invalid", "p1", "web", 0, 0, 0)],
                ),
                Err(RoutingError::InvalidTrustedPeer)
            );
            // Duplicate route slot or port collision refused.
            assert_eq!(
                RoutingTable::new(
                    ready,
                    None,
                    SLIRP_FORWARD_PEER,
                    vec![
                        route("a.example.invalid", "p1", "web", 0, 0, 0),
                        route("b.example.invalid", "p2", "web", 1, 0, 0),
                    ],
                ),
                Err(RoutingError::DuplicateRouteSlot)
            );
            assert_eq!(
                RoutingTable::new(
                    ready,
                    None,
                    SLIRP_FORWARD_PEER,
                    vec![
                        route("a.example.invalid", "p1", "web", 0, 0, 0),
                        route("b.example.invalid", "p1", "api", 0, 0, 1),
                    ],
                ),
                Err(RoutingError::PortCollision)
            );
            // Same project cannot map to two different project slots.
            assert_eq!(
                RoutingTable::new(
                    ready,
                    None,
                    SLIRP_FORWARD_PEER,
                    vec![
                        route("a.example.invalid", "p1", "web", 0, 0, 0),
                        route("b.example.invalid", "p1", "api", 1, 0, 1),
                    ],
                ),
                Err(RoutingError::ProjectSlotConflict)
            );
        }

        #[test]
        fn nginx_config_binds_origin_enforces_acl_host_dispatch_headers_and_ranges() {
            let table = two_host_table();
            let config = table.render_nginx_config();
            assert!(config.contains("listen 10.0.2.15:443 ssl default_server;"));
            assert!(config.contains("server_name _;\n    allow 10.0.2.2;\n    deny all;"));
            assert!(config.contains("return 444;"));
            assert!(config.contains("access_log off;"));
            assert!(config.contains("server_name arx-0123456789abcdef0123456789abcdef.invalid;"));
            assert!(config.contains("location = /arx-health"));
            assert!(config.contains("server_name app.example.invalid;"));
            assert!(config.contains("proxy_pass http://127.0.0.1:20000;"));
            assert!(config.contains("server_name beta.example.invalid;"));
            assert!(config.contains("proxy_pass http://127.0.0.1:20004;"));
            assert!(config.contains("proxy_set_header X-Forwarded-For $arx_cf_ip;"));
            assert!(config.contains("proxy_set_header Forwarded \"\";"));
            assert!(config.contains("proxy_set_header Range $http_range;"));
            assert!(config.contains("proxy_set_header If-Range $http_if_range;"));
            assert!(config.contains("proxy_force_ranges off;"));
            assert!(config.contains("proxy_buffering off;"));
            assert!(config.contains("proxy_cache off;"));
            assert!(config.contains("location ^~ /admin"));
            assert!(config.contains("location ^~ /events"));

            for route in table.routes() {
                let server = config
                    .split(&format!("server_name {};", route.hostname))
                    .nth(1)
                    .unwrap()
                    .split("  server {")
                    .next()
                    .unwrap();
                assert!(server.contains(&format!("if ($http_host != {})", route.hostname)));
                assert!(server.contains("if ($realip_remote_addr != 10.0.2.2)"));
                assert!(server.contains("if ($arx_cf_ip = \"\")"));
                assert!(server.contains(&format!(
                    "location = {} {{ return 204; }}",
                    route.probe_path()
                )));
                assert!(server.contains(&format!(
                    "proxy_pass http://127.0.0.1:{};",
                    route.loopback_port()
                )));
            }

            // Controlled Host dispatch.
            assert_eq!(
                dispatch_host(&table, Some(table.readiness_host())),
                HostDispatch::Readiness
            );
            assert_eq!(
                dispatch_host(&table, Some("app.example.invalid")),
                HostDispatch::Upstream(&table.routes()[0])
            );
            assert_eq!(
                dispatch_host(&table, Some("beta.example.invalid")),
                HostDispatch::Upstream(&table.routes()[1])
            );
            for rejected in [
                None,
                Some(""),
                Some("unknown.example.invalid"),
                Some("APP.example.invalid"),
                Some("app.example.invalid:443"),
                Some("app.example.invalid/evil"),
                Some("10.0.2.15"),
            ] {
                assert_eq!(dispatch_host(&table, rejected), HostDispatch::Rejected);
            }
        }

        struct FakeNginx {
            active: Option<String>,
            staging: Option<String>,
            validate_ok: bool,
            reload_ok: bool,
            commands: Vec<Vec<String>>,
        }

        impl FakeNginx {
            fn new(initial: Option<&str>) -> Self {
                Self {
                    active: initial.map(str::to_owned),
                    staging: None,
                    validate_ok: true,
                    reload_ok: true,
                    commands: Vec::new(),
                }
            }
        }

        impl NginxController for FakeNginx {
            fn read_active(&self) -> Result<Option<String>, NginxError> {
                Ok(self.active.clone())
            }
            fn write_staging(&mut self, config: &str) -> Result<(), NginxError> {
                self.staging = Some(config.to_owned());
                Ok(())
            }
            fn remove_staging(&mut self) {
                self.staging = None;
            }
            fn run_fixed(&mut self, argv: &[&str]) -> bool {
                self.commands
                    .push(argv.iter().map(|s| (*s).to_owned()).collect());
                if argv.contains(&"-t") {
                    self.validate_ok
                } else {
                    self.reload_ok
                }
            }
            fn activate_staging(&mut self) -> Result<(), NginxError> {
                self.active = Some(self.staging.take().ok_or(NginxError::ActivateFailed)?);
                Ok(())
            }
            fn restore_active(&mut self, previous: Option<&str>) -> Result<(), NginxError> {
                self.active = previous.map(str::to_owned);
                Ok(())
            }
        }

        #[test]
        fn nginx_atomic_validate_reload_and_rollback() {
            let table = two_host_table();
            // Validation failure leaves prior active untouched and never reloads.
            let mut fake = FakeNginx::new(Some("prior-config"));
            fake.validate_ok = false;
            assert_eq!(
                apply_nginx_config(&table, &mut fake),
                Err(NginxError::ValidationFailed)
            );
            assert_eq!(fake.active.as_deref(), Some("prior-config"));
            assert!(fake.staging.is_none());
            assert_eq!(fake.commands.len(), 1);
            assert_eq!(fake.commands[0], nginx_validate_argv());

            // Reload failure rolls back to prior active config.
            let mut fake = FakeNginx::new(Some("prior-config"));
            fake.reload_ok = false;
            assert_eq!(
                apply_nginx_config(&table, &mut fake),
                Err(NginxError::ReloadFailed)
            );
            assert_eq!(fake.active.as_deref(), Some("prior-config"));
            assert_eq!(fake.commands.len(), 2);

            // Success activates new config and reloads with fixed argv.
            let mut fake = FakeNginx::new(Some("prior-config"));
            let rendered = apply_nginx_config(&table, &mut fake).unwrap();
            assert_eq!(fake.active.as_deref(), Some(rendered.as_str()));
            assert!(fake.staging.is_none());
            assert_eq!(fake.commands[0], nginx_validate_argv());
            assert_eq!(fake.commands[1], nginx_reload_argv());
        }

        #[test]
        fn header_trust_and_log_redaction() {
            let trusted = IpAddr::V4(SLIRP_FORWARD_PEER);
            // Untrusted peers (Docker bridge, loopback, guest IP) are rejected.
            for peer in [
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V4(Ipv4Addr::new(172, 17, 0, 2)),
                IpAddr::V4(Ipv4Addr::new(10, 0, 2, 15)),
            ] {
                assert_eq!(
                    resolve_client_ip(peer, SLIRP_FORWARD_PEER, Some("203.0.113.7")),
                    Err(HeaderTrustError::UntrustedPeer)
                );
            }
            // Spoofed/multi-value CF-Connecting-IP rejected.
            for bad in [
                None,
                Some(""),
                Some("203.0.113.7, 198.51.100.1"),
                Some("203.0.113.7\r\nX-Injected: 1"),
                Some("not-an-ip"),
            ] {
                assert!(resolve_client_ip(trusted, SLIRP_FORWARD_PEER, bad).is_err());
            }
            // Single canonical IP replaces incoming forwarding headers.
            let headers =
                resolve_client_ip(trusted, SLIRP_FORWARD_PEER, Some("203.0.113.9")).unwrap();
            assert_eq!(headers.x_forwarded_for, "203.0.113.9");
            assert_eq!(headers.x_real_ip, "203.0.113.9");
            assert_eq!(headers.x_forwarded_proto, "https");
            assert!(headers.forwarded.is_none());
            assert!(headers.x_forwarded_host.is_none());

            // Upstream TRUST_PROXY only honors forwarded IP from dedicated nginx peer.
            let nginx_bridge = IpAddr::V4(Ipv4Addr::new(172, 18, 0, 1));
            let container_peer = IpAddr::V4(Ipv4Addr::new(172, 18, 0, 9));
            assert_eq!(
                validate_upstream_peer(nginx_bridge, nginx_bridge, Some("203.0.113.9")),
                EffectiveClientIp::TrustedForwarded("203.0.113.9".parse().unwrap())
            );
            assert_eq!(
                validate_upstream_peer(container_peer, nginx_bridge, Some("203.0.113.9")),
                EffectiveClientIp::SocketPeer(container_peer)
            );
            assert_eq!(
                validate_upstream_peer(nginx_bridge, nginx_bridge, Some("203.0.113.9, 1.2.3.4")),
                EffectiveClientIp::SocketPeer(nginx_bridge)
            );

            // Operational log contains no IP, auth, or payload.
            let log = format_operational_event(0, "GET", 206);
            assert_eq!(log, "route_slot=0 method=GET status=206");
            assert!(!log.contains("203.0.113.9"));
        }

        #[test]
        fn deterministic_206_and_416_byte_ranges_with_if_range() {
            let body = b"0123456789abcdef0123456789ABCDEF";
            let etag = "\"v1-media\"";

            let full = serve_byte_range(body, etag, None, None);
            assert_eq!(full.status, 200);
            assert_eq!(full.accept_ranges, "bytes");
            assert_eq!(full.content_range, None);
            assert_eq!(full.content_length, 32);
            assert_eq!(full.body, body);

            let partial = serve_byte_range(body, etag, Some("bytes=4-11"), Some(etag));
            assert_eq!(partial.status, 206);
            assert_eq!(partial.accept_ranges, "bytes");
            assert_eq!(partial.content_range.as_deref(), Some("bytes 4-11/32"));
            assert_eq!(partial.content_length, 8);
            assert_eq!(partial.body, b"456789ab");

            // Suffix range.
            let suffix = serve_byte_range(body, etag, Some("bytes=-6"), None);
            assert_eq!(suffix.status, 206);
            assert_eq!(suffix.content_range.as_deref(), Some("bytes 26-31/32"));
            assert_eq!(suffix.content_length, 6);
            assert_eq!(suffix.body, b"ABCDEF");

            // If-Range mismatch returns 200 full representation.
            let stale = serve_byte_range(body, etag, Some("bytes=4-11"), Some("\"old\""));
            assert_eq!(stale.status, 200);
            assert_eq!(stale.content_range, None);
            assert_eq!(stale.content_length, 32);
            assert_eq!(stale.body, body);

            // Unsatisfiable offsets return 416.
            for bad in ["bytes=32-40", "bytes=100-", "bytes=10-5", "bytes=-0"] {
                let err = serve_byte_range(body, etag, Some(bad), None);
                assert_eq!(err.status, 416, "{bad}");
                assert_eq!(err.accept_ranges, "bytes");
                assert_eq!(err.content_range.as_deref(), Some("bytes */32"));
                assert_eq!(err.content_length, 0);
                assert!(err.body.is_empty());
            }
        }
    }
}
