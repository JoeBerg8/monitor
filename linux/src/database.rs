use crate::event::EventRecord;
use anyhow::{Context, Result};
use rusqlite::{Connection, Row, params};
use std::path::Path;

pub struct EventDatabase {
    connection: Connection,
}

impl EventDatabase {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)
            .with_context(|| format!("open SQLite database {}", path.display()))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(SCHEMA)?;
        Ok(Self { connection })
    }

    pub fn insert(&self, event: &EventRecord) -> Result<()> {
        self.connection.execute(
            "INSERT INTO events (
                timestamp, event_type, app_bundle_id, app_name, process_id,
                window_title, mouse_x, mouse_y, key_code, modifiers, metadata_json
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                event.timestamp,
                event.event_type,
                event.app_bundle_id,
                event.app_name,
                event.process_id,
                event.window_title,
                event.mouse_x,
                event.mouse_y,
                event.key_code,
                event.modifiers,
                event.metadata_json,
            ],
        )?;
        Ok(())
    }

    pub fn count(&self) -> Result<u64> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?)
    }

    pub fn all_events(&self) -> Result<Vec<EventRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT id, timestamp, event_type, app_bundle_id, app_name, process_id,
                    window_title, mouse_x, mouse_y, key_code, modifiers, metadata_json
             FROM events ORDER BY timestamp, id",
        )?;
        let rows = statement.query_map([], Self::read_event)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn read_event(row: &Row<'_>) -> rusqlite::Result<EventRecord> {
        Ok(EventRecord {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            event_type: row.get(2)?,
            app_bundle_id: row.get(3)?,
            app_name: row.get(4)?,
            process_id: row.get(5)?,
            window_title: row.get(6)?,
            mouse_x: row.get(7)?,
            mouse_y: row.get(8)?,
            key_code: row.get(9)?,
            modifiers: row.get(10)?,
            metadata_json: row.get(11)?,
        })
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp REAL NOT NULL,
    event_type TEXT NOT NULL,
    app_bundle_id TEXT,
    app_name TEXT,
    process_id INTEGER,
    window_title TEXT,
    mouse_x REAL,
    mouse_y REAL,
    key_code INTEGER,
    modifiers INTEGER,
    metadata_json TEXT
);

CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);
CREATE INDEX IF NOT EXISTS idx_events_app ON events(app_bundle_id);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_schema_and_round_trips_event() {
        let directory = tempdir().unwrap();
        let database = EventDatabase::open(&directory.path().join("events.sqlite")).unwrap();
        let event = EventRecord {
            id: None,
            timestamp: 123.5,
            event_type: "window_changed".into(),
            app_bundle_id: Some("code".into()),
            app_name: Some("Code".into()),
            process_id: Some(77),
            window_title: Some("main.rs".into()),
            mouse_x: None,
            mouse_y: None,
            key_code: None,
            modifiers: None,
            metadata_json: None,
        };
        database.insert(&event).unwrap();
        let saved = database.all_events().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].event_type, "window_changed");
        assert_eq!(saved[0].window_title.as_deref(), Some("main.rs"));
        assert_eq!(database.count().unwrap(), 1);
    }
}
