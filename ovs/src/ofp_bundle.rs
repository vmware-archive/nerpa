/*
Copyright (c) 2022 VMware, Inc.
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

//! OpenFlow bundle support.
//!
//! Some versions of OpenFlow support "bundles", which are groups of OpenFlow messages that the
//! switch applies as a single transaction.  This module supports grouping OpenFlow messages into
//! bundles.
use super::sys;
use super::sys::ofperr;

use super::ds::Ds;
use super::ofpbuf::Ofpbuf;
use super::ofp_errors;
use super::ofp_msgs;
use super::ofp_protocol::Version;

use std::mem;

use anyhow::Result;

pub const OFPBCT_OPEN_REQUEST: u16 = sys::ofp14_bundle_ctrl_type_OFPBCT_OPEN_REQUEST as u16;
pub const OFPBCT_OPEN_REPLY: u16 = sys::ofp14_bundle_ctrl_type_OFPBCT_OPEN_REPLY as u16;
pub const OFPBCT_CLOSE_REQUEST: u16 = sys::ofp14_bundle_ctrl_type_OFPBCT_CLOSE_REQUEST as u16;
pub const OFPBCT_CLOSE_REPLY: u16 = sys::ofp14_bundle_ctrl_type_OFPBCT_CLOSE_REPLY as u16;
pub const OFPBCT_COMMIT_REQUEST: u16 = sys::ofp14_bundle_ctrl_type_OFPBCT_COMMIT_REQUEST as u16;
pub const OFPBCT_COMMIT_REPLY: u16 = sys::ofp14_bundle_ctrl_type_OFPBCT_COMMIT_REPLY as u16;
pub const OFPBCT_DISCARD_REQUEST: u16 = sys::ofp14_bundle_ctrl_type_OFPBCT_DISCARD_REQUEST as u16;
pub const OFPBCT_DISCARD_REPLY: u16 = sys::ofp14_bundle_ctrl_type_OFPBCT_DISCARD_REPLY as u16;

pub const OFPBF_ATOMIC: u16 = sys::ofp14_bundle_flags_OFPBF_ATOMIC as u16;
pub const OFPBF_ORDERED: u16 = sys::ofp14_bundle_flags_OFPBF_ORDERED as u16;

/// An OpenFlow "bundle control message", which operates on a bundle.
pub struct BundleCtrlMsg {
    /// An arbitrary client-assigned identifier for the bundle, which must be unique among the
    /// bundles that are currently open within the scope of a particular OpenFlow connection.
    pub bundle_id: u32,

    /// One of `OFPBCT_*`.
    pub type_: u16,

    /// Any combination of `OFPBF_*`.
    pub flags: u16
}

impl BundleCtrlMsg {
    pub fn decode(oh: &[u8]) -> Result<Self> {
        if oh.len() < mem::size_of::<sys::ofp_header>() {
            Err(ofp_errors::Error(ofperr::OFPERR_OFPBRC_BAD_LEN))?
        }

        unsafe {
            let mut bcm = sys::ofputil_bundle_ctrl_msg {
                bundle_id: 0,
                type_: 0,
                flags: 0
            };
            ofp_errors::parse(sys::ofputil_decode_bundle_ctrl(
                oh.as_ptr() as *const _,
                &mut bcm as *mut _))?;
            Ok(BundleCtrlMsg {
                bundle_id: bcm.bundle_id,
                type_: bcm.type_,
                flags: bcm.flags
            })
        }
    }
    fn to_bcm(&self) -> sys::ofputil_bundle_ctrl_msg {
        sys::ofputil_bundle_ctrl_msg {
            bundle_id: self.bundle_id,
            type_: self.type_,
            flags: self.flags
        }
    }
    pub fn encode_request(&self, version: Version) -> Ofpbuf {
        unsafe {
            let mut bcm = self.to_bcm();
            let b = sys::ofputil_encode_bundle_ctrl_request(
                version as sys::ofp_version, &mut bcm as *mut _);
            let mut b = Ofpbuf::from_ptr(b);
            ofp_msgs::update_length(&mut b);
            b
        }
    }
    pub fn encode_reply(&self, request: &sys::ofp_header) -> Ofpbuf {
        unsafe {
            let mut bcm = self.to_bcm();
            let b = sys::ofputil_encode_bundle_ctrl_reply(
                request as *const _, &mut bcm as *mut _);
            let mut b = Ofpbuf::from_ptr(b);
            ofp_msgs::update_length(&mut b);
            b
        }
    }
    pub fn format(&self) -> Result<String> {
        unsafe {
            let bcm = self.to_bcm();
            let mut ds = Ds::new();
            sys::ofputil_format_bundle_ctrl_request(&mut ds.0 as *mut _, &bcm as *const _);
            Ok(ds.into())
        }
    }
}

pub struct BundleAddMsg<'a> {
    pub bundle_id: u32,
    pub flags: u16,
    pub msg: &'a [u8]
}
impl<'a> BundleAddMsg<'a> {
    pub fn encode(&self, version: Version) -> Ofpbuf {
        unsafe {
            let mut bam = sys::ofputil_bundle_add_msg {
                bundle_id: self.bundle_id,
                flags: self.flags,
                msg: self.msg.as_ptr() as *const _,
            };

            let b = sys::ofputil_encode_bundle_add(
                version as sys::ofp_version, &mut bam as *mut _);
            Ofpbuf::from_ptr(b)
        }
    }
}

enum BundleSequenceState {
    Open,
    Inner,
    Done
}
pub struct BundleSequence<Inner: Iterator<Item=Ofpbuf>> {
    bundle_id: u32,
    flags: u16,
    version: Version,
    inner: Inner,
    state: BundleSequenceState
}
impl<Inner: Iterator<Item=Ofpbuf>> BundleSequence<Inner> {
    /// Returns an iterator that yields a sequence of OpenFlow messages that open a bundle,
    /// add all of the messages from `inner` to it, and then commit the bundle.
    ///
    /// # Arguments.
    /// * `bundle_id`: Identifier for the bundle.  The caller should use a different ID
    ///   for each bundle.
    /// * `flags`: A combination of the `OFPBF_ATOMIC` and `OFPBF_ORDERED` bit-flags.
    /// * `version`: The OpenFlow version to use for encoding.
    /// * `inner`: Sequence of OpenFlow messages to include in the bundle.
    pub fn new(bundle_id: u32, flags: u16, version: Version, inner: Inner) -> BundleSequence<Inner> {
        BundleSequence { bundle_id, flags, version, inner, state: BundleSequenceState::Open }
    }
    fn ctrl_msg(&self, type_: u16) -> Ofpbuf {
        BundleCtrlMsg {
            bundle_id: self.bundle_id,
            flags: self.flags,
            type_,
        }.encode_request(self.version)
    }
}
impl<Inner: Iterator<Item=Ofpbuf>> Iterator for BundleSequence<Inner> {
    type Item = Ofpbuf;
    fn next(&mut self) -> Option<Self::Item> {
        match self.state {
            BundleSequenceState::Open => {
                self.state = BundleSequenceState::Inner;
                Some(self.ctrl_msg(OFPBCT_OPEN_REQUEST))
            },
            BundleSequenceState::Inner => match self.inner.next() {
                Some(msg) => Some(BundleAddMsg {
                    bundle_id: self.bundle_id,
                    flags: self.flags,
                    msg: msg.as_slice()
                }.encode(self.version)),
                None => {
                    self.state = BundleSequenceState::Done;
                    Some(self.ctrl_msg(OFPBCT_COMMIT_REQUEST))
                }
            },
            BundleSequenceState::Done => None,
        }
    }
}
