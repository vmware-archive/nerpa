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

// Derived from lib/command-line.c in Open vSwitch, with the following license:
/*
 * Copyright (c) 2008, 2009, 2010, 2011, 2013, 2014 Nicira, Inc.
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

//! Controls the name of the running process, as shown by `ps`.
//!
//! `setproctitle` is the name of the function to set the process title on *BSD.
//!
//! This is operating-system specific functionality.  The current version of this code only
//! implements it for GNU/Linux.  It will be a no-op on other operating systems.

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux {
    use std::ffi::CStr;
    use std::ptr::null_mut;
    use std::sync::Mutex;
    use std::os::raw::c_int;

    struct ProcTitle {
        data: &'static mut [u8],
        original: Vec<u8>
    }

    impl ProcTitle {
        pub fn set(&mut self, s: &str) {
            let argv0 = std::env::args().nth(0);
            let program_name = match argv0 {
                Some(ref s) => match s.rsplit_once('/') {
                    Some((_, after)) => after,
                    None => &s
                },
                None => ""
            };
            let mut s = format!("{}: {}", program_name, s);
            if s.len() >= self.data.len() {
                s.truncate(self.data.len() - 4);
                s.push_str("...");
            }

            let mut v = Vec::with_capacity(self.data.len());
            v.extend(s.as_bytes());
            v.resize(self.data.len() - 1, 0);
            v.push(0);
            self.data.copy_from_slice(v.as_slice());
        }

        pub fn restore(&mut self) {
            self.data.copy_from_slice(&self.original);
        }

        /// The process name shown in `ps` and other tools is `argv[0]`.  If one replaces `argv[0]` by
        /// another string, then that string shows.  We can do that if we copy all the `argv' strings to
        /// new locations in memory and point `argv[*]` to these new locations.
        ///
        /// `unsafe` is an understatement, we need something like `cursed` for this.
        ///
        /// This function returns the space that was made available to replace by a program name, if any,
        /// but at least four bytes.
        unsafe fn new(argc: c_int, argv: *mut *mut u8) -> Option<Self> {
            if argc == 0 || *argv == null_mut() {
                return None
            }

            let argv0 = cstr_mut_slice_with_nul_from_ptr(*argv);
            let mut argv_space = argv0.as_mut_ptr_range();
            *argv = cstr_clone(argv0);
            for i in 1..argc as isize {
                let argvip = argv.offset(i);
                let argvi = cstr_mut_slice_with_nul_from_ptr(*argvip);
                *argvip = cstr_clone(argvi);

                let argvi = argvi.as_mut_ptr_range();

                // Add argvi to argv_space, if we can.  Linux always puts `argv[0]` at the lowest address
                // and puts the other arguments at increasing addresses.
                if argvi.start == argv_space.end {
                    argv_space = argv_space.start..argvi.end;
                }
            }
            let len = argv_space.end.offset_from(argv_space.start) as usize;
            let slice = std::slice::from_raw_parts_mut(argv_space.start, len);
            if slice.len() < 4 {
                return None
            }
            let original = slice.iter().copied().collect();
            Some(ProcTitle { data: slice, original })
        }

    }

    static PROC_TITLE: Mutex<Option<ProcTitle>> = Mutex::new(None);

    /// Return the length of the null-terminated string at `s`.
    unsafe fn strlen(s: *const u8) -> usize {
        CStr::from_ptr(s as *const i8).to_bytes().len()
    }

    /// Returns a slice containing null-terminated string `s`, including the null terminator.
    unsafe fn cstr_mut_slice_with_nul_from_ptr(s: *mut u8) -> &'static mut [u8] {
        std::slice::from_raw_parts_mut(s, strlen(s) + 1)
    }

    /// Returns a pointer to a clone of `s`.  `s` should include the null terminator, and the returned
    /// copy does too.
    fn cstr_clone(s: &mut [u8]) -> *mut u8 {
        let mut clone = Vec::with_capacity(s.len());
        clone.extend_from_slice(s);
        clone.leak().as_mut_ptr()
    }

    /// Changes the name of the process, as shown by `ps`, to the program name followed by `s`.
    /// `s` will be ellipsized, if necessary, to fit in the available space, which varies depending
    /// on the length of the program's arguments.
    pub fn set(s: &str) {
        if let Some(ref mut proc_title) = *PROC_TITLE.lock().unwrap() {
            proc_title.set(s);
        }
    }

    /// Restores the original name of the process, as shown by `ps`.
    pub fn restore() {
        if let Some(ref mut proc_title) = *PROC_TITLE.lock().unwrap() {
            proc_title.restore();
        }
    }

    // The following is adapted from the Rust standard library.
    #[used]
    #[link_section = ".init_array"]
    static ARGV_INIT_ARRAY: extern "C" fn(
        std::os::raw::c_int,
        *mut *mut u8,
        *const *const u8,
    ) = {
        extern "C" fn init_wrapper(argc: c_int, argv: *mut *mut u8, _envp: *const *const u8) {
            *PROC_TITLE.lock().unwrap() = unsafe { ProcTitle::new(argc, argv) };
        }
        init_wrapper
    };
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub use linux::{set, restore};

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn set(_s: &str) {
    // Don't know how to set the proctitle on this operating system.
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn restore() {
}
