import Darwin
import Foundation

enum InstanceLockError: Error {
  case alreadyRunning
  case systemError(Int32)
}

final class InstanceLock {
  private let descriptor: Int32

  init(url: URL) throws {
    let descriptor = open(url.path, O_CREAT | O_RDWR | O_CLOEXEC, S_IRUSR | S_IWUSR)
    guard descriptor >= 0 else {
      throw InstanceLockError.systemError(errno)
    }

    guard flock(descriptor, LOCK_EX | LOCK_NB) == 0 else {
      let lockError = errno
      close(descriptor)
      if lockError == EWOULDBLOCK {
        throw InstanceLockError.alreadyRunning
      }
      throw InstanceLockError.systemError(lockError)
    }

    self.descriptor = descriptor
  }

  deinit {
    flock(descriptor, LOCK_UN)
    close(descriptor)
  }
}
