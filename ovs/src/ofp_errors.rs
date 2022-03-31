use super::sys;

use std::error;
use std::ffi;
use std::fmt;
use std::io;
use std::os::raw;

use anyhow::Result;

#[derive(Debug)]
pub struct Error(pub sys::ofperr::Type);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        unsafe {
            if sys::ofperr_is_valid(self.0) {
                let s = ffi::CStr::from_ptr(sys::ofperr_get_name(self.0));
                write!(f, "{}", s.to_string_lossy())
            } else {
                write!(f, "<unknown ofperr {}>", self.0)
            }
        }
    }
}

impl error::Error for Error {}

#[derive(Debug)]
pub struct Eof;

impl fmt::Display for Eof {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "end of file")
    }
}

impl error::Error for Eof {}

#[derive(Debug)]
pub struct UnknownError(raw::c_int);

impl fmt::Display for UnknownError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "unknown error {}", self.0)
    }
}

impl error::Error for UnknownError {}

pub fn parse(retval: sys::ofperr::Type) -> Result<()> {
    let retval = retval as raw::c_int;
    let OFPERR_OFS = sys::OFPERR_OFS as raw::c_int;
    if retval == 0 {
        Ok(())
    } else if retval == -1 {
        Err(Eof)?
    } else if retval > 0 && retval < OFPERR_OFS {
        Err(io::Error::from_raw_os_error(retval as i32))?
    } else if retval >= OFPERR_OFS && unsafe {sys::ofperr_is_valid(retval) } {
        Err(Error(retval as sys::ofperr::Type))?
    } else {
        Err(UnknownError(retval))?
    }
}
