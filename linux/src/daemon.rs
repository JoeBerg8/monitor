use crate::capture::{CaptureRuntime, CaptureSourceStatus};
use crate::database::EventDatabase;
use crate::event::{ActivityContext, RawEvent, normalize};
use crate::paths::MonitorPaths;
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
pub struct RecorderHandle {
    sender: mpsc::Sender<RecorderCommand>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum RecorderCommand {
    Event(RawEvent),
    Snapshot(mpsc::Sender<ActivityContext>),
    SetEnabled(bool),
    Shutdown,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl RecorderHandle {
    pub fn submit(&self, event: RawEvent) {
        let _ = self.sender.send(RecorderCommand::Event(event));
    }

    pub fn context_snapshot(&self) -> ActivityContext {
        let (sender, receiver) = mpsc::channel();
        if self.sender.send(RecorderCommand::Snapshot(sender)).is_err() {
            return ActivityContext::default();
        }
        receiver.recv().unwrap_or_default()
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.sender.send(RecorderCommand::SetEnabled(enabled));
    }

    fn shutdown(&self) {
        let _ = self.sender.send(RecorderCommand::Shutdown);
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PersistentState {
    pub recording: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IpcRequest {
    pub command: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StatusResponse {
    pub running: bool,
    pub recording: bool,
    pub pid: u32,
    pub event_count: u64,
    pub database: String,
    pub screenshots: String,
    pub session_type: String,
    pub desktop_environment: String,
    pub sources: BTreeMap<String, CaptureSourceStatus>,
    pub error: Option<String>,
}

pub fn run(paths: MonitorPaths) -> Result<()> {
    paths.create_runtime()?;
    paths.create_storage()?;
    let lock_file = acquire_instance_lock(&paths)?;
    let _keep_lock_alive = lock_file;

    if paths.socket.exists() {
        fs::remove_file(&paths.socket)
            .with_context(|| format!("remove stale socket {}", paths.socket.display()))?;
    }

    let state = load_state(&paths).unwrap_or_default();
    let recording = Arc::new(AtomicBool::new(state.recording));
    let shutdown = Arc::new(AtomicBool::new(false));
    let signal_shutdown = Arc::clone(&shutdown);
    ctrlc::set_handler(move || signal_shutdown.store(true, Ordering::SeqCst))
        .context("install shutdown signal handler")?;
    let health = Arc::new(Mutex::new(BTreeMap::new()));
    let recorder = spawn_recorder(paths.clone(), state.recording)?;
    let capture = CaptureRuntime::start(
        paths.clone(),
        recorder.clone(),
        Arc::clone(&recording),
        Arc::clone(&shutdown),
        Arc::clone(&health),
    );
    crate::tray::start(
        paths.clone(),
        recorder.clone(),
        Arc::clone(&recording),
        Arc::clone(&shutdown),
        Arc::clone(&health),
    );

    let listener = UnixListener::bind(&paths.socket)
        .with_context(|| format!("bind {}", paths.socket.display()))?;
    fs::set_permissions(&paths.socket, unix_permissions(0o600))?;
    listener.set_nonblocking(true)?;

    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                handle_client(stream, &paths, &recording, &shutdown, &recorder, &health)
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error.into()),
        }
    }

    recorder.shutdown();
    capture.stop();
    let _ = fs::remove_file(&paths.socket);
    Ok(())
}

fn spawn_recorder(paths: MonitorPaths, initially_enabled: bool) -> Result<RecorderHandle> {
    let database = EventDatabase::open(&paths.database)?;
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("monitor-recorder".into())
        .spawn(move || {
            let mut context = ActivityContext::default();
            let mut enabled = initially_enabled;
            while let Ok(command) = receiver.recv() {
                match command {
                    RecorderCommand::Event(raw) if enabled => {
                        let event = normalize(raw, &mut context);
                        if let Err(error) = database.insert(&event) {
                            eprintln!("monitor: database insert failed: {error:#}");
                        }
                    }
                    RecorderCommand::Event(_) => {}
                    RecorderCommand::Snapshot(reply) => {
                        let _ = reply.send(context.clone());
                    }
                    RecorderCommand::SetEnabled(value) => enabled = value,
                    RecorderCommand::Shutdown => break,
                }
            }
        })?;
    Ok(RecorderHandle { sender })
}

fn handle_client(
    mut stream: UnixStream,
    paths: &MonitorPaths,
    recording: &Arc<AtomicBool>,
    shutdown: &Arc<AtomicBool>,
    recorder: &RecorderHandle,
    health: &Arc<Mutex<BTreeMap<String, CaptureSourceStatus>>>,
) {
    let result = (|| -> Result<StatusResponse> {
        let mut line = String::new();
        BufReader::new(stream.try_clone()?).read_line(&mut line)?;
        let request: IpcRequest = serde_json::from_str(&line).context("invalid IPC request")?;
        match request.command.as_str() {
            "start" => {
                persist_recording(paths, true)?;
                recording.store(true, Ordering::SeqCst);
                recorder.set_enabled(true);
            }
            "stop" => {
                persist_recording(paths, false)?;
                recording.store(false, Ordering::SeqCst);
                recorder.set_enabled(false);
            }
            "shutdown" => {
                persist_recording(paths, false)?;
                recording.store(false, Ordering::SeqCst);
                recorder.set_enabled(false);
                shutdown.store(true, Ordering::SeqCst);
            }
            "status" => {}
            other => bail!("unknown command {other}"),
        }
        Ok(make_status(
            paths,
            recording.load(Ordering::SeqCst),
            health,
            None,
        ))
    })();

    let response = match result {
        Ok(response) => response,
        Err(error) => make_status(
            paths,
            recording.load(Ordering::SeqCst),
            health,
            Some(format!("{error:#}")),
        ),
    };
    let _ = serde_json::to_writer(&mut stream, &response);
    let _ = stream.write_all(b"\n");
}

fn make_status(
    paths: &MonitorPaths,
    recording: bool,
    health: &Arc<Mutex<BTreeMap<String, CaptureSourceStatus>>>,
    error: Option<String>,
) -> StatusResponse {
    let event_count = EventDatabase::open(&paths.database)
        .and_then(|database| database.count())
        .unwrap_or(0);
    StatusResponse {
        running: true,
        recording,
        pid: std::process::id(),
        event_count,
        database: paths.database.display().to_string(),
        screenshots: paths.screenshots.display().to_string(),
        session_type: std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into()),
        desktop_environment: std::env::var("XDG_CURRENT_DESKTOP")
            .or_else(|_| std::env::var("DESKTOP_SESSION"))
            .unwrap_or_else(|_| "unknown".into()),
        sources: health.lock().map(|value| value.clone()).unwrap_or_default(),
        error,
    }
}

pub fn send_request(paths: &MonitorPaths, command: &str) -> Result<StatusResponse> {
    let mut stream = UnixStream::connect(&paths.socket)
        .with_context(|| format!("connect to monitor at {}", paths.socket.display()))?;
    serde_json::to_writer(
        &mut stream,
        &IpcRequest {
            command: command.to_owned(),
        },
    )?;
    stream.write_all(b"\n")?;
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response)?;
    Ok(serde_json::from_str(&response)?)
}

pub fn offline_status(paths: &MonitorPaths) -> StatusResponse {
    let state = load_state(paths).unwrap_or_default();
    let count = EventDatabase::open(&paths.database)
        .and_then(|database| database.count())
        .unwrap_or(0);
    StatusResponse {
        running: false,
        recording: state.recording,
        pid: 0,
        event_count: count,
        database: paths.database.display().to_string(),
        screenshots: paths.screenshots.display().to_string(),
        session_type: std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into()),
        desktop_environment: std::env::var("XDG_CURRENT_DESKTOP")
            .or_else(|_| std::env::var("DESKTOP_SESSION"))
            .unwrap_or_else(|_| "unknown".into()),
        sources: BTreeMap::new(),
        error: None,
    }
}

pub fn persist_recording(paths: &MonitorPaths, recording: bool) -> Result<()> {
    paths.create_storage()?;
    let state = serde_json::to_vec_pretty(&PersistentState { recording })?;
    let temporary = paths.state.with_extension("json.tmp");
    fs::write(&temporary, state)?;
    fs::set_permissions(&temporary, unix_permissions(0o600))?;
    fs::rename(temporary, &paths.state)?;
    Ok(())
}

fn load_state(paths: &MonitorPaths) -> Result<PersistentState> {
    let data = fs::read(&paths.state)?;
    Ok(serde_json::from_slice(&data)?)
}

fn acquire_instance_lock(paths: &MonitorPaths) -> Result<File> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&paths.lock)?;
    lock.try_lock_exclusive()
        .context("another monitor daemon is already running")?;
    Ok(lock)
}

#[cfg(unix)]
fn unix_permissions(mode: u32) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt;
    fs::Permissions::from_mode(mode)
}
