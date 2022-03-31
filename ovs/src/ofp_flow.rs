use super::sys;

use super::ofpbuf::Ofpbuf;
use super::ofp_errors;
use super::ofp_protocol::{Protocol, Protocols};
use super::ds::Ds;

use std::error;
use std::ffi;
use std::fmt;
use std::os::raw;
use std::ptr::{null, null_mut};

use anyhow::Result;

pub struct FlowMod(sys::ofputil_flow_mod);

const OFPFC_ADD: u8 = sys::ofp_flow_mod_command_OFPFC_ADD as u8;
const OFPFC_MODIFY: u8 = sys::ofp_flow_mod_command_OFPFC_MODIFY as u8;
const OFPFC_MODIFY_STRICT: u8 = sys::ofp_flow_mod_command_OFPFC_MODIFY_STRICT as u8;
const OFPFC_DELETE: u8 = sys::ofp_flow_mod_command_OFPFC_DELETE as u8;
const OFPFC_DELETE_STRICT: u8 = sys::ofp_flow_mod_command_OFPFC_DELETE_STRICT as u8;

pub enum FlowModCommand {
    Add,
    Modify { strict: bool },
    Delete { strict: bool }
}

impl Drop for FlowMod {
    fn drop(&mut self) {
        unsafe {
            libc::free(self.0.ofpacts as *mut raw::c_void);
            sys::minimatch_destroy(&mut self.0.match_ as *mut _);
        }
    }
}

impl FlowModCommand {
    fn to_openflow(&self) -> u8 {
        match self {
            FlowModCommand::Add => OFPFC_ADD,
            FlowModCommand::Modify { strict: false } => OFPFC_MODIFY,
            FlowModCommand::Modify { strict: true } => OFPFC_MODIFY_STRICT,
            FlowModCommand::Delete { strict: false } => OFPFC_DELETE,
            FlowModCommand::Delete { strict: true } => OFPFC_DELETE_STRICT
        }
    }
}

#[derive(Debug)]
pub struct FlowModParseError(pub String);

impl fmt::Display for FlowModParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl error::Error for FlowModParseError {}

unsafe impl Send for FlowMod {}
unsafe impl Sync for FlowMod {}
impl FlowMod {
    pub fn parse(s: &str, command: Option<FlowModCommand>) -> Result<(FlowMod, Protocols)> {
        let mut fm: sys::ofputil_flow_mod = Default::default();
        let s = match ffi::CString::new(s) {
            Ok(cs) => cs,
            Err(_) => Err(FlowModParseError("unexpected NUL in string".into()))?
        };
        let mut usable_protocols = Protocols::all().bits();
        unsafe {
            let command = match command {
                None => -2,
                Some(command) => command.to_openflow() as raw::c_int
            };
            let s = sys::parse_ofp_flow_mod_str(&mut fm as *mut sys::ofputil_flow_mod, s.as_ptr(),
                                                      null(), null(), command,
                                                      &mut usable_protocols as *mut sys::ofputil_protocol);
            if s == null_mut() {
                Ok((FlowMod(fm), Protocols::from_bits_unchecked(usable_protocols)))
            } else {
                let cs = ffi::CStr::from_ptr(s).to_string_lossy().into();
                libc::free(s as *mut ffi::c_void);
                Err(FlowModParseError(cs))?
            }
        }
    }
    pub fn encode(&self, protocol: Protocol) -> Ofpbuf {
        unsafe {
            let b = sys::ofputil_encode_flow_mod(&self.0 as *const sys::ofputil_flow_mod,
                                                 protocol.into());
            Ofpbuf::from_ptr(b)
        }
    }
    pub fn format(oh: *const sys::ofp_header, verbosity: i32) -> Result<String> {
        unsafe {
            let mut ds = Ds::new();
            ofp_errors::parse(sys::ofputil_flow_mod_format(&mut ds.0 as *mut _, oh, null(), null(), verbosity))?;
            Ok(ds.into())
        }
    }
}

mod tests {
    #[test]
    fn it_works() {
        match FlowMod::from_string("priority=0 actions=drop", FlowModCommand::Add) {
            Ok(fm) => println!("ok! {:?}", fm.to_string().unwrap()),
            Err(s) => println!("{}", s)
        }
    }
}
