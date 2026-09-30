use crate::daemon::{self, StatusResponse};
use crate::paths::MonitorPaths;
use anyhow::{Context, Result, bail};
use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::time::Instant;

pub fn install(paths: &MonitorPaths) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = paths;
        bail!("the Linux monitor service can only be installed on Linux");
    }

    #[cfg(target_os = "linux")]
    {
        paths.create_storage()?;
        paths.create_runtime()?;
        let current = std::env::current_exe()?;
        if current != paths.binary {
            if let Some(parent) = paths.binary.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&current, &paths.binary).with_context(|| {
                format!(
                    "install {} to {}",
                    current.display(),
                    paths.binary.display()
                )
            })?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&paths.binary, fs::Permissions::from_mode(0o755))?;
        }

        if let Some(parent) = paths.service.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&paths.service, service_unit())?;
        if !paths.state.exists() {
            daemon::persist_recording(paths, false)?;
        }
        if systemctl_available() {
            let _ = import_graphical_environment();
            run_systemctl(&["daemon-reload"])?;
            run_systemctl(&["enable", "monitor.service"])?;
        }
        Ok(())
    }
}

pub fn start(paths: &MonitorPaths) -> Result<StatusResponse> {
    if let Ok(status) = daemon::send_request(paths, "start") {
        return Ok(status);
    }

    #[cfg(not(target_os = "linux"))]
    bail!("the Linux monitor daemon can only run on Linux");

    #[cfg(target_os = "linux")]
    {
        if !paths.binary.exists() {
            bail!("monitor is not installed; run `monitor install` first")
        }
        daemon::persist_recording(paths, true)?;
        if systemctl_available() && paths.service.exists() {
            let _ = import_graphical_environment();
            run_systemctl(&["start", "monitor.service"])?;
        } else {
            Command::new(&paths.binary)
                .arg("daemon")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("start monitor daemon")?;
        }
        wait_for_daemon(paths)
    }
}

pub fn stop(paths: &MonitorPaths) -> Result<StatusResponse> {
    daemon::persist_recording(paths, false)?;
    match daemon::send_request(paths, "stop") {
        Ok(status) => Ok(status),
        Err(_) => Ok(daemon::offline_status(paths)),
    }
}

pub fn uninstall(paths: &MonitorPaths) -> Result<()> {
    if daemon::send_request(paths, "shutdown").is_ok() {
        thread::sleep(Duration::from_millis(200));
    }
    if systemctl_available() {
        let _ = run_systemctl(&["disable", "--now", "monitor.service"]);
    }
    remove_if_exists(&paths.service)?;
    remove_if_exists(&paths.binary)?;
    if systemctl_available() {
        let _ = run_systemctl(&["daemon-reload"]);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn wait_for_daemon(paths: &MonitorPaths) -> Result<StatusResponse> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let Ok(status) = daemon::send_request(paths, "start") {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!("monitor service did not become ready; run `monitor doctor`")
}

fn systemctl_available() -> bool {
    Command::new("systemctl")
        .args(["--user", "show-environment"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn run_systemctl(arguments: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(arguments)
        .status()
        .context("run systemctl --user")?;
    if !status.success() {
        bail!("systemctl --user {} failed", arguments.join(" "));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn import_graphical_environment() -> Result<()> {
    let status = Command::new("systemctl")
        .args([
            "--user",
            "import-environment",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XAUTHORITY",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_TYPE",
            "DBUS_SESSION_BUS_ADDRESS",
        ])
        .status()?;
    if !status.success() {
        bail!("unable to import the graphical session environment")
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_unit() -> &'static str {
    r#"[Unit]
Description=monitor activity recorder
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/monitor daemon
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
"#
}

fn remove_if_exists(path: &std::path::Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
