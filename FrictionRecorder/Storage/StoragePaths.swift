import Foundation

struct StoragePaths {
  let applicationSupportURL: URL
  let databaseURL: URL
  let screenshotRootURL: URL

  static func live(fileManager: FileManager = .default) throws -> StoragePaths {
    let applicationSupport = try fileManager.url(
      for: .applicationSupportDirectory,
      in: .userDomainMask,
      appropriateFor: nil,
      create: true
    )
    return try make(in: applicationSupport, fileManager: fileManager)
  }

  static func make(in applicationSupport: URL, fileManager: FileManager = .default) throws
    -> StoragePaths
  {
    let root = applicationSupport.appendingPathComponent("monitor", isDirectory: true)
    let legacyRoot = applicationSupport.appendingPathComponent(
      "FrictionRecorder",
      isDirectory: true
    )

    if !fileManager.fileExists(atPath: root.path),
      fileManager.fileExists(atPath: legacyRoot.path)
    {
      try fileManager.moveItem(at: legacyRoot, to: root)
    }

    let screenshots = root.appendingPathComponent("screenshots", isDirectory: true)
    try fileManager.createDirectory(at: screenshots, withIntermediateDirectories: true)

    return StoragePaths(
      applicationSupportURL: root,
      databaseURL: root.appendingPathComponent("events.sqlite"),
      screenshotRootURL: screenshots
    )
  }
}
