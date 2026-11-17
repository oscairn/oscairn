// SPDX-License-Identifier: AGPL-3.0-or-later
//! A helper program run isolated (the scan helper, the library's extractors): its own process group, its output
//! bounded, the last of its stderr kept, killed at a timeout, on request, or past a memory limit; and the helper's side,
//! which ends it when the process that started it dies.
use std::ffi::c_int;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::{Duration, Instant};

/// How a helper ended (C++'s IsolatedRun::Outcome, in its order).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Exited,
    Failed,
    TimedOut,
    Canceled,
    Oversized,
    OverMemory,
    NoPipe,
    NoLaunch,
}
/// A helper's run: how it ended, its output, and the last 2 KiB of its stderr, its newlines as spaces.
#[derive(Debug)]
pub struct Isolated {
    pub outcome: Outcome,
    pub output: Vec<u8>,
    pub stderr_tail: String,
}

#[repr(C)]
pub(crate) struct PollFd {
    pub fd: c_int,
    pub events: i16,
    pub revents: i16,
}
pub(crate) const POLLIN: i16 = 1;
const SIGKILL: c_int = 9;
#[cfg(target_os = "macos")]
type Nfds = u32;
#[cfg(not(target_os = "macos"))]
type Nfds = std::ffi::c_ulong;
extern "C" {
    pub(crate) fn poll(fds: *mut PollFd, count: Nfds, milliseconds: c_int) -> c_int;
    fn kill(pid: c_int, signal: c_int) -> c_int;
    fn _exit(status: c_int) -> !;
}
/// Whether `fd` can be read without waiting (data, or its end), waiting up to `milliseconds` for that.
fn ready(fd: &impl AsRawFd, milliseconds: c_int) -> bool {
    let mut p = PollFd { fd: fd.as_raw_fd(), events: POLLIN, revents: 0 };
    // SAFETY: one pollfd, ours.
    unsafe { poll(&mut p, 1, milliseconds) > 0 && p.revents != 0 }
}

/// A pipe read as far as it goes without waiting; false once it ended.
fn drain(pipe: &mut impl Read, ready: impl Fn() -> bool, mut take: impl FnMut(&[u8]) -> bool) -> bool {
    let mut buffer = [0u8; 4096];
    while ready() {
        match pipe.read(&mut buffer) {
            Ok(0) | Err(_) => return false,
            Ok(n) if !take(&buffer[..n]) => return true,
            Ok(_) => {}
        }
    }
    true
}

/// Runs `arguments[0]` with the rest isolated: a process group of its own, its stdout kept up to `max_output` bytes,
/// the last 2 KiB of its stderr, and a stdin pipe only this process holds, which it can watch (`watch_parent_pipe`).
/// It is killed with its group at the timeout, when `canceled`, when its output passes `max_output`, or when
/// `memory_of(its pid)`, read every 10 ms or so, passes `memory_limit` (0: none; a spike between two readings is not
/// seen). Exited: it ended with status 0. Off any thread that must keep answering.
pub fn run_isolated(
    arguments: &[String],
    timeout: Duration,
    max_output: usize,
    canceled: Option<&AtomicBool>,
    memory_limit: u64,
    memory_of: Option<&dyn Fn(u32) -> u64>,
) -> Isolated {
    use std::os::unix::process::CommandExt;
    let mut run = Isolated { outcome: Outcome::NoLaunch, output: Vec::new(), stderr_tail: String::new() };
    let Some((program, rest)) = arguments.split_first() else { return run };
    let spawned = Command::new(program).args(rest).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0).spawn();
    let Ok(mut child) = spawned else { return run };
    let _life = child.stdin.take(); // dropped at our return, which ends the helper's watch
    let (Some(mut output), Some(mut errors)) = (child.stdout.take(), child.stderr.take()) else {
        run.outcome = Outcome::NoPipe;
        return run;
    };
    let (out_fd, errors_fd) = (Fd(output.as_raw_fd()), Fd(errors.as_raw_fd()));
    let (mut open, mut errors_open, mut oversized, mut over_memory, mut stopped) = (true, true, false, false, false);
    let mut tail = Vec::new();
    // Both pipes read as far as they go, stdout up to max_output.
    let mut read_all = |open: &mut bool, errors_open: &mut bool, oversized: &mut bool, kept: &mut Vec<u8>| {
        if *open && !*oversized {
            *open = drain(
                &mut output,
                || ready(&out_fd, 0),
                |b| {
                    *oversized = kept.len() + b.len() > max_output;
                    if !*oversized {
                        kept.extend_from_slice(b);
                    }
                    !*oversized
                },
            );
        }
        if *errors_open {
            *errors_open = drain(
                &mut errors,
                || ready(&errors_fd, 0),
                |b| {
                    tail.extend_from_slice(b);
                    tail.drain(..tail.len().saturating_sub(2048));
                    true
                },
            );
        }
    };
    let deadline = Instant::now() + timeout;
    let status = loop {
        read_all(&mut open, &mut errors_open, &mut oversized, &mut run.output);
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Err(_) => break None,
            Ok(None) => {}
        }
        over_memory = memory_limit != 0 && memory_of.is_some_and(|f| f(child.id()) > memory_limit);
        if oversized || over_memory || Instant::now() >= deadline || canceled.is_some_and(|c| c.load(Relaxed)) {
            stopped = !oversized && !over_memory;
            // SAFETY: a signal to the helper's own process group.
            unsafe { kill(-(child.id() as c_int), SIGKILL) };
            break child.wait().ok();
        }
        if open {
            ready(&out_fd, 10);
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    read_all(&mut open, &mut errors_open, &mut oversized, &mut run.output);
    run.stderr_tail = String::from_utf8_lossy(&tail).trim_end().replace(['\n', '\r'], " ");
    run.outcome = if stopped {
        if canceled.is_some_and(|c| c.load(Relaxed)) {
            Outcome::Canceled
        } else {
            Outcome::TimedOut
        }
    } else if oversized {
        Outcome::Oversized
    } else if over_memory {
        Outcome::OverMemory
    } else if status.is_some_and(|s| s.success()) {
        Outcome::Exited
    } else {
        Outcome::Failed
    };
    run
}
struct Fd(c_int);
impl AsRawFd for Fd {
    fn as_raw_fd(&self) -> c_int {
        self.0
    }
}

/// The helper's side: when the stdin pipe its launcher holds ends (the launcher died), the helper exits at once, so a
/// hung plugin never leaves it running alone. Nothing when stdin is not a pipe (a run by hand keeps its input).
pub fn watch_parent_pipe() {
    use std::os::fd::FromRawFd;
    use std::os::unix::fs::FileTypeExt;
    std::thread::spawn(|| {
        // SAFETY: stdin, borrowed, never closed here.
        let stdin = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(0) });
        if !stdin.metadata().is_ok_and(|m| m.file_type().is_fifo()) {
            return;
        }
        let mut byte = [0u8];
        loop {
            match (&*stdin).read(&mut byte) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Ok(1) => return,
                // SAFETY: the launcher is gone and nobody waits for this process; it ends without unwinding the plugin's threads.
                _ => unsafe { _exit(1) },
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sh(script: &str, timeout_ms: u64, max_output: usize) -> Isolated {
        let arguments = ["/bin/sh", "-c", script].map(String::from);
        run_isolated(&arguments, Duration::from_millis(timeout_ms), max_output, None, 0, None)
    }
    #[test]
    fn exited_with_output_and_errors() {
        let run = sh("printf out; printf 'one\\ntwo\\n' >&2", 5000, 100);
        assert_eq!((run.outcome, run.output.as_slice(), run.stderr_tail.as_str()), (Outcome::Exited, &b"out"[..], "one two"));
        assert_eq!(sh("exit 3", 5000, 100).outcome, Outcome::Failed);
    }
    #[test]
    fn stopped_by_time_size_memory_or_request() {
        let start = Instant::now();
        assert_eq!(sh("sleep 5 & wait", 100, 100).outcome, Outcome::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(3), "the group killed, its child too");
        assert_eq!(sh("yes", 5000, 1000).outcome, Outcome::Oversized);
        let arguments = ["/bin/sh", "-c", "sleep 5"].map(String::from);
        let heavy = |_: u32| 1 << 30;
        assert_eq!(run_isolated(&arguments, Duration::from_secs(5), 100, None, 1 << 20, Some(&heavy)).outcome, Outcome::OverMemory);
        let canceled = AtomicBool::new(true);
        assert_eq!(run_isolated(&arguments, Duration::from_secs(5), 100, Some(&canceled), 0, None).outcome, Outcome::Canceled);
        assert_eq!(run_isolated(&["/nonexistent/helper".into()], Duration::from_secs(1), 100, None, 0, None).outcome, Outcome::NoLaunch);
    }
}
