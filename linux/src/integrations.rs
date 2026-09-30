use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::Serialize;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SKILL: &str = include_str!("../../integrations/monitor/SKILL.md");
const MANAGED_MARKER: &str = "<!-- managed-by: monitor -->";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum AgentIntegration {
    Auto,
    Codex,
    Claude,
    All,
}

impl AgentIntegration {
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::All => "all",
        }
    }
}

#[derive(Debug)]
pub struct IntegrationInstallResult {
    pub installed: Vec<(AgentIntegration, PathBuf)>,
    pub auto_detected_none: bool,
}

pub fn install(requested: &[AgentIntegration]) -> Result<IntegrationInstallResult> {
    let home = dirs::home_dir().context("unable to determine the home directory")?;
    let clients = resolve_clients(requested, &home);
    let auto_detected_none = requested.contains(&AgentIntegration::Auto) && clients.is_empty();
    let mut installed = Vec::new();

    for client in clients {
        let path = skill_path(client, &home);
        write_managed_skill(&path)?;
        installed.push((client, path));
    }

    Ok(IntegrationInstallResult {
        installed,
        auto_detected_none,
    })
}

pub fn uninstall_managed() -> Result<Vec<PathBuf>> {
    let home = dirs::home_dir().context("unable to determine the home directory")?;
    let mut removed = Vec::new();
    for client in [AgentIntegration::Codex, AgentIntegration::Claude] {
        let path = skill_path(client, &home);
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        if !contents.contains(MANAGED_MARKER) {
            continue;
        }
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
        removed.push(path);
    }
    Ok(removed)
}

fn resolve_clients(requested: &[AgentIntegration], home: &Path) -> BTreeSet<AgentIntegration> {
    let mut clients = BTreeSet::new();
    for request in requested {
        match request {
            AgentIntegration::Auto => {
                if client_is_present(AgentIntegration::Codex, home) {
                    clients.insert(AgentIntegration::Codex);
                }
                if client_is_present(AgentIntegration::Claude, home) {
                    clients.insert(AgentIntegration::Claude);
                }
            }
            AgentIntegration::All => {
                clients.insert(AgentIntegration::Codex);
                clients.insert(AgentIntegration::Claude);
            }
            client => {
                clients.insert(*client);
            }
        }
    }
    clients
}

fn client_is_present(client: AgentIntegration, home: &Path) -> bool {
    match client {
        AgentIntegration::Codex => codex_root(home).is_dir() || command_on_path("codex"),
        AgentIntegration::Claude => home.join(".claude").is_dir() || command_on_path("claude"),
        AgentIntegration::Auto | AgentIntegration::All => false,
    }
}

fn skill_path(client: AgentIntegration, home: &Path) -> PathBuf {
    match client {
        AgentIntegration::Codex => codex_root(home).join("skills/monitor/SKILL.md"),
        AgentIntegration::Claude => home.join(".claude/skills/monitor/SKILL.md"),
        AgentIntegration::Auto | AgentIntegration::All => unreachable!("selector is not a client"),
    }
}

fn codex_root(home: &Path) -> PathBuf {
    env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"))
}

fn command_on_path(name: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|directory| directory.join(name).is_file())
}

fn write_managed_skill(path: &Path) -> Result<()> {
    if let Ok(existing) = fs::read_to_string(path)
        && !existing.contains(MANAGED_MARKER)
        && existing != SKILL
    {
        bail!(
            "refusing to replace unmanaged agent skill at {}; move it or choose another integration",
            path.display()
        );
    }
    let parent = path.parent().context("agent skill path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    fs::write(path, SKILL).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolves_explicit_clients_without_auto_detection() {
        let home = tempdir().unwrap();
        let clients = resolve_clients(
            &[AgentIntegration::Claude, AgentIntegration::Codex],
            home.path(),
        );
        assert_eq!(
            clients.into_iter().collect::<Vec<_>>(),
            vec![AgentIntegration::Codex, AgentIntegration::Claude]
        );
    }

    #[test]
    fn writes_and_updates_managed_skill() {
        let root = tempdir().unwrap();
        let path = root.path().join("monitor/SKILL.md");
        write_managed_skill(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), SKILL);
        write_managed_skill(&path).unwrap();
    }

    #[test]
    fn refuses_to_replace_unmanaged_skill() {
        let root = tempdir().unwrap();
        let path = root.path().join("monitor/SKILL.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "user-owned skill").unwrap();
        assert!(write_managed_skill(&path).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "user-owned skill");
    }
}
