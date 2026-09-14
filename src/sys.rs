//! Minimal, audited wrappers around platform primitives.
//!
//! `unsafe_code` is denied crate-wide, so this module is the single place that
//! is allowed to drop down to a raw syscall. Keeping the unsafe block small and
//! documented makes the safety argument easy to review.

use std::io;

/// Waits for events on the given file descriptors using `poll(2)`.
///
/// Returns the number of descriptors with pending events. `timeout_ms` follows
/// the `poll(2)` convention: a negative value blocks until an event arrives and
/// `0` polls without blocking.
///
/// Failures are returned as [`io::Error`] carrying the underlying `errno`,
/// including [`io::ErrorKind::Interrupted`] when the wait is interrupted by a
/// signal so callers can decide whether to retry.
#[allow(unsafe_code)]
pub fn poll(fds: &mut [libc::pollfd], timeout_ms: i32) -> io::Result<usize> {
    // SAFETY: `fds` is a live mutable slice, so `as_mut_ptr()` points to
    // `fds.len()` initialized `pollfd` entries. `poll(2)` only reads the `fd`
    // and `events` fields and writes `revents`, never touching memory outside
    // that range, which makes the slice length a correct `nfds` argument.
    let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
    if ready < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ready as usize)
    }
}
