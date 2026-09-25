import Foundation
import XCTest

@testable import monitor

final class LifecycleSafetyTests: XCTestCase {
  func testTestProcessDisablesCaptureStartup() {
    XCTAssertTrue(RuntimeEnvironment.isRunningTests)
  }

  func testInstanceLockRejectsASecondOwner() throws {
    let directory = FileManager.default.temporaryDirectory
      .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }

    let lockURL = directory.appendingPathComponent("instance.lock")
    var firstLock: InstanceLock? = try InstanceLock(url: lockURL)
    XCTAssertNotNil(firstLock)

    XCTAssertThrowsError(try InstanceLock(url: lockURL)) { error in
      guard case InstanceLockError.alreadyRunning = error else {
        return XCTFail("Expected alreadyRunning, received \(error)")
      }
    }

    firstLock = nil
    XCTAssertNoThrow(try InstanceLock(url: lockURL))
  }

  func testPermissionStatusReportsDegradedState() {
    let status = CapturePermissionStatus(
      accessibility: true,
      inputMonitoring: false,
      screenRecording: false
    )

    XCTAssertFalse(status.allGranted)
    XCTAssertTrue(status.canCaptureInput)
    XCTAssertEqual(status.grantedCount, 2)
    XCTAssertEqual(status.missingPermissionNames, ["Screen Recording"])
  }
}
