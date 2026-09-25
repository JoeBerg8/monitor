import CoreGraphics
import Foundation
import OSLog

enum ShortcutMapper {
  // Hardware key codes for X, C, and V. Character data is never requested.
  private static let xKeyCode: Int64 = 7
  private static let cKeyCode: Int64 = 8
  private static let vKeyCode: Int64 = 9

  static func kind(keyCode: Int64, flags: CGEventFlags) -> ShortcutKind? {
    guard flags.contains(.maskCommand),
      !flags.contains(.maskShift),
      !flags.contains(.maskControl),
      !flags.contains(.maskAlternate)
    else { return nil }

    switch keyCode {
    case cKeyCode: return .copy
    case vKeyCode: return .paste
    case xKeyCode: return .cut
    default: return nil
    }
  }
}

final class InputMonitor {
  private let recorder: Recorder
  private let logger = Logger(
    subsystem: "com.joeberg.monitor",
    category: "input"
  )
  private var eventTap: CFMachPort?
  private var runLoopSource: CFRunLoopSource?

  init(recorder: Recorder) {
    self.recorder = recorder
  }

  func start() {
    guard eventTap == nil else { return }

    let eventTypes: [CGEventType] = [
      .leftMouseDown,
      .rightMouseDown,
      .otherMouseDown,
      .scrollWheel,
      .keyDown,
    ]
    let mask = eventTypes.reduce(CGEventMask(0)) {
      $0 | (CGEventMask(1) << $1.rawValue)
    }

    let opaqueSelf = Unmanaged.passUnretained(self).toOpaque()
    guard
      let tap = CGEvent.tapCreate(
        tap: .cgSessionEventTap,
        place: .headInsertEventTap,
        options: .listenOnly,
        eventsOfInterest: mask,
        callback: Self.callback,
        userInfo: opaqueSelf
      )
    else {
      logger.error("Unable to create event tap; input capture is disabled")
      return
    }

    let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
    eventTap = tap
    runLoopSource = source
    CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
    CGEvent.tapEnable(tap: tap, enable: true)
  }

  func stop() {
    if let runLoopSource {
      CFRunLoopRemoveSource(CFRunLoopGetMain(), runLoopSource, .commonModes)
    }
    if let eventTap {
      CFMachPortInvalidate(eventTap)
    }
    runLoopSource = nil
    eventTap = nil
  }

  func restart() {
    stop()
    start()
  }

  private func handle(type: CGEventType, event: CGEvent) {
    if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
      if let eventTap { CGEvent.tapEnable(tap: eventTap, enable: true) }
      return
    }

    let timestamp = Date()
    switch type {
    case .leftMouseDown:
      submitMouse(event: event, timestamp: timestamp, button: "left")
    case .rightMouseDown:
      submitMouse(event: event, timestamp: timestamp, button: "right")
    case .otherMouseDown:
      let number = event.getIntegerValueField(.mouseEventButtonNumber)
      submitMouse(event: event, timestamp: timestamp, button: "other_\(number)")
    case .scrollWheel:
      recorder.submit(
        rawEvent: .scroll(
          timestamp: timestamp,
          location: event.location,
          deltaX: event.getIntegerValueField(.scrollWheelEventPointDeltaAxis2),
          deltaY: event.getIntegerValueField(.scrollWheelEventPointDeltaAxis1),
          continuous: event.getIntegerValueField(.scrollWheelEventIsContinuous) != 0
        ))
    case .keyDown:
      let keyCode = event.getIntegerValueField(.keyboardEventKeycode)
      guard let kind = ShortcutMapper.kind(keyCode: keyCode, flags: event.flags) else {
        return
      }
      recorder.submit(
        rawEvent: .shortcut(
          timestamp: timestamp,
          kind: kind,
          keyCode: keyCode,
          modifiers: Int64(bitPattern: event.flags.rawValue)
        ))
    default:
      break
    }
  }

  private func submitMouse(event: CGEvent, timestamp: Date, button: String) {
    recorder.submit(
      rawEvent: .mouseDown(
        timestamp: timestamp,
        location: event.location,
        button: button
      ))
  }

  private static let callback: CGEventTapCallBack = { _, type, event, userInfo in
    guard let userInfo else { return Unmanaged.passUnretained(event) }
    let monitor = Unmanaged<InputMonitor>.fromOpaque(userInfo).takeUnretainedValue()
    monitor.handle(type: type, event: event)
    return Unmanaged.passUnretained(event)
  }
}
