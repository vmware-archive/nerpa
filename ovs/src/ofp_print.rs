use super::sys;

use libc;
use std::ffi;
use std::fmt;
use std::os::raw;
use std::ptr::null;

pub struct Printer<'a>(pub &'a [u8]);

impl<'a> fmt::Display for Printer<'a> {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        unsafe {
            let p = sys::ofp_to_string(
                self.0.as_ptr() as *const raw::c_void,
                self.0.len() as sys::size_t, null(), null(), 1);
            let s = ffi::CStr::from_ptr(p);
            write!(formatter, "{}", s.to_string_lossy())?;
            libc::free(p as *mut raw::c_void);
            Ok(())
        }
    }
}
