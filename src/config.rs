use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Component;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::git::git_output;

const DEFAULT_WORKTREE_BASE_DIR: &str = ".worktrees";

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
    let raw = git_output(["config", "--get-regexp", r"^(ws|wt)\.basedir$"]).unwrap_or_default();
    let mut ws_base_dir = None;
    let mut wt_base_dir = None;
    for line in raw.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key {
            "ws.basedir" => ws_base_dir = Some(value.to_string()),
            "wt.basedir" => wt_base_dir = Some(value.to_string()),
            _ => {}
        }
    }
    GitConfig {
        ws_base_dir,
        wt_base_dir,
    }
}

pub fn resolve_base_dir(
    repo_root: &Path,
    file_config: &FileConfig,
    git_config: &GitConfig,
) -> PathBuf {
    let configured =
        configured_base_dir(file_config, git_config).unwrap_or(DEFAULT_WORKTREE_BASE_DIR);
    expand_base_dir(repo_root, configured)
}

pub(crate) fn ensure_base_dir_ignored(repo_root: &Path, base_dir: &Path) -> Result<()> {
    let Some(entry) = repo_local_exclude_pattern(repo_root, base_dir) else {
        return Ok(());
    };
    ensure_local_exclude_entry(repo_root, &entry)
}

fn configured_base_dir<'a>(
    file_config: &'a FileConfig,
    git_config: &'a GitConfig,
) -> Option<&'a str> {
    file_config
        .worktree_base_dir
        .as_deref()
        .or(git_config.ws_base_dir.as_deref())
        .or(git_config.wt_base_dir.as_deref())
}

pub fn ensure_init_trusted(primary_root: &Path, file_config: &FileConfig) -> Result<()> {
    if file_config.init_commands.is_empty() {
        return Ok(());
    }

    let key = primary_root.display().to_string();
    let trust_value = init_trust_value(file_config);
    let path = trust_store_path()?;
    let mut store = read_trust_store(&path)?;

    if store.repos.get(&key) == Some(&trust_value) {
        return Ok(());
    }

    if !io::stdin().is_terminal() {
        return Err(anyhow!(
            "init commands are not trusted for {}; run interactively once or pass --no-init",
            key
        ));
    }

    eprintln!("git-ws: init commands from .git-ws.toml:");
    for command in &file_config.init_commands {
        eprintln!("  {}", format_init_command_for_prompt(command));
    }
    eprint!("Trust and run these commands for this repo? [y/N] ");
    io::stderr().flush().ok();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes" | "YES") {
        return Err(anyhow!("init commands were not trusted"));
    }

    store.repos.insert(key, trust_value);
    write_trust_store(&path, &store)
}

fn ensure_local_exclude_entry(repo_root: &Path, entry: &str) -> Result<()> {
    let path = local_exclude_path(repo_root)?;
    let mut raw = match fs::read(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let entry = entry.as_bytes();
    if raw
        .split(|byte| *byte == b'\n')
        .any(|line| trim_ascii_bytes(line) == entry)
    {
        return Ok(());
    }

    if !raw.is_empty() && !raw.ends_with(b"\n") {
        raw.push(b'\n');
    }
    raw.extend_from_slice(entry);
    raw.push(b'\n');

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))
}

fn trim_ascii_bytes(value: &[u8]) -> &[u8] {
    let start = value
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(value.len());
    let end = value
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &value[start..end]
}

fn local_exclude_path(repo_root: &Path) -> Result<PathBuf> {
    let value = git_output([
        OsStr::new("-C"),
        repo_root.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("--git-path"),
        OsStr::new("info/exclude"),
    ])
    .with_context(|| {
        format!(
            "failed to resolve git exclude path for {}",
            repo_root.display()
        )
    })?;
    let path = PathBuf::from(value.trim());
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(repo_root.join(path))
    }
}

fn repo_local_exclude_pattern(repo_root: &Path, path: &Path) -> Option<String> {
    let repo_root = normalize_path_lexically(repo_root);
    let path = normalize_path_lexically(path);
    if path == repo_root {
        return None;
    }
    let relative = path.strip_prefix(&repo_root).ok()?;
    let value = relative.to_string_lossy().replace('\\', "/");
    (!value.is_empty()).then(|| format!("/{value}/"))
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
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

fn init_trust_value(file_config: &FileConfig) -> String {
    let mut value = String::from("git-ws-init-trust-v1\n");
    match &file_config.worktree_base_dir {
        Some(base_dir) => push_trust_field(&mut value, "worktree_base_dir", base_dir),
        None => value.push_str("worktree_base_dir:none\n"),
    }
    value.push_str(&format!(
        "init_commands:{}\n",
        file_config.init_commands.len()
    ));
    for command in &file_config.init_commands {
        push_trust_field(&mut value, "init_command", command);
    }
    value
}

fn push_trust_field(output: &mut String, label: &str, value: &str) {
    output.push_str(label);
    output.push(':');
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push('\n');
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

fn format_init_command_for_prompt(command: &str) -> String {
    command.escape_debug().to_string()
}

#[cfg(test)]
mod tests {
    use super::{FileConfig, format_init_command_for_prompt, init_trust_value};

    #[test]
    fn format_init_command_for_prompt_escapes_control_characters() {
        let command = "printf '\x1b]0;owned\x07' && echo hidden\ncargo test";
        let formatted = format_init_command_for_prompt(command);

        assert!(!formatted.contains('\x1b'));
        assert!(!formatted.contains('\x07'));
        assert!(!formatted.contains('\n'));
        assert!(formatted.contains("\\u{1b}"));
        assert!(formatted.contains("\\u{7}"));
        assert!(formatted.contains("\\n"));
    }

    #[test]
    fn init_trust_value_stores_normalized_config_content() {
        let value = init_trust_value(&FileConfig {
            worktree_base_dir: Some("../repo-worktrees".to_string()),
            init_commands: vec!["mise install".to_string(), "cargo test".to_string()],
        });

        assert!(value.starts_with("git-ws-init-trust-v1\n"));
        assert!(value.contains("worktree_base_dir:17:../repo-worktrees\n"));
        assert!(value.contains("init_command:12:mise install\n"));
        assert!(value.contains("init_command:10:cargo test\n"));
        assert_ne!(value.len(), 16, "trust value should not be a short hash");
    }
}
