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

@controller_header("packet_in")
header Packet_in_h {
    bit<9> ingressPort;
    bit<7> pad;
}

// Used for table lookup.
// Initialized with parsed headers, 0 if invalid.
// Not updated by the pipe.
// If both outer and inner IPv4 headers are valid,
// this should carry the inner ones.
struct lookup_metadata_t {
    bool isIpv4;
    bit<32> ipv4Src;
    bit<32> ipv4Dst;
    bit<8> ipProto;
    bit<16> l4SrcPort;
    bit<16> l4DstPort;
    bit<8> icmpType;
    bit<8> icmpCode; 
}

header ethernet_t {
    bit<48> dstAddr;
    bit<48> srcAddr;
}

header vlan_tag_t {
    bit<16> ethType;
    bit<3> pri;
    bit<1> cfi;
    vlan_id_t vlanId;
}

header mpls_t {
    bit<20> label;
    bit<3> tc;
    bit<1> bos;
    bit<8> ttl;
}

header ipv4_t {
    bit<4> version;
    bit<4> ihl;
    bit<6> dscp;
    bit<2> ecn;
    bit<16> totalLen;
    bit<16> identification;
    bit<3> flags;
    bit<13> fragOffset;
    bit<8> ttl;
    bit<8> protocol;
    bit<16> hdrChecksum;
    bit<32> srcAddr;
    bit<32> dstAddr;
}

header tcp_t {
    bit<16> sport;
    bit<16> dport;
    bit<32> seqNo;
    bit<32> ackNo;
    bit<4>  dataOffset;
    bit<3>  res;
    bit<3>  ecn;
    bit<6>  ctrl;
    bit<16> window;
    bit<16> checksum;
    bit<16> urgentPtr;
}

header udp_t {
    bit<16> sport;
    bit<16> dport;
    bit<16> len;
    bit<16> checksum;
}

header eth_type_t {
    bit<16> value;
}

header icmp_t {
    bit<8> icmpType;
    bit<8> icmpCode;
    bit<16> checksum;
    bit<16> identifier;
}

header gtpu_t {
    bit<3> version;
    bit<1> pt;
    bit<1> spare;
    bit<1> exFlag;
    bit<1> seqFlag;
    bit<1> npduFlag;
    bit<8> msgType;
    bit<16> msgLen;
    teid_t teid;
}

// Custom metadata struct
struct ovn_metadata_t {
    lookup_metadata_t lkp;
    bit<16> ipEthType;
    vlan_id_t vlanId;
    mpls_label_t mplsLabel;
    bit<8> mplsTtl;
    bool skip_next;
    next_id_t nextId;
    bit<8> ipProto;
    bit<16> l4SrcPort;
    bit<16> l4DstPort;
    bit<32> ipv4Src;
    bit<32> ipv4Dst;
    port_type_t portType;
}

// Parsed headers
struct parsed_headers_t {
    ethernet_t ethernet;
    vlan_tag_t vlanTag;
    eth_type_t ethType;
    icmp_t icmp;
    mpls_t mpls;
    gtpu_t gtpu;
    ipv4_t ipv4;
    tcp_t tcp;
    udp_t udp;
}