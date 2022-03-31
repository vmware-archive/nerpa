use super::sys;

use libc;
use std::slice;
use std::ptr::null_mut;

pub struct Ofpbuf(pub *mut sys::ofpbuf);

unsafe impl Send for Ofpbuf {}
unsafe impl Sync for Ofpbuf {}
impl Ofpbuf {
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts((*self.0).data as *const u8, (*self.0).size as usize) }
    }
    pub fn as_ptr(&self) -> *const u8 {
        unsafe { (*self.0).data as *const u8 }
    }
    pub fn from_ptr(buf: *mut sys::ofpbuf) -> Ofpbuf {
        Ofpbuf(buf)
    }
    pub unsafe fn leak(&mut self) -> *mut sys::ofpbuf {
        let ptr = self.0;
        self.0 = null_mut();
        ptr
    }
}

impl From<Ofpbuf> for Vec<u8> {
    fn from(buf: Ofpbuf) -> Vec<u8> {
        buf.as_slice().into()
    }
}

impl Drop for Ofpbuf {
    fn drop(&mut self) {
        unsafe {
            if self.0 != null_mut() {
                sys::ofpbuf_uninit(self.0);
                libc::free(self.0 as *mut _);
            }
        }
    }
}

