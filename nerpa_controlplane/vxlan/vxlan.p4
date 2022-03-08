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

/* Headers */
header Ethernet_t {
    bit<48> dstAddr;
    bit<48> srcAddr;
    bit<16> etherType;
}

header Ipv4_t {
    bit<4>  version;
    bit<4>  ihl;
    bit<8>  diffserv;
    bit<16> totalLen;
    bit<16> identification;
    bit<3>  flags;
    bit<13> fragOffset;
    bit<8>  ttl;
    bit<8>  protocol;
    bit<16> hdrChecksum;
    bit<32> srcAddr;
    bit<32> dstAddr;
}

header Udp_t {
    bit<16> srcPort;
    bit<16> dstPort;
    bit<16> length;
    bit<16> checksum;
}

header Vxlan_t {
    bit<8>  flags;
    bit<24> reserved;
    bit<24> vni;
    bit<8>  reserved_2;
}

// TODO: Determine the form of packet out from host to switch.
header Packetout_t {}

struct headers {
    @name("ethernet")
    Ethernet_t ethernet;

    @name("ipv4")
    Ipv4_t ipv4;

    @name("udp")
    Udp_t udp;

    @name("vxlan")
    Vxlan_t vxlan;

    Ethernet_t inner_ethernet;
    Ipv4_t inner_ipv4;

    @name("packet_out")
    Packetout_t packet_out;
}

struct metadata {
    bit<24> vxlan_vni;
    bit<32> nexthop;
    bit<32> vtepIP;
}

const bit<9> CPU_PORT = 510;

/* Parsers */
#define ETH_HDR_SIZE 14
#define IPV4_HDR_SIZE 20
#define UDP_HDR_SIZE 8
#define VXLAN_HDR_SIZE 8

#define UDP_PORT_VXLAN 4789
#define UDP_PROTO 17
#define TYPE_IPV4 0x800

#define IP_VERSION_4 4
#define IPV4_MIN_IHL 5

parser VxlanParser(
    packet_in packet,
    out headers hdr,
    inout metadata meta,
    inout standard_metadata_t standard_metadata
) {
    // TODO: Add ARP states.
    state start {
        transition select(standard_metadata.ingress_port) {
            CPU_PORT: parse_packet_out;
            default: parse_ethernet;
        }
    }

    state parse_packet_out {
        packet.extract(hdr.packet_out);
        transition parse_ethernet;
    }

    state parse_ethernet {
        packet.extract(hdr.ethernet);
        transition select(hdr.ethernet.etherType) {
            TYPE_IPV4: parse_ipv4;
            default: accept;
        }
    }

    state parse_ipv4 {
        packet.extract(hdr.ipv4);
        transition select(hdr.ipv4.protocol) {
            UDP_PROTO: parse_vxlan;
            default: accept;
        }
    }

    state parse_vxlan {
        packet.extract(hdr.vxlan);
        transition parse_inner_ethernet;
    }

    state parse_inner_ethernet {
        packet.extract(hdr.inner_ethernet);
        transition select(hdr.ethernet.etherType) {
            TYPE_IPV4: parse_inner_ipv4;
            default: accept;
        }
    }

    state parse_inner_ipv4 {
        packet.extract(hdr.inner_ipv4);
        transition accept;
    }
}

control VxlanIngress(
    inout headers hdr,
    inout metadata meta,
    inout standard_metadata_t standard_metadata
) {
    action VxlanDecap() {
        // Set outer headers as invalid.
        hdr.ethernet.setInvalid();
        hdr.ipv4.setInvalid();
        hdr.udp.setInvalid();
        hdr.vxlan.setInvalid();
    }

    table VxlanTerm {
        key = {
            hdr.inner_ethernet.dstAddr: exact @name("dst");
        }

        actions = {
            @defaultonly NoAction;
            VxlanDecap();
        }
    }

    action Forward(bit<9> port) {
        standard_metadata.egress_spec = port;
    }

    table L2Forward {
        key = {
            hdr.inner_ethernet.dstAddr: exact @name("dst");
        }

        actions = {
            Forward;
        }
    }

    action SetVni(bit<24> vni) {
        meta.vxlan_vni = vni;
    }

    action SetIpv4Nexthop(bit<32> nexthop) {
        meta.nexthop = nexthop;
    }

    table VxlanSegment {
        key = {
            hdr.ipv4.dstAddr: lpm @name("dst");
        }

        actions = {
            @defaultonly NoAction;
            SetVni;
        }
    }

    table VxlanNexthop {
        key = {
            hdr.ethernet.dstAddr: exact @name("dst");
        }

        actions = {
            SetIpv4Nexthop;
        }
    }

    action SetVtepIp(bit<32> vtep_ip) {
        meta.vtepIP = vtep_ip;
    }

    table Vtep {
        key = {
            hdr.ethernet.srcAddr: exact @name("src");
        }

        actions = {
            SetVtepIp;
        }
    }

    action Route(bit<9> port) {
        standard_metadata.egress_spec = port;
    }

    table VxlanRouting {
        key = {
            meta.nexthop: exact @name("nexthop");
        }

        actions = {
            Route;
        }
    }

    apply {
        if (!hdr.ipv4.isValid()) {
            return;
        }

        if (hdr.vxlan.isValid()) {
            if (VxlanTerm.apply().hit) {
                L2Forward.apply();
            }
        } else {
            Vtep.apply();
            if(VxlanSegment.apply().hit) {
                if(VxlanNexthop.apply().hit) {
                    VxlanRouting.apply();
                }
            }
        }
    }
}

control VxlanEgress(
    inout headers hdr,
    inout metadata meta,
    inout standard_metadata_t standard_metadata
) {
    action RewriteMacs(bit<48> smac, bit<48> dmac) {
        hdr.ethernet.srcAddr = smac;
        hdr.ethernet.dstAddr = dmac;
    }

    table SendFrame {
        key = {
            hdr.ipv4.dstAddr: exact @name("dst");
        }

        actions = {
            RewriteMacs;
        }
    }

    action VxlanEncap() {
        // Set the inner headers.
        hdr.inner_ethernet = hdr.ethernet;
        hdr.inner_ipv4 = hdr.ipv4;

        hdr.ethernet.setValid();

        // Define the IPv4 header.
        hdr.ipv4.setValid();
        hdr.ipv4.version = IP_VERSION_4;
        hdr.ipv4.ihl = IPV4_MIN_IHL;
        hdr.ipv4.diffserv = 0;
        hdr.ipv4.totalLen = hdr.ipv4.totalLen + (ETH_HDR_SIZE + IPV4_HDR_SIZE + UDP_HDR_SIZE + VXLAN_HDR_SIZE);
        hdr.ipv4.identification = 0x1513; // From NGIC
        hdr.ipv4.flags = 0;
        hdr.ipv4.fragOffset = 0;
        hdr.ipv4.ttl = 64;
        hdr.ipv4.protocol = UDP_PROTO;
        hdr.ipv4.dstAddr = meta.nexthop;
        hdr.ipv4.srcAddr = meta.vtepIP;
        hdr.ipv4.hdrChecksum = 0;

        // Define the UDP header.
        hdr.udp.setValid();
        // The VTEP calculates the source port by performing the hash of the inner Ethernet frame's header.
        hash(hdr.udp.srcPort, HashAlgorithm.crc16, (bit<13>)0, { hdr.inner_ethernet }, (bit<32>)65536);
        hdr.udp.dstPort = UDP_PORT_VXLAN;
        hdr.udp.length = hdr.ipv4.totalLen + (UDP_HDR_SIZE + VXLAN_HDR_SIZE);
        hdr.udp.checksum = 0;

        // Define VXLAN header.
        hdr.vxlan.setValid();
        hdr.vxlan.reserved = 0;
        hdr.vxlan.reserved_2 = 0;
        hdr.vxlan.flags = 0;
        hdr.vxlan.vni = meta.vxlan_vni;
    }

    apply {
        if (!hdr.vxlan.isValid() && meta.vxlan_vni != 0) {
            VxlanEncap();
            if (hdr.vxlan.isValid()) {
                SendFrame.apply();
            }
        }
    }
}

control verifyChecksum(inout headers hdr, inout metadata meta) {
    apply {}
}

control computeChecksum(inout headers hdr, inout metadata meta) {
    apply {}
}

control VxlanDeparser(
    packet_out packet,
    in headers hdr
) {
    apply {
        packet.emit(hdr.ethernet);
        packet.emit(hdr.ipv4);
        packet.emit(hdr.udp);
        packet.emit(hdr.vxlan);
        packet.emit(hdr.inner_ethernet);
        packet.emit(hdr.inner_ipv4);
    }
}

V1Switch(
    VxlanParser(),
    verifyChecksum(),
    VxlanIngress(),
    VxlanEgress(),
    computeChecksum(),
    VxlanDeparser()
) main;