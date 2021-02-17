/*
Copyright (c) 2018-2020 VMware, Inc.
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

// The auto-generated crate `nerpa_ddlog` declares the `HDDlog` type.
// This serves as a reference to a running DDlog program.
// It implements `trait differential_datalog::DDlog`.
use nerpa_ddlog::api::HDDlog;
use nerpa_ddlog::Relations::Port;

// `differential_datalog` contains the DDlog runtime copied to each generated workspace.
use differential_datalog::DDlog; // Trait that must be implemented by DDlog program.
use differential_datalog::DeltaMap; // Represents a set of changes to DDlog relations.
use differential_datalog::ddval::DDValue; // Generic type wrapping all DDlog values.
use differential_datalog::ddval::DDValConvert; // Trait to convert Rust types to/from DDValue.
use differential_datalog::program::RelId;
use differential_datalog::program::Update;

// DDlogNerpa contains a handle to the DDlog program.
pub struct DDlogNerpa {
    hddlog: HDDlog,
}

impl DDlogNerpa {
    pub fn new() -> Result<DDlogNerpa, String> {
        // Instantiate a DDlog program.
        // Returns a handle to the DDlog program and initial contents of output relations.
        let (hddlog, _init_state) = HDDlog::run(1, false)?;
        return Ok(Self{hddlog});
    }

    pub fn stop(&mut self) {
        self.hddlog.stop().unwrap();
    }

    pub fn add_input(&mut self, ports: Vec<types::Port>) -> Result<DeltaMap<DDValue>, String> {
        self.hddlog.transaction_start()?;

        // TODO: Clean up type conversion.
        // We shouldn't need an iterator, since _num is unused.
        let updates = ports.into_iter().map(|port|
            Update::Insert {
                relid: Port as RelId,
                v: types::Port{number: port.number, config: port.config}.into_ddvalue(),
            }
        ).collect::<Vec<_>>();

        self.hddlog.apply_valupdates(updates.into_iter())?;
        let delta = self.hddlog.transaction_commit_dump_changes()?;
        return Ok(delta);
    }

    // dump_delta prints the delta in output relations
    pub fn dump_delta(delta: &DeltaMap<DDValue>) {
        for (rel, changes) in delta.iter() {
            println!("Changes to relation {}", nerpa_ddlog::relid2name(*rel).unwrap());
            for (val, weight) in changes.iter() {
                println!("{} {:+}", val, weight);
            }
        }
    }

    // TODO: Implement function to convert output relation delta into P4. 
}

fn main() {
    // Instantiate DDlog program.
    let mut nerpa = DDlogNerpa::new().unwrap();

    // TODO: Better define the API for the management plane (i.e., the user interaction).
    // We should read in the vector of port configs, or whatever the input becomes.
    // Add input to DDlog program.
    let ports = vec!(types::Port{number: 11, config: types::port_config_t::Access{tag: 1}});

    // Compute and print output relation.
    let delta = nerpa.add_input(ports).unwrap();
    DDlogNerpa::dump_delta(&delta);
}
