//! Helper process supervision: one implementation for every native helper call.
//!
//! The verification found four separate problems in the previous inline version:
//! the stdin writer thread was never joined, `stderr` was discarded wholesale, the
//! deadline only covered part of the exchange, and a timeout left no typed answer —
//! so callers could not tell "nothing happened" from "we do not know".
//!
//! This module owns all of that:
//!
//! - **Every thread is joined.** The stdin writer is joined even when the child
//!   dies early (a broken pipe), and the stdout reader is joined on every path, so
//!   no thread outlives the call holding a copy of the payload.
//! - **One deadline covers the whole exchange** — spawn, stdin write, run, stdout
//!   read — not just the wait for exit.
//! - **`stderr` is bounded and never surfaced.** Up to
//!   [`STDERR_CAPTURE_LIMIT`] bytes are drained so the child cannot block on a full
//!   pipe, but only the byte count is returned: helper diagnostics can contain
//!   clipboard content or filesystem paths.
//! - **The outcome is typed.** [`HelperOutcome::TimedOut`] is distinct from
//!   [`HelperOutcome::SpawnFailed`], which is the difference between "we do not
//!   know whether the clipboard changed" and "we never started".
//! - **Termination is complete before returning.** On timeout the child is killed,
//!   reaped and both pipes are joined, which is the precondition for establishing a
//!   resolution revision (ADR-004 K4). Killing without reaping would leave the
//!   question of whether the OS service still holds a pending request open.

use std::io::{Read, Write};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Bytes of `stderr` drained per call. Enough to keep the child from blocking on a
/// full pipe; the content is deliberately not returned.
pub const STDERR_CAPTURE_LIMIT: usize = 4 * 1024;
/// Poll interval while waiting for the child.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// What a helper invocation did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperOutcome {
    /// The helper ran to completion. `stdout` is its output, bounded by the request.
    Exited {
        success: bool,
        stdout: Vec<u8>,
        stderr_bytes: usize,
        elapsed: Duration,
    },
    /// The helper produced more output than the request allowed and was stopped. The
    /// output cannot be trusted, and the helper *did* run, so a side effect may have
    /// happened.
    OutputExceeded {
        stderr_bytes: usize,
        elapsed: Duration,
    },
    /// The deadline expired. The child was killed, reaped and both pipes joined, so
    /// no further write can come from it — but whether the OS already applied a side
    /// effect is **unknown**.
    TimedOut {
        stderr_bytes: usize,
        elapsed: Duration,
    },
    /// The child never started (missing binary, not executable, resource limits).
    /// Nothing was attempted, so the outcome is a definite "no effect".
    SpawnFailed { detail: String, elapsed: Duration },
}

impl HelperOutcome {
    /// Whether the helper provably never touched the platform.
    pub fn is_definite_no_effect(&self) -> bool {
        matches!(self, Self::SpawnFailed { .. })
    }

    /// Whether the platform may have been modified. `false` only when nothing was
    /// attempted, which is the sole case a caller may treat as retryable.
    pub fn may_have_had_an_effect(&self) -> bool {
        !self.is_definite_no_effect()
    }

    pub fn stdout(&self) -> Option<&[u8]> {
        match self {
            Self::Exited { stdout, .. } => Some(stdout),
            _ => None,
        }
    }
}

/// A helper invocation.
pub struct HelperRequest<'a> {
    pub program: &'a Path,
    pub args: &'a [&'a str],
    /// Payload written to the child's stdin, if any.
    pub stdin: Option<Vec<u8>>,
    /// One deadline for the entire exchange.
    pub deadline: Duration,
    /// Hard cap on captured stdout. A helper that floods stdout is killed rather
    /// than allowed to grow the buffer.
    pub stdout_limit: usize,
}

/// Run one helper invocation under a single deadline.
pub fn run(request: HelperRequest<'_>) -> HelperOutcome {
    run_cancellable(request, None)
}

/// Cancellation follows the same kill/reap/join path as a deadline. Once started
/// it is an unknown effect; cancellation before spawn is a proven no-effect.
pub fn run_cancellable(
    request: HelperRequest<'_>,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> HelperOutcome {
    run_registered(request, cancelled, None)
}
/// Register process identity durably before submitting stdin that can cause an effect.
pub fn run_registered(
    request: HelperRequest<'_>,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
    on_spawn: Option<&dyn Fn(u32) -> Result<(), String>>,
) -> HelperOutcome {
    let started = Instant::now();
    if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return HelperOutcome::SpawnFailed {
            detail: "clipboard operation cancelled before spawn".into(),
            elapsed: started.elapsed(),
        };
    }
    let mut command = Command::new(request.program);
    command.args(request.args);
    command.stdin(if request.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    // Keep the helper out of a console window on Windows.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return HelperOutcome::SpawnFailed {
                detail: format!("could not start clipboard helper: {}", error.kind()),
                elapsed: started.elapsed(),
            }
        }
    };

    if on_spawn.is_some_and(|register| register(child.id()).is_err()) {
        let _ = child.kill();
        let _ = child.wait();
        return HelperOutcome::TimedOut {
            stderr_bytes: 0,
            elapsed: started.elapsed(),
        };
    }

    // stderr is drained on its own thread and reduced to a byte count.
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 1024];
            let mut total = 0usize;
            loop {
                match pipe.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => total = total.saturating_add(count),
                }
            }
            total
        })
    });

    // The stdin writer is a joined thread, not a detached one: a detached writer
    // would keep a copy of the payload alive for as long as the child lives.
    let stdin_writer = match (request.stdin, child.stdin.take()) {
        (Some(payload), Some(mut pipe)) => Some(std::thread::spawn(move || {
            let _ = pipe.write_all(&payload);
            // Dropping the pipe closes it, which the helper needs to see EOF.
        })),
        _ => None,
    };

    let stdout_limit = request.stdout_limit;
    // Shared so the wait loop can stop the child as soon as its output overruns,
    // instead of sitting until the deadline.
    let overflowed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let overflow_flag = overflowed.clone();
    let stdout_reader = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                match pipe.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if bytes.len().saturating_add(count) > stdout_limit {
                            overflow_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                            break;
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                }
            }
            bytes
        })
    });

    let expiry = started + request.deadline;
    let mut timed_out = false;
    let mut status = None;
    loop {
        match child.try_wait() {
            Ok(Some(exit)) => {
                status = Some(exit);
                break;
            }
            Ok(None) => {
                if overflowed.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                if Instant::now() >= expiry
                    || cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire))
                {
                    timed_out = true;
                    break;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(_) => {
                timed_out = true;
                break;
            }
        }
    }

    // Terminate completely before returning: kill, reap, then join every pipe. Only
    // after this may the caller establish a resolution revision (K4).
    let terminate = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
    };
    if timed_out || overflowed.load(std::sync::atomic::Ordering::SeqCst) {
        terminate(&mut child);
        if let Some(writer) = stdin_writer {
            let _ = writer.join();
        }
        if let Some(reader) = stdout_reader {
            let _ = reader.join();
        }
        let stderr_bytes = stderr_reader
            .and_then(|reader| reader.join().ok())
            .unwrap_or(0);
        return if timed_out {
            HelperOutcome::TimedOut {
                stderr_bytes,
                elapsed: started.elapsed(),
            }
        } else {
            HelperOutcome::OutputExceeded {
                stderr_bytes,
                elapsed: started.elapsed(),
            }
        };
    }

    if let Some(writer) = stdin_writer {
        let _ = writer.join();
    }
    let stdout = stdout_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let stderr_bytes = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or(0);
    HelperOutcome::Exited {
        success: status.is_some_and(|exit| exit.success()),
        stdout,
        stderr_bytes,
        elapsed: started.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request<'a>(
        program: &'a Path,
        args: &'a [&'a str],
        deadline: Duration,
        limit: usize,
    ) -> HelperRequest<'a> {
        HelperRequest {
            program,
            args,
            stdin: None,
            deadline,
            stdout_limit: limit,
        }
    }

    /// A helper that exits normally is reported with its output and status.
    #[test]
    fn a_normal_exit_returns_its_output() {
        let program = PathBuf::from("/bin/sh");
        let outcome = run(request(
            &program,
            &["-c", "printf hello"],
            Duration::from_secs(5),
            1024,
        ));
        match outcome {
            HelperOutcome::Exited {
                success, stdout, ..
            } => {
                assert!(success, "the child exited successfully");
                assert_eq!(stdout, b"hello");
            }
            other => panic!("expected a definite exit, got {other:?}"),
        }
    }

    /// A helper that never exits is killed, reaped and joined, and reported as
    /// `TimedOut` — not as a success and not as a definite no-effect.
    #[test]
    fn a_hanging_helper_is_terminated_and_reported_as_timed_out() {
        let program = PathBuf::from("/bin/sleep");
        let started = Instant::now();
        let outcome = run(request(&program, &["30"], Duration::from_millis(200), 1024));
        assert!(
            matches!(outcome, HelperOutcome::TimedOut { .. }),
            "a hanging helper must be reported as TimedOut, got {outcome:?}"
        );
        assert!(
            outcome.may_have_had_an_effect(),
            "a timeout is not a definite no-effect"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "termination must not wait for the child's own exit"
        );
    }

    /// A missing program is a definite no-effect: nothing was attempted, so a caller
    /// may safely retry.
    #[test]
    fn a_missing_program_is_a_definite_no_effect() {
        let program = PathBuf::from("/nonexistent/tailsync-helper");
        let outcome = run(request(
            &program,
            &["--self-check"],
            Duration::from_secs(1),
            1024,
        ));
        assert!(outcome.is_definite_no_effect());
        assert!(!outcome.may_have_had_an_effect());
        assert!(matches!(outcome, HelperOutcome::SpawnFailed { .. }));
    }

    /// stdout is capped, and a flooding helper is stopped promptly rather than at the
    /// deadline — and it is not reported as a truncated success.
    #[test]
    fn stdout_is_capped_and_flooding_is_stopped_early() {
        let program = PathBuf::from("/bin/cat");
        let started = Instant::now();
        let outcome = run(request(
            &program,
            &["/dev/zero"],
            Duration::from_secs(30),
            4096,
        ));
        assert!(
            matches!(outcome, HelperOutcome::OutputExceeded { .. }),
            "a flooding helper must be reported as OutputExceeded, got {outcome:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the overrun must stop the child, not wait for the 30s deadline"
        );
        assert!(outcome.may_have_had_an_effect());
    }

    /// `stderr` is drained (so the child cannot block on a full pipe) but its content
    /// is never surfaced — only the byte count.
    #[test]
    fn stderr_is_drained_but_never_surfaced() {
        let program = PathBuf::from("/bin/sh");
        let outcome = run(request(
            &program,
            &[
                "-c",
                "printf 'secret clipboard body /Users/me/private' >&2; printf ok",
            ],
            Duration::from_secs(5),
            1024,
        ));
        match outcome {
            HelperOutcome::Exited {
                stdout,
                stderr_bytes,
                ..
            } => {
                assert_eq!(stdout, b"ok");
                assert!(stderr_bytes > 0, "stderr must actually be drained");
            }
            other => panic!("expected a definite exit, got {other:?}"),
        }
    }

    /// A large stdin payload is written and the writer thread is joined, so nothing
    /// keeps the payload alive after the call returns.
    #[test]
    fn stdin_is_written_and_the_writer_is_reclaimed() {
        let program = PathBuf::from("/bin/cat");
        let payload = vec![b'x'; 4 * 1024 * 1024];
        let outcome = run(HelperRequest {
            program: &program,
            args: &[],
            stdin: Some(payload.clone()),
            deadline: Duration::from_secs(10),
            stdout_limit: payload.len() + 16,
        });
        match outcome {
            HelperOutcome::Exited { stdout, .. } => assert_eq!(stdout.len(), payload.len()),
            other => panic!("expected the payload to round-trip, got {other:?}"),
        }
    }

    /// A child that exits without reading stdin must not leave the writer blocked:
    /// the write fails with a broken pipe and the thread is joined.
    #[test]
    fn an_early_exit_does_not_leak_the_stdin_writer() {
        let program = PathBuf::from("/usr/bin/true");
        let payload = vec![b'y'; 8 * 1024 * 1024];
        let started = Instant::now();
        let outcome = run(HelperRequest {
            program: &program,
            args: &[],
            stdin: Some(payload),
            deadline: Duration::from_secs(5),
            stdout_limit: 1024,
        });
        assert!(
            matches!(outcome, HelperOutcome::Exited { .. }),
            "got {outcome:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the writer must not hold the call open"
        );
    }
}
