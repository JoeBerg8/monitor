use crate::daemon::{self, StatusResponse};
use crate::database::EventDatabase;
use crate::integrations::{self, AgentIntegration};
use crate::paths::MonitorPaths;
use crate::service;
use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Parser)]
#[command(name = "monitor", version, about = "Local Linux activity recorder")]
pub struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Subcommand)]
enum CommandKind {
    /// Install the binary and systemd user service.
    Install {
        /// Install a user-level skill for a supported agent client. Repeatable.
        #[arg(long = "agent-integration", value_enum)]
        agent_integrations: Vec<AgentIntegration>,
    },
    /// Start or resume recording.
    Start {
        #[arg(long)]
        json: bool,
    },
    /// Pause all recording while leaving the service available.
    Stop {
        #[arg(long)]
        json: bool,
    },
    /// Show daemon, recording, storage, and capture-source state.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Check whether this desktop session supports each capture source.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Export the ordered event timeline as JSON Lines.
    Export { output: Option<PathBuf> },
    /// Print the canonical command, consent, and privacy contract for agents.
    Instructions {
        #[arg(long)]
        json: bool,
    },
    /// Remove the binary and service. Recorded data is preserved.
    Uninstall,
    #[command(hide = true)]
    Daemon,
}

#[derive(Serialize)]
struct DoctorReport {
    supported_os: bool,
    installed: bool,
    service_installed: bool,
    session_type: String,
    desktop_environment: String,
    graphical_session: bool,
    window_capture: Check,
    input_capture: Check,
    screenshot_capture: Check,
    session_state: Check,
    database: String,
}

#[derive(Serialize)]
struct Check {
    available: bool,
    detail: String,
}

pub fn run(cli: Cli) -> Result<()> {
    let paths = MonitorPaths::discover()?;
    match cli.command {
        CommandKind::Install { agent_integrations } => {
            service::install(&paths)?;
            let result = integrations::install(&agent_integrations)?;
            println!("monitor installed; run `monitor start`");
            for (client, path) in result.installed {
                println!("{} integration: {}", client.name(), path.display());
            }
            if result.auto_detected_none {
                println!(
                    "no supported agent client detected; use `monitor install --agent-integration <codex|claude>`"
                );
            }
        }
        CommandKind::Start { json } => print_status(&service::start(&paths)?, json)?,
        CommandKind::Stop { json } => print_status(&service::stop(&paths)?, json)?,
        CommandKind::Status { json } => {
            let status = daemon::send_request(&paths, "status")
                .unwrap_or_else(|_| daemon::offline_status(&paths));
            print_status(&status, json)?;
        }
        CommandKind::Doctor { json } => print_doctor(&doctor(&paths), json)?,
        CommandKind::Export { output } => export(&paths, output)?,
        CommandKind::Instructions { json } => print_instructions(json)?,
        CommandKind::Uninstall => {
            service::uninstall(&paths)?;
            for path in integrations::uninstall_managed()? {
                println!("removed agent integration: {}", path.display());
            }
            println!(
                "monitor uninstalled; recorded data remains at {}",
                paths.data_dir.display()
            );
        }
        CommandKind::Daemon => daemon::run(paths)?,
    }
    Ok(())
}

#[derive(Serialize)]
struct InstructionsDocument {
    schema_version: u8,
    program: &'static str,
    purpose: &'static str,
    consent: &'static str,
    commands: Vec<InstructionCommand>,
    privacy: Vec<&'static str>,
    agent_integrations: Vec<&'static str>,
    platform_notes: Vec<&'static str>,
}

#[derive(Serialize)]
struct InstructionCommand {
    command: &'static str,
    description: &'static str,
    mutates_state: bool,
}

fn instructions_document() -> InstructionsDocument {
    InstructionsDocument {
        schema_version: 1,
        program: "monitor",
        purpose: "Local, user-controlled Linux desktop activity recorder",
        consent: "Start recording only after an explicit user request",
        commands: vec![
            InstructionCommand {
                command: "monitor start --json",
                description: "Start or resume recording",
                mutates_state: true,
            },
            InstructionCommand {
                command: "monitor stop --json",
                description: "Pause recording",
                mutates_state: true,
            },
            InstructionCommand {
                command: "monitor status --json",
                description: "Read daemon, recording, storage, and source state",
                mutates_state: false,
            },
            InstructionCommand {
                command: "monitor doctor --json",
                description: "Check desktop capture capabilities",
                mutates_state: false,
            },
            InstructionCommand {
                command: "monitor export [OUTPUT]",
                description: "Export ordered events as JSON Lines",
                mutates_state: true,
            },
            InstructionCommand {
                command: "monitor uninstall",
                description: "Remove the service, binary, and managed agent integrations",
                mutates_state: true,
            },
        ],
        privacy: vec![
            "Screenshots may contain sensitive visible information",
            "Typed text and clipboard contents are never stored",
            "All telemetry remains local unless the user explicitly exports or transfers it",
        ],
        agent_integrations: vec!["codex", "claude"],
        platform_notes: vec![
            "X11 supports foreground windows, clicks, scrolling, and Ctrl+C/V/X",
            "Wayland intentionally does not support passive global input capture",
            "Use monitor status or monitor doctor for live source availability",
        ],
    }
}

fn print_instructions(json: bool) -> Result<()> {
    let document = instructions_document();
    if json {
        println!("{}", serde_json::to_string_pretty(&document)?);
        return Ok(());
    }

    println!("{} — {}", document.program, document.purpose);
    println!("Consent: {}.", document.consent);
    println!();
    println!("Commands:");
    for command in document.commands {
        println!("  {:<28} {}", command.command, command.description);
    }
    println!();
    println!("Privacy:");
    for note in document.privacy {
        println!("  - {note}");
    }
    println!();
    println!("Run `monitor instructions --json` for structured output.");
    Ok(())
}

fn print_status(status: &StatusResponse, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(status)?);
    } else {
        let state = if status.running {
            if status.recording {
                "recording"
            } else {
                "paused"
            }
        } else {
            "not running"
        };
        println!("monitor: {state}");
        println!("events: {}", status.event_count);
        println!("database: {}", status.database);
        for (source, value) in &status.sources {
            println!("{source}: {} ({})", value.state, value.detail);
        }
        if let Some(error) = &status.error {
            println!("error: {error}");
        }
    }
    Ok(())
}

fn doctor(paths: &MonitorPaths) -> DoctorReport {
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| std::env::var("DESKTOP_SESSION"))
        .unwrap_or_else(|_| "unknown".into());
    let graphical =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    let wayland = session_type.eq_ignore_ascii_case("wayland");
    let linux = cfg!(target_os = "linux");

    DoctorReport {
        supported_os: linux,
        installed: paths.binary.exists(),
        service_installed: paths.service.exists(),
        session_type: session_type.clone(),
        desktop_environment: desktop,
        graphical_session: graphical,
        window_capture: Check {
            available: linux && graphical,
            detail: if wayland {
                "availability depends on the compositor; `monitor status` reports the live result"
                    .into()
            } else {
                "X11 foreground-window capture is supported".into()
            },
        },
        input_capture: Check {
            available: linux && graphical && !wayland,
            detail: if wayland {
                "passive global input is intentionally unavailable on Wayland".into()
            } else {
                "X11 clicks, scrolling, and Ctrl+C/V/X are supported".into()
            },
        },
        screenshot_capture: Check {
            available: linux && graphical,
            detail: if wayland {
                "Wayland may display a desktop permission prompt on first capture".into()
            } else {
                "per-display capture is supported".into()
            },
        },
        session_state: Check {
            available: command_exists("loginctl"),
            detail: if command_exists("loginctl") {
                "systemd-logind is available".into()
            } else {
                "loginctl is missing; screenshots pause unless lock state is available over D-Bus"
                    .into()
            },
        },
        database: paths.database.display().to_string(),
    }
}

fn print_doctor(report: &DoctorReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
    } else {
        println!("supported OS: {}", yes_no(report.supported_os));
        println!("installed: {}", yes_no(report.installed));
        println!("session: {}", report.session_type);
        println!("desktop: {}", report.desktop_environment);
        println!(
            "window capture: {} ({})",
            yes_no(report.window_capture.available),
            report.window_capture.detail
        );
        println!(
            "input capture: {} ({})",
            yes_no(report.input_capture.available),
            report.input_capture.detail
        );
        println!(
            "screenshots: {} ({})",
            yes_no(report.screenshot_capture.available),
            report.screenshot_capture.detail
        );
    }
    Ok(())
}

fn export(paths: &MonitorPaths, output: Option<PathBuf>) -> Result<()> {
    paths.create_storage()?;
    let database = EventDatabase::open(&paths.database)?;
    let destination = output.unwrap_or_else(|| {
        PathBuf::from(format!(
            "monitor-export-{}.jsonl",
            Utc::now().format("%Y%m%d-%H%M%S")
        ))
    });
    let file = File::create(&destination)
        .with_context(|| format!("create export {}", destination.display()))?;
    let mut writer = BufWriter::new(file);
    for event in database.all_events()? {
        serde_json::to_writer(&mut writer, &event)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    println!("exported timeline to {}", destination.display());
    Ok(())
}

fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
