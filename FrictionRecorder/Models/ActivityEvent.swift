import CoreGraphics
import Foundation

enum EventType: String, Equatable {
  case appActivated = "app_activated"
  case windowChanged = "window_changed"
  case mouseDown = "mouse_down"
  case scroll
  case shortcutCopy = "shortcut_copy"
  case shortcutPaste = "shortcut_paste"
  case shortcutCut = "shortcut_cut"
  case screenshot
}

struct ApplicationIdentity: Equatable {
  let bundleID: String?
  let name: String?
  let processID: Int32
}

struct ActivityContext: Equatable {
  var application: ApplicationIdentity?
  var windowTitle: String?

  static let empty = ActivityContext(application: nil, windowTitle: nil)
}

enum ShortcutKind: Equatable {
  case copy
  case paste
  case cut

  var eventType: EventType {
    switch self {
    case .copy: return .shortcutCopy
    case .paste: return .shortcutPaste
    case .cut: return .shortcutCut
    }
  }
}

enum RawEvent {
  case appActivated(timestamp: Date, application: ApplicationIdentity)
  case windowChanged(timestamp: Date, title: String?)
  case mouseDown(timestamp: Date, location: CGPoint, button: String)
  case scroll(
    timestamp: Date,
    location: CGPoint,
    deltaX: Int64,
    deltaY: Int64,
    continuous: Bool
  )
  case shortcut(timestamp: Date, kind: ShortcutKind, keyCode: Int64, modifiers: Int64)
  case screenshot(
    timestamp: Date,
    context: ActivityContext,
    relativePath: String,
    displayID: UInt32,
    pixelWidth: Int,
    pixelHeight: Int
  )
}

struct EventRecord: Equatable {
  let id: Int64?
  let timestamp: TimeInterval
  let type: EventType
  let appBundleID: String?
  let appName: String?
  let processID: Int32?
  let windowTitle: String?
  let mouseX: Double?
  let mouseY: Double?
  let keyCode: Int64?
  let modifiers: Int64?
  let metadataJSON: String?
}
