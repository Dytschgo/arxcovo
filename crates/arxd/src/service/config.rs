//! Strict, bounded service-only schema. No foreground or CLI config overrides.
use super::layout::{Error, Layout, Platform};
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
    fn account(&self, name_or_identity: &str) -> Result<Account, Error>;
    /// Metadata-only: existing directory, local disk, no symlinks/reparse ancestors.
    fn workspace_safe(&self, path: &str) -> Result<bool, Error>;
}
impl ServiceConfig {
    pub fn parse(bytes: &[u8], layout: &Layout, resolver: &impl Resolver) -> Result<Self, Error> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(Error::Config);
        }
        let wire: Wire = serde_json::from_slice(bytes).map_err(|_| Error::Config)?;
        let config = Self {
            wire,
            platform: layout.platform(),
        };
        config.validate(layout, resolver)?;
        Ok(config)
    }
    pub fn revalidate(&self, layout: &Layout, resolver: &impl Resolver) -> Result<(), Error> {
        self.validate(layout, resolver)
    }
    fn validate(&self, layout: &Layout, resolver: &impl Resolver) -> Result<(), Error> {
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
        if self.platform == Platform::Macos && w.rights == Rights::Admin {
            return Err(Error::Rights);
        }
        if !identity_valid(&w.client.identity, self.platform)
            || !identity_valid(&w.build_account.identity, self.platform)
            || w.client.identity == w.build_account.identity
            || service_identity(&w.client.identity, self.platform)
            || service_identity(&w.build_account.identity, self.platform)
        {
            return Err(Error::Identity);
        }
        let build = resolver
            .account(&w.build_account.name)
            .map_err(|_| Error::Identity)?;
        let client = resolver
            .account(&w.client.identity)
            .map_err(|_| Error::Identity)?;
        if build.kind != AccountKind::User
            || client.kind != AccountKind::User
            || build.identity != w.build_account.identity
            || client.identity != w.client.identity
            || build.gid != w.build_account.gid
            || (self.platform == Platform::Windows && build.gid.is_some())
        {
            return Err(Error::Identity);
        }
        if build.administrative != (w.rights == Rights::Admin) {
            return Err(Error::Rights);
        }
        if !layout.workspace_disjoint(&w.workspace)
            || !resolver
                .workspace_safe(&w.workspace)
                .map_err(|_| Error::Workspace)?
        {
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
        }) {
            return Err(Error::Config);
        }
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
        }
        impl Resolver for CountingResolver {
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
                        // Model an old validated config reaching a newer
                        // configure path: revalidation must refuse it too.
                        let previous = ServiceConfig {
                            wire: serde_json::from_value(value).unwrap(),
                            platform,
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
