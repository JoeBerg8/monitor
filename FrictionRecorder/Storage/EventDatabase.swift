import Foundation
import SQLite3

private let sqliteTransient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

enum EventDatabaseError: Error, CustomStringConvertible {
  case sqlite(message: String)

  var description: String {
    switch self {
    case .sqlite(let message): message
    }
  }
}

final class EventDatabase {
  private var connection: OpaquePointer?
  private var insertStatement: OpaquePointer?

  init(url: URL) throws {
    var database: OpaquePointer?
    let result = sqlite3_open_v2(
      url.path,
      &database,
      SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX,
      nil
    )

    guard result == SQLITE_OK, let database else {
      let message =
        database.map { String(cString: sqlite3_errmsg($0)) }
        ?? "Unable to open SQLite database"
      if let database { sqlite3_close(database) }
      throw EventDatabaseError.sqlite(message: message)
    }

    connection = database

    do {
      try execute("PRAGMA journal_mode = WAL;")
      try execute("PRAGMA synchronous = NORMAL;")
      try execute("PRAGMA busy_timeout = 5000;")
      try execute(Self.schema)
      try prepareInsert()
    } catch {
      sqlite3_close(database)
      connection = nil
      throw error
    }
  }

  deinit {
    if let insertStatement { sqlite3_finalize(insertStatement) }
    if let connection { sqlite3_close(connection) }
  }

  func insert(_ event: EventRecord) throws {
    guard let statement = insertStatement else {
      throw EventDatabaseError.sqlite(message: "Insert statement is unavailable")
    }

    sqlite3_reset(statement)
    sqlite3_clear_bindings(statement)

    sqlite3_bind_double(statement, 1, event.timestamp)
    bind(event.type.rawValue, to: 2, in: statement)
    bind(event.appBundleID, to: 3, in: statement)
    bind(event.appName, to: 4, in: statement)
    bind(event.processID.map(Int64.init), to: 5, in: statement)
    bind(event.windowTitle, to: 6, in: statement)
    bind(event.mouseX, to: 7, in: statement)
    bind(event.mouseY, to: 8, in: statement)
    bind(event.keyCode, to: 9, in: statement)
    bind(event.modifiers, to: 10, in: statement)
    bind(event.metadataJSON, to: 11, in: statement)

    guard sqlite3_step(statement) == SQLITE_DONE else {
      throw currentError()
    }
  }

  func allEvents() throws -> [EventRecord] {
    let query = """
      SELECT id, timestamp, event_type, app_bundle_id, app_name, process_id,
             window_title, mouse_x, mouse_y, key_code, modifiers, metadata_json
      FROM events
      ORDER BY timestamp, id;
      """
    var statement: OpaquePointer?
    guard sqlite3_prepare_v2(connection, query, -1, &statement, nil) == SQLITE_OK,
      let statement
    else {
      throw currentError()
    }
    defer { sqlite3_finalize(statement) }

    var events: [EventRecord] = []
    while sqlite3_step(statement) == SQLITE_ROW {
      guard let typeText = string(at: 2, in: statement),
        let type = EventType(rawValue: typeText)
      else {
        continue
      }
      events.append(
        EventRecord(
          id: sqlite3_column_int64(statement, 0),
          timestamp: sqlite3_column_double(statement, 1),
          type: type,
          appBundleID: string(at: 3, in: statement),
          appName: string(at: 4, in: statement),
          processID: int64(at: 5, in: statement).map(Int32.init),
          windowTitle: string(at: 6, in: statement),
          mouseX: double(at: 7, in: statement),
          mouseY: double(at: 8, in: statement),
          keyCode: int64(at: 9, in: statement),
          modifiers: int64(at: 10, in: statement),
          metadataJSON: string(at: 11, in: statement)
        ))
    }
    return events
  }

  private func prepareInsert() throws {
    let sql = """
      INSERT INTO events (
          timestamp, event_type, app_bundle_id, app_name, process_id,
          window_title, mouse_x, mouse_y, key_code, modifiers, metadata_json
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);
      """
    guard sqlite3_prepare_v2(connection, sql, -1, &insertStatement, nil) == SQLITE_OK else {
      throw currentError()
    }
  }

  private func execute(_ sql: String) throws {
    var errorMessage: UnsafeMutablePointer<CChar>?
    let result = sqlite3_exec(connection, sql, nil, nil, &errorMessage)
    guard result == SQLITE_OK else {
      let message = errorMessage.map { String(cString: $0) } ?? currentError().description
      sqlite3_free(errorMessage)
      throw EventDatabaseError.sqlite(message: message)
    }
  }

  private func currentError() -> EventDatabaseError {
    guard let connection else {
      return .sqlite(message: "SQLite connection is closed")
    }
    return .sqlite(message: String(cString: sqlite3_errmsg(connection)))
  }

  private func bind(_ value: String?, to index: Int32, in statement: OpaquePointer) {
    if let value {
      sqlite3_bind_text(statement, index, value, -1, sqliteTransient)
    } else {
      sqlite3_bind_null(statement, index)
    }
  }

  private func bind(_ value: Int64?, to index: Int32, in statement: OpaquePointer) {
    if let value {
      sqlite3_bind_int64(statement, index, value)
    } else {
      sqlite3_bind_null(statement, index)
    }
  }

  private func bind(_ value: Double?, to index: Int32, in statement: OpaquePointer) {
    if let value {
      sqlite3_bind_double(statement, index, value)
    } else {
      sqlite3_bind_null(statement, index)
    }
  }

  private func string(at index: Int32, in statement: OpaquePointer) -> String? {
    guard sqlite3_column_type(statement, index) != SQLITE_NULL,
      let value = sqlite3_column_text(statement, index)
    else { return nil }
    return String(cString: value)
  }

  private func int64(at index: Int32, in statement: OpaquePointer) -> Int64? {
    guard sqlite3_column_type(statement, index) != SQLITE_NULL else { return nil }
    return sqlite3_column_int64(statement, index)
  }

  private func double(at index: Int32, in statement: OpaquePointer) -> Double? {
    guard sqlite3_column_type(statement, index) != SQLITE_NULL else { return nil }
    return sqlite3_column_double(statement, index)
  }

  private static let schema = """
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
    """
}
