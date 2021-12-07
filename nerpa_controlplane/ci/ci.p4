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

/* CI pipeline program. */
#include <core.p4>
#include <v1model.p4>

typedef bit<12> VlanID;

struct metadata {
    VlanID vlan;
}
struct headers {}

parser CiParser(packet_in packet,
                out headers hdr,
                inout metadata meta,
                inout standard_metadata_t standard_metadata) {
    state start {
        transition accept;
    }
}

control CiVerifyChecksum(inout headers hdr, inout metadata meta) {
    apply {}
}

control CiIngress(inout headers hdr,
                  inout metadata meta,
                  inout standard_metadata_t standard_metadata) {
    apply {}
}

control CiEgress(inout headers hdr,
                 inout metadata meta,
                 inout standard_metadata_t standard_metadata) {
    // Output VLAN processing.
    table OutputVlan {
        key = {
            standard_metadata.egress_port: exact @name("port");
            meta.vlan: optional @name("vlan");
        }
        actions = { NoAction; }
    }

    apply {
        OutputVlan.apply();
    }
}

control CiComputeChecksum(inout headers hdr, inout metadata meta) {
    apply {}
}

control CiDeparser(packet_out packet, in headers hdr) {
    apply {
        packet.emit(hdr);
    }
}

V1Switch (
    CiParser(),
    CiVerifyChecksum(),
    CiIngress(),
    CiEgress(),
    CiComputeChecksum(),
    CiDeparser()
) main;
