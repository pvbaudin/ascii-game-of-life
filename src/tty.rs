//! The hard-won crossterm-on-macOS stdin fix.
//!
//! When stdin is a pipe, crossterm tries to open `/dev/tty` itself to read keys
//! and fails to initialize its input reader on macOS ("Failed to initialize
//! input reader"), because kqueue can't poll the `/dev/tty` alias device. The
//! fix: *before* crossterm initializes, dup an already-open real terminal fd
//! (stdout or stderr) onto fd 0, having first saved the original pipe fd so we
//! can still read the piped data from it.
//!
//! Do NOT open `/dev/tty` and dup that onto fd 0 — that's the wrong device and
//! won't work. We dup a fd that is *already* a working terminal.

use std::fs::File;
use std::os::fd::FromRawFd;

/// If stdin (fd 0) is not a tty, redirect fd 0 to a real terminal fd (stdout or
/// stderr) and return a `File` wrapping a dup of the *original* stdin, from
/// which the caller should read the piped data. If stdin is already a tty (or no
/// terminal fd is available), returns `None` and the caller reads normal stdin.
pub fn redirect_stdin_if_piped() -> Option<File> {
    unsafe {
        if libc::isatty(0) == 1 {
            return None; // stdin is a real terminal; nothing to do.
        }
        // Find an already-open real terminal to borrow for keyboard input.
        let real_tty = if libc::isatty(1) == 1 {
            1
        } else if libc::isatty(2) == 1 {
            2
        } else {
            return None; // no terminal anywhere; interactive keys won't work.
        };

        // Save the original piped stdin so we can still read the data.
        let saved = libc::dup(0);
        if saved < 0 {
            return None;
        }
        // Point fd 0 at the real terminal *before* crossterm initializes.
        if libc::dup2(real_tty, 0) < 0 {
            libc::close(saved);
            return None;
        }
        Some(File::from_raw_fd(saved))
    }
}
