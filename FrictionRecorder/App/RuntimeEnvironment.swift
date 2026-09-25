import Foundation

enum RuntimeEnvironment {
  static var isRunningTests: Bool {
    let environment = ProcessInfo.processInfo.environment
    return environment["MONITOR_DISABLE_CAPTURE"] == "1"
      || environment["XCTestConfigurationFilePath"] != nil
      || environment["XCTestSessionIdentifier"] != nil
  }
}
