/*
Copyright (c) 2021 VMware, Inc.
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

/* OVN v2.5 implementation */
#include <core.p4>
#include <v1model.p4>

struct metadata {}

struct headers {}

parser OvnParser(
    packet_in packet,
    out headers hdr,
    inout metadata meta,
    inout standard_metadata_t standard_metadata
) {
    state start {
        transition accept;
    }
}

control OvnVerifyChecksum(
    inout headers hdr,
    inout metadata meta
) {
    apply {}
}

// TODO: Understand how to incorporate the logical router tables.
// We've only incorporated the logical switch tables thus far.
control OvnIngress(
    inout headers hdr,
    inout metadata meta,
    inout standard_metadata_t standard_metadata
) {    
    apply {
        // TODO: Table 0 - admission control framework.

        // TODO: Table 0 - ingress port security.

        // TODO: Table 1 - pre-ACL.

        // TODO: Table 2 - ACL.

        // TODO: Table 3 - destination lookup, broadcast and multicast handling.

        // TODO: Table 3 - destination lookup, unicast handling.

        // TODO: Table 3 - destination lookup, unknown MACs.
    }
}

// TODO: Understand how to incorporate the logical router tables.
// We've only incorporated the logical switch tables thus far.
control OvnEgress(
    inout headers hdr,
    inout metadata meta,
    inout standard_metadata_t standard_metadata
) {
    apply {
        // TODO: Table 0 - pre-ACL.

        // TODO: Table 1 - ACL.
        
        // TODO: Table 2 - egress port security.
    }
}

control OvnComputeChecksum(
    inout headers hdr,
    inout metadata meta
) {
    apply {}
}

control OvnDeparser(
    packet_out packet,
    in headers hdr
) {
    apply {
        packet.emit(hdr);
    }
}

V1Switch (
    OvnParser(),
    OvnVerifyChecksum(),
    OvnIngress(),
    OvnEgress(),
    OvnComputeChecksum(),
    OvnDeparser()
) main;