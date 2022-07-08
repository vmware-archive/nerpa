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
#include <of_model.p4>

const bit<16> ETH_VLAN = 0x8100;

typedef bit<48>  EthernetAddress;
typedef bit<32>  IPv4Address;
typedef bit<12>  VlanID;
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
    return a[40:40] != 0;
}

const PortID DROP_PORT = 511;   // This is meaningful to simple_switch.
const PortID FLOOD_PORT = 510;  // Just an internal constant.

struct metadata {
    // The packet's conceptual VLAN, which might not be in a VLAN header.
    VlanID vlan;

    // Whether to flood this packet.
    bool flood;
}

const bit<32> MAC_LEARN_RCVR = 1;
struct LearnDigest {
    PortID port;
    VlanID vlan;
    EthernetAddress mac;
}

control SnvsIngress(inout Headers hdr,
out metadata meta,
in input_metadata_t meta_in,
inout ingress_to_arch_t itoa,
inout output_metadata_t meta_out) {
    action Drop() {
        meta_out.out_port = 0;
        exit;
    }

    // Drop packets received on mirror destination port.
    table MirrorDstDrop {
        key = { meta_in.in_port: exact @name("port"); }
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
        hdr.vlan.vid = vid;
	hdr.vlan.present = 1;
    }
    action UseTaggedVlan() {
        meta.vlan = hdr.vlan.vid;
    }
    table InputVlan {
        key = {
            meta_in.in_port: exact @name("port");
            hdr.vlan.present: exact @name("has_vlan") @nerpa_bool;
            hdr.vlan.vid: optional @name("vid");
        }
        actions = { Drop; SetVlan; UseTaggedVlan; }
        default_action = Drop;
    }

    #if 0
    // Mirroring packet selection.
    table MirrorSelectProduct {
        key = {
            meta_in.in_port: optional @name("port");
            meta.vlan: optional @name("vlan");
        }
        actions = { NoAction; }
    }
    #endif

    // Tracks VLANs in which all packets are flooded.
    action set_flood() {
        meta.flood = true;
    }
    table FloodVlan {
        key = { meta.vlan: exact @name("vlan"); }
        actions = { set_flood; }
    }

    // Known VLAN+MAC -> port mappings.
    //
    // We should only need one table for this, with one lookup for the source
    // MAC and one for the destination MAC per packet, but hardware and BMv2
    // don't support that.  So we need two different tables.
    table LearnedSrc {
        key = {
	    meta.vlan: exact @name("vlan");
	    hdr.eth.src: exact @name("mac");
	    meta_in.in_port: exact @name("port");
	}
	actions = { NoAction; }
    }

    PortID output;
    action KnownDst(PortID port) {
        output = port;
    }
    table LearnedDst {
        key = {
	    meta.vlan: exact @name("vlan");
	    hdr.eth.dst: exact @name("mac");
	}
	actions = { KnownDst; }
    }

    apply {
        meta = { 0, false };

        // Drop packets received on mirror destination port.
        MirrorDstDrop.apply();

        // Drop packets to reserved Ethernet multicast address.
        ReservedMcastDstDrop.apply();

        // Input VLAN processing.
        InputVlan.apply();

        #if 0 /* Disabled because we don't have an equivalent to "clone" in this architecture yet. */
        // Mirroring packet selection.
        if (MirrorSelectProduct.apply().hit) {
            clone(CloneType.I2E, 1);
        }
        #endif

        // Is this a flood VLAN?
        meta.flood = false;
        FloodVlan.apply();

        // If the source MAC isn't known, send it to the control plane to
	// be learned.
        if (!meta.flood && !eth_addr_is_multicast(hdr.eth.src)
	&& !LearnedSrc.apply().hit) {
            #if 0 /* Disabled because we don't have digests yet. */
	    LearnDigest d;
	    d.port = meta_in.in_port;
	    d.vlan = meta.vlan;
	    d.mac = hdr.eth.src;
	    digest<LearnDigest>(MAC_LEARN_RCVR, d);
            #endif
	}

        // Look up destination MAC.
        output = FLOOD_PORT;
        if (!meta.flood && !eth_addr_is_multicast(hdr.eth.dst)) {
            LearnedDst.apply();
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
            itoa.out_group = (bit<16>) meta.vlan;
        } else {
            meta_out.out_port = output;
        }
    }
}

control SnvsEgress(inout Headers hdr,
inout metadata meta,
in input_metadata_t meta_in,
inout output_metadata_t meta_out) {
    // Output VLAN processing.
    table OutputVlan {
        key = {
            meta_out.out_port: exact @name("port");
            meta.vlan: optional @name("vlan");
        }
        actions = { NoAction; }
    }

    // Priority tagging mode.
    table PriorityTagging {
        key = {
            meta_out.out_port: exact @name("port");
            hdr.vlan.present == 1 && hdr.vlan.pcp != 0: exact @name("nonzero_pcp") @nerpa_bool;
        }
        actions = { NoAction; }
    }

    apply {
        #if 0 /* Disabled because we don't have "clone" yet. */
        // If this is a clone for the purpose of port mirroring, we're all
        // done.
        if (itoa.clone) {
            exit;
        }
        #endif

        // Drop loopback.
        if (meta_out.out_port == meta_in.in_port) {
            meta_out.out_port = 0;
            exit;
        }

        // Output VLAN processing, including priority tagging.
        bool tag_vlan = OutputVlan.apply().hit;
        VlanID vid = tag_vlan ? meta.vlan : 0;
        bool include_vlan_header = tag_vlan || PriorityTagging.apply().hit;
        if (include_vlan_header && hdr.vlan.present == 0) {
            hdr.vlan.present = 1;
	    hdr.vlan.vid = vid;
        } else if (!include_vlan_header && hdr.vlan.present == 1) {
            hdr.vlan.present = 0;
        }
    }
}

OfSwitch (
SnvsIngress(),
SnvsEgress()
) main;
