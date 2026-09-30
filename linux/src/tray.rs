use crate::capture::CaptureSourceStatus;
use crate::daemon::RecorderHandle;
use crate::paths::MonitorPaths;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

#[cfg(any(target_os = "linux", test))]
pub fn start(
    paths: MonitorPaths,
    recorder: RecorderHandle,
    recording: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    health: Arc<Mutex<BTreeMap<String, CaptureSourceStatus>>>,
) {
    platform::start(paths, recorder, recording, shutdown, health);
}

#[cfg(all(not(target_os = "linux"), not(test)))]
pub fn start(
    _paths: MonitorPaths,
    _recorder: RecorderHandle,
    _recording: Arc<AtomicBool>,
    _shutdown: Arc<AtomicBool>,
    _health: Arc<Mutex<BTreeMap<String, CaptureSourceStatus>>>,
) {
}

#[cfg(any(target_os = "linux", test))]
mod platform {
    use super::*;
    use crate::daemon;
    use ksni::blocking::TrayMethods;
    use ksni::menu::{MenuItem, StandardItem};
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::Duration;

    struct MonitorTray {
        paths: MonitorPaths,
        recorder: RecorderHandle,
        recording: Arc<AtomicBool>,
        shutdown: Arc<AtomicBool>,
    }

    impl MonitorTray {
        fn set_recording(&self, enabled: bool) {
            if daemon::persist_recording(&self.paths, enabled).is_ok() {
                self.recording.store(enabled, Ordering::SeqCst);
                self.recorder.set_enabled(enabled);
            }
        }
    }

    impl ksni::Tray for MonitorTray {
        const MENU_ON_ACTIVATE: bool = true;

        fn id(&self) -> String {
            "monitor".into()
        }

        fn title(&self) -> String {
            if self.recording.load(Ordering::Relaxed) {
                "monitor — Recording"
            } else {
                "monitor — Paused"
            }
            .into()
        }

        fn icon_name(&self) -> String {
            if self.recording.load(Ordering::Relaxed) {
                "media-record"
            } else {
                "media-playback-pause"
            }
            .into()
        }

        fn status(&self) -> ksni::Status {
            ksni::Status::Active
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let recording = self.recording.load(Ordering::Relaxed);
            vec![
                StandardItem {
                    label: if recording {
                        "Pause recording"
                    } else {
                        "Start recording"
                    }
                    .into(),
                    icon_name: if recording {
                        "media-playback-pause"
                    } else {
                        "media-record"
                    }
                    .into(),
                    activate: Box::new(move |tray: &mut MonitorTray| {
                        tray.set_recording(!recording)
                    }),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Quit monitor".into(),
                    icon_name: "application-exit".into(),
                    activate: Box::new(|tray: &mut MonitorTray| {
                        tray.set_recording(false);
                        tray.shutdown.store(true, Ordering::SeqCst);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    pub fn start(
        paths: MonitorPaths,
        recorder: RecorderHandle,
        recording: Arc<AtomicBool>,
        shutdown: Arc<AtomicBool>,
        health: Arc<Mutex<BTreeMap<String, CaptureSourceStatus>>>,
    ) {
        thread::Builder::new()
            .name("monitor-tray".into())
            .spawn(move || {
                let tray = MonitorTray {
                    paths,
                    recorder,
                    recording,
                    shutdown: Arc::clone(&shutdown),
                };
                match tray.spawn() {
                    Ok(handle) => {
                        set_health(&health, "tray", "active", "status indicator is available");
                        while !shutdown.load(Ordering::Relaxed) && !handle.is_closed() {
                            handle.update(|_| {});
                            thread::sleep(Duration::from_secs(1));
                        }
                        drop(handle.shutdown());
                    }
                    Err(error) => set_health(
                        &health,
                        "tray",
                        "unavailable",
                        format!("desktop status notifier unavailable: {error}"),
                    ),
                }
            })
            .expect("start status notifier");
    }

    fn set_health(
        health: &Arc<Mutex<BTreeMap<String, CaptureSourceStatus>>>,
        source: &str,
        state: &str,
        detail: impl Into<String>,
    ) {
        if let Ok(mut statuses) = health.lock() {
            statuses.insert(
                source.into(),
                CaptureSourceStatus {
                    state: state.into(),
                    detail: detail.into(),
                },
            );
        }
    }
}
