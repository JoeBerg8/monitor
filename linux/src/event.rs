use serde::Serialize;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ApplicationIdentity {
    pub bundle_id: Option<String>,
    pub name: Option<String>,
    pub process_id: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActivityContext {
    pub application: Option<ApplicationIdentity>,
    pub window_title: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub enum ShortcutKind {
    Copy,
    Paste,
    Cut,
}

impl ShortcutKind {
    pub fn event_type(self) -> &'static str {
        match self {
            Self::Copy => "shortcut_copy",
            Self::Paste => "shortcut_paste",
            Self::Cut => "shortcut_cut",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub enum RawEvent {
    AppActivated {
        timestamp: f64,
        application: ApplicationIdentity,
    },
    WindowChanged {
        timestamp: f64,
        title: Option<String>,
    },
    MouseDown {
        timestamp: f64,
        x: f64,
        y: f64,
        button: String,
    },
    Scroll {
        timestamp: f64,
        x: f64,
        y: f64,
        delta_x: i64,
        delta_y: i64,
        continuous: bool,
    },
    Shortcut {
        timestamp: f64,
        kind: ShortcutKind,
        key_code: i64,
        modifiers: i64,
    },
    Screenshot {
        timestamp: f64,
        context: ActivityContext,
        relative_path: String,
        display_id: u32,
        pixel_width: u32,
        pixel_height: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EventRecord {
    pub id: Option<i64>,
    pub timestamp: f64,
    pub event_type: String,
    pub app_bundle_id: Option<String>,
    pub app_name: Option<String>,
    pub process_id: Option<i64>,
    pub window_title: Option<String>,
    pub mouse_x: Option<f64>,
    pub mouse_y: Option<f64>,
    pub key_code: Option<i64>,
    pub modifiers: Option<i64>,
    pub metadata_json: Option<String>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn now_timestamp() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn normalize(raw: RawEvent, context: &mut ActivityContext) -> EventRecord {
    match raw {
        RawEvent::AppActivated {
            timestamp,
            application,
        } => {
            *context = ActivityContext {
                application: Some(application),
                window_title: None,
            };
            make_record(
                timestamp,
                "app_activated",
                context,
                None,
                None,
                None,
                None,
                None,
            )
        }
        RawEvent::WindowChanged { timestamp, title } => {
            context.window_title = title;
            make_record(
                timestamp,
                "window_changed",
                context,
                None,
                None,
                None,
                None,
                None,
            )
        }
        RawEvent::MouseDown {
            timestamp,
            x,
            y,
            button,
        } => make_record(
            timestamp,
            "mouse_down",
            context,
            Some(x),
            Some(y),
            None,
            None,
            Some(json!({ "button": button })),
        ),
        RawEvent::Scroll {
            timestamp,
            x,
            y,
            delta_x,
            delta_y,
            continuous,
        } => make_record(
            timestamp,
            "scroll",
            context,
            Some(x),
            Some(y),
            None,
            None,
            Some(json!({
                "continuous": continuous,
                "deltaX": delta_x,
                "deltaY": delta_y
            })),
        ),
        RawEvent::Shortcut {
            timestamp,
            kind,
            key_code,
            modifiers,
        } => make_record(
            timestamp,
            kind.event_type(),
            context,
            None,
            None,
            Some(key_code),
            Some(modifiers),
            None,
        ),
        RawEvent::Screenshot {
            timestamp,
            context: captured_context,
            relative_path,
            display_id,
            pixel_width,
            pixel_height,
        } => make_record(
            timestamp,
            "screenshot",
            &captured_context,
            None,
            None,
            None,
            None,
            Some(json!({
                "displayID": display_id,
                "path": relative_path,
                "pixelHeight": pixel_height,
                "pixelWidth": pixel_width
            })),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn make_record(
    timestamp: f64,
    event_type: &str,
    context: &ActivityContext,
    mouse_x: Option<f64>,
    mouse_y: Option<f64>,
    key_code: Option<i64>,
    modifiers: Option<i64>,
    metadata: Option<Value>,
) -> EventRecord {
    let application = context.application.as_ref();
    EventRecord {
        id: None,
        timestamp,
        event_type: event_type.to_owned(),
        app_bundle_id: application.and_then(|value| value.bundle_id.clone()),
        app_name: application.and_then(|value| value.name.clone()),
        process_id: application.and_then(|value| value.process_id),
        window_title: context.window_title.clone(),
        mouse_x,
        mouse_y,
        key_code,
        modifiers,
        metadata_json: metadata.map(|value| value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_inherits_current_context() {
        let mut context = ActivityContext::default();
        normalize(
            RawEvent::AppActivated {
                timestamp: 1.0,
                application: ApplicationIdentity {
                    bundle_id: Some("code".into()),
                    name: Some("Code".into()),
                    process_id: Some(42),
                },
            },
            &mut context,
        );
        normalize(
            RawEvent::WindowChanged {
                timestamp: 2.0,
                title: Some("main.rs".into()),
            },
            &mut context,
        );
        let event = normalize(
            RawEvent::MouseDown {
                timestamp: 3.0,
                x: 10.0,
                y: 20.0,
                button: "left".into(),
            },
            &mut context,
        );
        assert_eq!(event.app_bundle_id.as_deref(), Some("code"));
        assert_eq!(event.window_title.as_deref(), Some("main.rs"));
        assert_eq!(
            event.metadata_json.as_deref(),
            Some("{\"button\":\"left\"}")
        );
    }

    #[test]
    fn screenshot_uses_capture_context() {
        let mut live = ActivityContext::default();
        let captured = ActivityContext {
            application: Some(ApplicationIdentity {
                bundle_id: Some("terminal".into()),
                name: Some("Terminal".into()),
                process_id: Some(7),
            }),
            window_title: Some("agent".into()),
        };
        let event = normalize(
            RawEvent::Screenshot {
                timestamp: 3.0,
                context: captured,
                relative_path: "screenshots/2026/01/02/3000-1.jpg".into(),
                display_id: 1,
                pixel_width: 100,
                pixel_height: 50,
            },
            &mut live,
        );
        assert_eq!(event.app_bundle_id.as_deref(), Some("terminal"));
        assert_eq!(event.window_title.as_deref(), Some("agent"));
    }
}
