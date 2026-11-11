use anyhow::{Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::Read,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
};
use subtle::ConstantTimeEq;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(skip)]
    pub execution: Option<std::sync::Arc<crate::development::Execution>>,
    pub development_execution: Option<crate::development::Settings>,
    pub listen: Option<SocketAddr>,
    pub state_dir: PathBuf,
    pub agents: Vec<Agent>,
    pub local: Option<crate::local::Endpoint>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub principal: String,
    pub token_sha256: String,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        checked_path(path)?;
        crate::security::private(path)?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(16385)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 16384 {
            bail!("configuration too large");
        }
        let mut value: Self =
            serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid configuration"))?;
        value.validate()?;
        if let Some(dev) = &value.development_execution {
            dev.disjoint(
                path.parent()
                    .ok_or_else(|| anyhow::anyhow!("invalid configuration"))?,
            )?;
        }
        value.execution = value
            .development_execution
            .clone()
            .map(crate::development::Execution::new)
            .transpose()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(dev) = &self.development_execution {
            if !cfg!(windows) || self.listen.is_some() {
                bail!("invalid development configuration");
            }
            dev.validate()?;
            dev.disjoint(&self.state_dir)?;
        }
        if let Some(endpoint) = &self.local {
            endpoint.validate()?;
        }
        if self.listen.is_some_and(|a| !a.ip().is_loopback()) {
            bail!("listener must be loopback");
        }
        if !(1..=32).contains(&self.agents.len()) {
            bail!("agent count out of bounds");
        }
        let mut names = HashSet::new();
        let mut hashes = HashSet::new();
        for agent in &self.agents {
            if agent.principal.is_empty()
                || agent.principal.len() > 64
                || !agent
                    .principal
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                || !names.insert(&agent.principal)
            {
                bail!("invalid principal");
            }
            if agent.token_sha256.len() != 64
                || hex::decode(&agent.token_sha256).is_err()
                || !hashes.insert(agent.token_sha256.to_ascii_lowercase())
            {
                bail!("invalid token hash");
            }
        }
        checked_path(&self.state_dir)?;
        if !self.state_dir.is_dir() {
            bail!("state directory required");
        }
        crate::security::private(&self.state_dir)?;
        Ok(())
    }
    pub fn authenticate(&self, token: &str) -> Option<String> {
        // Provision 32 random bytes as 64 lowercase hex characters. Never accept passwords.
        if token.len() != 64
            || !token
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return None;
        }
        let hash = Sha256::digest(token.as_bytes());
        let mut principal = None;
        for agent in &self.agents {
            let expected = hex::decode(&agent.token_sha256).ok()?;
            if bool::from(hash.as_slice().ct_eq(&expected)) {
                principal = Some(agent.principal.clone());
            }
        }
        principal
    }
}
pub fn checked_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        bail!("absolute normalized path required");
    }
    let mut prefix = PathBuf::new();
    for part in path.components() {
        #[cfg(windows)]
        {
            use std::path::Prefix;
            match part {
                Component::Prefix(p)
                    if !matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) =>
                {
                    bail!("local disk paths required")
                }
                Component::Normal(value) if value.to_string_lossy().contains(':') => {
                    bail!("alternate streams forbidden")
                }
                _ => {}
            }
        }
        prefix.push(part);
        // A Windows drive prefix is not a filesystem root until RootDir follows.
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        let m = std::fs::symlink_metadata(&prefix)?;
        if m.file_type().is_symlink() {
            bail!("links forbidden");
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                bail!("reparse points forbidden");
            }
        }
    }
    Ok(())
}
