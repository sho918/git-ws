use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::git::git_output;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileConfig {
    pub worktree_base_dir: Option<String>,
    pub init_commands: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitConfig {
    pub ws_base_dir: Option<String>,
    pub wt_base_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawFileConfig {
    worktree: Option<RawWorktreeConfig>,
    init: Option<RawInitConfig>,
}

#[derive(Debug, Deserialize)]
struct RawWorktreeConfig {
    base_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawInitConfig {
    on_create: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct TrustStore {
    repos: BTreeMap<String, String>,
}

pub fn load_file_config(repo_root: &Path) -> Result<FileConfig> {
    let path = repo_root.join(".git-ws.toml");
    if !path.exists() {
        return Ok(FileConfig::default());
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let config: RawFileConfig =
        toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(FileConfig {
        worktree_base_dir: config.worktree.and_then(|worktree| worktree.base_dir),
        init_commands: config
            .init
            .and_then(|init| init.on_create)
            .unwrap_or_default(),
    })
}

pub fn load_git_config() -> GitConfig {
    GitConfig {
        ws_base_dir: git_config_value("ws.basedir"),
        wt_base_dir: git_config_value("wt.basedir"),
    }
}

pub fn resolve_base_dir(
    repo_root: &Path,
    file_config: &FileConfig,
    git_config: &GitConfig,
) -> PathBuf {
    let configured = file_config
        .worktree_base_dir
        .as_deref()
        .or(git_config.ws_base_dir.as_deref())
        .or(git_config.wt_base_dir.as_deref())
        .unwrap_or(".worktrees");
    expand_base_dir(repo_root, configured)
}

pub fn ensure_init_trusted(repo_root: &Path, file_config: &FileConfig) -> Result<()> {
    if file_config.init_commands.is_empty() {
        return Ok(());
    }

    let key = repo_root.display().to_string();
    let hash = init_hash(file_config);
    let path = trust_store_path()?;
    let mut store = read_trust_store(&path)?;

    if store.repos.get(&key) == Some(&hash) {
        return Ok(());
    }

    if !io::stdin().is_terminal() {
        return Err(anyhow!(
            "init commands are not trusted for {}; run interactively once or pass --no-init",
            repo_root.display()
        ));
    }

    eprintln!("git-ws: init commands from .git-ws.toml:");
    for command in &file_config.init_commands {
        eprintln!("  {command}");
    }
    eprint!("Trust and run these commands for this repo? [y/N] ");
    io::stderr().flush().ok();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes" | "YES") {
        return Err(anyhow!("init commands were not trusted"));
    }

    store.repos.insert(key, hash);
    write_trust_store(&path, &store)
}

fn expand_base_dir(repo_root: &Path, configured: &str) -> PathBuf {
    let value = configured.replace(
        "{gitroot}",
        repo_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repo"),
    );
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        repo_root.join(path)
    }
}

fn git_config_value(key: &str) -> Option<String> {
    git_output(["config", "--get", key])
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn init_hash(file_config: &FileConfig) -> String {
    let mut hasher = Fnv64::default();
    file_config.worktree_base_dir.hash(&mut hasher);
    file_config.init_commands.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[derive(Default)]
struct Fnv64(u64);

impl Hasher for Fnv64 {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        if self.0 == 0 {
            self.0 = 0xcbf29ce484222325;
        }
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
}

fn trust_store_path() -> Result<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| anyhow!("HOME or XDG_CONFIG_HOME is required"))?;
    Ok(base.join("git-ws").join("trust.toml"))
}

fn read_trust_store(path: &Path) -> Result<TrustStore> {
    if !path.exists() {
        return Ok(TrustStore::default());
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn write_trust_store(path: &Path, store: &TrustStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let raw = toml::to_string_pretty(store).context("failed to serialize trust store")?;
    fs::write(path, raw).with_context(|| format!("failed to write {}", path.display()))
}
