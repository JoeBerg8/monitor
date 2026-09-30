use crate::daemon::RecorderHandle;
use crate::paths::MonitorPaths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaptureSourceStatus {
    pub state: String,
    pub detail: String,
}

impl CaptureSourceStatus {
    fn new(state: &str, detail: impl Into<String>) -> Self {
        Self {
            state: state.into(),
            detail: detail.into(),
        }
    }
}

pub struct CaptureRuntime {
    shutdown: Arc<AtomicBool>,
}

impl CaptureRuntime {
    pub fn start(
        paths: MonitorPaths,
        recorder: RecorderHandle,
        recording: Arc<AtomicBool>,
        shutdown: Arc<AtomicBool>,
        health: Arc<Mutex<BTreeMap<String, CaptureSourceStatus>>>,
    ) -> Self {
        platform::start(paths, recorder, recording, Arc::clone(&shutdown), health);
        Self { shutdown }
    }

    pub fn stop(&self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(any(target_os = "linux", test))]
mod platform {
    use super::CaptureSourceStatus;
    use crate::daemon::RecorderHandle;
    use crate::event::{ApplicationIdentity, RawEvent, ShortcutKind, now_timestamp};
    use crate::paths::MonitorPaths;
    use chrono::{Datelike, TimeZone, Utc};
    use image::ExtendedColorType;
    use image::codecs::jpeg::JpegEncoder;
    use rdev::{Button, Event, EventType, Key};
    use std::collections::BTreeMap;
    use std::fs::{self, File};
    use std::io::{BufWriter, Write};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant, UNIX_EPOCH};
    use xcap::{Monitor, Window};

    type Health = Arc<Mutex<BTreeMap<String, CaptureSourceStatus>>>;

    pub fn start(
        paths: MonitorPaths,
        recorder: RecorderHandle,
        recording: Arc<AtomicBool>,
        shutdown: Arc<AtomicBool>,
        health: Health,
    ) {
        set_health(&health, "database", "active", "SQLite writer ready");
        start_window_monitor(
            recorder.clone(),
            Arc::clone(&recording),
            Arc::clone(&shutdown),
            Arc::clone(&health),
        );

        let last_input = Arc::new(Mutex::new(Instant::now()));
        start_input_monitor(
            recorder.clone(),
            Arc::clone(&recording),
            Arc::clone(&shutdown),
            Arc::clone(&last_input),
            Arc::clone(&health),
        );
        start_screenshot_monitor(paths, recorder, recording, shutdown, last_input, health);
    }

    fn start_window_monitor(
        recorder: RecorderHandle,
        recording: Arc<AtomicBool>,
        shutdown: Arc<AtomicBool>,
        health: Health,
    ) {
        thread::Builder::new()
            .name("monitor-window".into())
            .spawn(move || {
                let mut last_application: Option<(String, u32)> = None;
                let mut last_title: Option<String> = None;
                while !shutdown.load(Ordering::SeqCst) {
                    if recording.load(Ordering::SeqCst) {
                        match focused_window() {
                            Ok(Some(window)) => {
                                set_health(
                                    &health,
                                    "window",
                                    "active",
                                    "foreground window capture is active",
                                );
                                let identity = (window.app_name.clone(), window.pid);
                                if last_application.as_ref() != Some(&identity) {
                                    recorder.submit(RawEvent::AppActivated {
                                        timestamp: now_timestamp(),
                                        application: ApplicationIdentity {
                                            bundle_id: Some(application_id(&window.app_name)),
                                            name: Some(window.app_name.clone()),
                                            process_id: Some(i64::from(window.pid)),
                                        },
                                    });
                                    last_application = Some(identity);
                                    last_title = None;
                                }
                                if last_title.as_ref() != Some(&window.title) {
                                    recorder.submit(RawEvent::WindowChanged {
                                        timestamp: now_timestamp(),
                                        title: Some(window.title.clone()),
                                    });
                                    last_title = Some(window.title);
                                }
                            }
                            Ok(None) => set_health(
                                &health,
                                "window",
                                "degraded",
                                "no focused window is currently exposed",
                            ),
                            Err(error) => set_health(
                                &health,
                                "window",
                                "unavailable",
                                format!("foreground window unavailable: {error}"),
                            ),
                        }
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            })
            .expect("start foreground window monitor");
    }

    struct FocusedWindow {
        title: String,
        app_name: String,
        pid: u32,
    }

    fn focused_window() -> Result<Option<FocusedWindow>, String> {
        let windows = Window::all().map_err(|error| error.to_string())?;
        for window in windows {
            if window.is_focused().unwrap_or(false) {
                return Ok(Some(FocusedWindow {
                    title: window.title().unwrap_or_default(),
                    app_name: window.app_name().unwrap_or_else(|_| "unknown".into()),
                    pid: window.pid().unwrap_or(0),
                }));
            }
        }
        Ok(None)
    }

    fn start_input_monitor(
        recorder: RecorderHandle,
        recording: Arc<AtomicBool>,
        shutdown: Arc<AtomicBool>,
        last_input: Arc<Mutex<Instant>>,
        health: Health,
    ) {
        let session_type = std::env::var("XDG_SESSION_TYPE")
            .unwrap_or_default()
            .to_lowercase();
        if session_type == "wayland" {
            set_health(
                &health,
                "input",
                "unsupported",
                "Wayland does not expose passive global input to applications",
            );
            return;
        }

        thread::Builder::new()
            .name("monitor-input".into())
            .spawn(move || {
                while !shutdown.load(Ordering::SeqCst) {
                    let modifiers = Arc::new(Mutex::new(Modifiers::default()));
                    let pointer = Arc::new(Mutex::new((0.0, 0.0)));
                    let callback_recorder = recorder.clone();
                    let callback_recording = Arc::clone(&recording);
                    let callback_shutdown = Arc::clone(&shutdown);
                    let callback_last_input = Arc::clone(&last_input);
                    let callback_modifiers = Arc::clone(&modifiers);
                    let callback_pointer = Arc::clone(&pointer);
                    set_health(
                        &health,
                        "input",
                        "active",
                        "X11 global input listener is active",
                    );
                    let result = rdev::listen(move |event| {
                        if callback_shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        handle_input(
                            event,
                            &callback_recorder,
                            &callback_recording,
                            &callback_last_input,
                            &callback_modifiers,
                            &callback_pointer,
                        );
                    });
                    if shutdown.load(Ordering::SeqCst) {
                        break;
                    }
                    let detail = match result {
                        Ok(()) => "X11 input listener stopped; retrying".into(),
                        Err(error) => format!("X11 input listener failed: {error:?}; retrying"),
                    };
                    set_health(&health, "input", "unavailable", detail);
                    sleep_interruptibly(&shutdown, Duration::from_secs(5));
                }
            })
            .expect("start input monitor");
    }

    #[derive(Default)]
    struct Modifiers {
        control: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    }

    impl Modifiers {
        fn mask(&self) -> i64 {
            i64::from(self.control)
                | (i64::from(self.shift) << 1)
                | (i64::from(self.alt) << 2)
                | (i64::from(self.meta) << 3)
        }

        fn is_plain_control(&self) -> bool {
            self.control && !self.shift && !self.alt && !self.meta
        }
    }

    fn handle_input(
        event: Event,
        recorder: &RecorderHandle,
        recording: &Arc<AtomicBool>,
        last_input: &Arc<Mutex<Instant>>,
        modifiers: &Arc<Mutex<Modifiers>>,
        pointer: &Arc<Mutex<(f64, f64)>>,
    ) {
        if let Ok(mut value) = last_input.lock() {
            *value = Instant::now();
        }
        let timestamp = event
            .time
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        match event.event_type {
            EventType::MouseMove { x, y } => {
                if let Ok(mut location) = pointer.lock() {
                    *location = (x, y);
                }
            }
            EventType::ButtonPress(button) if recording.load(Ordering::Relaxed) => {
                let (x, y) = pointer.lock().map(|value| *value).unwrap_or_default();
                recorder.submit(RawEvent::MouseDown {
                    timestamp,
                    x,
                    y,
                    button: button_name(button),
                });
            }
            EventType::Wheel { delta_x, delta_y } if recording.load(Ordering::Relaxed) => {
                let (x, y) = pointer.lock().map(|value| *value).unwrap_or_default();
                recorder.submit(RawEvent::Scroll {
                    timestamp,
                    x,
                    y,
                    delta_x,
                    delta_y,
                    continuous: true,
                });
            }
            EventType::KeyPress(key) => {
                let mut state = match modifiers.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };
                update_modifier(&mut state, key, true);
                if recording.load(Ordering::Relaxed)
                    && state.is_plain_control()
                    && let Some((kind, code)) = shortcut(key)
                {
                    recorder.submit(RawEvent::Shortcut {
                        timestamp,
                        kind,
                        key_code: code,
                        modifiers: state.mask(),
                    });
                }
            }
            EventType::KeyRelease(key) => {
                if let Ok(mut state) = modifiers.lock() {
                    update_modifier(&mut state, key, false);
                }
            }
            _ => {}
        }
    }

    fn update_modifier(state: &mut Modifiers, key: Key, pressed: bool) {
        match key {
            Key::ControlLeft | Key::ControlRight => state.control = pressed,
            Key::ShiftLeft | Key::ShiftRight => state.shift = pressed,
            Key::Alt | Key::AltGr => state.alt = pressed,
            Key::MetaLeft | Key::MetaRight => state.meta = pressed,
            _ => {}
        }
    }

    fn shortcut(key: Key) -> Option<(ShortcutKind, i64)> {
        match key {
            Key::KeyC => Some((ShortcutKind::Copy, 67)),
            Key::KeyV => Some((ShortcutKind::Paste, 86)),
            Key::KeyX => Some((ShortcutKind::Cut, 88)),
            _ => None,
        }
    }

    fn button_name(button: Button) -> String {
        match button {
            Button::Left => "left".into(),
            Button::Right => "right".into(),
            Button::Middle => "middle".into(),
            Button::Unknown(value) => format!("other_{value}"),
        }
    }

    fn start_screenshot_monitor(
        paths: MonitorPaths,
        recorder: RecorderHandle,
        recording: Arc<AtomicBool>,
        shutdown: Arc<AtomicBool>,
        last_input: Arc<Mutex<Instant>>,
        health: Health,
    ) {
        thread::Builder::new()
            .name("monitor-screenshot".into())
            .spawn(move || {
                set_health(
                    &health,
                    "screenshot",
                    "active",
                    "waiting for the first 15-second capture interval",
                );
                while !shutdown.load(Ordering::SeqCst) {
                    sleep_interruptibly(&shutdown, Duration::from_secs(15));
                    if shutdown.load(Ordering::SeqCst) || !recording.load(Ordering::SeqCst) {
                        continue;
                    }
                    let recent_local_input = last_input
                        .lock()
                        .map(|value| value.elapsed() < Duration::from_secs(60))
                        .unwrap_or(false);
                    if !recent_local_input {
                        match idle_seconds() {
                            Some(idle) if idle < 60 => {}
                            Some(_) => {
                                set_health(
                                    &health,
                                    "screenshot",
                                    "paused",
                                    "user has been idle for at least 60 seconds",
                                );
                                continue;
                            }
                            None => {
                                set_health(
                                    &health,
                                    "screenshot",
                                    "degraded",
                                    "idle time is unavailable; capture is paused",
                                );
                                continue;
                            }
                        }
                    }
                    if !session_is_active() {
                        set_health(
                            &health,
                            "screenshot",
                            "paused",
                            "session is locked, inactive, or cannot be verified",
                        );
                        continue;
                    }
                    if !displays_are_awake() {
                        set_health(&health, "screenshot", "paused", "displays are asleep");
                        continue;
                    }
                    match capture_displays(&paths, &recorder, &recording, &shutdown) {
                        Ok((captured, failed)) => set_health(
                            &health,
                            "screenshot",
                            "active",
                            format!(
                                "captured {captured} display(s) in the last cycle; {failed} failed"
                            ),
                        ),
                        Err(error) => set_health(
                            &health,
                            "screenshot",
                            "unavailable",
                            format!("screenshot capture failed: {error}"),
                        ),
                    }
                }
            })
            .expect("start screenshot monitor");
    }

    fn capture_displays(
        paths: &MonitorPaths,
        recorder: &RecorderHandle,
        recording: &Arc<AtomicBool>,
        shutdown: &Arc<AtomicBool>,
    ) -> Result<(usize, usize), String> {
        let captured_at = now_timestamp();
        let context = recorder.context_snapshot();
        let monitors = Monitor::all().map_err(|error| error.to_string())?;
        let mut count = 0;
        let mut failures = Vec::new();
        for monitor in monitors {
            if !recording.load(Ordering::SeqCst) || shutdown.load(Ordering::SeqCst) {
                break;
            }
            match capture_display(
                paths,
                recorder,
                &monitor,
                captured_at,
                &context,
                recording,
                shutdown,
            ) {
                Ok(true) => count += 1,
                Ok(false) => break,
                Err(error) => failures.push(error),
            }
        }
        if count == 0 && !failures.is_empty() {
            return Err(failures.join("; "));
        }
        Ok((count, failures.len()))
    }

    #[allow(clippy::too_many_arguments)]
    fn capture_display(
        paths: &MonitorPaths,
        recorder: &RecorderHandle,
        monitor: &Monitor,
        captured_at: f64,
        context: &crate::event::ActivityContext,
        recording: &Arc<AtomicBool>,
        shutdown: &Arc<AtomicBool>,
    ) -> Result<bool, String> {
        let display_id = monitor.id().map_err(|error| error.to_string())?;
        let image = monitor.capture_image().map_err(|error| error.to_string())?;
        if !recording.load(Ordering::SeqCst)
            || shutdown.load(Ordering::SeqCst)
            || !session_is_active()
        {
            return Ok(false);
        }

        let relative = screenshot_relative_path(captured_at, display_id);
        let destination = paths.data_dir.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        let temporary = destination.with_extension("jpg.tmp");
        let write_result = (|| -> Result<(), String> {
            let file = File::create(&temporary).map_err(|error| error.to_string())?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
                .map_err(|error| error.to_string())?;
            let mut writer = BufWriter::new(file);
            JpegEncoder::new_with_quality(&mut writer, 65)
                .encode(
                    image.as_raw(),
                    image.width(),
                    image.height(),
                    ExtendedColorType::Rgba8,
                )
                .map_err(|error| error.to_string())?;
            writer.flush().map_err(|error| error.to_string())
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary);
            return Err(format!("display {display_id}: {error}"));
        }
        if !recording.load(Ordering::SeqCst)
            || shutdown.load(Ordering::SeqCst)
            || !session_is_active()
        {
            let _ = fs::remove_file(&temporary);
            return Ok(false);
        }
        if let Err(error) = fs::rename(&temporary, &destination) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("display {display_id}: {error}"));
        }
        if !recording.load(Ordering::SeqCst)
            || shutdown.load(Ordering::SeqCst)
            || !session_is_active()
        {
            let _ = fs::remove_file(&destination);
            return Ok(false);
        }
        recorder.submit(RawEvent::Screenshot {
            timestamp: captured_at,
            context: context.clone(),
            relative_path: relative,
            display_id,
            pixel_width: image.width(),
            pixel_height: image.height(),
        });
        Ok(true)
    }

    pub(super) fn screenshot_relative_path(timestamp: f64, display_id: u32) -> String {
        let milliseconds = (timestamp * 1_000.0).floor() as i64;
        let date = Utc
            .timestamp_millis_opt(milliseconds)
            .single()
            .unwrap_or_else(Utc::now);
        format!(
            "screenshots/{:04}/{:02}/{:02}/{milliseconds}-{display_id}.jpg",
            date.year(),
            date.month(),
            date.day()
        )
    }

    fn application_id(name: &str) -> String {
        let normalized: String = name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '.' || character == '-' {
                    character.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        normalized.trim_matches('-').to_owned()
    }

    fn session_is_active() -> bool {
        let session = std::env::var("XDG_SESSION_ID").unwrap_or_else(|_| "self".into());
        let output = Command::new("loginctl")
            .args(["show-session", &session, "-p", "Active", "-p", "LockedHint"])
            .stderr(Stdio::null())
            .output();
        if let Ok(output) = output
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            return text.lines().any(|line| line == "Active=yes")
                && !text.lines().any(|line| line == "LockedHint=yes");
        }

        screen_saver_call("/org/freedesktop/ScreenSaver", "GetActive")
            .or_else(|| screen_saver_call("/ScreenSaver", "GetActive"))
            .is_some_and(|value| value.contains("false"))
    }

    fn displays_are_awake() -> bool {
        if std::env::var("XDG_SESSION_TYPE").unwrap_or_default() != "x11" {
            return true;
        }
        Command::new("xset")
            .arg("q")
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).contains("Monitor is On"))
            .unwrap_or(false)
    }

    fn idle_seconds() -> Option<u64> {
        let x11_idle = Command::new("xprintidle")
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .map(|milliseconds| milliseconds / 1_000)
            });
        if x11_idle.is_some() {
            return x11_idle;
        }

        screen_saver_call("/org/freedesktop/ScreenSaver", "GetSessionIdleTime")
            .or_else(|| screen_saver_call("/ScreenSaver", "GetSessionIdleTime"))
            .and_then(|value| {
                value
                    .split(|character: char| !character.is_ascii_digit())
                    .find(|part| !part.is_empty())
                    .and_then(|part| part.parse().ok())
            })
    }

    fn screen_saver_call(object_path: &str, method: &str) -> Option<String> {
        let method_name = format!("org.freedesktop.ScreenSaver.{method}");
        let output = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.ScreenSaver",
                "--object-path",
                object_path,
                "--method",
            ])
            .arg(method_name)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn sleep_interruptibly(shutdown: &Arc<AtomicBool>, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline && !shutdown.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
        }
    }

    fn set_health(health: &Health, source: &str, state: &str, detail: impl Into<String>) {
        if let Ok(mut statuses) = health.lock() {
            statuses.insert(source.into(), CaptureSourceStatus::new(state, detail));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn builds_utc_screenshot_path() {
            assert_eq!(
                screenshot_relative_path(1_767_312_000.123, 9),
                "screenshots/2026/01/02/1767312000123-9.jpg"
            );
        }

        #[test]
        fn normalizes_application_name() {
            assert_eq!(application_id("Visual Studio Code"), "visual-studio-code");
        }
    }
}

#[cfg(all(not(target_os = "linux"), not(test)))]
mod platform {
    use super::CaptureSourceStatus;
    use crate::daemon::RecorderHandle;
    use crate::paths::MonitorPaths;
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    pub fn start(
        _paths: MonitorPaths,
        _recorder: RecorderHandle,
        _recording: Arc<AtomicBool>,
        _shutdown: Arc<AtomicBool>,
        health: Arc<Mutex<BTreeMap<String, CaptureSourceStatus>>>,
    ) {
        if let Ok(mut statuses) = health.lock() {
            statuses.insert(
                "capture".into(),
                CaptureSourceStatus::new(
                    "unsupported",
                    "this binary implements capture only on Linux",
                ),
            );
        }
    }
}
