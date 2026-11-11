//! Strict, bounded service-only schema. No foreground or CLI config overrides.
use super::layout::{Error, Layout, Platform};
use super::preflight::ExecutionUnavailable;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_CONFIG_BYTES: usize = 16 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rights {
    Standard,
    Admin,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Client {
    identity: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildAccount {
    name: String,
    identity: String,
    gid: Option<u32>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Agent {
    principal: String,
    token_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    max_concurrent_runs: u8,
    memory_bytes: u64,
    pids: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema: String,
    client: Client,
    build_account: BuildAccount,
    rights: Rights,
    workspace: String,
    environment: BTreeMap<String, String>,
    limits: Limits,
    agents: Vec<Agent>,
    granted_batch_right: bool,
}
/// No Debug implementation: names, paths and hashes must not appear in errors.
#[derive(Clone)]
pub struct ServiceConfig {
    wire: Wire,
    platform: Platform,
    revisions: [Option<String>; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    User,
    Group,
}
/// Native resolvers must observe current group membership, not trust config claims.
pub struct Account {
    pub identity: String,
    pub gid: Option<u32>,
    pub kind: AccountKind,
    pub administrative: bool,
}
pub trait Resolver {
    /// Internal point-in-time native incarnation evidence; never serialized.
    fn revision(&self, _value: &str) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn account(&self, name_or_identity: &str) -> Result<Account, Error>;
    /// Owner configure only: existing directory, local disk, no symlinks/reparse ancestors.
    /// Service inventory/preflight must never call this method.
    fn workspace_safe(&self, path: &str) -> Result<bool, Error>;
}
fn validate_platform_rights(platform: Platform, rights: Rights) -> Result<(), Error> {
    if platform == Platform::Macos && rights == Rights::Admin {
        return Err(Error::Rights);
    }
    Ok(())
}
impl ServiceConfig {
    pub fn parse(bytes: &[u8], layout: &Layout, resolver: &impl Resolver) -> Result<Self, Error> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(Error::Config);
        }
        let wire: Wire = serde_json::from_slice(bytes).map_err(|_| Error::Config)?;
        // Revision resolution is also native account observation.
        validate_platform_rights(layout.platform(), wire.rights)?;
        let revisions = [
            resolver.revision(&wire.build_account.name)?,
            resolver.revision(&wire.client.identity)?,
        ];
        let config = Self {
            wire,
            platform: layout.platform(),
            revisions,
        };
        config.revalidate(layout, resolver)?;
        Ok(config)
    }
    pub fn revalidate(&self, layout: &Layout, resolver: &impl Resolver) -> Result<(), Error> {
        self.validate(layout, resolver)?;
        if self.revisions
            != [
                resolver.revision(&self.wire.build_account.name)?,
                resolver.revision(&self.wire.client.identity)?,
            ]
        {
            return Err(Error::Identity);
        }
        Ok(())
    }
    fn validate(&self, layout: &Layout, resolver: &impl Resolver) -> Result<(), Error> {
        self.validate_inventory(layout, resolver)?;
        self.execution_preflight(layout, resolver)
            .map_err(|reason| match reason {
                ExecutionUnavailable::RightsMismatch
                | ExecutionUnavailable::AdminUnsupportedOnPlatform => Error::Rights,
                _ => Error::Identity,
            })?;
        if !resolver
            .workspace_safe(&self.wire.workspace)
            .map_err(|_| Error::Workspace)?
        {
            return Err(Error::Workspace);
        }
        Ok(())
    }
    /// Startup validation deliberately makes no build-account observations.
    pub fn parse_inventory(
        bytes: &[u8],
        layout: &Layout,
        resolver: &impl Resolver,
    ) -> Result<Self, Error> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(Error::Config);
        }
        let wire: Wire = serde_json::from_slice(bytes).map_err(|_| Error::Config)?;
        validate_platform_rights(layout.platform(), wire.rights)?;
        let config = Self {
            wire,
            platform: layout.platform(),
            revisions: [None, None],
        };
        config.validate_inventory(layout, resolver)?;
        Ok(config)
    }
    fn validate_inventory(&self, layout: &Layout, resolver: &impl Resolver) -> Result<(), Error> {
        let w = &self.wire;
        if self.platform != layout.platform()
            || w.schema != "arxd-service/1"
            || w.build_account.name.is_empty()
            || w.build_account.name.len() > 256
            || w.build_account.name.contains('\0')
        {
            return Err(Error::Config);
        }
        // A build-UID sweep cannot contain descendants that acquired root.
        // Refuse before resolving accounts or preparing owner operations.
        validate_platform_rights(self.platform, w.rights)?;
        if !identity_valid(&w.client.identity, self.platform)
            || !identity_valid(&w.build_account.identity, self.platform)
            || w.client.identity == w.build_account.identity
            || service_identity(&w.client.identity, self.platform)
            || service_identity(&w.build_account.identity, self.platform)
        {
            return Err(Error::Identity);
        }
        let client = resolver
            .account(&w.client.identity)
            .map_err(|_| Error::Identity)?;
        if client.kind != AccountKind::User
            || client.identity != w.client.identity
            || (self.platform == Platform::Windows && w.build_account.gid.is_some())
        {
            return Err(Error::Identity);
        }
        if !layout.workspace_disjoint(&w.workspace) {
            return Err(Error::Workspace);
        }
        let allowed: &[&str] = if self.platform == Platform::Windows {
            &[
                "PATH",
                "PATHEXT",
                "SystemRoot",
                "SystemDrive",
                "TEMP",
                "TMP",
                "CARGO_HOME",
                "RUSTUP_HOME",
                "LANG",
                "TZ",
            ]
        } else {
            &[
                "PATH",
                "HOME",
                "TMPDIR",
                "CARGO_HOME",
                "RUSTUP_HOME",
                "LANG",
                "LC_ALL",
                "TZ",
            ]
        };
        if w.environment.len() > 16
            || w.environment
                .iter()
                .any(|(k, v)| !allowed.contains(&k.as_str()) || v.len() > 4096 || v.contains('\0'))
        {
            return Err(Error::Config);
        }
        if !(1..=4).contains(&w.limits.max_concurrent_runs)
            || (self.platform == Platform::Macos && w.limits.max_concurrent_runs != 1)
            || w.limits.memory_bytes == 0
            || w.limits.pids == 0
            || w.agents.is_empty()
            || w.agents.len() > 64
            || (self.platform != Platform::Windows && w.granted_batch_right)
        {
            return Err(Error::Config);
        }
        let mut names = BTreeSet::new();
        let mut hashes = BTreeSet::new();
        if w.agents.iter().any(|a| {
            a.principal == "arxd"
                || a.principal.is_empty()
                || a.principal.len() > 64
                || !a
                    .principal
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                || !names.insert(&a.principal)
                || a.token_sha256.len() != 64
                || !a.token_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || !hashes.insert(a.token_sha256.to_ascii_lowercase())
        }) {
            return Err(Error::Config);
        }
        Ok(())
    }
    /// Dynamic execution facts are independent of inventory availability.
    pub fn execution_preflight(
        &self,
        layout: &Layout,
        resolver: &impl Resolver,
    ) -> Result<(), ExecutionUnavailable> {
        validate_platform_rights(self.platform, self.wire.rights)
            .map_err(|_| ExecutionUnavailable::AdminUnsupportedOnPlatform)?;
        let build =
            resolver
                .account(&self.wire.build_account.name)
                .map_err(|error| match error {
                    Error::AccountMissing => ExecutionUnavailable::AccountMissing,
                    _ => ExecutionUnavailable::PreflightFailed,
                })?;
        if build.kind != AccountKind::User {
            return Err(ExecutionUnavailable::PreflightFailed);
        }
        if build.identity != self.wire.build_account.identity
            || build.gid != self.wire.build_account.gid
        {
            return Err(ExecutionUnavailable::AccountIdentityChanged);
        }
        if build.administrative != (self.wire.rights == Rights::Admin) {
            return Err(ExecutionUnavailable::RightsMismatch);
        }
        if !layout.workspace_disjoint(&self.wire.workspace) {
            return Err(ExecutionUnavailable::PreflightFailed);
        }
        // Workspace/PATH observation belongs after switching identity. Until
        // that backend exists, the caller always reports execution unavailable.
        Ok(())
    }
    pub fn client_identity(&self) -> &str {
        &self.wire.client.identity
    }
    pub fn build_name(&self) -> &str {
        &self.wire.build_account.name
    }
    pub fn build_gid(&self) -> Option<u32> {
        self.wire.build_account.gid
    }
    pub fn workspace(&self) -> &str {
        &self.wire.workspace
    }
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.wire.environment
    }
    pub fn agents(&self) -> impl Iterator<Item = (&str, &str)> {
        self.wire
            .agents
            .iter()
            .map(|a| (a.principal.as_str(), a.token_sha256.as_str()))
    }
    pub fn limits(&self) -> (u8, u64, u32) {
        (
            self.wire.limits.max_concurrent_runs,
            self.wire.limits.memory_bytes,
            self.wire.limits.pids,
        )
    }
    pub fn rights(&self) -> Rights {
        self.wire.rights
    }
    pub fn build_identity(&self) -> &str {
        &self.wire.build_account.identity
    }
    pub fn granted_batch_right(&self) -> bool {
        self.wire.granted_batch_right
    }
    pub(crate) fn serialized(&self, granted: bool) -> Result<Vec<u8>, Error> {
        let mut w = self.wire.clone();
        w.granted_batch_right = granted;
        let bytes = serde_json::to_vec(&w).map_err(|_| Error::Config)?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(Error::Config);
        }
        Ok(bytes)
    }
}
fn service_identity(s: &str, os: Platform) -> bool {
    if os == Platform::Windows {
        matches!(s, "S-1-5-18" | "S-1-5-19" | "S-1-5-20")
            || s.starts_with("S-1-5-80-")
            || s.starts_with("S-1-5-82-")
    } else {
        s == "0"
    }
}
fn identity_valid(s: &str, os: Platform) -> bool {
    if os == Platform::Windows {
        let parts: Vec<_> = s.split('-').collect();
        parts.len() >= 4
            && parts.len() <= 18
            && parts[0] == "S"
            && parts[1] == "1"
            && parts[2..].iter().all(|p| {
                !p.is_empty()
                    && (p.len() == 1 || !p.starts_with('0'))
                    && p.bytes().all(|b| b.is_ascii_digit())
                    && p.parse::<u32>().is_ok()
            })
    } else {
        !s.is_empty()
            && (s.len() == 1 || !s.starts_with('0'))
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u32>().is_ok()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub struct FakeResolver {
        pub admin: bool,
        pub group: bool,
        pub changed: bool,
        pub safe: bool,
    }
    impl Default for FakeResolver {
        fn default() -> Self {
            Self {
                admin: false,
                group: false,
                changed: false,
                safe: true,
            }
        }
    }
    impl Resolver for FakeResolver {
        fn account(&self, n: &str) -> Result<Account, Error> {
            Ok(Account {
                identity: if n == "builder" {
                    if self.changed { "1003" } else { "1002" }
                } else {
                    n
                }
                .into(),
                gid: Some(1002),
                kind: if self.group {
                    AccountKind::Group
                } else {
                    AccountKind::User
                },
                administrative: self.admin,
            })
        }
        fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
            Ok(self.safe)
        }
    }
    pub fn fixture() -> serde_json::Value {
        serde_json::json!({"schema":"arxd-service/1","client":{"identity":"1001"},"build_account":{"name":"builder","identity":"1002","gid":1002},"rights":"standard","workspace":"/work","environment":{"PATH":"/usr/bin"},"limits":{"max_concurrent_runs":1,"memory_bytes":1024,"pids":32},"agents":[{"principal":"test","token_sha256":"a".repeat(64)}],"granted_batch_right":false})
    }
    pub fn parse(v: serde_json::Value, r: &FakeResolver) -> Result<ServiceConfig, Error> {
        ServiceConfig::parse(
            &serde_json::to_vec(&v).unwrap(),
            &Layout::for_test(Platform::Linux),
            r,
        )
    }
    #[test]
    fn macos_admin_refuses_before_observation_or_owner_operations() {
        use std::cell::Cell;
        struct CountingResolver {
            platform: Platform,
            admin: bool,
            calls: Cell<usize>,
            revision_calls: Cell<usize>,
        }
        impl Resolver for CountingResolver {
            fn revision(&self, _: &str) -> Result<Option<String>, Error> {
                self.revision_calls.set(self.revision_calls.get() + 1);
                Ok(None)
            }
            fn account(&self, value: &str) -> Result<Account, Error> {
                self.calls.set(self.calls.get() + 1);
                Ok(Account {
                    identity: if value == "builder" {
                        if self.platform == Platform::Windows {
                            "S-1-5-21-100-200-300-1002".into()
                        } else {
                            "1002".into()
                        }
                    } else {
                        value.into()
                    },
                    gid: (self.platform != Platform::Windows).then_some(1002),
                    kind: AccountKind::User,
                    administrative: self.admin,
                })
            }
            fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
                Ok(true)
            }
        }
        for platform in [Platform::Macos, Platform::Linux, Platform::Windows] {
            let layout = Layout::for_test(platform);
            for rights in [Rights::Admin, Rights::Standard] {
                for admin in [false, true] {
                    let resolver = CountingResolver {
                        platform,
                        admin,
                        calls: Cell::new(0),
                        revision_calls: Cell::new(0),
                    };
                    let mut value = fixture();
                    value["rights"] = serde_json::to_value(rights).unwrap();
                    if platform == Platform::Windows {
                        value["client"]["identity"] = "S-1-5-21-100-200-300-1001".into();
                        value["build_account"]["identity"] = "S-1-5-21-100-200-300-1002".into();
                        value["build_account"]["gid"] = serde_json::Value::Null;
                        value["workspace"] = r"D:\work".into();
                    }
                    let result = ServiceConfig::parse(
                        &serde_json::to_vec(&value).unwrap(),
                        &layout,
                        &resolver,
                    );
                    if platform == Platform::Macos && rights == Rights::Admin {
                        assert_eq!(result.err(), Some(Error::Rights));
                        assert_eq!(resolver.calls.get(), 0);
                        assert_eq!(resolver.revision_calls.get(), 0);
                        // Model an old validated config reaching a newer
                        // configure path: revalidation must refuse it too.
                        let previous = ServiceConfig {
                            wire: serde_json::from_value(value).unwrap(),
                            platform,
                            revisions: [None, None],
                        };
                        let context = super::super::owner_plan::OwnerContext {
                            admin: true,
                            running_binary: layout.binary(),
                        };
                        let plan = super::super::owner_plan::configure_plan(
                            &layout, &context, &previous, None, false, &resolver,
                        );
                        assert_eq!(plan.as_ref().err(), Some(&Error::Rights));
                        assert_eq!(plan.as_ref().map(|p| p.operations().len()).unwrap_or(0), 0);
                        assert_eq!(resolver.calls.get(), 0);
                        assert_eq!(resolver.revision_calls.get(), 0);
                    } else {
                        assert_eq!(resolver.calls.get(), 2);
                        if admin == (rights == Rights::Admin) {
                            assert!(result.is_ok());
                        } else {
                            assert_eq!(result.err(), Some(Error::Rights));
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn distinct_principals_require_distinct_decoded_credentials() {
        for hash in ["a".repeat(64), "A".repeat(64)] {
            let mut value = fixture();
            value["agents"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "principal": "second",
                    "token_sha256": hash,
                }));
            assert_eq!(
                parse(value.clone(), &FakeResolver::default()).err(),
                Some(Error::Config)
            );
            assert_eq!(
                ServiceConfig::parse_inventory(
                    &serde_json::to_vec(&value).unwrap(),
                    &Layout::for_test(Platform::Linux),
                    &FakeResolver::default(),
                )
                .err(),
                Some(Error::Config),
            );
        }
        let mut value = fixture();
        value["agents"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "principal": "second",
                "token_sha256": "b".repeat(64),
            }));
        assert!(parse(value.clone(), &FakeResolver::default()).is_ok());
        assert!(
            ServiceConfig::parse_inventory(
                &serde_json::to_vec(&value).unwrap(),
                &Layout::for_test(Platform::Linux),
                &FakeResolver::default(),
            )
            .is_ok()
        );
    }
    #[test]
    fn strict_schema_and_redacted_errors() {
        assert!(parse(fixture(), &FakeResolver::default()).is_ok());
        for key in ["mode", "dev_execution", "listen", "local", "secret-marker"] {
            let mut v = fixture();
            v[key] = "secret-marker".into();
            let e = parse(v, &FakeResolver::default()).err().unwrap();
            assert_eq!(e, Error::Config);
            assert!(!format!("{e:?} {e}").contains("secret-marker"));
        }
        let mut v = fixture();
        v["client"]["extra"] = "secret-marker".into();
        assert!(parse(v, &FakeResolver::default()).is_err());
        assert!(
            ServiceConfig::parse(
                &vec![b' '; MAX_CONFIG_BYTES + 1],
                &Layout::for_test(Platform::Linux),
                &FakeResolver::default()
            )
            .is_err()
        );
        assert!(
            parse(
                fixture(),
                &FakeResolver {
                    changed: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            parse(
                fixture(),
                &FakeResolver {
                    group: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert_eq!(
            parse(
                fixture(),
                &FakeResolver {
                    admin: true,
                    ..Default::default()
                }
            )
            .err(),
            Some(Error::Rights)
        );
        assert_eq!(
            parse(
                fixture(),
                &FakeResolver {
                    safe: false,
                    ..Default::default()
                }
            )
            .err(),
            Some(Error::Workspace)
        );
    }
    #[test]
    fn rejects_unsafe_config_values() {
        for (pointer, value) in [
            ("/agents/0/principal", serde_json::json!("arxd")),
            ("/agents/0/token_sha256", serde_json::json!("z".repeat(64))),
            ("/rights", serde_json::json!("implicit")),
            ("/build_account/identity", serde_json::json!("1001")),
            ("/client/identity", serde_json::json!("0")),
            ("/workspace", serde_json::json!("/etc/arxcovo/child")),
            ("/environment/PATH", serde_json::json!("bad\u{0}value")),
            ("/environment/PATH", serde_json::json!("x".repeat(4097))),
            ("/limits/max_concurrent_runs", serde_json::json!(5)),
        ] {
            let mut v = fixture();
            *v.pointer_mut(pointer).unwrap() = value;
            assert!(parse(v, &FakeResolver::default()).is_err(), "{pointer}");
        }
        let mut v = fixture();
        v["environment"]["LD_PRELOAD"] = "/bad".into();
        assert!(parse(v, &FakeResolver::default()).is_err());
        let mut v = fixture();
        v["agents"] = serde_json::json!([v["agents"][0], v["agents"][0]]);
        assert!(parse(v, &FakeResolver::default()).is_err());
        let c = parse(fixture(), &FakeResolver::default()).unwrap();
        assert!(
            c.revalidate(
                &Layout::for_test(Platform::Linux),
                &FakeResolver {
                    changed: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod revision_tests {
    use super::*;
    use std::cell::Cell;
    struct Observations {
        revision: Cell<u8>,
    }
    impl Resolver for Observations {
        fn account(&self, value: &str) -> Result<Account, Error> {
            tests::FakeResolver::default().account(value)
        }
        fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
            Ok(true)
        }
        fn revision(&self, _: &str) -> Result<Option<String>, Error> {
            Ok(Some(self.revision.get().to_string()))
        }
    }
    #[test]
    fn revalidation_refuses_changed_incarnation_even_with_same_numeric_pins() {
        let r = Observations {
            revision: Cell::new(0),
        };
        let layout = Layout::for_test(Platform::Macos);
        let c = ServiceConfig::parse(&serde_json::to_vec(&tests::fixture()).unwrap(), &layout, &r)
            .unwrap();
        assert!(c.revalidate(&layout, &r).is_ok());
        r.revision.set(1);
        assert!(c.revalidate(&layout, &r) == Err(Error::Identity));
    }
}

#[cfg(test)]
mod inventory_tests {
    use super::*;
    struct Observe(std::sync::Mutex<Vec<String>>);
    impl Resolver for Observe {
        fn account(&self, value: &str) -> Result<Account, Error> {
            self.0.lock().unwrap().push(value.into());
            if value == "builder" {
                return Err(Error::Identity);
            }
            Ok(Account {
                identity: value.into(),
                gid: Some(1001),
                kind: AccountKind::User,
                administrative: false,
            })
        }
        fn revision(&self, _: &str) -> Result<Option<String>, Error> {
            panic!("no startup incarnation lookup")
        }
        fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
            panic!("no startup workspace lookup")
        }
    }
    #[test]
    fn missing_build_account_is_dynamic_and_macos_admin_refuses_before_observation() {
        let r = Observe(Default::default());
        let layout = Layout::for_test(Platform::Linux);
        let bytes = serde_json::to_vec(&tests::fixture()).unwrap();
        let config = ServiceConfig::parse_inventory(&bytes, &layout, &r).unwrap();
        assert_eq!(*r.0.lock().unwrap(), ["1001"]);
        assert_eq!(
            config.execution_preflight(&layout, &r),
            Err(ExecutionUnavailable::PreflightFailed)
        );
        let changed = tests::FakeResolver {
            changed: true,
            ..Default::default()
        };
        let mismatched = ServiceConfig::parse_inventory(&bytes, &layout, &changed).unwrap();
        assert_eq!(
            mismatched.execution_preflight(&layout, &changed),
            Err(ExecutionUnavailable::AccountIdentityChanged)
        );
        r.0.lock().unwrap().clear();
        let mut v = tests::fixture();
        v["rights"] = "admin".into();
        assert!(matches!(
            ServiceConfig::parse_inventory(
                &serde_json::to_vec(&v).unwrap(),
                &Layout::for_test(Platform::Macos),
                &r
            ),
            Err(Error::Rights)
        ));
        assert!(r.0.lock().unwrap().is_empty());
    }
    #[test]
    fn inventory_schema_still_refuses_http_development_and_bad_static_values() {
        let r = tests::FakeResolver::default();
        let layout = Layout::for_test(Platform::Linux);
        for key in [
            "listen",
            "development_execution",
            "config",
            "state_dir",
            "local",
        ] {
            let mut v = tests::fixture();
            v[key] = serde_json::json!({});
            assert!(
                ServiceConfig::parse_inventory(&serde_json::to_vec(&v).unwrap(), &layout, &r)
                    .is_err()
            );
        }
        let mut v = tests::fixture();
        v["agents"][0]["token_sha256"] = "invalid".into();
        assert!(
            ServiceConfig::parse_inventory(&serde_json::to_vec(&v).unwrap(), &layout, &r).is_err()
        );
    }
}

#[cfg(test)]
mod service_boundary_tests {
    use super::*;
    use std::cell::Cell;
    struct Observe {
        workspace: Cell<usize>,
        safe: bool,
        failure: Option<Error>,
        changed: bool,
        admin: bool,
    }
    impl Resolver for Observe {
        fn account(&self, value: &str) -> Result<Account, Error> {
            if value == "builder"
                && let Some(error) = self.failure
            {
                return Err(error);
            }
            tests::FakeResolver {
                changed: self.changed,
                admin: self.admin,
                ..Default::default()
            }
            .account(value)
        }
        fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
            self.workspace.set(self.workspace.get() + 1);
            assert!(self.safe, "service must not observe workspace");
            Ok(true)
        }
    }
    #[test]
    fn service_never_observes_workspace_and_owner_configure_still_does() {
        let layout = Layout::for_test(Platform::Linux);
        let bytes = serde_json::to_vec(&tests::fixture()).unwrap();
        let mut r = Observe {
            workspace: Cell::new(0),
            safe: false,
            failure: None,
            changed: false,
            admin: false,
        };
        let config = ServiceConfig::parse_inventory(&bytes, &layout, &r).unwrap();
        for _ in 0..3 {
            assert_eq!(config.execution_preflight(&layout, &r), Ok(()));
        }
        assert_eq!(r.workspace.get(), 0);
        for (failure, changed, admin, expected) in [
            (
                Some(Error::AccountMissing),
                false,
                false,
                ExecutionUnavailable::AccountMissing,
            ),
            (
                Some(Error::Identity),
                false,
                false,
                ExecutionUnavailable::PreflightFailed,
            ),
            (
                Some(Error::Permissions),
                false,
                false,
                ExecutionUnavailable::PreflightFailed,
            ),
            (
                None,
                true,
                false,
                ExecutionUnavailable::AccountIdentityChanged,
            ),
            (None, false, true, ExecutionUnavailable::RightsMismatch),
        ] {
            r.failure = failure;
            r.changed = changed;
            r.admin = admin;
            assert_eq!(config.execution_preflight(&layout, &r), Err(expected));
            assert_eq!(r.workspace.get(), 0);
        }
        r.failure = None;
        r.changed = false;
        r.admin = false;
        r.safe = true;
        let owner = ServiceConfig::parse(&bytes, &layout, &r).unwrap();
        assert_eq!(r.workspace.get(), 1);
        let context = super::super::owner_plan::OwnerContext {
            admin: true,
            running_binary: layout.binary(),
        };
        assert!(
            super::super::owner_plan::configure_plan(&layout, &context, &owner, None, false, &r)
                .is_ok()
        );
        assert_eq!(r.workspace.get(), 2);
        assert!(
            ServiceConfig::parse(
                &bytes,
                &layout,
                &tests::FakeResolver {
                    safe: false,
                    ..Default::default()
                }
            )
            .err()
                == Some(Error::Workspace)
        );
    }
    #[test]
    fn macos_legacy_admin_preflight_observes_no_accounts_or_workspace() {
        struct Poison;
        impl Resolver for Poison {
            fn account(&self, _: &str) -> Result<Account, Error> {
                panic!("no account observation")
            }
            fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
                panic!("no workspace observation")
            }
        }
        let mut wire: Wire = serde_json::from_value(tests::fixture()).unwrap();
        wire.rights = Rights::Admin;
        let config = ServiceConfig {
            wire,
            platform: Platform::Macos,
            revisions: [None, None],
        };
        assert_eq!(
            config.execution_preflight(&Layout::for_test(Platform::Macos), &Poison),
            Err(ExecutionUnavailable::AdminUnsupportedOnPlatform)
        );
    }
}
