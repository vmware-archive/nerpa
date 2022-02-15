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

const bit<16> TYPE_ARP = 0x0806;
const bit<16> TYPE_CPU_METADATA = 0x081b;
const bit<16> TYPE_IPV4 = 0x800;

const bit<9> CPU_PORT = 510;

const bit<16> ARP_OP_REQ = 0x0001;
const bit<16> ARP_OP_REPLY = 0x002;

header Ethernet_t {
    bit<48> dstAddr;
    bit<48> srcAddr;
    bit<16> etherType;
}

header Cpu_metadata_t {
    bit<8> fromCpu;
    bit<16> origEtherType;
    bit<16> srcPort;
}

header Ipv4_t {
    bit<4>    version;
    bit<4>    ihl;
    bit<8>    diffserv;
    bit<16>   totalLen; // Update length.
    bit<16>   identification;
    bit<3>    flags;
    bit<13>   fragOffset;
    bit<8>    ttl;
    bit<8>    protocol;
    bit<16>   hdrChecksum; // Update this after cache hit.
    bit<32> srcAddr; // Swap source and destinction MAC addresses
    bit<32> dstAddr; //  after the cache hit.
}

header Arp_t {
    bit<16> hwType;
    bit<16> protoType;
    bit<8> hwAddrLen;
    bit<8> protoAddrLen;
    bit<16> opcode;
    // assumes hardware type is ethernet and protocol is IP
    bit<48> srcEth;
    bit<32> srcIP;
    bit<48> dstEth;
    bit<32> dstIP;
}

@controller_header("packet_in")
header Packetin_t {
    bit<48> mac;
    bit<32> ip;
    bit<9> port;
    bit<7> pad;
    bit<16> opcode;
}

@controller_header("packet_out")
header Packetout_t {
    bit<9> port;
    bit<7> pad;
}

struct headers {
    Ethernet_t ethernet;
    Cpu_metadata_t cpu_metadata;
    Ipv4_t ipv4;
    Arp_t arp;
    Packetin_t packet_in;
    Packetout_t packet_out;
}

struct metadata {}

parser ArpParser(
    packet_in packet,
    out headers hdr,
    inout metadata meta,
    inout standard_metadata_t standard_metadata
) {
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
            TYPE_ARP: parse_arp;
            TYPE_CPU_METADATA: parse_cpu_metadata;
            TYPE_IPV4: parse_ipv4;
            default: accept;
        }
    }

    state parse_arp {
        packet.extract(hdr.arp);
        transition accept;
    }

    state parse_cpu_metadata {
        packet.extract(hdr.cpu_metadata);
        transition select(hdr.cpu_metadata.origEtherType) {
            TYPE_ARP: parse_arp;
            TYPE_IPV4: parse_ipv4;
            default: accept;
        }
    }

    state parse_ipv4 {
        packet.extract(hdr.ipv4);
        transition accept;
    }
}

control ArpVerifyChecksum(
    inout headers hdr,
    inout metadata meta
) {
    apply {}
}

control ArpIngress(
    inout headers hdr,
    inout metadata meta,
    inout standard_metadata_t standard_metadata
) {
    action Drop() {
        mark_to_drop(standard_metadata);
    }

    action SetEgressPort(bit<9> port) {
        standard_metadata.egress_spec = port;
    }

    action CpuMetadataEncap() {
        hdr.cpu_metadata.setValid();
        hdr.cpu_metadata.origEtherType = hdr.ethernet.etherType;
        hdr.cpu_metadata.srcPort = (bit<16>) standard_metadata.ingress_port;
        hdr.ethernet.etherType = TYPE_CPU_METADATA;
    }

    action CpuMetadataDecap() {
        hdr.ethernet.etherType = hdr.cpu_metadata.origEtherType;
        hdr.cpu_metadata.setInvalid();
    }

    action SendToCpu() {
        CpuMetadataEncap();
        standard_metadata.egress_spec = CPU_PORT;

        hdr.packet_in.port = standard_metadata.ingress_port;
        hdr.packet_in.mac = hdr.arp.srcEth;
        hdr.packet_in.ip = hdr.arp.srcIP;
        hdr.packet_in.opcode = hdr.arp.opcode;
    }

    action ArpReply(bit<48> mac) {
        // Set the ethernet source to the actual host MAC address and destination to the original requester.
        hdr.ethernet.dstAddr = hdr.ethernet.srcAddr;
        hdr.ethernet.srcAddr = mac;

        // Set source and destination MAC and IP addresses appropriately.
        bit<48> tmpSrcEth = hdr.arp.srcEth;
        bit<32> tmpSrcIP = hdr.arp.srcIP;
        hdr.arp.srcEth = mac;
        hdr.arp.srcIP = hdr.arp.dstIP;
        hdr.arp.dstEth = tmpSrcEth;
        hdr.arp.dstIP = tmpSrcIP;

        // Change ARP header from request to reply.
        hdr.arp.opcode = ARP_OP_REPLY;

        // Send the packet back out on the physical port it arrived on.
        standard_metadata.egress_spec = standard_metadata.ingress_port;
    }

    table Arp {
        key = {
            hdr.arp.dstIP: lpm @name("dst");
        }

        actions = {
            SendToCpu;
            ArpReply;
            Drop;
            NoAction;
        }

        size = 64;
        default_action = SendToCpu();
    }

    action IPv4Route(bit<9> port) {
        standard_metadata.egress_spec = port;
        hdr.ipv4.ttl = hdr.ipv4.ttl - 1;
    }

    action IPv4Forward(bit<48> dstAddr, bit<9> port) {
        standard_metadata.egress_spec = port;
        hdr.ethernet.dstAddr = dstAddr;
        hdr.ipv4.ttl = hdr.ipv4.ttl - 1;
    }

    table IPv4Lpm {
        key = {
            hdr.ipv4.dstAddr: lpm @name("dst");
        }

        actions = {
            IPv4Route;
            IPv4Forward;
            SendToCpu;
            Drop;
            NoAction;
        }

        size = 1024;
        default_action = SendToCpu();
    }

    action SetEgress(bit<9> port) {
        standard_metadata.egress_spec = port;
    }

    action SetMulticastGroup(bit<16> mgid) {
        standard_metadata.mcast_grp = mgid;
    }

    table ForwardL2 {
        key = {
            hdr.ethernet.dstAddr: exact @name("dst");
        }

        actions = {
            SetEgress;
            SetMulticastGroup;
            SendToCpu;
            Drop;
            NoAction;
        }

        size = 1024;
        default_action = NoAction;
    }

    apply {
        if (standard_metadata.ingress_port == CPU_PORT) {
            CpuMetadataDecap();
        }
        // Apply the ARP table for an ARP request.
        else if (hdr.arp.isValid() && hdr.arp.opcode == ARP_OP_REQ && standard_metadata.ingress_port != CPU_PORT) {
            Arp.apply();
        }
        // Send any different type of ARP packet to the CPU.
        else if (hdr.arp.isValid() && standard_metadata.ingress_port != CPU_PORT) {
            SendToCpu();
        }
        // Apply the Ipv4 table for an IPv4 packet.
        else if (hdr.ipv4.isValid()) {
            IPv4Lpm.apply();
        }
        // Apply the L2 forwarding table for an ethernet packet.
        else if (hdr.ethernet.isValid() && !hdr.ipv4.isValid()) {
            ForwardL2.apply();
        }
    }
}

control ArpEgress(
    inout headers hdr,
    inout metadata meta,
    inout standard_metadata_t standard_metadata
) {
    apply {}
}

control ArpComputeChecksum(
    inout headers hdr,
    inout metadata meta
) {
    apply {
        update_checksum(
            hdr.ipv4.isValid(),
            {
                hdr.ipv4.version,
                hdr.ipv4.ihl,
                hdr.ipv4.diffserv,
                hdr.ipv4.totalLen,
                hdr.ipv4.identification,
                hdr.ipv4.flags,
                hdr.ipv4.fragOffset,
                hdr.ipv4.ttl,
                hdr.ipv4.protocol,
                hdr.ipv4.srcAddr,
                hdr.ipv4.dstAddr
            },
            hdr.ipv4.hdrChecksum,
            HashAlgorithm.csum16
        );
    }
}

control ArpDeparser(
    packet_out packet,
    in headers hdr
) {
    apply {
        packet.emit(hdr.ethernet);
        packet.emit(hdr.cpu_metadata);
        packet.emit(hdr.arp);
        packet.emit(hdr.ipv4);
    }
}

V1Switch(
    ArpParser(),
    ArpVerifyChecksum(),
    ArpIngress(),
    ArpEgress(),
    ArpComputeChecksum(),
    ArpDeparser()
) main;