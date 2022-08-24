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

// Derived from lib/fatal-signal.c in Open vSwitch, with the following license:
/*
 * Copyright (c) 2008, 2009, 2010, 2011, 2012, 2013 Nicira, Inc.
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

use anyhow::{anyhow, Result};
use rand::random;
use signal_hook::{self, consts::signal::*, iterator::Signals};
use std::collections::HashSet;
use std::default::Default;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::{event, Level};

#[cfg(doc)]
use crate::Daemonize;

#[derive(Default)]
struct Actions {
    kill_pids: HashSet<u32>,
    remove_dirs: HashSet<PathBuf>,
    remove_files: HashSet<PathBuf>,
    kill_pidfiles: HashSet<PathBuf>,
    keep_dirs: bool
}

impl Actions {
    fn new() -> Actions {
        Default::default()
    }

    fn terminate(pid: u32) {
        unsafe { libc::kill(pid as libc::pid_t, SIGTERM); }
    }

    fn read_pidfile(filename: &PathBuf) -> Result<u32> {
        // XXX We should technically do a more elaborate dance here for OVS pidfiles (see
        // lib/daemon-unix.c in the OVS tree) involving file locks, but this is OK for now.
        let mut file = File::open(filename)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents.trim().parse()?)
    }

    fn run(&mut self) {
        for pidfile in self.kill_pidfiles.drain() {
            match Self::read_pidfile(&pidfile) {
                Ok(pid) => drop(self.kill_pids.insert(pid)),
                Err(err) => event!(Level::WARN, "{}: reading pidfile failed ({err})",
                                   pidfile.to_string_lossy())
            }
        }
        for pid in self.kill_pids.drain() {
            Self::terminate(pid)
        }
        for file in self.remove_files.drain() {
            if let Err(err) = fs::remove_file(&file) {
                event!(Level::WARN, "{}: removing file failed ({err})", file.to_string_lossy());
            }
        }
        if !self.keep_dirs {
            for dir in self.remove_dirs.drain() {
                loop {
                    match fs::remove_dir_all(&dir) {
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                            // Ignore ENOENT and retry because it's common for the processes we just
                            // killed to remove some of their temporary files as they die.
                        },
                        Err(err) => {
                            event!(Level::WARN, "{}: removing directory failed ({err})",
                                   dir.to_string_lossy());
                            break;
                        },
                        _ => break,
                    }
                }
            }
        }
    }

    fn keep_dirs(&mut self) {
        self.keep_dirs = true;
    }
}

/// Release resources on orderly exit or due to a signal.
///
/// This struct supports releasing resources (such as killing child processes and deleting
/// temporary files) when the running process terminates due to a signal or when it exits in the
/// usual way.
///
/// Create a `Cleanup` before launching the first subprocess, then use `Cleanup::spawn()` to create
/// each subprocess.  If a fatal signal arrives, `Cleanup` will send each registered subprocess a
/// `SIGTERM` signal.  `Cleanup` will do the same thing when it is `drop`ped (thus, it's a good
/// idea to drop it only just before exiting).
///
/// [`Daemonize::start()`] and [`Daemonize::run()`] create and return a `Cleanup`.  Code that uses
/// `Daemonize` should use the `Cleanup` that it provides.
pub struct Cleanup {
    signals_handle: signal_hook::iterator::backend::Handle,
    thread_handle: Option<thread::JoinHandle<()>>,
    actions: Arc<Mutex<Actions>>
}

impl Cleanup {
    /// Creates and returns a new `Cleanup`, registering signal handlers.  When the `Cleanup` is
    /// dropped, or when the program is killed by a signal, it takes actions registered with it to
    /// clean up after resources registered with the object.
    ///
    /// Cleanup on signal handling happens in a thread that this object creates.  This means that
    /// calling `fork` will prevent cleanup due to a signal from happening in the child process
    /// (but not cleanup due to drop).  Therefore, a process that forks should create a `Cleanup`
    /// only in the child, not in the parent.  `Daemonize` creates and returns a `Cleanup`.  Code
    /// that uses `Daemonize` should use the `Cleanup` that it provides.
    ///
    /// `Cleanup` should be a singleton.  (This is forced because signals have only one handler.)
    pub fn new() -> Result<Cleanup> {
        let mut signals = Signals::new(&[SIGTERM, SIGINT, SIGHUP, SIGALRM])?;
        let signals_handle = signals.handle();
        let actions = Arc::new(Mutex::new(Actions::new()));
        let actions2 = actions.clone();
        let thread_handle = Some(thread::spawn(move || {
            for signal in signals.forever() {
                actions2.lock().unwrap().run();
                signal_hook::low_level::emulate_default_handler(signal).unwrap();
                unreachable!();
            }
            actions2.lock().unwrap().run();
        }));
        Ok(Cleanup { signals_handle, thread_handle, actions })
    }

    /// Spawns a process according to `command` and registers it to be killed on exit.
    ///
    /// Creating the child process and registering it in one step, as opposed to using a
    /// `register_pid()` API, prevents a race condition where the process is created before it is
    /// registered.
    pub fn spawn(&mut self, command: &mut Command) -> Result<Child> {
        let mut actions = self.actions.lock().unwrap();
        let child = command.spawn()?;
        actions.kill_pids.insert(child.id());
        Ok(child)
    }

    /// Runs `command` and returns its output.  If this process is killed by a signal while
    /// `command` runs, the child will be killed too.
    pub fn output(&mut self, command: &mut Command) -> Result<Output> {
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let child = self.spawn(command)?;
        let child_id = child.id();
        let output = child.wait_with_output();
        self.actions.lock().unwrap().kill_pids.remove(&child_id);
        Ok(output?)
    }

    /// Creates and returns the name of a new temporary directory under `parent_dir`, registering
    /// the directory **and all of its contents** to be removed on exit.
    pub fn create_temp_dir<P: AsRef<Path>>(&mut self, parent_dir: P) -> Result<PathBuf> {
        let max_attempts = 10;
        let parent_dir = parent_dir.as_ref().canonicalize()?;
        for _i in 0..max_attempts {
            let tmp_dir = parent_dir.join(format!("tmp{}", random::<u32>()));
            let mut actions = self.actions.lock().unwrap();
            match fs::create_dir(&tmp_dir) {
                Ok(()) => {
                    actions.remove_dirs.insert(tmp_dir.clone());
                    return Ok(tmp_dir);
                },
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => Err(e)?
            }
        }
        Err(anyhow!("{} attempts to create directory failed", max_attempts))
    }

    /// Makes this Cleanup refrain from deleting temporary directories created by
    /// `create_temp_dir`, to allow them to be inspected after exit.
    pub fn keep_temp_dirs(&mut self) {
        self.actions.lock().unwrap().keep_dirs();
    }

    /// Registers `pidfile` as a file that contains the pid of a process to kill on exit.
    pub fn register_pidfile<P: AsRef<Path>>(&mut self, pidfile: P) -> Result<()> {
        self.actions.lock().unwrap().kill_pidfiles.insert(absolute_path(pidfile.as_ref())?);
        Ok(())
    }

    /// Registers `file` as a file to delete on exit.
    pub fn register_remove_file<P: AsRef<Path>>(&mut self, file: P) -> Result<()> {
        self.actions.lock().unwrap().remove_files.insert(absolute_path(file.as_ref())?);
        Ok(())
    }
}

impl Drop for Cleanup {
    /// Executes all the registered cleanup actions: deleting files and directories, killing
    /// processes, and so on.  Thus, it's generally unwise to drop `Cleanup` much before the
    /// process exits.
    fn drop(&mut self) {
        self.signals_handle.close();
        self.thread_handle.take().map(thread::JoinHandle::join);
    }
}

// When std::path::absolute() becomes stable, we should use that instead.
fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(PathBuf::from(path))
    } else {
        let mut abspath = std::env::current_dir()?;
        abspath.extend(path);
        Ok(abspath)
    }
}
