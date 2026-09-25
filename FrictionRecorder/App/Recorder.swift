import Foundation
import OSLog

final class Recorder {
  private let queue = DispatchQueue(label: "com.joeberg.monitor.recorder")
  private let database: EventDatabase
  private let logger = Logger(subsystem: "com.joeberg.monitor", category: "recorder")
  private var context = ActivityContext.empty
  private var isRecordingEnabled = true

  init(database: EventDatabase) {
    self.database = database
  }

  func submit(rawEvent: RawEvent) {
    queue.async { [weak self] in
      self?.process(rawEvent)
    }
  }

  func contextSnapshot(_ completion: @escaping (ActivityContext) -> Void) {
    queue.async { [weak self] in
      completion(self?.context ?? .empty)
    }
  }

  func setRecordingEnabled(_ enabled: Bool) {
    queue.async { [weak self] in
      self?.isRecordingEnabled = enabled
    }
  }

  func flushForTesting() {
    queue.sync {}
  }

  private func process(_ rawEvent: RawEvent) {
    guard isRecordingEnabled else { return }
    let event: EventRecord

    switch rawEvent {
    case .appActivated(let timestamp, let application):
      context = ActivityContext(application: application, windowTitle: nil)
      event = makeRecord(timestamp: timestamp, type: .appActivated, context: context)

    case .windowChanged(let timestamp, let title):
      context.windowTitle = title
      event = makeRecord(timestamp: timestamp, type: .windowChanged, context: context)

    case .mouseDown(let timestamp, let location, let button):
      event = makeRecord(
        timestamp: timestamp,
        type: .mouseDown,
        context: context,
        mouseX: location.x,
        mouseY: location.y,
        metadata: ["button": button]
      )

    case .scroll(let timestamp, let location, let deltaX, let deltaY, let continuous):
      event = makeRecord(
        timestamp: timestamp,
        type: .scroll,
        context: context,
        mouseX: location.x,
        mouseY: location.y,
        metadata: [
          "deltaX": deltaX,
          "deltaY": deltaY,
          "continuous": continuous,
        ]
      )

    case .shortcut(let timestamp, let kind, let keyCode, let modifiers):
      event = makeRecord(
        timestamp: timestamp,
        type: kind.eventType,
        context: context,
        keyCode: keyCode,
        modifiers: modifiers
      )

    case .screenshot(
      let
        timestamp,
      let
        capturedContext,
      let
        relativePath,
      let
        displayID,
      let
        pixelWidth,
      let
        pixelHeight
    ):
      event = makeRecord(
        timestamp: timestamp,
        type: .screenshot,
        context: capturedContext,
        metadata: [
          "path": relativePath,
          "displayID": displayID,
          "pixelWidth": pixelWidth,
          "pixelHeight": pixelHeight,
        ]
      )
    }

    do {
      try database.insert(event)
    } catch {
      logger.error("Database insert failed: \(String(describing: error), privacy: .public)")
    }
  }

  private func makeRecord(
    timestamp: Date,
    type: EventType,
    context: ActivityContext,
    mouseX: Double? = nil,
    mouseY: Double? = nil,
    keyCode: Int64? = nil,
    modifiers: Int64? = nil,
    metadata: [String: Any]? = nil
  ) -> EventRecord {
    EventRecord(
      id: nil,
      timestamp: timestamp.timeIntervalSince1970,
      type: type,
      appBundleID: context.application?.bundleID,
      appName: context.application?.name,
      processID: context.application?.processID,
      windowTitle: context.windowTitle,
      mouseX: mouseX,
      mouseY: mouseY,
      keyCode: keyCode,
      modifiers: modifiers,
      metadataJSON: metadata.flatMap(Self.encodeMetadata)
    )
  }

  private static func encodeMetadata(_ metadata: [String: Any]) -> String? {
    guard JSONSerialization.isValidJSONObject(metadata),
      let data = try? JSONSerialization.data(withJSONObject: metadata, options: [.sortedKeys])
    else {
      return nil
    }
    return String(data: data, encoding: .utf8)
  }
}
