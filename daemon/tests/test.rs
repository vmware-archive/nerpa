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

// Derived from tests/daemon.at in Open vSwitch, with the following license:
/*
Copyright (c) 2009, 2010, 2011, 2012, 2013, 2014, 2015 Nicira, Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at:

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

use anyhow::{anyhow, Result, Context};
use std::process::{Child, Command, ExitStatus};
use std::path::{Path, PathBuf};
use std::os::raw::c_int;
use std::io::ErrorKind;
use std::sync::Mutex;

fn parent_pid(pid: libc::pid_t) -> Result<libc::pid_t> {
    let output = String::from_utf8(Command::new("ps")
                                   .arg("-o")
                                   .arg("ppid=")
                                   .arg("-p")
                                   .arg(format!("{}", pid))
                                   .output()?
                                   .stdout)?;
    let pid: libc::pid_t = output.trim().parse()
        .with_context(|| format!("parsing 'ps' output \"{output}\""))?;
    Ok(pid)
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn check_process_name(pid: libc::pid_t, expected_name: &str) -> Result<()> {
    let path: PathBuf = format!("/proc/{pid}/comm").into();
    let actual_name = String::from_utf8(std::fs::read(&path)?)?;
    assert_eq!(actual_name.trim(), expected_name);
    Ok(())
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn check_process_name(_pid: libc::pid_t, _expected_name: &str) -> Result<()> {
    Ok(())
}

fn remove_if_exists<P: AsRef<Path>>(path: P) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e)?,
    }
}

enum Completion<T> {
    Incomplete,
    Complete(T)
}
use Completion::*;

/// Repeatedly evaluates `condition`, sleeping a bit between calls, until it yields
/// Complete(value), then returns Ok(value).  After a while, however, give up and return an error
/// instead.
fn wait_until<T, F>(mut condition: F) -> Result<T>
    where F: FnMut() -> Completion<T>
{
    for i in 0..10 {
        if let Complete(result) = condition() {
            return Ok(result)
        }
        let ms = match i {
            0 => 10,
            1 => 100,
            _ => 1000,
        };
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    Err(anyhow!("wait_until timed out"))
}

fn test_daemon_command() -> Result<Command>
{
    let examples_dir = std::env::current_dir()?.join("target/debug/examples");
    Ok(Command::new(examples_dir.join("test-daemon")))
}

fn unique_filename(extension: &str) -> Result<PathBuf> {
    static COUNTER: Mutex<usize> = Mutex::new(0);
    let count = match *COUNTER.lock().unwrap() {
        ref mut counter => { *counter += 1; *counter }
    };

    let pid = std::process::id();
    let pidfile_name: PathBuf = format!("test{pid}.{count}.{extension}").into();
    remove_if_exists(&pidfile_name)?;
    Ok(pidfile_name)
}

fn pidfile_name() -> Result<PathBuf> {
    unique_filename("pid")
}

fn send_signal(pid: libc::pid_t, signal: c_int) -> Result<(), std::io::Error> {
    if unsafe { libc::kill(pid, signal) } < 0 {
        Err(std::io::Error::last_os_error())?
    } else {
        Ok(())
    }
}

fn init_tracing() {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
}

fn process_exists(pid: libc::pid_t) -> Result<(), std::io::Error> {
    send_signal(pid, 0)
}

fn read_pidfile<P>(path: P) -> Result<libc::pid_t>
    where P: AsRef<Path>
{
    let pidfile_string = String::from_utf8(std::fs::read(path)?)?;
    Ok(pidfile_string.trim().parse()?)
}

/// This won't work if `pid` is our direct child.  Use `wait_for_child_to_die` in that case.
fn wait_for_process_to_die(pid: libc::pid_t) -> Result<()> {
    wait_until(|| match process_exists(pid) {
        Ok(()) => Incomplete,
        Err(_) => Complete(())
    })
}

/// Wait until 'file' exists.
fn wait_until_file_exists<P>(path: P) -> Result<()>
    where P: AsRef<Path>
{
    wait_until(|| match path.as_ref().exists() {
        true => Complete(()),
        false => Incomplete
    })?;
    Ok(())
}

#[test]
fn test_pidfile() -> Result<()> {
    init_tracing();

    // Start the daemon and wait for the pidfile to get created and that its contents are the
    // correct pid.
    let pidfile_name = pidfile_name()?;
    let mut child = test_daemon_command()?.arg("--pidfile").arg(&pidfile_name).spawn()?;
    wait_until_file_exists(&pidfile_name)?;
    assert_eq!(read_pidfile(&pidfile_name)?, child.id() as libc::pid_t);

    // Kill the child and wait for it to die.
    send_signal(child.id() as libc::pid_t, libc::SIGTERM)?;
    child.wait()?;

    // Verify that the pidfile was deleted.
    match std::fs::File::open(&pidfile_name) {
        Err(e) if e.kind() == ErrorKind::NotFound => (),
        other => Err(anyhow!("expected NotFound, got {other:?}"))?
    }

    Ok(())
}

/// Waits for `child` to die, and returns:
///    - `Ok(Ok(status))`: Child exited with `status`.
///    - `Ok(Err(e))`: System reported error waiting for `child` (e.g. we already waited for it).
///    - `Err(e)`: Timeout.
fn wait_for_child_to_die(child: &mut Child) -> Result<Result<ExitStatus>> {
    match wait_until(|| match child.try_wait() {
        Ok(Some(status)) => Complete(Ok(status)),
        Ok(None) => Incomplete,
        Err(e) => Complete(Err(e)),
    }) {
        Ok(Ok(result)) => Ok(Ok(result)),
        Ok(Err(error)) => Ok(Err(error.into())),
        Err(error) => Err(error),
    }
}

fn check_file_does_not_exist<P>(path: P) -> Result<()>
    where P: AsRef<Path>
{
    match std::fs::File::open(path.as_ref()) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => Err(anyhow!("{}: expected NotFound, got {other:?}", path.as_ref().display()))?
    }
}

#[test]
fn test_daemon() -> Result<()> {
    // Start the daemon and wait for the pidfile to get created
    // and check that its contents are the correct pid.
    let pidfile_name = pidfile_name()?;
    let mut child = test_daemon_command()?.arg("--pidfile").arg(&pidfile_name).spawn()?;
    let daemon_pid = child.id() as libc::pid_t;
    wait_until_file_exists(&pidfile_name)?;
    assert_eq!(read_pidfile(&pidfile_name)?, daemon_pid);

    // Kill the daemon and ensure that the pidfile gets deleted.
    send_signal(daemon_pid, libc::SIGTERM)?;
    wait_for_child_to_die(&mut child)??;
    check_file_does_not_exist(&pidfile_name)?;
    Ok(())
}

#[test]
fn test_monitor() -> Result<()> {
    // Start a monitored daemon and wait for the pidfile to get created.
    let pidfile_name = pidfile_name()?;
    let greeting_name = unique_filename("txt")?;
    let mut child = test_daemon_command()?.arg("--pidfile").arg(&pidfile_name)
        .arg("--greeting-file").arg(&greeting_name)
        .arg("--monitor").spawn()?;
    let monitor_pid = child.id() as libc::pid_t;
    wait_until_file_exists(&pidfile_name)?;

    // Check that the pidfile names a running process,
    // and that the parent process of that process is the monitor process,
    // and that (with a Linux kernel) the daemon's process name is correct.
    let daemon_pid = read_pidfile(&pidfile_name)?;
    assert_eq!(parent_pid(daemon_pid)?, monitor_pid);
    assert_eq!(parent_pid(monitor_pid)?, std::process::id() as libc::pid_t);
    check_process_name(daemon_pid, "test-daemon")?;

    // Wait for the greeting file to be created.  This avoids a race, because the daemon will
    // create the pidfile and then notify the monitor process that it's successfully started.  If
    // we don't wait here, then we could kill it before the monitor process knows it's started,
    // which the monitor process would treat as a different kind of error.
    wait_until_file_exists(&greeting_name)?;

    // Kill the daemon process, making it look like an abort(),
    // and wait for a new daemon process to get spawned.
    //
    // (Rust has weird behavior for SIGSEGV, so don't use that.)
    send_signal(daemon_pid, libc::SIGABRT)?;
    wait_for_process_to_die(daemon_pid)?;
    let daemon_pid = wait_for_pidfile_to_change(&pidfile_name, daemon_pid)?;

    // Check that the pidfile names a running process,
    // and that the parent process of that process is the monitor process,
    // and that (with a Linux kernel) the daemon's process name is correct.
    assert_eq!(parent_pid(daemon_pid)?, monitor_pid);
    assert_eq!(parent_pid(monitor_pid)?, std::process::id() as libc::pid_t);
    check_process_name(daemon_pid, "test-daemon")?;

    // Kill the daemon and wait for it and the monitor process to die.
    send_signal(daemon_pid, libc::SIGTERM)?;
    wait_for_process_to_die(daemon_pid)?;
    wait_for_child_to_die(&mut child)??;
    check_file_does_not_exist(&pidfile_name)?;

    Ok(())
}

#[test]
fn test_detach() -> Result<()> {
    // Start the daemon and make sure that the pidfile exists immediately.
    // We don't wait for the pidfile to get created because the daemon is
    // supposed to do so before the parent exits.
    let pidfile_name = pidfile_name()?;
    let mut child = test_daemon_command()?.arg("--pidfile").arg(&pidfile_name).arg("--detach")
        .spawn()?;
    let child_pid = child.id() as libc::pid_t;
    wait_for_child_to_die(&mut child)??;
    let daemon_pid = read_pidfile(&pidfile_name)?;

    // Read the pidfile and ensure that it:
    // - Identifies a real process...
    // - ...that is not the child process.
    // - and whose parent is not the child process (rather, it should be 'init', either global
    //   or in a container).
    process_exists(daemon_pid)?;
    assert_ne!(child_pid, daemon_pid);
    assert_ne!(child_pid, parent_pid(daemon_pid)?);

    // Kill the daemon and wait for it to die, and verify that the pidfile was deleted.
    send_signal(daemon_pid, libc::SIGTERM)?;
    wait_for_process_to_die(daemon_pid)?;
    check_file_does_not_exist(&pidfile_name)?;

    Ok(())
}

/// Waits for `path` to become a pidfile with a pid other than `old_pid`.  Returns the new pid.
fn wait_for_pidfile_to_change<P>(path: P, old_pid: libc::pid_t) -> Result<libc::pid_t>
    where P: AsRef<Path>
{
    let new_pid = wait_until(|| match read_pidfile(path.as_ref()) {
        Ok(new_pid) if new_pid != old_pid => Complete(new_pid),
        _ => Incomplete
    })?;
    Ok(new_pid)
}

#[test]
fn test_detach_monitor() -> Result<()> {
    // Start the daemon and make sure that the pidfile exists immediately.
    // We don't wait for the pidfile to get created because the daemon is
    // supposed to do so before the parent exits.
    let pidfile_name = pidfile_name()?;
    let mut child = test_daemon_command()?.arg("--pidfile").arg(&pidfile_name).arg("--detach")
        .arg("--monitor").spawn()?;
    let child_pid = child.id() as libc::pid_t;
    wait_for_child_to_die(&mut child)??;
    let daemon_pid = read_pidfile(&pidfile_name)?;

    // Check process naming and ancestry.
    check_process_name(daemon_pid, "test-daemon")?;
    let monitor_pid = parent_pid(daemon_pid)?;
    assert_ne!(monitor_pid, daemon_pid);
    assert_ne!(monitor_pid, child_pid);
    assert_ne!(parent_pid(monitor_pid)?, child_pid);

    // Kill the daemon process, making it look like an abort(),
    // and wait for a new daemon process to get spawned.
    //
    // (Rust has weird behavior for SIGSEGV, so don't use that.)
    send_signal(daemon_pid, libc::SIGABRT)?;
    wait_for_process_to_die(daemon_pid)?;
    let daemon_pid = wait_for_pidfile_to_change(&pidfile_name, daemon_pid)?;

    // Check process naming and ancestry.
    check_process_name(daemon_pid, "test-daemon")?;
    let monitor_pid = parent_pid(daemon_pid)?;
    assert_ne!(monitor_pid, daemon_pid);
    assert_ne!(monitor_pid, child_pid);
    assert_ne!(parent_pid(monitor_pid)?, child_pid);

    // Kill the daemon and wait for it to die, and verify that the pidfile was deleted.
    send_signal(daemon_pid, libc::SIGTERM)?;
    wait_for_process_to_die(daemon_pid)?;
    check_file_does_not_exist(&pidfile_name)?;

    Ok(())
}

fn test_startup_errors_with_args(args: &[&str]) -> Result<()> {
    // Try to start a daemon, forcing an error, and make sure that the exit status is correct.
    let pidfile_name = "/not/a/valid/path";
    let mut child = test_daemon_command()?
    .arg("--pidfile").arg(&pidfile_name)
        .args(args)
        .spawn()?;
    assert_eq!(wait_for_child_to_die(&mut child)??.code(), Some(1));
    check_file_does_not_exist(&pidfile_name)?;
    Ok(())
}

#[test]
fn test_startup_errors() -> Result<()> {
    test_startup_errors_with_args(&[])
}

#[test]
fn test_startup_errors_detach() -> Result<()> {
    test_startup_errors_with_args(&["--detach"])
}

#[test]
fn test_startup_errors_monitor() -> Result<()> {
    test_startup_errors_with_args(&["--monitor"])
}

#[test]
fn test_startup_errors_detach_monitor() -> Result<()> {
    test_startup_errors_with_args(&["--detach", "--monitor"])
}
