use super::sys;

use std::ffi;
use std::ptr::null_mut;

pub struct Ds(pub sys::ds);

impl Ds {
    pub fn new() -> Ds {
        Ds(sys::ds { string: null_mut(), length: 0, allocated: 0 })
    }
}

impl Drop for Ds {
    fn drop(&mut self) {
        unsafe { sys::ds_destroy(&mut self.0 as *mut _); }
    }
}

impl From<Ds> for String {
    fn from(ds: Ds) -> Self {
        unsafe {
            let p = sys::ds_cstr_ro(&ds.0 as *const _);
            ffi::CStr::from_ptr(p).to_string_lossy().into()
        }
    }
}
