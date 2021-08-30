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

#ifndef __PARSER__
#define __PARSER__

#include "headers.p4"

parser EdgeParser(packet_in packet,
                  out parsed_headers_t hdr,
                  inout edge_metadata_t edge_metadata,
                  inout standard_metadata_t standard_metadata) {
    state start {
        transition select(standard_metadata.ingress_port) {
            CPU_PORT: check_packet_out;
            default: parse_ethernet;
        }
    }

    state check_packet_out {
        packet_out_header_t tmp = packet.lookahead<packet_out_header_t>();
        transition select(tmp.do_forwarding) {
            0: parse_packet_out_and_accept;
            default: strip_packet_out;
        }
    }

    state parse_packet_out_and_accept {
        // Transmit as-is over requested egress port.
        packet.extract(hdr.packet_out);
        transition accept;
    }

    state strip_packet_out {
        // Remove packet-out header and process as regular packet.
        packet.advance(PACKET_OUT_HDR_SIZE * 8);
        transition parse_ethernet;
    }

    state parse_ethernet {
        packet.extract(hdr.ethernet);
        edge_metadata.vlan_id = DEFAULT_VLAN_ID;
        transition select(packet.lookahead<bit<16>>()) {
            ETHERTYPE_QINQ: parse_vlan_tag;
            ETHERTYPE_QINQ_NON_STD: parse_vlan_tag;
            ETHERTYPE_VLAN: parse_vlan_tag;
            default: parse_eth_type;
        }
    }

    state parse_vlan_tag {
        packet.extract(hdr.vlan_tag);
        transition select(packet.lookahead<bit<16>>()){
            ETHERTYPE_VLAN: parse_inner_vlan_tag;
            default: parse_eth_type;
        }
    }

    state parse_inner_vlan_tag {
        packet.extract(hdr.inner_vlan_tag);
        transition parse_eth_type;
    }

    state parse_eth_type {
        packet.extract(hdr.eth_type);
        transition select(hdr.eth_type.value) {
            ETHERTYPE_MPLS: parse_mpls;
            ETHERTYPE_ARP: parse_arp;
            ETHERTYPE_IPV4: parse_ipv4;
            ETHERTYPE_IPV6: parse_ipv6;
            default: accept;
        }
    }

    state parse_mpls {
        packet.extract(hdr.mpls);
        edge_metadata.mpls_label = hdr.mpls.label;
        edge_metadata.mpls_ttl = hdr.mpls.ttl;
        
        // There seems to be only one MPLS label in an edge packet.
        // Assume the next header is IPv4 or IPv6. We may have to move this after ARP.
        // Lookup the first 4 bits for version.
        transition select(packet.lookahead<bit<IP_VER_LENGTH>>()) {
            IP_VERSION_4: parse_ipv4;
            IP_VERSION_6: parse_ipv6;
            default: parse_ethernet;
        }
    }

    state parse_arp {
        packet.extract(hdr.arp);

        // TODO: Assign any needed ARP metadata.

        transition accept;
    }

    state parse_ipv4 {
        packet.extract(hdr.ipv4);
        edge_metadata.ip_proto = hdr.ipv4.protocol;
        edge_metadata.ip_eth_type = ETHERTYPE_IPV4;
        edge_metadata.ipv4_src_addr = hdr.ipv4.src_addr;
        edge_metadata.ipv4_dst_addr = hdr.ipv4.dst_addr;

        // TODO: Parse based on protocol.

        transition accept;
    }

    state parse_ipv6 {
        packet.extract(hdr.ipv6);
        edge_metadata.ip_proto = hdr.ipv6.next_hdr;
        edge_metadata.ip_eth_type = ETHERTYPE_IPV6;

        // TODO: Parse based on protocol.

        transition accept;
    }
}

control EdgeDeparser(packet_out packet, in parsed_headers_t hdr) {
    apply {
        packet.emit(hdr.ethernet);
        packet.emit(hdr.vlan_tag);
        packet.emit(hdr.inner_vlan_tag);
        packet.emit(hdr.eth_type);
        packet.emit(hdr.mpls);
        packet.emit(hdr.arp);
        packet.emit(hdr.ipv4);
        packet.emit(hdr.ipv6);
    }
}

#endif
