import ApplicationServices
import CoreGraphics
import OSLog

struct CapturePermissionStatus: Equatable {
  let accessibility: Bool
  let inputMonitoring: Bool
  let screenRecording: Bool

  var canCaptureInput: Bool {
    accessibility || inputMonitoring
  }

  var allGranted: Bool {
    accessibility && canCaptureInput && screenRecording
  }

  var grantedCount: Int {
    [accessibility, canCaptureInput, screenRecording].filter { $0 }.count
  }

  var missingPermissionNames: [String] {
    var names: [String] = []
    if !accessibility { names.append("Accessibility") }
    if !canCaptureInput { names.append("Input Monitoring") }
    if !screenRecording { names.append("Screen Recording") }
    return names
  }
}

enum PermissionManager {
  private static let logger = Logger(
    subsystem: "com.joeberg.monitor",
    category: "permissions"
  )

  static func requestAll() -> CapturePermissionStatus {
    let accessibilityOptions =
      [
        kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true
      ] as CFDictionary
    let accessibilityGranted = AXIsProcessTrustedWithOptions(accessibilityOptions)
    let inputGranted = CGPreflightListenEventAccess() || CGRequestListenEventAccess()
    let screenGranted = CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess()

    let status = CapturePermissionStatus(
      accessibility: accessibilityGranted,
      inputMonitoring: inputGranted,
      screenRecording: screenGranted
    )
    log(status)
    return status
  }

  static func currentStatus() -> CapturePermissionStatus {
    CapturePermissionStatus(
      accessibility: AXIsProcessTrusted(),
      inputMonitoring: CGPreflightListenEventAccess(),
      screenRecording: CGPreflightScreenCaptureAccess()
    )
  }

  private static func log(_ status: CapturePermissionStatus) {
    logger.info("Accessibility permission: \(status.accessibility)")
    logger.info("Input Monitoring permission: \(status.inputMonitoring)")
    logger.info("Screen Recording permission: \(status.screenRecording)")
  }
}
