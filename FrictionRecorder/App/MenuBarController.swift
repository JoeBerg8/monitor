import AppKit

final class MenuBarController: NSObject {
  var onToggleRequested: (() -> Void)?
  var onRequestPermissionsRequested: (() -> Void)?
  var onQuitRequested: (() -> Void)?

  private let statusItem: NSStatusItem
  private let statusSummaryItem = NSMenuItem()
  private let toggleItem = NSMenuItem()
  private let requestPermissionsItem = NSMenuItem()
  private let accessibilityItem = NSMenuItem()
  private let inputMonitoringItem = NSMenuItem()
  private let screenRecordingItem = NSMenuItem()

  override init() {
    statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    super.init()

    let menu = NSMenu()
    statusSummaryItem.isEnabled = false
    menu.addItem(statusSummaryItem)
    menu.addItem(.separator())

    toggleItem.target = self
    toggleItem.action = #selector(toggleRecording)
    menu.addItem(toggleItem)
    requestPermissionsItem.title = "Request Permissions Again…"
    requestPermissionsItem.target = self
    requestPermissionsItem.action = #selector(requestPermissions)
    menu.addItem(requestPermissionsItem)
    menu.addItem(.separator())

    configureSettingsItem(
      accessibilityItem,
      action: #selector(openAccessibilitySettings)
    )
    configureSettingsItem(
      inputMonitoringItem,
      action: #selector(openInputMonitoringSettings)
    )
    configureSettingsItem(
      screenRecordingItem,
      action: #selector(openScreenRecordingSettings)
    )
    menu.addItem(accessibilityItem)
    menu.addItem(inputMonitoringItem)
    menu.addItem(screenRecordingItem)
    menu.addItem(.separator())

    let quitItem = NSMenuItem(
      title: "Quit monitor",
      action: #selector(quit),
      keyEquivalent: "q"
    )
    quitItem.target = self
    menu.addItem(quitItem)
    menu.autoenablesItems = false
    statusItem.menu = menu

    setState(
      recording: false,
      permissions: CapturePermissionStatus(
        accessibility: false,
        inputMonitoring: false,
        screenRecording: false
      )
    )
  }

  deinit {
    NSStatusBar.system.removeStatusItem(statusItem)
  }

  func setState(recording: Bool, permissions: CapturePermissionStatus) {
    toggleItem.title = recording ? "Stop Recording" : "Start Recording"
    statusSummaryItem.title = statusTitle(recording: recording, permissions: permissions)
    accessibilityItem.title = permissionTitle("Accessibility", permissions.accessibility)
    inputMonitoringItem.title = inputPermissionTitle(permissions)
    screenRecordingItem.title = permissionTitle("Screen Recording", permissions.screenRecording)

    guard let button = statusItem.button else { return }
    let isDegraded = recording && !permissions.allGranted
    let symbolName: String
    let color: NSColor
    if isDegraded {
      symbolName = "exclamationmark.triangle.fill"
      color = .systemOrange
    } else if recording {
      symbolName = "record.circle.fill"
      color = .systemRed
    } else {
      symbolName = "record.circle"
      color = .secondaryLabelColor
    }

    let description = statusTitle(recording: recording, permissions: permissions)
    button.image = NSImage(
      systemSymbolName: symbolName,
      accessibilityDescription: description
    )
    button.contentTintColor = color
    button.toolTip = "monitor: \(description)"
  }

  @objc private func toggleRecording() {
    onToggleRequested?()
  }

  @objc private func requestPermissions() {
    onRequestPermissionsRequested?()
  }

  @objc private func quit() {
    onQuitRequested?()
  }

  @objc private func openAccessibilitySettings() {
    openPrivacySettings(pane: "Privacy_Accessibility")
  }

  @objc private func openInputMonitoringSettings() {
    openPrivacySettings(pane: "Privacy_ListenEvent")
  }

  @objc private func openScreenRecordingSettings() {
    openPrivacySettings(pane: "Privacy_ScreenCapture")
  }

  private func configureSettingsItem(_ item: NSMenuItem, action: Selector) {
    item.target = self
    item.action = action
  }

  private func openPrivacySettings(pane: String) {
    guard
      let url = URL(
        string: "x-apple.systempreferences:com.apple.preference.security?\(pane)"
      )
    else { return }
    NSWorkspace.shared.open(url)
  }

  private func statusTitle(
    recording: Bool,
    permissions: CapturePermissionStatus
  ) -> String {
    if !recording { return "Paused" }
    if permissions.allGranted { return "Recording" }
    return "Recording limited (\(permissions.grantedCount)/3 permissions)"
  }

  private func permissionTitle(_ name: String, _ granted: Bool) -> String {
    "\(granted ? "✓" : "⚠") \(name)"
  }

  private func inputPermissionTitle(_ permissions: CapturePermissionStatus) -> String {
    if permissions.inputMonitoring {
      return permissionTitle("Input Monitoring", true)
    }
    if permissions.accessibility {
      return "✓ Input capture (via Accessibility)"
    }
    return permissionTitle("Input Monitoring", false)
  }
}
