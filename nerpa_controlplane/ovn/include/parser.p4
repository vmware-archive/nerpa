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

#ifndef __PARSER__
#define __PARSER__

#include "define.p4"

parser OvnParser(
    packet_in packet,
    out parsed_headers_t hdr,
    inout ovn_metadata_t meta,
    inout standard_metadata_t std_meta
) {
    state start {
        transition parse_ethernet;
    }

    state parse_ethernet {
        packet.extract(hdr.ethernet);
        // meta.vlanId = DEFAULT_VLAN_ID;
        meta.vlanId = DEFAULT_VLAN_ID;
        transition select(packet.lookahead<bit<16>>()) {
            ETHERTYPE_QINQ: parse_vlan_tag;
            ETHERTYPE_QINQ_NON_STD: parse_vlan_tag;
            ETHERTYPE_VLAN: parse_vlan_tag;
            default: parse_eth_type;
        }
    }

    state parse_vlan_tag {
        packet.extract(hdr.vlanTag);
        transition select(packet.lookahead<bit<16>>()) {
            default: parse_eth_type;
        }
    }

    state parse_eth_type {
        packet.extract(hdr.ethType);
        transition select(hdr.ethType.value) {
            ETHERTYPE_MPLS: parse_mpls;
            ETHERTYPE_IPV4: parse_ipv4;
            default: accept;
        }
    }

    state parse_mpls {
        packet.extract(hdr.mpls);
        meta.mplsLabel = hdr.mpls.label;
        meta.mplsTtl = hdr.mpls.ttl;

        // This only has one MPLS label.
        // Assume header after MPLS is IPv4/IPv6
        // Lookup the first 4 bits for the version
        transition select(packet.lookahead<bit<4>>()) {
            // Only handle IPv4 for now
            IP_VERSION_4: parse_ipv4;
            default: parse_ethernet;
        }
    }

    state parse_ipv4 {
        packet.extract(hdr.ipv4);
        meta.ipProto = hdr.ipv4.protocol;
        meta.ipEthType = ETHERTYPE_IPV4;
        meta.ipv4Src = hdr.ipv4.srcAddr;
        meta.ipv4Dst = hdr.ipv4.dstAddr;
        
        // TODO: Assign last_ipv4_dscp.

        transition select(hdr.ipv4.protocol) {
            PROTO_TCP: parse_tcp;
            PROTO_UDP: parse_udp;
            PROTO_ICMP: parse_icmp;
            default: accept;
        }
    }

    state parse_tcp {
        packet.extract(hdr.tcp);
        meta.l4SrcPort = hdr.tcp.sport;
        meta.l4DstPort = hdr.tcp.dport;
        transition accept;
    }

    state parse_udp {
        packet.extract(hdr.udp);
        meta.l4SrcPort = hdr.udp.sport;
        meta.l4DstPort = hdr.udp.dport;

        gtpu_t gtpu = packet.lookahead<gtpu_t>();
        transition select(hdr.udp.dport, gtpu.version, gtpu.msgType) {
            // Treat the GTP control traffic as payload.
            (UDP_PORT_GTPU, GTP_V1, GTP_GPDU): parse_gtpu;
            default: accept;
        }
    }

    state parse_icmp {
        packet.extract(hdr.icmp);
        transition accept;
    }

    // TODO: Fully implement.
    state parse_gtpu {
        packet.extract(hdr.gtpu);
        transition accept;
    }
}

#endif
