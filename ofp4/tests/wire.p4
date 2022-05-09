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

/* "Wire" pipeline for ofp4.
 *
 * This implements a very simple P4 program.  Packets that arrive on port 1
 * are output to port 2, and vice versa.  Other packets are dropped.
 */

#include <of_model.p4>

struct metadata_t {
    bit<32> b;
    bit<8>  c;
    bit<8>  d;
}

control WireIngress(inout Headers hdr,
                    inout metadata_t meta,
                    inout standard_metadata_t std) {

    action SetOutPort(PortID port) {
        std.out_port = port;
    }

    table MapPorts {
        key = { std.in_port: exact @name("in_port"); }
        actions = { SetOutPort; }
        const entries = {
            1: SetOutPort(2);
            2: SetOutPort(1);
        }
    }

    apply {
        MapPorts.apply();
    }
}

control WireEgress(inout Headers hdr,
                   inout metadata_t meta,
                   inout standard_metadata_t standard_metadata) {
    apply {
        // Nothing to do.
    }
}

OfSwitch (
    WireIngress(),
    WireEgress()
) main;
