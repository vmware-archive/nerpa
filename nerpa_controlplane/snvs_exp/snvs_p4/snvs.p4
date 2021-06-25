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

/* Simple Nerpa Virtual Switch pipeline. */
#include <core.p4>
#include <v1model.p4>

const bit<16> ETH_VLAN = 0x8100;

typedef bit<48>  EthernetAddress;
typedef bit<32>  IPv4Address;
typedef bit<12>  VlanID;
typedef bit<9>   PortID;
typedef bit<3>   PCP;

// simple_switch.md suggests defining these constants for the values
// of standard_metadata.instance_type.
const bit<32> PKT_INSTANCE_TYPE_NORMAL = 0;
const bit<32> PKT_INSTANCE_TYPE_INGRESS_CLONE = 1;
const bit<32> PKT_INSTANCE_TYPE_EGRESS_CLONE = 2;
const bit<32> PKT_INSTANCE_TYPE_COALESCED = 3;
const bit<32> PKT_INSTANCE_TYPE_INGRESS_RECIRC = 4;
const bit<32> PKT_INSTANCE_TYPE_REPLICATION = 5;
const bit<32> PKT_INSTANCE_TYPE_RESUBMIT = 6;

bool eth_addr_is_multicast(in EthernetAddress a) {
    return (a & (1 << 40)) != 0;
}
    
const PortID DROP_PORT = 511;   // This is meaningful to simple_switch.
const PortID FLOOD_PORT = 510;  // Just an internal constant.

header Ethernet_h {
    EthernetAddress dst;
    EthernetAddress src;
    bit<16>         type;
}

header Vlan_h {
  PCP     pcp;
  bit<1>  dei;
  VlanID  vid;  
  bit<16> type;
}

header Ipv4_h {
    bit<4>       version;
    bit<4>       ihl;
    bit<8>       diffserv;
    bit<16>      totalLen;
    bit<16>      identification;
    bit<3>       flags;
    bit<13>      fragOffset;
    bit<8>       ttl;
    bit<8>       protocol;
    bit<16>      hdrChecksum;
    IPv4Address  srcAddr;
    IPv4Address  dstAddr;
}

struct metadata {
    // The packet's conceptual VLAN, which might not be in a VLAN header.
    VlanID vlan;

    // Whether to flood this packet.
    bool flood;
}

struct headers {
    Ethernet_h eth;
    Vlan_h vlan;
}

parser SnvsParser(packet_in packet,
                  out headers hdr,
                  inout metadata meta,
                  inout standard_metadata_t standard_metadata) {
    state start {
        packet.extract(hdr.eth);
        transition select(hdr.eth.type) {
            ETH_VLAN: parse_vlan;
        }
    }

    state parse_vlan {
        packet.extract(hdr.vlan);
        transition accept;
    }
}

control SnvsVerifyChecksum(inout headers hdr, inout metadata meta) {
    apply {}
}

// MAC learning table.
//
// Each entry is 48 bits of Ethernet address, 12 bits of VLAN,
// and 9 bits of port number.  There's no expiration currently.
typedef bit<(48 + 12 + 9)> Mac_register_entry;
const bit<32> N_MAC_ENTRIES = 4096;
struct Mac_entry {
    EthernetAddress addr;
    VlanID vlan;
    PortID port;
}
void hash_mac_entry(in EthernetAddress addr, in VlanID vlan,
                    out bit<32> i0, out bit<32> i1)
{
    hash(i0, HashAlgorithm.crc32, 32w0, { addr, vlan, 1w0 }, N_MAC_ENTRIES);
    hash(i1, HashAlgorithm.crc32, 32w0, { addr, vlan, 1w1 }, N_MAC_ENTRIES);
}
void get_mac_entry(register<Mac_register_entry> reg,
                   in bit<32> index,
                   out Mac_entry me)
{
    Mac_register_entry me_raw;
    reg.read(me_raw, index);
    me.addr = (EthernetAddress) (me_raw >> (12 + 9));
    me.vlan = (VlanID) (me_raw >> 9);
    me.port = (PortID) me_raw;
}
void hash_and_get_mac_entries(in EthernetAddress addr,
    in VlanID vlan,
    register<Mac_register_entry> mac_table,
    out bit<32> i0,
    out bit<32> i1,
    out Mac_entry b0,
    out Mac_entry b1)
{
    // Hash Ethernet address in two different buckets.
    hash_mac_entry(addr, vlan, i0, i1);

    // Fetch each bucket and look for the existing MAC entry.
    get_mac_entry(mac_table, i0, b0);
    get_mac_entry(mac_table, i1, b1);
}
void put_mac_entry(register<Mac_register_entry> reg,
                   in bit<32> index,
                   in Mac_entry me)
{
    Mac_register_entry me_raw
        = ((((Mac_register_entry) me.addr) << (12 + 9))
           | (((Mac_register_entry) me.vlan) << 9)
           | (Mac_register_entry) me.port);
    reg.write(index, me_raw);
}

control SnvsIngress(inout headers hdr,
                    inout metadata meta,
                    inout standard_metadata_t standard_metadata) {
    action Drop() {
        mark_to_drop(standard_metadata);
        exit;
    }

    // Drop packets received on mirror destination port.
    table MirrorDstDrop {
        key = { standard_metadata.ingress_port: exact @name("port"); }
        actions = { Drop; }
    }

    // Drop packets to reserved Ethernet multicast address.
    @nerpa_singleton
    table ReservedMcastDstDrop {
        key = { hdr.eth.dst: exact @name("dst"); }
        actions = { Drop; }
    }

    // Input VLAN processing.
    action SetVlan(VlanID vid) {
        meta.vlan = vid;
    }
    action UseTaggedVlan() {
        meta.vlan = hdr.vlan.vid;
    }
    table InputVlan {
        key = {
            standard_metadata.ingress_port: exact @name("port");
            hdr.vlan.isValid(): exact @name("has_vlan") @nerpa_bool;
            hdr.vlan.vid: optional @name("vid");
        }
        actions = { Drop; SetVlan; UseTaggedVlan; }
        default_action = Drop;
    }

    // Mirroring packet selection.
    table MirrorSelectProduct {
        key = {
            standard_metadata.ingress_port: optional @name("port");
            meta.vlan: optional @name("vlan");
        }
        actions = { NoAction; }
    }

    // Tracks VLANs in which all packets are flooded.
    action set_flood() {
        meta.flood = true;
    }
    table FloodVlan {
        key = { meta.vlan: exact @name("vlan"); }
        actions = { set_flood; }
    }

    register<Mac_register_entry>(N_MAC_ENTRIES) mac_table;    

    apply {
        // Drop packets received on mirror destination port.
        MirrorDstDrop.apply();

        // Drop packets to reserved Ethernet multicast address.
        ReservedMcastDstDrop.apply();

        // Input VLAN processing.
        InputVlan.apply();

        // Mirroring packet selection.
        if (MirrorSelectProduct.apply().hit) {
            clone(CloneType.I2E, 1);
        }

        // Is this a flood VLAN?
        meta.flood = false;
        FloodVlan.apply();

        // TODO: Factor out common logic between the two if-statements.

        // Learn source MAC.
        if (!meta.flood && !eth_addr_is_multicast(hdr.eth.src)) {
            bit<32> i0;
            bit<32> i1;
            Mac_entry b0;
            Mac_entry b1;
            hash_and_get_mac_entries(hdr.eth.src, meta.vlan, mac_table, i0, i1, b0, b1);
            if (!(b0.addr != hdr.eth.src || b0.vlan != meta.vlan) &&
                !(b1.addr != hdr.eth.src || b1.vlan != meta.vlan)) {
                // No match.  Replace one entry randomly.
                bit<1> bucket;
                random(bucket, 0, 1);

                put_mac_entry(mac_table, bucket == 0 ? i0 : i1,
                              { hdr.eth.src, meta.vlan,
                                standard_metadata.ingress_port });
            }
        }

        // Lookup destination MAC.
        PortID output = FLOOD_PORT;
        if (!meta.flood && !eth_addr_is_multicast(hdr.eth.dst)) {
            bit<32> i0;
            bit<32> i1;
            Mac_entry b0;
            Mac_entry b1;
            hash_and_get_mac_entries(hdr.eth.dst, meta.vlan, mac_table, i0, i1, b0, b1);
            if (b0.addr == hdr.eth.dst && b0.vlan == meta.vlan) {
                output = b0.port;
            } else if (b1.addr == hdr.eth.dst && b1.vlan == meta.vlan) {
                output = b1.port;
            } else {
                /* No learned port for this MAC and VLAN. */
            }
        }

        // If we're flooding, then use the VLAN as the multicast group
        // (we assume that the control plane has configured one multicast
        // group per VLAN, with the VLAN number as the multicast group ID).
        //
        // If we have a destination port, then it becomes the output port.
        //
        // We don't bother to try to drop output to the input port here
        // because it happens in the egress pipeline.
        if (output == FLOOD_PORT) {
            standard_metadata.mcast_grp = (bit<16>) meta.vlan;
        } else {
            standard_metadata.egress_spec = output;
        }
    }
}

control SnvsEgress(inout headers hdr,
                   inout metadata meta,
                   inout standard_metadata_t standard_metadata) {
    // Output VLAN processing.
    table OutputVlan {
        key = {
            standard_metadata.egress_port: exact @name("port");
            meta.vlan: optional @name("vlan");
        }
        actions = { NoAction; }
    }

    // Priority tagging mode.
    table PriorityTagging {
        key = {
            standard_metadata.egress_port: exact @name("port");
            hdr.vlan.isValid() && hdr.vlan.pcp != 0: exact @name("nonzero_pcp") @nerpa_bool;
        }
        actions = { NoAction; }
    }

    apply {
      // If this is a clone for the purpose of port mirroring, we're all
      // done.
      if (standard_metadata.instance_type == PKT_INSTANCE_TYPE_INGRESS_CLONE) {
          exit;
      }

      // Drop loopback.
      if (standard_metadata.egress_port == standard_metadata.ingress_port) {
          mark_to_drop(standard_metadata);
          exit;
      }
      
      // Output VLAN processing, including priority tagging.
      bool tag_vlan = OutputVlan.apply().hit;
      VlanID vid = tag_vlan ? meta.vlan : 0;
      bool include_vlan_header = tag_vlan || PriorityTagging.apply().hit;
      if (include_vlan_header && !hdr.vlan.isValid()) {
          hdr.vlan = { 0, 0, vid, hdr.eth.type };
          hdr.eth.type = ETH_VLAN;
      } else if (!include_vlan_header && hdr.vlan.isValid()) {
          hdr.eth.type = hdr.vlan.type;
          hdr.vlan.setInvalid();
      }
    }
}

control SnvsComputeChecksum(inout headers hdr, inout metadata meta) {
    apply {}
}

control SnvsDeparser(packet_out packet, in headers hdr) {
    apply {
        packet.emit(hdr);
    }
}

V1Switch (
    SnvsParser(),
    SnvsVerifyChecksum(),
    SnvsIngress(),
    SnvsEgress(),
    SnvsComputeChecksum(),
    SnvsDeparser()
) main;