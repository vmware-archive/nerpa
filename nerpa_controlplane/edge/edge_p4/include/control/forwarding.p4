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

#include <core.p4>
#include <v1model.p4>

#include "../define.p4"
#include "../header.p4"
#include "../size.p4"

control Forwarding(inout parsed_headers_t hdr,
                   inout edge_metadata_t edge_metadata,
                   inout standard_metadata_t standard_metadata) {
    @hidden
    action set_next_id(next_id_t next_id) {
        edge_metadata.next_id = next_id;
    }

    // Bridging Table.
    direct_counter(CounterType.packets_and_bytes) bridging_counter;

    action set_next_id_bridging(next_id_t next_id) {
        set_next_id(next_id);
        bridging_counter.count();
    }

    // The Fabric codebase says that using ternary for eth_dst can stop
    // scaling in bridging heavy environments. Not sure if this affects us.
    table bridging {
        key = {
            edge_metadata.vlan_id: exact @name("vlan_id");
            hdr.ethernet.dst_addr: ternary @name("eth_dst");
        }
        actions = {
            set_next_id_bridging;
            @defaultonly nop;
        }
        const default_action = nop();
        counters = bridging_counter;
        size = BRIDGING_TABLE_SIZE;
    }

    /* MPLS Table. */
    direct_counter(CounterType.packets_and_bytes) mpls_counter;

    action pop_mpls_and_next(next_id_t next_id) {
        edge_metadata.mpls_label = 0;
        set_next_id(next_id);
        mpls_counter.count();
    }

    table mpls {
        key = {
            edge_metadata.mpls_label: exact @name("mpls_label");
        }
        actions = {
            pop_mpls_and_next;
            @defaultonly nop;
        }
        const default_action = nop();
        counters = mpls_counter;
        size = MPLS_TABLE_SIZE;
    }

    /* IPv4 Routing. */
    direct_counter(CounterType.packets_and_bytes) routing_v4_counter;

    action set_next_id_routing_v4(next_id_t next_id) {
        set_next_id(next_id);
        routing_v4_counter.count();
    }

    action nop_routing_v4() {
        routing_v4_counter.count();
    }

    table routing_v4 {
        key = {
            edge_metadata.ipv4_dst_addr: lpm @name("ipv4_dst");
        }
        actions = {
            set_next_id_routing_v4;
            nop_routing_v4;
            @defaultonly nop;
        }
        default_action = nop();
        counters = routing_v4_counter;
        size = ROUTING_V4_TABLE_SIZE;
    }

    /* IPv6 Routing Table. */
    direct_counter(CounterType.packets_and_bytes) routing_v6_counter;

    action set_next_id_routing_v6(next_id_t next_id) {
        set_next_id(next_id);
        routing_v6_counter.count();
    }

    table routing_v6 {
        key = {
            hdr.ipv6.dst_addr: lpm @name("ipv6_dst");
        }
        actions = {
            set_next_id_routing_v6;
            @defaultonly nop;
        }
        const default_action = nop();
        counters = routing_v6_counter;
        size = ROUTING_V6_TABLE_SIZE;
    }

    apply {
        if (edge_metadata.fwd_type == FWD_BRIDGING) bridging.apply();
        else if (edge_metadata.fwd_type == FWD_MPLS) mpls.apply();
        else if (edge_metadata.fwd_type == FWD_IPV4_UNICAST) routing_v4.apply();
        else if (edge_metadata.fwd_type == FWD_IPV6_UNICAST) routing_v6.apply();
    }
} 