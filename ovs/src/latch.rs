use super::sys;
use std::ptr::null;

/// A `Latch` is an Open vSwitch implementation of a thread-safe, signal-safe doorbell that can be
/// polled with `select` and `poll` system calls.  This makes it useful for interfacing with OVS
/// because the OVS libraries otherwise fully encapsulate the OS entities that they work this.  For
/// example, an OVS [`Rconn`] has a file descriptor in it, but the `Rconn` library doesn't provide
/// any way to get the file descriptor out, only a way to call `poll` with it through the OVS
/// `poll_loop`, so it's not possible to make it directly work with anything (like Rust futures)
/// that can't use `poll_loop`.  But Rust futures, etc., can wake up an OVS `poll_loop` using a
/// `Latch`.
pub struct Latch(sys::latch);

impl Latch {
    pub fn new() -> Self {
        let mut latch = Latch(sys::latch { fds: [0, 0] });
        unsafe { sys::latch_init(&mut latch.0) }
        latch
    }
    pub fn poll(&mut self) -> bool { unsafe { sys::latch_poll(&mut self.0) } }
    pub fn set(&mut self) { unsafe { sys::latch_set(&mut self.0) } }
    pub fn is_set(&self) -> bool { unsafe { sys::latch_is_set(&self.0) } }
    pub fn wait(&self) { unsafe { sys::latch_wait_at(&self.0, null()) } }
}

impl Drop for Latch {
    fn drop(&mut self) {
        unsafe { sys::latch_destroy(&mut self.0) }
    }
}
