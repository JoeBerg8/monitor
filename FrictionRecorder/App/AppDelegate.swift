import AppKit
import OSLog

final class AppDelegate: NSObject, NSApplicationDelegate {
  private let logger = Logger(subsystem: "com.joeberg.monitor", category: "lifecycle")
  private var recorder: Recorder?
  private var appWindowMonitor: AppWindowMonitor?
  private var inputMonitor: InputMonitor?
  private var screenshotMonitor: ScreenshotMonitor?
  private var menuBarController: MenuBarController?
  private var instanceLock: InstanceLock?
  private var permissionStatus: CapturePermissionStatus?
  private var permissionTimer: Timer?
  private var workspaceObservers: [NSObjectProtocol] = []
  private var isRecording = false

  func applicationDidFinishLaunching(_ notification: Notification) {
    guard !RuntimeEnvironment.isRunningTests else {
      logger.info("Capture startup suppressed under XCTest")
      return
    }

    do {
      let paths = try StoragePaths.live()
      do {
        instanceLock = try InstanceLock(
          url: paths.applicationSupportURL.appendingPathComponent(".instance.lock")
        )
      } catch InstanceLockError.alreadyRunning {
        logger.notice("Another monitor instance is already running")
        NSApplication.shared.terminate(nil)
        return
      }

      let database = try EventDatabase(url: paths.databaseURL)
      let recorder = Recorder(database: database)
      let appWindowMonitor = AppWindowMonitor(recorder: recorder)
      let inputMonitor = InputMonitor(recorder: recorder)
      let screenshotMonitor = ScreenshotMonitor(
        recorder: recorder,
        applicationSupportURL: paths.applicationSupportURL
      )

      self.recorder = recorder
      self.appWindowMonitor = appWindowMonitor
      self.inputMonitor = inputMonitor
      self.screenshotMonitor = screenshotMonitor

      let menuBarController = MenuBarController()
      menuBarController.onToggleRequested = { [weak self] in
        self?.toggleRecording()
      }
      menuBarController.onRequestPermissionsRequested = { [weak self] in
        self?.applyPermissionStatus(PermissionManager.requestAll())
      }
      menuBarController.onQuitRequested = {
        NSApplication.shared.terminate(nil)
      }
      self.menuBarController = menuBarController

      installWorkspaceObservers()
      applyPermissionStatus(PermissionManager.requestAll())
      startPermissionMonitoring()
      startRecording()
      logger.info("monitor started")
    } catch {
      logger.fault("Startup failed: \(String(describing: error), privacy: .public)")
      NSApplication.shared.terminate(nil)
    }
  }

  func applicationWillTerminate(_ notification: Notification) {
    stopRecording()
    permissionTimer?.invalidate()
    permissionTimer = nil

    let center = NSWorkspace.shared.notificationCenter
    workspaceObservers.forEach(center.removeObserver)
    workspaceObservers.removeAll()
  }

  private func installWorkspaceObservers() {
    let center = NSWorkspace.shared.notificationCenter
    workspaceObservers = [
      center.addObserver(
        forName: NSWorkspace.sessionDidResignActiveNotification,
        object: nil,
        queue: .main
      ) { [weak self] _ in self?.screenshotMonitor?.setSessionActive(false) },
      center.addObserver(
        forName: NSWorkspace.sessionDidBecomeActiveNotification,
        object: nil,
        queue: .main
      ) { [weak self] _ in self?.screenshotMonitor?.setSessionActive(true) },
      center.addObserver(
        forName: NSWorkspace.screensDidSleepNotification,
        object: nil,
        queue: .main
      ) { [weak self] _ in self?.screenshotMonitor?.setScreensAwake(false) },
      center.addObserver(
        forName: NSWorkspace.screensDidWakeNotification,
        object: nil,
        queue: .main
      ) { [weak self] _ in self?.screenshotMonitor?.setScreensAwake(true) },
      center.addObserver(
        forName: NSWorkspace.didWakeNotification,
        object: nil,
        queue: .main
      ) { [weak self] _ in
        guard let self, self.isRecording else { return }
        self.appWindowMonitor?.refresh()
        if self.permissionStatus?.canCaptureInput == true {
          self.inputMonitor?.restart()
        }
      },
    ]
  }

  private func toggleRecording() {
    if isRecording {
      stopRecording()
    } else {
      startRecording()
    }
  }

  private func startRecording() {
    guard !isRecording else { return }
    recorder?.setRecordingEnabled(true)
    appWindowMonitor?.start()
    if permissionStatus?.canCaptureInput == true {
      inputMonitor?.start()
    }
    screenshotMonitor?.start()
    isRecording = true
    refreshMenuState()
    logger.info("Recording started")
  }

  private func stopRecording() {
    guard isRecording else { return }
    appWindowMonitor?.stop()
    inputMonitor?.stop()
    screenshotMonitor?.stop()
    recorder?.setRecordingEnabled(false)
    isRecording = false
    refreshMenuState()
    logger.info("Recording stopped")
  }

  private func startPermissionMonitoring() {
    permissionTimer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) {
      [weak self] _ in
      self?.applyPermissionStatus(PermissionManager.currentStatus())
    }
  }

  private func applyPermissionStatus(_ status: CapturePermissionStatus) {
    let previous = permissionStatus
    guard previous != status else { return }
    permissionStatus = status

    if status.allGranted {
      logger.info("All capture permissions are granted")
    } else {
      logger.warning(
        "Capture is limited; missing permissions: \(status.missingPermissionNames.joined(separator: ", "), privacy: .public)"
      )
    }

    if isRecording {
      if status.canCaptureInput && previous?.canCaptureInput != true {
        inputMonitor?.start()
      } else if !status.canCaptureInput && previous?.canCaptureInput == true {
        inputMonitor?.stop()
      }

      if status.accessibility && previous?.accessibility != true {
        appWindowMonitor?.refresh()
      }
    }

    refreshMenuState()
  }

  private func refreshMenuState() {
    let status =
      permissionStatus
      ?? CapturePermissionStatus(
        accessibility: false,
        inputMonitoring: false,
        screenRecording: false
      )
    menuBarController?.setState(recording: isRecording, permissions: status)
  }
}
