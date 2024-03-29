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

/* drop_port0 pipeline for ofp4.
 *
 * Packets that dropped based on the input port.
 */

#include <of_model.p4>

struct metadata_t {}

control PIngress(inout Headers hdr,
                 out metadata_t meta,
                 in input_metadata_t meta_in,
                 inout ingress_to_arch_t itoa,
                 inout output_metadata_t meta_out) {
    action Drop() {
        meta_out.out_port = 0;
        exit;
    }

    table DropPort {
        key = { meta_in.in_port: exact @name("in_port"); }
        actions = { Drop; NoAction; }
        const default_action = NoAction();
    }

    apply {
        DropPort.apply();
    }
}

control PEgress(inout Headers hdr,
                inout metadata_t meta,
                in input_metadata_t meta_in,
                inout output_metadata_t from_ingress) {
    apply {
        // Nothing to do.
    }
}

OfSwitch (
    PIngress(),
    PEgress()
) main;
