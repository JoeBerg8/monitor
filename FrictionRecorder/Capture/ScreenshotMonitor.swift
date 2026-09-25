import AppKit
import CoreGraphics
import Foundation
import OSLog
import ScreenCaptureKit

enum ScreenshotPathBuilder {
  static func relativePath(capturedAt: Date, displayID: UInt32) -> String {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(secondsFromGMT: 0)!
    let components = calendar.dateComponents([.year, .month, .day], from: capturedAt)
    let milliseconds = Int64((capturedAt.timeIntervalSince1970 * 1_000).rounded(.down))
    return String(
      format: "screenshots/%04d/%02d/%02d/%lld-%u.jpg",
      components.year ?? 0,
      components.month ?? 0,
      components.day ?? 0,
      milliseconds,
      displayID
    )
  }
}

enum ScreenshotError: Error {
  case missingImage
  case jpegEncodingFailed
}

final class ScreenshotMonitor: @unchecked Sendable {
  private let recorder: Recorder
  private let applicationSupportURL: URL
  private let stateQueue = DispatchQueue(label: "com.joeberg.monitor.screenshot-state")
  private let fileQueue = DispatchQueue(label: "com.joeberg.monitor.screenshot-files")
  private let logger = Logger(
    subsystem: "com.joeberg.monitor",
    category: "screenshots"
  )
  private var timer: DispatchSourceTimer?
  private var isCaptureInProgress = false
  private var isRunning = false
  private var generation: UInt64 = 0
  private var isSessionActive = true
  private var areScreensAwake = true

  init(recorder: Recorder, applicationSupportURL: URL) {
    self.recorder = recorder
    self.applicationSupportURL = applicationSupportURL
  }

  func start() {
    stateQueue.sync {
      guard timer == nil else { return }
      isRunning = true
      generation &+= 1
      let timer = DispatchSource.makeTimerSource(queue: self.stateQueue)
      timer.schedule(deadline: .now() + 15, repeating: 15, leeway: .seconds(1))
      timer.setEventHandler { [weak self] in self?.captureIfEligible() }
      self.timer = timer
      timer.resume()
    }
  }

  func stop() {
    stateQueue.sync {
      isRunning = false
      generation &+= 1
      timer?.cancel()
      timer = nil
    }
  }

  func setSessionActive(_ active: Bool) {
    stateQueue.async { [weak self] in self?.isSessionActive = active }
  }

  func setScreensAwake(_ awake: Bool) {
    stateQueue.async { [weak self] in self?.areScreensAwake = awake }
  }

  private func captureIfEligible() {
    guard isRunning,
      !isCaptureInProgress,
      isSessionActive,
      areScreensAwake,
      CGPreflightScreenCaptureAccess(),
      CGEventSource.secondsSinceLastEventType(
        .combinedSessionState,
        eventType: CGEventType(rawValue: UInt32.max)!
      ) < 60
    else { return }

    isCaptureInProgress = true
    let captureGeneration = generation
    let capturedAt = Date()
    recorder.contextSnapshot { [weak self] context in
      guard let self else { return }
      Task {
        await self.captureAllDisplays(
          capturedAt: capturedAt,
          context: context,
          generation: captureGeneration
        )
      }
    }
  }

  private func captureAllDisplays(
    capturedAt: Date,
    context: ActivityContext,
    generation captureGeneration: UInt64
  ) async {
    defer {
      stateQueue.async { [weak self] in self?.isCaptureInProgress = false }
    }

    do {
      let content = try await SCShareableContent.current
      for display in content.displays {
        guard isCurrentGeneration(captureGeneration) else { return }
        do {
          let image = try await capture(display: display)
          guard isCurrentGeneration(captureGeneration) else { return }
          let relativePath = ScreenshotPathBuilder.relativePath(
            capturedAt: capturedAt,
            displayID: display.displayID
          )
          try await writeJPEG(image, relativePath: relativePath)
          guard isCurrentGeneration(captureGeneration) else {
            try? FileManager.default.removeItem(
              at: applicationSupportURL.appendingPathComponent(relativePath)
            )
            return
          }
          recorder.submit(
            rawEvent: .screenshot(
              timestamp: capturedAt,
              context: context,
              relativePath: relativePath,
              displayID: display.displayID,
              pixelWidth: image.width,
              pixelHeight: image.height
            ))
        } catch {
          logger.error(
            "Display \(display.displayID) screenshot failed: \(String(describing: error), privacy: .public)"
          )
        }
      }
    } catch {
      logger.error("Unable to enumerate displays: \(String(describing: error), privacy: .public)")
    }
  }

  private func isCurrentGeneration(_ captureGeneration: UInt64) -> Bool {
    stateQueue.sync {
      isRunning && generation == captureGeneration
    }
  }

  private func capture(display: SCDisplay) async throws -> CGImage {
    let filter = SCContentFilter(display: display, excludingWindows: [])
    let configuration = SCStreamConfiguration()
    configuration.width = Int(CGDisplayPixelsWide(display.displayID))
    configuration.height = Int(CGDisplayPixelsHigh(display.displayID))
    configuration.showsCursor = true
    configuration.capturesAudio = false

    return try await withCheckedThrowingContinuation { continuation in
      SCScreenshotManager.captureImage(
        contentFilter: filter,
        configuration: configuration
      ) { image, error in
        if let error {
          continuation.resume(throwing: error)
        } else if let image {
          continuation.resume(returning: image)
        } else {
          continuation.resume(throwing: ScreenshotError.missingImage)
        }
      }
    }
  }

  private func writeJPEG(_ image: CGImage, relativePath: String) async throws {
    try await withCheckedThrowingContinuation { continuation in
      fileQueue.async { [applicationSupportURL] in
        do {
          let fileURL = applicationSupportURL.appendingPathComponent(relativePath)
          try FileManager.default.createDirectory(
            at: fileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
          )
          let representation = NSBitmapImageRep(cgImage: image)
          guard
            let data = representation.representation(
              using: .jpeg,
              properties: [.compressionFactor: 0.65]
            )
          else {
            throw ScreenshotError.jpegEncodingFailed
          }
          try data.write(to: fileURL, options: .atomic)
          continuation.resume()
        } catch {
          continuation.resume(throwing: error)
        }
      }
    }
  }
}
