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

#include <core.p4>
#include <v1model.p4>

#include "../define.p4"
#include "../header.p4"

control Acl(
    inout parsed_headers_t hdr,
    inout ovn_metadata_t meta,
    inout standard_metadata_t std_meta
) {

    direct_counter(CounterType.packets_and_bytes) AclCounter;

    action SetNextIdAcl(next_id_t nextId) {
        meta.nextId = nextId;
        AclCounter.count();
    }

    // Send to CPU immediately. Skip the rest of ingress.
    action PuntToCpu() {
        std_meta.egress_spec = CPU_PORT;
        meta.skip_next = true;
        AclCounter.count();
    }

    // Set clone session ID for an I2E clone session.
    action SetCloneSessionId(bit<32> clone_id) {
        clone3(CloneType.I2E, clone_id, {std_meta.ingress_port});
        AclCounter.count();
    }

    action Drop() {
        mark_to_drop(std_meta);
        meta.skip_next = true;
        AclCounter.count();
    }

    action NoOpAcl() {
        AclCounter.count();
    }

    table ACL {
        key = {
            std_meta.ingress_port: ternary @name("port");
            hdr.ethernet.dstAddr: ternary @name("dstAddr");
            hdr.ethernet.srcAddr: ternary @name("srcAddr");
            hdr.vlanTag.vlanId: ternary @name("vlanId");
            hdr.ethType.value: ternary @name("ethType");
            meta.lkp.ipv4Src: ternary @name("ipv4Src");
            meta.lkp.ipv4Dst: ternary @name("ipv4Dst");
            meta.lkp.ipProto: ternary @name("ipProto");
            hdr.icmp.icmpType: ternary @name("icmpType");
            hdr.icmp.icmpCode: ternary @name("icmpCode");
            meta.lkp.l4SrcPort: ternary @name("l4SrcPort");
            meta.lkp.l4DstPort: ternary @name("l4DstPort");
            meta.portType: ternary @name("portType");
        }

        actions = {
            SetNextIdAcl;
            PuntToCpu;
            SetCloneSessionId;
            Drop;
            NoOpAcl;
        }

        const default_action = NoOpAcl();
        // size = ACL_TABLE_SIZE;
        counters = AclCounter;
    }
    
    apply {
        ACL.apply();
    }
}