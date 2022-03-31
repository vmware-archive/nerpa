use super::sys;

use super::ofpbuf::Ofpbuf;

pub fn update_length(buf: &mut Ofpbuf) {
    unsafe {
        sys::ofpmsg_update_length(buf.0 as *mut _);
    }
}
