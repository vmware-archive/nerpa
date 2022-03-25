/* The data plane program for VLAN assignment. 
 * 
 * Defines per-packet processing instructions. */
#include <core.p4> 
#include <v1model.p4> // The V1model architecture.

const bit<16> TYPE_VLAN = 0x8100;

header Ethernet_t {
    bit<48> dst;
    bit<48> src;
    bit<16> type;
}

header Vlan_t {
    bit<3> pcp;
    bit<1> dei;
    bit<12> vid;
    bit<16> etherType;
}

header Ipv4_t {
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

struct metadata {
    // The packet's conceptual VLAN, which may not be in a VLAN header.
    bit<12> vid;

    // Whether to flood this packet.
    bool flood;
}

struct headers {
    Ethernet_t eth;
    Vlan_t vlan;
    Ipv4_t ipv4;
}

parser TutorialParser(packet_in packet,
                      out headers hdr,
                      inout metadata meta,
                      inout standard_metadata_t standard_metadata) {
    state start {
        packet.extract(hdr.eth);
        transition select(hdr.eth.type) {
            TYPE_VLAN: parse_vlan;
        }
    }

    state parse_vlan {
        packet.extract(hdr.vlan);
        transition accept;
    }
}

control TutorialVerifyChecksum(inout headers hdr, inout metadata meta) {
    apply {
        verify_checksum(hdr.ipv4.isValid(),
            {
                hdr.ipv4.version,
                hdr.ipv4.ihl,
                hdr.ipv4.dscp,
                hdr.ipv4.ecn,
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

control TutorialIngress(inout headers hdr,
                        inout metadata meta,
                        inout standard_metadata_t standard_metadata) {
    action Drop() {
        mark_to_drop(standard_metadata);
    }

    action SetVlan(bit<12> vid) {
        meta.vid = vid;
    }

    action UseTaggedVlan() {
        meta.vid = hdr.vlan.vid;
    }

    table InputVlan {
        key = {
            standard_metadata.ingress_port: exact @name("port");
            hdr.vlan.isValid(): exact @name("has_vlan") @nerpa_bool;
            hdr.vlan.vid: optional @name("vid");
        }

        actions = {
            Drop;
            SetVlan;
            UseTaggedVlan;
        }

        default_action = Drop;
    }

    apply {
        InputVlan.apply();
    }
}

control TutorialEgress(inout headers hdr,
                       inout metadata meta,
                       inout standard_metadata_t standard_metadata) {
    // Output VLAN processing.
    table OutputVlan {
        key = {
            standard_metadata.egress_port: exact @name("port");
            meta.vid: optional @name("vlan");
        }

        actions = {
            NoAction;
        }
    }

    // Priority tagging mode.
    table PriorityTagging {
        key = {
            standard_metadata.egress_port: exact @name("port");
            hdr.vlan.isValid() && hdr.vlan.pcp != 0: exact @name("nonzero_pcp") @nerpa_bool;
        }

        actions = {
            NoAction;
        }
    }

    apply {
        // Drop loopback.
        if (standard_metadata.egress_port == standard_metadata.ingress_port) {
            mark_to_drop(standard_metadata);
            exit;
        }

        // Output VLAN processing, including priority tagging.
        bool tag_vlan = OutputVlan.apply().hit;
        bit<12> vid = tag_vlan ? meta.vid : 0;
        bool include_vlan_header = tag_vlan || PriorityTagging.apply().hit;
        if (include_vlan_header && !hdr.vlan.isValid()) {
            hdr.vlan = {
                0,
                0,
                vid,
                hdr.eth.type
            };
            hdr.eth.type = TYPE_VLAN;
        } else if (!include_vlan_header && hdr.vlan.isValid()) {
            hdr.eth.type = hdr.vlan.etherType;
            hdr.vlan.setInvalid();
        }
    }
}

control TutorialComputeChecksum(inout headers hdr, inout metadata meta) {
    apply {
        update_checksum(hdr.ipv4.isValid(),
            {
                hdr.ipv4.version,
                hdr.ipv4.ihl,
                hdr.ipv4.dscp,
                hdr.ipv4.ecn,
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

control TutorialDeparser(packet_out packet, in headers hdr) {
    apply {
        packet.emit(hdr);
    }
}

V1Switch (
    TutorialParser(),
    TutorialVerifyChecksum(),
    TutorialIngress(),
    TutorialEgress(),
    TutorialComputeChecksum(),
    TutorialDeparser()
) main;