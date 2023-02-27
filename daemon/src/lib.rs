/*
Copyright (c) 2022 VMware, Inc.
SPDX-License-Identifier: MIT
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:
The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
 */

// Derived from lib/daemon-unix.c in Open vSwitch, with the following license:
/*
 * Copyright (c) 2008, 2009, 2010, 2011, 2012, 2013, 2015 Nicira, Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at:
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Utilities for a daemon to detach and monitor itself.
//!
//! In Unix-like environments, it's traditional for a daemon process to be able to **detach**
//! itself from the current session and run, isolated, in the background.  It's also useful to have
//! a mechanism for a daemon to **monitor** itself and automatically restart if it dies due to a
//! signal that indicates an error (such as SIGABRT or SIGSEGV).  These days, both of these
//! functions are ones that can be implemented externally (e.g. by `systemd`), but it can still be
//! convenient for a daemon to implement them internally, especially for automated testing.
//!
//! This crate includes the [`Daemonize`] object that can both detach and monitor the running
//! process.  When both are enabled at runtime, daemonization involves three processes:
//!
//!   - The "parent process", which is the one that is initially created.  This process waits for
//!     (the first instance of) the "daemon process" to finish its initialization, then exits
//!     successfully.  If the daemon process fails to initialize, it instead exits unsuccessfully.
//!     This deferred exit gives the daemon the ability to report errors to the process that
//!     invoked it (the "caller").  It also helps to prevent race conditions, since the caller
//!     knows that the daemon is initialized as soon as it exits.
//!
//!   - The "monitor process", forked off from the parent process.  This process forks the daemon
//!     process and, if it dies due to a signal that indicates an error (such as `SIGABRT` or
//!     `SIGSEGV`), forks another, and so on.  If the daemon process exits normally or due to an
//!     ordinary termination signal (such as `SIGTERM`), it exits too.
//!
//!   - The "daemon process", which does the real work of the daemon.  When it successfully
//!     completes initialization, it tells the monitor process, which tells the parent process,
//!     which exit successfully.  If it exits or dies before completing initialization, then that
//!     gets passed along too and both the monitor and parent processes exit unsuccessfully.
//!
//! This crate also includes [`Cleanup`], for deleting files and temporary directories either when
//! dropped or when the process exits unexpectedly due to a signal, and [`proctitle`] for changing
//! the name of the current process as shown by `ps`.

use anyhow::{anyhow, Context};
use clap::Parser;
use libc::{self, c_int};
use std::env::set_current_dir;
use std::ffi::{CString, OsString};
use std::fs::{File, read_dir};
use std::io::{prelude::*, BufReader, Error, ErrorKind};
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{exit, ExitStatus};
use std::thread::sleep;
use std::time::{Duration, Instant};
use tracing::{event, Level};

mod cleanup;
pub mod proctitle;

pub use cleanup::Cleanup;

/// Options for daemonizing a process.
///
/// If `detach` is true, the process will fork and the parent will exit.  If `monitor` is true,
/// the process will fork a monitor process that monitors the child and restarts it if it dies
/// due to a signal that indicates an error, such as SIGSEGV.  `detach` and `monitor` may be
/// used individually or (commonly) together.  If they are used together, the process forks
/// twice initially (once for the monitor, once for the child), and then more times if the
/// child dies due to an error.
///
/// Unless `no_chdir` is true, the child will change its current directory to the root.
///
/// If `pidfile` is specified, the child will create a pidfile with the given name.
///
/// # Usage
///
/// `Daemonize::start` returns a tuple of two important objects:
///
///   * `daemonizing`, a [`Daemonizing`] object that represents that the process is still preparing
///   to be fully operational.  Call `finish()` on it to notify the parent process that the daemon
///   is ready; the parent process will then exit normally.  If the daemon exits without this
///   notification, or if it dies due to a signal, then the parent process exits with the same
///   error or signal status.
///
///   * `cleanup`, a [`Cleanup`] object that will delete the daemon's pidfile when it is dropped or
///     when the process exits due to a signal.  Keep it around until the process exits to ensure
///     that the pidfile is only deleted at the right time.
///
/// You can use this with `clap` to parse command-line arguments and daemonize based on them, e.g.:
///
/// ```
/// use clap::Parser;
/// use daemon::Daemonize;
///
/// let (daemonizing, _cleanup) = unsafe { Daemonize::parse().start() };
/// // ...anything else needed before the parent exits...
/// daemonizing.finish();
/// ```
///
/// If the process doesn't have anything to do before completing daemonization, then it can call
/// `run` to start and immediately finish daemonizing.  It still needs to keep around the `Cleanup`
/// object:
///
/// ```
/// use clap::Parser;
/// use daemon::Daemonize;
///
/// let _cleanup = unsafe { Daemonize::parse().run() };
/// ```
///
/// If the daemon needs to parse additional options beyond those needed just for daemonization,
/// you can flatten `Daemonize` into a larger `Args` structure, e.g.:
///
/// ```
/// use clap::Parser;
/// use daemon::Daemonize;
///
/// #[derive(Parser, Debug)]
/// #[clap(version, about)]
/// struct Args {
///     #[clap(flatten)]
///     daemonize: Daemonize,
///
///     //...other options...
/// }
///
/// let Args { daemonize, .. } = Args::parse();
/// let (daemonizing, _cleanup) = unsafe { daemonize.start() };
/// // ...anything else needed before the parent exits...
/// daemonizing.finish();
/// ```
///

/// # Safety
///
/// It's important to call [`Daemonize::start()`] or [`Daemonize::run()`] as soon as possible at
/// the beginning of a program.  Forking is unsafe because any threads other than the one that
/// calls `fork` will be dead in the child, and `fork` has other undesirable consequences, too.
/// Daemonizing will assert-fail if any additional threads have been started.
///
/// About all a program should do beforehand is to parse command-line options (needed to determine
/// whether to detach) and initialize logging (because daemonization will emit log entries).
#[derive(Clone, Debug, Default, Parser, PartialEq, Eq)]
pub struct Daemonize {
    /// Detach from foreground session
    #[clap(long)]
    pub detach: bool,

    /// Create monitoring process
    #[clap(long)]
    pub monitor: bool,

    /// Do not change directory to root
    #[clap(long)]
    pub no_chdir: bool,

    /// Create pidfile
    #[clap(long)]
    pub pidfile: Option<PathBuf>,
}

impl Daemonize {
    pub fn new() -> Daemonize {
        Self::default()
    }

    /// Daemonizes the current process based on the configured options.  See the module comment for
    /// full usage.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it forks and exits in the parent: any threads, other than
    /// the calling thread, will be dead in the child.  Thus, it is only safe to call this function
    /// while the program is single-threaded, and this function will assert-fail if additional
    /// threads have been started.
    pub unsafe fn start(self) -> (Daemonizing, Cleanup) {
        Daemonizing::new(self)
    }

    pub unsafe fn run(self) -> Cleanup {
        let (daemonizing, cleanup) = self.start();
        daemonizing.finish();
        cleanup
    }
}

/// The current process, in the process of daemonizing.
///
/// Call `finish()` on this object to finish the daemonization process and notify the parent that
/// it can exit successfully.  (If `finish()` is not called and the process does not exit, the
/// parent will hang until one or the other occurs.)
pub struct Daemonizing {
    options: Daemonize,
    notify_pipe: Option<File>
}

impl Daemonizing {
    /// Completes the daemonization process:
    ///
    ///   - If we detached, changes the current directory to the root, unless that behavior is
    ///     disabled.
    ///
    ///   - If we detached, closes the `stdin`, `stdout`, and `stderr` fds.  For safety, instead
    ///     of leaving fds 0, 1, and 2 unpopulated, we replace them by `/dev/null`.
    ///
    ///   - Notifies the parent process that daemonization is complete.  This allows the parent
    ///     process to exit successfully, indicating to the process that in turn
    pub fn finish(mut self) {
        if self.options.detach {
            if !self.options.no_chdir {
                drop(set_current_dir("/"));
            }
            close_standard_fds();
        }
        if let Some(ref mut pipe) = self.notify_pipe {
            fork_notify_startup(pipe);
        }
    }

    unsafe fn new(options: Daemonize) -> (Self, Cleanup) {
        assert_single_threaded();

        let mut notify_pipe = None;
        if options.detach {
            notify_pipe = match fork_and_wait_for_startup() {
                ForkAndWaitResult::ForkFailed { status, .. } => {
                    event!(Level::ERROR, "could not detach from foreground session ({status})");
                    exit(1);
                },
                ForkAndWaitResult::InParent { .. } => exit(0),
                ForkAndWaitResult::InChild { notify_pipe } => Some(notify_pipe),
            };

            // Running in daemon or monitor process.
            libc::setsid();
        }

        if options.monitor {
            notify_pipe = Some(match fork_and_wait_for_startup() {
                ForkAndWaitResult::ForkFailed { status, .. } => {
                    event!(Level::ERROR, "could not initiate process monitoring ({status})");
                    exit(1);
                },
                ForkAndWaitResult::InParent { child_pid } => {
                    // Running in monitor process.
                    if let Some(ref mut notify_pipe) = notify_pipe {
                        fork_notify_startup(notify_pipe);
                    }
                    if options.detach {
                        close_standard_fds();
                    }
                    Self::monitor_daemon(child_pid)
                },
                ForkAndWaitResult::InChild { notify_pipe } => notify_pipe
            })
            // Running in daemon process.
        }

        // Running in daemon process.
        proctitle::restore();
        let mut cleanup = match cleanup::Cleanup::new() {
            Ok(cleanup) => cleanup,
            Err(error) => {
                event!(Level::ERROR, "could not arrange for cleanup on process exit ({error})");
                exit(1);
            }
        };
        if let Some(ref pidfile) = options.pidfile {
            if let Err(error) = Self::make_pidfile(pidfile, &mut cleanup) {
                event!(Level::ERROR, "failed to create pidfile ({error})");
                exit(1);
            }
        }

        (Daemonizing { options, notify_pipe }, cleanup)
    }

    fn monitor_daemon(mut child_pid: c_int) -> File {
        let mut next_restart = None;
        let mut crashes = 0;
        let mut child_status = None;
        let mut status_msg = String::from("healthy");
        loop {
            proctitle::set(&format!("monitoring pid {child_pid} ({status_msg})"));
            let status = match child_status {
                Some(status) => status,
                None => sys::xwaitpid(child_pid, 0).1,
            };
            if !Self::should_restart(status) {
                event!(Level::INFO, "pid {child_pid} died ({status}), exiting");
                exit(0);
            }

            // Disable further core dumps to save disk space.
            if status.core_dumped() {
                let rlimit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
                if let Err(e) = sys::setrlimit(libc::RLIMIT_CORE, rlimit) {
                    event!(Level::WARN, "failed to disable core dumps ({e})");
                }
            }

            crashes += 1;
            status_msg = format!("{crashes} crashes: pid {child_pid} died ({status})");

            // Throttle restarts to no more than once every 10 seconds.
            let now = Instant::now();
            match next_restart {
                Some(time) if now < time => {
                    event!(Level::ERROR, "{}, waiting until 10 seconds since last restart", status_msg);
                    sleep(time - now);
                },
                _ => (),
            }
            next_restart = Some(Instant::now() + Duration::from_secs(10));

            // Restart.
            event!(Level::INFO, "{}, restarting", status_msg);
            (child_pid, child_status) = match fork_and_wait_for_startup() {
                ForkAndWaitResult::ForkFailed { child_pid, status } => (child_pid, Some(status)),
                ForkAndWaitResult::InParent { child_pid } => (child_pid, None),
                ForkAndWaitResult::InChild { notify_pipe } => break notify_pipe
            };
        }
    }

    fn should_restart(status: ExitStatus) -> bool {
        match status.signal() {
            Some(signal) => {
                const ERROR_SIGNALS: &[c_int] = &[
                    libc::SIGABRT, libc::SIGALRM, libc::SIGBUS, libc::SIGFPE, libc::SIGILL,
                    libc::SIGPIPE, libc::SIGSEGV, libc::SIGXCPU, libc::SIGXFSZ];
                ERROR_SIGNALS.contains(&signal)
            },
            None => false,
        }
    }

    fn make_pidfile(pidfile: &Path, cleanup: &mut Cleanup) -> anyhow::Result<()> {
        // Everyone shares the same file which will be treated as a lock.  To avoid some
        // uncomfortable race conditions, we can't set up the fatal signal unlink until we've
        // acquired it.
        let mut tmpfile = OsString::from(pidfile);
        tmpfile.push(".tmp");
        let tmpfile: PathBuf = tmpfile.into();

        let mut file = File::options().append(true).create(true).open(&tmpfile)
            .with_context(|| format!("{}: create failed", tmpfile.display()))?;

        sys::fcntl_set_lock(&file)
            .with_context(|| format!("{}: fcntl(F_SETLK) failed", tmpfile.display()))?;

        // We acquired the lock.  Make sure to clean up on exit, and verify
        // that we're allowed to create the actual pidfile.
        Self::check_already_running(pidfile)?;
        cleanup.register_remove_file(pidfile)?;

        file.set_len(0).with_context(|| format!("{}: truncate failed", tmpfile.display()))?;

        file.write_all(format!("{}\n", std::process::id()).as_bytes())
            .with_context(|| format!("{}: write failed", tmpfile.display()))?;

        std::fs::rename(&tmpfile, &pidfile)
            .with_context(|| format!("failed to rename {} to {}",
                                     tmpfile.display(), pidfile.display()))?;

        // Clean up.  Leak 'file' because its file descriptor must remain open to hold the lock.
        std::mem::forget(file);

        Ok(())
    }

    fn check_already_running(pidfile: &Path) -> anyhow::Result<()> {
        match Self::read_pidfile(pidfile, true) {
            Ok(Some(pid)) => Err(anyhow!("{}: already running as pid {pid}, aborting",
                                         pidfile.display()))?,
            Ok(None) => Ok(()),
            Err(error) => Err(error)?,
        }
    }

    fn read_pidfile(pidfile: &Path, delete_if_stale: bool) -> anyhow::Result<Option<c_int>> {
        let file = match File::options().read(true).write(true).open(pidfile) {
            Err(error) if error.kind() == ErrorKind::NotFound && delete_if_stale => return Ok(None),
            Err(error) => Err(anyhow!("{}: open failed ({error})", pidfile.display()))?,
            Ok(file) => file,
        };

        match sys::fcntl_get_lock(&file)? {
            None => {
                // `pidfile` exists but it isn't locked by anyone.  We need to delete it so that a
                // new pidfile can go in its place.  But just unlinking it directly makes a nasty
                // race: what if someone else unlinks it before we do and then replaces it by a
                // valid pidfile?  We'd unlink their valid pidfile.  We do a little dance to avoid
                // the race, by locking the invalid pidfile.  Only one process can have the invalid
                // pidfile locked, and only that process has the right to unlink it.
                if !delete_if_stale {
                    Err(anyhow!("{}: pidfile is stale", pidfile.display()))?;
                }

                // Get the lock.
                sys::fcntl_set_lock(&file)
                    .with_context(|| format!("{}: lost race to lock pidfile", pidfile.display()))?;

                // Is the file we have locked still named `pidfile`?
                match (std::fs::metadata(pidfile), file.metadata()) {
                    (Ok(m1), Ok(m2)) if m1.dev() == m2.dev() && m1.ino() == m2.ino() => (),
                    _ => {
                        // No.  We lost a race with someone else who got the lock before us,
                        // deleted the pidfile, and closed it (releasing the lock).
                        Err(anyhow!("{}: lost race to delete pidfile", pidfile.display()))?;
                    }
                };

                // We won the right to delete the stale pidfile.
                std::fs::remove_file(pidfile)
                    .with_context(|| format!("{}: failed to delete stale pidfile", pidfile.display()))?;
                Ok(None)
            },
            Some(lock_pid) => {
                let mut reader = BufReader::new(file);
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Err(e) => Err(e).with_context(|| format!("{}: read failed", pidfile.display()))?,
                    Ok(0) => Err(anyhow!("{}: read: unexpected end of file", pidfile.display()))?,
                    Ok(_) => (),
                };
                let read_pid: i32 = line.trim().parse()?;
                if lock_pid != read_pid {
                    Err(anyhow!("{}: stale pidfile for pid {read_pid} being deleted by id {lock_pid}",
                                pidfile.display()))?;
                }
                Ok(Some(lock_pid))
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn count_threads(pid: u32) -> Result<usize, Error> {
    Ok(read_dir(format!("/proc/{pid}/task"))?.count())
}

#[cfg(target_os = "linux")]
fn assert_single_threaded() {
    assert_eq!(count_threads(std::process::id()).unwrap(), 1);
}

#[cfg(not(target_os = "linux"))]
fn assert_single_threaded() {
    // Don't know how to count our threads.
}

enum ForkAndWaitResult {
    ForkFailed { child_pid: c_int, status: ExitStatus },
    InParent { child_pid: c_int },
    InChild { notify_pipe: File },
}

fn fork_and_wait_for_startup() -> ForkAndWaitResult {
    let (rfd, wfd) = sys::xpipe();
    match unsafe { sys::xfork() } {
        Some(child_pid) => {
            // Running in parent process.
            drop(wfd);

            let mut buf: [u8; 1] = [0; 1];
            match File::from(rfd).read_exact(&mut buf) {
                Ok(_) => {
                    // The child successfully started up.
                    ForkAndWaitResult::InParent { child_pid }
                },
                Err(_) => {
                    // The child exited (or closed the pipe) without writing anything to it,
                    // which signifies an error.  Wait for it to die and get the exit status.
                    let (child_pid, status) = sys::xwaitpid(child_pid, 0);
                    if status.code().unwrap_or_default() > 0 {
                        // Child exited with an error.  Convey the same error to our parent
                        // process as a courtesy.
                        exit(status.code().unwrap());
                    }

                    event!(Level::ERROR, "fork child died before signaling startup ({status})");
                    ForkAndWaitResult::ForkFailed { child_pid, status }
                },
            }
        },
        None => {
            // Running in child process.
            drop(rfd);
            ForkAndWaitResult::InChild { notify_pipe: wfd.into() }
        },
    }
}

fn fork_notify_startup(notify_pipe: &mut File) {
    if let Err(error) = notify_pipe.write_all(&[0; 1]) {
        event!(Level::ERROR, "pipe write failed ({error})");
        exit(1);
    }
}

fn close_standard_fds() {
    let filename = "/dev/null";
    let dev_null = CString::new(filename).unwrap();
    let null_fd = unsafe { libc::open(dev_null.as_ptr(), libc::O_RDWR) };
    if null_fd < 0 {
        event!(Level::ERROR, "could not open {filename} ({})", Error::last_os_error());
        exit(1);
    }

    for fd in 0..=2 {
        unsafe { libc::dup2(null_fd, fd) };
    }
    unsafe { libc::close(null_fd) };
}

mod sys {
    //! System call wrappers.
    //!
    //! The ones whose names begin with `x` panic on error.

    use super::*;

    pub fn setrlimit(resource: libc::__rlimit_resource_t, rlim: libc::rlimit) -> Result<(), Error> {
        match unsafe { libc::setrlimit(resource, &rlim as *const libc::rlimit) } {
            -1 => Err(Error::last_os_error()),
            _ => Ok(())
        }
    }

    pub fn pipe() -> Result<(OwnedFd, OwnedFd), Error> {
        let mut fds: [std::os::unix::io::RawFd; 2] = [0; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } < 0 {
            Err(Error::last_os_error())?;
        }
        Ok((unsafe { OwnedFd::from_raw_fd(fds[0]) },
            unsafe { OwnedFd::from_raw_fd(fds[1]) }))
    }

    pub fn xpipe() -> (OwnedFd, OwnedFd) {
        match pipe() {
            Ok(fds) => fds,
            Err(error) => {
                event!(Level::ERROR, "fork failed ({error})");
                exit(1);
            }
        }
    }

    pub unsafe fn fork() -> Result<Option<c_int>, Error> {
        let pid = libc::fork();
        if pid < 0 {
            Err(Error::last_os_error())
        } else if pid == 0 {
            Ok(None)
        } else {
            Ok(Some(pid))
        }
    }

    pub unsafe fn xfork() -> Option<c_int> {
        assert_single_threaded();
        match fork() {
            Ok(result) => result,
            Err(error) => {
                event!(Level::ERROR, "fork failed ({error})");
                exit(1);
            }
        }
    }

    pub fn waitpid(pid: c_int, flags: c_int) -> Result<(c_int, ExitStatus), Error> {
        loop {
            let mut status = 0;
            let retval = unsafe { libc::waitpid(pid, &mut status as *mut c_int, flags) };
            if retval != -1 {
                return Ok((retval, ExitStatus::from_raw(status)));
            }
            let err = Error::last_os_error();
            if err.kind() != ErrorKind::Interrupted {
                return Err(err);
            }
        }
    }

    pub fn xwaitpid(pid: c_int, flags: c_int) -> (c_int, ExitStatus) {
        match waitpid(pid, flags) {
            Err(error) => {
                event!(Level::ERROR, "waitpid failed ({error})");
                exit(1);
            },
            Ok((pid, status)) => (pid, status)
        }
    }

    fn fcntl_lock_op(file: &File, command: c_int) -> Result<libc::flock, Error> {
        let mut lck = libc::flock {
            l_type: libc::F_WRLCK as i16,
            l_whence: libc::SEEK_SET as i16,
            l_start: 0,
            l_len: 0,
            l_pid: 0
        };

        loop {
            let retval = unsafe { libc::fcntl(file.as_raw_fd(), command, &mut lck as *mut libc::flock) };
            if retval != -1 {
                return Ok(lck)
            }
            let err = Error::last_os_error();
            if err.kind() != ErrorKind::Interrupted {
                return Err(err);
            }
        }
    }

    pub fn fcntl_set_lock(file: &File) -> Result<(), Error> {
        let _ = fcntl_lock_op(file, libc::F_SETLK)?;
        Ok(())
    }

    /// Check whether `file` is locked.  Returns `Ok(Some(pid))` if it's locked by process `pid` or
    /// Ok(None) if it's not locked.
    pub fn fcntl_get_lock(file: &File) -> Result<Option<c_int>, Error> {
        let lck = fcntl_lock_op(file, libc::F_GETLK)?;
        if lck.l_type == libc::F_UNLCK as i16 {
            Ok(None)
        } else {
            Ok(Some(lck.l_pid))
        }
    }

}
