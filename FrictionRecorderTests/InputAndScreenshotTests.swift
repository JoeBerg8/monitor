import CoreGraphics
import Foundation
import XCTest

@testable import monitor

final class InputAndScreenshotTests: XCTestCase {
  func testShortcutMapperAcceptsOnlyCommandCopyPasteCut() {
    XCTAssertEqual(ShortcutMapper.kind(keyCode: 8, flags: .maskCommand), .copy)
    XCTAssertEqual(ShortcutMapper.kind(keyCode: 9, flags: .maskCommand), .paste)
    XCTAssertEqual(ShortcutMapper.kind(keyCode: 7, flags: .maskCommand), .cut)

    XCTAssertNil(ShortcutMapper.kind(keyCode: 0, flags: .maskCommand))
    XCTAssertNil(ShortcutMapper.kind(keyCode: 8, flags: []))
    XCTAssertNil(
      ShortcutMapper.kind(
        keyCode: 8,
        flags: [.maskCommand, .maskShift]
      ))
  }

  func testScreenshotPathUsesUTCAndDisplayID() {
    let date = Date(timeIntervalSince1970: 1_798_200_000.123)
    let path = ScreenshotPathBuilder.relativePath(capturedAt: date, displayID: 1234)

    XCTAssertTrue(path.hasPrefix("screenshots/2026/12/25/"))
    XCTAssertTrue(path.hasSuffix("-1234.jpg"))
    XCTAssertFalse(path.hasPrefix("/"))
  }
}
