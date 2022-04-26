/*
Copyright 2022 VMWare, Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

/*

This file describes a P4 architectural model for Open vSwitch.  To use
it with Open vSwitch, use the ofp4 gateway.

For complete details on the fields and metadata described here, please
read the Open vSwitch protocol header fields manual available at
http://www.openvswitch.org/support/dist-docs/ovs-fields.7.pdf

*/

#ifndef _OF_MODEL_P4
#define _OF_MODEL_P4

#include <core.p4>

/* An OpenFlow port number.  OpenFlow 1.1 and later adopted a 32-bit port
 * number, but OVS forces ports to be in the 16-bit range, so 16 bits is still
 * sufficient. */
typedef bit<16>  PortID;

/* OpenFlow reserved port numbers.  These have little relevance in a P4
   context. */
const PortID OFPP_UNSET      = 0xfff7; /* For OXM_OF_ACTSET_OUTPUT only. */
const PortID OFPP_IN_PORT    = 0xfff8; /* Where the packet came in. */
const PortID OFPP_TABLE      = 0xfff9; /* Perform actions in flow table. */
const PortID OFPP_NORMAL     = 0xfffa; /* Process with normal L2/L3. */
const PortID OFPP_FLOOD      = 0xfffb; /* All ports except input port and
                                        * ports disabled by STP. */
const PortID OFPP_ALL        = 0xfffc; /* All ports except input port. */
const PortID OFPP_CONTROLLER = 0xfffd; /* Send to controller. */
const PortID OFPP_LOCAL      = 0xfffe; /* Local openflow "port". */
const PortID OFPP_NONE       = 0xffff; /* Not associated with any port. */

match_kind {
    optional
}

/* Metadata fields.  These are always present for every packet. */
struct Metadata {
    PortID in_port;             /* Ingress port. */
    bit<32> skb_priority;       /* Linux packet scheduling class. */
    bit<32> pkt_mark;           /* Linux kernel metadata. */
    bit<32> packet_type;        /* OpenFlow packet type.  0 for Ethernet. */
    Tunnel tunnel;
    Conntrack ct;
}

/* Tunnel metadata.  These are all-zero for packets that did not arrive in
 * a tunnel. */
struct Tunnel {
    bit<64> tun_id;             /* VXLAN VNI, GRE key, Geneve VNI, ... */
    bit<32> tun_src;		/* Outer IPv4 source address. */
    bit<32> tun_dst;            /* Outer IPv4 destination address. */
    bit<128> tun_ipv6_src;      /* Outer IPv6 source address. */
    bit<128> tun_ipv6_dst;      /* Outer IPv6 destination address. */
    bit<16> tun_gbp_id;         /* VXLAN Group Based Policy ID. */
    bit<8> tun_gbp_flags;       /* VXLAN Group Based Policy flags. */
    bit<8> tun_erspan_ver;      /* ERSPAN version number (low 4 bits). */
    bit<32> tun_erspan_idx;     /* ERSPAN index (low 20 bits). */
    bit<8> tun_erspan_dir;      /* ERSPAN direction (low bit only). */
    bit<8> tun_erspan_hwid;     /* ERSPAN ERSPAN engine ID (low 6 bits). */
    bit<8> tun_gtpu_flags;      /* GTP-U flags. */
    bit<8> tun_gtpu_msgtype;	/* GTP-U message type. */
    bit<16> tun_flags;          /* Tunnel flags (low bit only). */

    /* Access to Geneve tunneling TLV options. */
    bit<992> tun_metadata0;
    /* ... */
    bit<992> tun_metadata63;
}

/* Flags for Conntrack.ct_state. */
const bit<32> CS_NEW  = 1 << 0; // 1 for an uncommitted connection.
const bit<32> CS_EST  = 1 << 1; // 1 for an established connection.
const bit<32> CS_REL  = 1 << 2; // 1 for packet relating to established connection.
const bit<32> CS_RPL  = 1 << 3; // 1 for packet in reply direction.
const bit<32> CS_INV  = 1 << 4; // 1 for invalid packet.
const bit<32> CS_TRK  = 1 << 5; // 1 for tracked packet.
const bit<32> CS_SNAT = 1 << 6; // 1 if packet was already SNATed.
const bit<32> CS_DNAT = 1 << 7; // 1 if packet awas already DNATed.

/* Connection-tracking metadata.
 *
 * All of these fields are read-only, but connection-tracking actions update
 * them.
 */
struct Conntrack {
    bit<32> ct_state;		// CS_*.
    bit<16> ct_zone;            // Connection-tracking zone.
    bit<32> ct_mark;		// Arbitrary metadata.
    bit<128> ct_label;          // More arbitrary metadata.

    /* The following fields require a match to a valid connection tracking state
     * as a prerequisite, in addition to the IP or IPv6 ethertype
     * match. Examples of valid connection tracking state matches in‐ clude
     * ct_state=+new, ct_state=+est, ct_state=+rel, and ct_state=+trk-inv. */
    bit<32> ct_nw_src;
    bit<32> ct_nw_dst;
    bit<128> ct_ipv6_src;
    bit<128> ct_ipv6_dst;
    bit<8> ct_nw_proto;
    bit<16> ct_tp_src;
    bit<16> ct_tp_dst;
}

header Ethernet {
    bit<48> src;
    bit<48> dst;
    bit<16> type;
}

header Vlan {
    bit<3> pcp;
    bit<1> present;
    bit<12> vid;
}

header Mpls {
    bit<32> label;		// Label (low 20 bits).
    bit<8> tc;			// Traffic class (low 3 bits).
    bit<8> bos;			// Bottom of Stack (low bit only).
    bit<8> ttl			// Time to live.
}

const bit<8> FRAG_ANY = 1 << 0;	  // Set for any IP fragment.
const bit<8> FRAG_LATER = 1 << 1; // Set for IP fragment with nonzero offset.

header Ipv4 {
    bit<32> src;
    bit<32> dst;
    bit<8> proto;
    bit<8> ttl;
    bit<8> frag;		// 0, or FRAG_ANY, or (FRAG_ANY | FRAG_LATER)
    bit<8> tos;                 // DSCP in top 6 bits, ECN in low 2 bits.
}

header Ipv6 {
    bit<128> src;
    bit<128> dst;
    bit<8> proto;
    bit<8> ttl;
    bit<8> frag;		// 0, or FRAG_ANY, or (FRAG_ANY | FRAG_LATER)
    bit<8> tos;                 // DSCP in top 6 bits, ECN in low 2 bits.
}

header Arp {
    bit<16> op;
    bit<32> spa;
    bit<32> tpa;
    bit<48> sha;
    bit<48> tha;
}

// Network Service Header (https://www.rfc-editor.org/rfc/rfc8300.html).
header Nsh {
    bit<8> flags;
    bit<8> ttl;
    bit<8> mdtype;
    bit<8> np;
    bit<32> spi; 		// Low 24 bits only.
    bit<8> si;
    bit<32> c1;
    bit<32> c2;
    bit<32> c3;
    bit<32> c4;
}

struct Tcp {
    bit<16> src;
    bit<16> dst;
    bit<16> flags;		// Low 12 bits only.
}

struct Udp {
    bit<16> src;
    bit<16> dst;
}

struct Sctp {
    bit<16> src;
    bit<16> dst;
}

struct Icmp {
    bit<8> type;
    bit<8> code;
}

struct Icmpv6 {
    bit<8> type;
    bit<8> code;
}

// IPv6 neighbor discovery.
// Valid only if Icmpv6 'type' is 135 or 136 and 'code' is 0.
struct Nd {
    bit<128> target;
    bit<32> reserved;
    bit<8> options_type;
}

// IPv6 neighbor discovery source link layer.
// Valid only if the Icmpv6 'type' is 135 and 'code' is 0.
struct NdSll {
    bit<48> sll;
}

// IPv6 neighbor discovery target link layer.
// Valid only if the Icmpv6 'type' is 136 and 'code' is 0.
struct NdTll {
    bit<48> tll;
}

struct Headers {
    Ethernet eth;
    Vlan vlan;
    Mpls mpls;
    Ipv4 ipv4;
    Ipv6 ipv6;
    Arp arp;
    Nsh nsh;
    Tcp tcp;
    Udp udp;
    Sctp sctp;
    Icmp icmp;
    Icmpv6 icmpv6;
    Nd nd;
    NdSll ndsll;
    NdTll ndtll;
}

@pipeline
control Ingress<H, M>(inout H hdr, inout M meta);
@pipeline
control Egress<H, M>(inout H hdr, inout M meta);

package OfSwitch<H, M>(Ingress<H, M> ig, Egress<H, M> eg);

#endif  /* _OF_MODEL_P4 */
