import Darwin
import Foundation

/// Cancellation shuts down the connection; only the I/O worker closes it.
/// Serializing shutdown and close prevents cancellation from touching a reused fd.
final class RequestSocketOwnership: @unchecked Sendable {
  private let lock = NSLock()
  private var descriptor: Int32 = -1
  private var cancelled = false

  var isCancelled: Bool {
    lock.lock()
    defer { lock.unlock() }
    return cancelled
  }

  func checkCancellation() throws {
    if isCancelled { throw CancellationError() }
  }

  func install(_ descriptor: Int32) throws {
    lock.lock()
    defer { lock.unlock() }
    if cancelled {
      close(descriptor)
      throw CancellationError()
    }
    self.descriptor = descriptor
  }

  func cancel() {
    lock.lock()
    defer { lock.unlock() }
    cancelled = true
    if descriptor >= 0 { shutdown(descriptor, SHUT_RDWR) }
  }

  func finish() {
    lock.lock()
    defer { lock.unlock() }
    if descriptor >= 0 { close(descriptor) }
    descriptor = -1
  }
}
