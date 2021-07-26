/*
Copyright (c) 2021 VMware, Inc.
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

extern crate ovsdb_sys;

use differential_datalog::api::HDDlog;

use differential_datalog::ddval::DDValue;
use differential_datalog::DeltaMap;


// TODO: Fill out Context with necessary fields.
pub struct Context {}

impl Context {
    pub fn run(&mut self) {
        unsafe {
            let cs : *mut ovsdb_sys::ovsdb_cs = std::ptr::null_mut();
            let events : *mut ovsdb_sys::ovs_list = std::ptr::null_mut();

            println!("This should cause a segfault");
            ovsdb_sys::ovsdb_cs_run(cs, events);
        }
    }
}


// TODO: Loop over this function.
pub fn export_input_from_ovsdb(
    mut ddlog: &HDDlog
) -> Option<DeltaMap<DDValue>> {
    let mut ctx = Context{};
    ctx.run();

    None
}