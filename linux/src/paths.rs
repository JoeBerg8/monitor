use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct MonitorPaths {
    pub data_dir: PathBuf,
    pub database: PathBuf,
    pub screenshots: PathBuf,
    pub config_dir: PathBuf,
    pub state: PathBuf,
    pub runtime_dir: PathBuf,
    pub socket: PathBuf,
    pub lock: PathBuf,
    pub binary: PathBuf,
    pub service: PathBuf,
}

impl MonitorPaths {
    pub fn discover() -> Result<Self> {
        let home = dirs::home_dir().context("unable to determine the home directory")?;
        let data_base = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        let config_base = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let runtime_base = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| env::temp_dir().join(format!("monitor-{}", user_id())));

        let data_dir = data_base.join("monitor");
        let config_dir = config_base.join("monitor");
        let runtime_dir = runtime_base.join("monitor");

        Ok(Self {
            database: data_dir.join("events.sqlite"),
            screenshots: data_dir.join("screenshots"),
            state: config_dir.join("state.json"),
            socket: runtime_dir.join("monitor.sock"),
            lock: runtime_dir.join("monitor.lock"),
            binary: home.join(".local/bin/monitor"),
            service: config_base.join("systemd/user/monitor.service"),
            data_dir,
            config_dir,
            runtime_dir,
        })
    }

    pub fn create_runtime(&self) -> Result<()> {
        create_private_dir(&self.runtime_dir)
    }

    pub fn create_storage(&self) -> Result<()> {
        create_private_dir(&self.data_dir)?;
        create_private_dir(&self.screenshots)?;
        create_private_dir(&self.config_dir)
    }
}

fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("secure {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn user_id() -> u32 {
    // SAFETY: getuid takes no arguments and has no failure mode.
    unsafe { libc_getuid() }
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

#[cfg(not(unix))]
fn user_id() -> u32 {
    0
}
