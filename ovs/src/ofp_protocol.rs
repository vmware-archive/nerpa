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
use super::sys;

use bitflags::bitflags;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Protocol {
    OF10_STD = sys::ofputil_protocol_OFPUTIL_P_OF10_STD as isize,
    OF10_STD_TID = sys::ofputil_protocol_OFPUTIL_P_OF10_STD_TID as isize,
    OF10_NXM = sys::ofputil_protocol_OFPUTIL_P_OF10_NXM as isize,
    OF10_NXM_TID = sys::ofputil_protocol_OFPUTIL_P_OF10_NXM_TID as isize,
    OF11_STD = sys::ofputil_protocol_OFPUTIL_P_OF11_STD as isize,
    OF12_OXM = sys::ofputil_protocol_OFPUTIL_P_OF12_OXM as isize,
    OF13_OXM = sys::ofputil_protocol_OFPUTIL_P_OF13_OXM as isize,
    OF14_OXM = sys::ofputil_protocol_OFPUTIL_P_OF14_OXM as isize,
    OF15_OXM = sys::ofputil_protocol_OFPUTIL_P_OF15_OXM as isize
}

impl From<Protocol> for sys::ofputil_protocol {
    fn from(p: Protocol) -> sys::ofputil_protocol {
        p as sys::ofputil_protocol
    }
}

bitflags! {
    pub struct Protocols: sys::ofputil_protocol {
        const OF10_STD = Protocol::OF10_STD as sys::ofputil_protocol;
        const OF10_STD_TID = Protocol::OF10_STD_TID as sys::ofputil_protocol;
        const OF10_NXM = Protocol::OF10_NXM as sys::ofputil_protocol;
        const OF10_NXM_TID = Protocol::OF10_NXM_TID as sys::ofputil_protocol;
        const OF11_STD = Protocol::OF11_STD as sys::ofputil_protocol;
        const OF12_OXM = Protocol::OF12_OXM as sys::ofputil_protocol;
        const OF13_OXM = Protocol::OF13_OXM as sys::ofputil_protocol;
        const OF14_OXM = Protocol::OF14_OXM as sys::ofputil_protocol;
        const OF15_OXM = Protocol::OF15_OXM as sys::ofputil_protocol;

        /* OpenFlow 1.0 protocols.
         *
         * The "STD" protocols use the standard OpenFlow 1.0 flow format.
         * The "NXM" protocols use the Nicira Extensible Match (NXM) flow format.
         *
         * The protocols with "TID" mean that the nx_flow_mod_table_id Nicira
         * extension has been enabled.  The other protocols have it disabled.
         */
        const OF10_STD_ANY = Self::OF10_STD.bits | Self::OF10_STD_TID.bits;
        const OF10_NXM_ANY = Self::OF10_NXM.bits | Self::OF10_NXM_TID.bits;
        const OF10_ANY = Self::OF10_STD_ANY.bits | Self::OF10_NXM_ANY.bits;
        
        /* OpenFlow 1.1 protocol.
         *
         * We only support the standard OpenFlow 1.1 flow format.
         *
         * OpenFlow 1.1 always operates with an equivalent of the
         * nx_flow_mod_table_id Nicira extension enabled, so there is no "TID"
         * variant. */

        /* OpenFlow 1.2+ protocols (only one variant each).
         *
         * These use the standard OpenFlow Extensible Match (OXM) flow format.
         *
         * OpenFlow 1.2+ always operates with an equivalent of the
         * nx_flow_mod_table_id Nicira extension enabled, so there is no "TID"
         * variant. */
        const ANY_OXM = (Self::OF12_OXM.bits |
                         Self::OF13_OXM.bits |
                         Self::OF14_OXM.bits |
                         Self::OF15_OXM.bits);

        const NXM_OXM_ANY = Self::OF10_NXM_ANY.bits | Self::ANY_OXM.bits;

        const OF15_UP = Self::OF15_OXM.bits;
        const OF14_UP = Self::OF15_UP.bits | Self::OF14_OXM.bits;
        const OF13_UP = Self::OF14_UP.bits | Self::OF13_OXM.bits;
        const OF12_UP = Self::OF13_UP.bits | Self::OF12_OXM.bits;
        const OF11_UP = Self::OF12_UP.bits | Self::OF11_STD.bits;

        /* Protocols in which a specific table may be specified in flow_mods. */
        const TID = (Self::OF10_STD_TID.bits |
                     Self::OF10_NXM_TID.bits |
                     Self::OF11_STD.bits |
                     Self::ANY_OXM.bits);
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Version {
    OFP10 = sys::ofp_version_OFP10_VERSION as isize,
    OFP11 = sys::ofp_version_OFP11_VERSION as isize,
    OFP12 = sys::ofp_version_OFP12_VERSION as isize,
    OFP13 = sys::ofp_version_OFP13_VERSION as isize,
    OFP14 = sys::ofp_version_OFP14_VERSION as isize,
    OFP15 = sys::ofp_version_OFP15_VERSION as isize
}

bitflags! {
    pub struct Versions: u32 {
        const OFP10 = 1 << sys::ofp_version_OFP10_VERSION;
        const OFP11 = 1 << sys::ofp_version_OFP11_VERSION;
        const OFP12 = 1 << sys::ofp_version_OFP12_VERSION;
        const OFP13 = 1 << sys::ofp_version_OFP13_VERSION;
        const OFP14 = 1 << sys::ofp_version_OFP14_VERSION;
        const OFP15 = 1 << sys::ofp_version_OFP15_VERSION;

        /* Bitmaps of OpenFlow versions that Open vSwitch supports,
         * and that it enables by default.  When Open vSwitch has
         * experimental or incomplete support for newer versions of
         * OpenFlow, those versions should not be supported by default
         * and thus should be omitted from the latter bitmap. */
        const SUPPORTED = (Self::OFP10.bits |
                           Self::OFP11.bits |
                           Self::OFP12.bits |
                           Self::OFP13.bits |
                           Self::OFP14.bits |
                           Self::OFP15.bits);
        const DEFAULT = Self::SUPPORTED.bits;
    }
}

impl From<Version> for Versions {
    fn from(v: Version) -> Versions {
        Versions { bits: (1 << (v as isize)) }
    }
}
