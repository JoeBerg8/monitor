import CoreGraphics
import Foundation
import XCTest

@testable import monitor

final class EventDatabaseTests: XCTestCase {
  private var temporaryDirectory: URL!

  override func setUpWithError() throws {
    temporaryDirectory = FileManager.default.temporaryDirectory
      .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(
      at: temporaryDirectory,
      withIntermediateDirectories: true
    )
  }

  override func tearDownWithError() throws {
    try? FileManager.default.removeItem(at: temporaryDirectory)
  }

  func testDatabasePersistsNormalizedTimeline() throws {
    let databaseURL = temporaryDirectory.appendingPathComponent("events.sqlite")
    let database = try EventDatabase(url: databaseURL)
    let recorder = Recorder(database: database)
    let application = ApplicationIdentity(
      bundleID: "com.example.Browser",
      name: "Browser",
      processID: 42
    )
    let baseDate = Date(timeIntervalSince1970: 1_000)

    recorder.submit(rawEvent: .appActivated(timestamp: baseDate, application: application))
    recorder.submit(
      rawEvent: .windowChanged(
        timestamp: baseDate.addingTimeInterval(1),
        title: "Customers"
      ))
    recorder.submit(
      rawEvent: .mouseDown(
        timestamp: baseDate.addingTimeInterval(2),
        location: CGPoint(x: 100, y: 200),
        button: "left"
      ))
    recorder.flushForTesting()

    let events = try database.allEvents()
    XCTAssertEqual(events.map(\.type), [.appActivated, .windowChanged, .mouseDown])
    XCTAssertEqual(events[2].appBundleID, "com.example.Browser")
    XCTAssertEqual(events[2].windowTitle, "Customers")
    XCTAssertEqual(events[2].mouseX, 100)
    XCTAssertEqual(events[2].mouseY, 200)

    let metadata = try XCTUnwrap(events[2].metadataJSON?.data(using: .utf8))
    let object = try XCTUnwrap(
      JSONSerialization.jsonObject(with: metadata) as? [String: String]
    )
    XCTAssertEqual(object["button"], "left")
  }

  func testDatabaseCanBeReopened() throws {
    let databaseURL = temporaryDirectory.appendingPathComponent("events.sqlite")
    do {
      let database = try EventDatabase(url: databaseURL)
      try database.insert(
        EventRecord(
          id: nil,
          timestamp: 123,
          type: .appActivated,
          appBundleID: "com.example.App",
          appName: "App",
          processID: 7,
          windowTitle: nil,
          mouseX: nil,
          mouseY: nil,
          keyCode: nil,
          modifiers: nil,
          metadataJSON: nil
        ))
    }

    let reopened = try EventDatabase(url: databaseURL)
    XCTAssertEqual(try reopened.allEvents().count, 1)
  }

  func testPausedRecorderDropsEventsAndCanResume() throws {
    let database = try EventDatabase(
      url: temporaryDirectory.appendingPathComponent("events.sqlite")
    )
    let recorder = Recorder(database: database)
    let application = ApplicationIdentity(
      bundleID: "com.example.App",
      name: "App",
      processID: 7
    )

    recorder.setRecordingEnabled(false)
    recorder.submit(rawEvent: .appActivated(timestamp: Date(), application: application))
    recorder.flushForTesting()
    XCTAssertTrue(try database.allEvents().isEmpty)

    recorder.setRecordingEnabled(true)
    recorder.submit(rawEvent: .appActivated(timestamp: Date(), application: application))
    recorder.flushForTesting()
    XCTAssertEqual(try database.allEvents().count, 1)
  }

  func testLegacyStorageDirectoryMigratesToMonitor() throws {
    let legacyDirectory = temporaryDirectory.appendingPathComponent(
      "FrictionRecorder",
      isDirectory: true
    )
    try FileManager.default.createDirectory(
      at: legacyDirectory,
      withIntermediateDirectories: true
    )
    let marker = legacyDirectory.appendingPathComponent("marker")
    try Data("existing-data".utf8).write(to: marker)

    let paths = try StoragePaths.make(in: temporaryDirectory)

    XCTAssertEqual(paths.applicationSupportURL.lastPathComponent, "monitor")
    XCTAssertTrue(
      FileManager.default.fileExists(
        atPath: paths.applicationSupportURL.appendingPathComponent("marker").path
      )
    )
    XCTAssertFalse(FileManager.default.fileExists(atPath: legacyDirectory.path))
  }
}
