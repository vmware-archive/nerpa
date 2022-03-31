use super::sys;
use std::os::raw;
use std::ptr::null;

pub fn fd_wait(fd: raw::c_int, events: raw::c_short) {
    unsafe { sys::poll_fd_wait_at(fd, events, null()) }
}
pub fn timer_wait(msec: i64) {
    unsafe { sys::poll_timer_wait_at(msec, null()) }
}
pub fn timer_wait_until(msec: i64) {
    unsafe { sys::poll_timer_wait_until_at(msec, null()) }
}
pub fn immediate_wake() {
    unsafe { sys::poll_immediate_wake_at(null()) }
}
pub fn block() {
    unsafe { sys::poll_block() }
}

