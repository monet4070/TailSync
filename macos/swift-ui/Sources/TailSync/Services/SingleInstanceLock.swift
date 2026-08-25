import Darwin
import Foundation

/// A per-user advisory lock held for the lifetime of the TailSync UI process.
/// `flock` is released by the kernel on crashes, so a stale lock file cannot
/// prevent a later launch.
final class SingleInstanceLock {
    private let lockFileURL: URL
    private var descriptor: Int32 = -1

    init(lockFileURL: URL = SingleInstanceLock.defaultLockFileURL()) {
        self.lockFileURL = lockFileURL
    }

    deinit {
        release()
    }

    /// Returns `false` only when another process already owns the lock.
    /// Filesystem and POSIX failures are surfaced so startup can fail closed.
    @discardableResult
    func acquire() throws -> Bool {
        guard descriptor < 0 else { return true }

        try FileManager.default.createDirectory(
            at: lockFileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )

        let openedDescriptor = Darwin.open(
            lockFileURL.path,
            O_CREAT | O_RDWR | O_CLOEXEC | O_EXLOCK | O_NONBLOCK,
            S_IRUSR | S_IWUSR
        )
        guard openedDescriptor >= 0 else {
            let openError = errno
            if openError == EWOULDBLOCK || openError == EAGAIN {
                return false
            }
            throw Self.posixError(code: openError)
        }

        descriptor = openedDescriptor
        return true
    }

    func release() {
        guard descriptor >= 0 else { return }
        Darwin.close(descriptor)
        descriptor = -1
    }

    private static func defaultLockFileURL() -> URL {
        let supportDirectory = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first ?? FileManager.default.temporaryDirectory
        return supportDirectory
            .appendingPathComponent("com.tailsync.app", isDirectory: true)
            .appendingPathComponent("instance.lock", isDirectory: false)
    }

    private static func posixError(code: Int32 = errno) -> POSIXError {
        POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)
    }
}
