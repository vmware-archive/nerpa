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

/* NSX Edge pipeline. */
#include <core.p4>
#include <v1model.p4>

#include "include/header.p4"
#include "include/parser.p4"

#include "include/control/forwarding.p4"
#include "include/control/packetio.p4"

control EdgeVerifyChecksum(inout parsed_headers_t hdr,
                           inout edge_metadata_t meta) {
    apply {}
}

control EdgeIngress(inout parsed_headers_t hdr,
                    inout edge_metadata_t edge_metadata,
                    inout standard_metadata_t standard_metadata) {
    PacketIoIngress() pkt_io_ingress;
    Forwarding() forwarding;
    
    apply {
        pkt_io_ingress.apply(hdr, edge_metadata, standard_metadata);
        if (edge_metadata.skip_forwarding == _FALSE) {
            forwarding.apply(hdr, edge_metadata, standard_metadata);
        }
    }
}

control EdgeEgress(inout parsed_headers_t hdr,
                   inout edge_metadata_t meta,
                   inout standard_metadata_t standard_metadata) {
    apply {}
}

control EdgeComputeChecksum(inout parsed_headers_t hdr, inout edge_metadata_t meta) {
    apply {}
}

V1Switch (
    EdgeParser(),
    EdgeVerifyChecksum(),
    EdgeIngress(),
    EdgeEgress(),
    EdgeComputeChecksum(),
    EdgeDeparser()
) main;
