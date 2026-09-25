import AppKit
import ApplicationServices
import Foundation
import OSLog

final class AppWindowMonitor {
  private let recorder: Recorder
  private let workspace = NSWorkspace.shared
  private let logger = Logger(
    subsystem: "com.joeberg.monitor",
    category: "app-window"
  )
  private var activationObserver: NSObjectProtocol?
  private var timer: Timer?
  private var activeApplication: ApplicationIdentity?
  private var lastWindowTitle: String?
  private var hasObservedWindow = false

  init(recorder: Recorder) {
    self.recorder = recorder
  }

  func start() {
    activationObserver = workspace.notificationCenter.addObserver(
      forName: NSWorkspace.didActivateApplicationNotification,
      object: nil,
      queue: .main
    ) { [weak self] notification in
      guard
        let application = notification.userInfo?[NSWorkspace.applicationUserInfoKey]
          as? NSRunningApplication
      else { return }
      self?.recordActivation(application)
    }

    timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
      self?.pollWindowTitle()
    }
    refresh()
  }

  func stop() {
    timer?.invalidate()
    timer = nil
    if let activationObserver {
      workspace.notificationCenter.removeObserver(activationObserver)
    }
    activationObserver = nil
    activeApplication = nil
    lastWindowTitle = nil
    hasObservedWindow = false
  }

  func refresh() {
    guard let application = workspace.frontmostApplication else { return }
    let identity = Self.identity(for: application)
    if identity != activeApplication {
      recordActivation(application)
    }
    pollWindowTitle()
  }

  private func recordActivation(_ application: NSRunningApplication) {
    let identity = Self.identity(for: application)
    activeApplication = identity
    lastWindowTitle = nil
    hasObservedWindow = false
    recorder.submit(rawEvent: .appActivated(timestamp: Date(), application: identity))
    pollWindowTitle()
  }

  private func pollWindowTitle() {
    guard AXIsProcessTrusted(),
      let application = workspace.frontmostApplication
    else { return }

    let identity = Self.identity(for: application)
    if identity != activeApplication {
      recordActivation(application)
      return
    }

    let title = focusedWindowTitle(processID: application.processIdentifier)
    guard !hasObservedWindow || title != lastWindowTitle else { return }
    hasObservedWindow = true
    lastWindowTitle = title
    recorder.submit(rawEvent: .windowChanged(timestamp: Date(), title: title))
  }

  private func focusedWindowTitle(processID: pid_t) -> String? {
    let application = AXUIElementCreateApplication(processID)
    var windowValue: CFTypeRef?
    let windowResult = AXUIElementCopyAttributeValue(
      application,
      kAXFocusedWindowAttribute as CFString,
      &windowValue
    )
    guard windowResult == .success,
      let windowValue,
      CFGetTypeID(windowValue) == AXUIElementGetTypeID()
    else {
      return nil
    }

    let window = unsafeBitCast(windowValue, to: AXUIElement.self)
    var titleValue: CFTypeRef?
    let titleResult = AXUIElementCopyAttributeValue(
      window,
      kAXTitleAttribute as CFString,
      &titleValue
    )
    guard titleResult == .success else {
      if titleResult != .noValue && titleResult != .attributeUnsupported {
        logger.debug("Unable to read window title: \(titleResult.rawValue)")
      }
      return nil
    }
    return titleValue as? String
  }

  private static func identity(for application: NSRunningApplication) -> ApplicationIdentity {
    ApplicationIdentity(
      bundleID: application.bundleIdentifier,
      name: application.localizedName,
      processID: application.processIdentifier
    )
  }
}
