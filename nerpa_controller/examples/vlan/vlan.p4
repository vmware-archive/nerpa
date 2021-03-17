/* -*- P4_16 -*- */
#include <core.p4>
#include <v1model.p4>

const bit<16> TYPE_IPV4 = 0x0800;
//ARP
const bit<16> TYPE_ARP = 0x0806;
//VLAN 
const bit<16> TYPE_VLAN = 0x8100;

// ARP RELATED CONST VARS
const bit<16> ARP_HTYPE = 0x0001; //Ethernet Hardware type is 1
const bit<16> ARP_PTYPE = TYPE_IPV4; //Protocol used for ARP is IPV4
const bit<8>  ARP_HLEN  = 6; //Ethernet address size is 6 bytes
const bit<8>  ARP_PLEN  = 4; //IP address size is 4 bytes
const bit<16> ARP_REQ = 1; //Operation 1 is request
const bit<16> ARP_REPLY = 2; //Operation 2 is reply

/* FURTHER ARP HEADER FIELDS TO BE AWARE OF
 * bit<48> ARP_SRC_MAC (requester's MAC)
 * bit<32> ARP_SRC_IP  (tell DST_MAC to this IP)
 * bit<48> ARP_DST_MAC (Looking for this MAC)
 * bit<32> ARP_DST_IP  (who has this IP)
 */

/*************************************************************************
*********************** H E A D E R S  ***********************************
*************************************************************************/

typedef bit<9>  egressSpec_t;
typedef bit<48> macAddr_t;
typedef bit<32> ip4Addr_t;

header vlan_t {
//order is changed compared to how offically a VLAN header looks like 
//as in a vlan tagged packet, vlan's TPID becomes the 
//Ethernet.ethertype (8100), then comes pcp,dei,vid and then comes the 
//encapsulated packet's ethertype
  bit<3>  pcp;
  bit<1>  dei;
  bit<12> vid;  
  bit<16> etherType;
}

header ethernet_t {
  macAddr_t dstAddr;
  macAddr_t srcAddr;
  bit<16>   etherType;
}

header arp_t {
  bit<16>   h_type;
  bit<16>   p_type;
  bit<8>    h_len;
  bit<8>    p_len;
  bit<16>   op_code;
  macAddr_t src_mac;
  ip4Addr_t src_ip;
  macAddr_t dst_mac;
  ip4Addr_t dst_ip;
}

header ipv4_t {
    bit<4>    version;
    bit<4>    ihl;
    bit<8>    diffserv;
    bit<16>   totalLen;
    bit<16>   identification;
    bit<3>    flags;
    bit<13>   fragOffset;
    bit<8>    ttl;
    bit<8>    protocol;
    bit<16>   hdrChecksum;
    ip4Addr_t srcAddr;
    ip4Addr_t dstAddr;
}

struct metadata {
    /* empty */
}

struct headers {
    ethernet_t   ethernet;
    vlan_t       vlan;
    arp_t        arp;
    ipv4_t       ipv4;
}

/*************************************************************************
*********************** P A R S E R  ***********************************
*************************************************************************/

parser MyParser(packet_in packet,
                out headers hdr,
                inout metadata meta,
                inout standard_metadata_t standard_metadata) {
    state start {
        packet.extract(hdr.ethernet);
        transition select(hdr.ethernet.etherType) {
            TYPE_ARP: parse_arp;
            TYPE_IPV4: parse_ipv4;
            TYPE_VLAN: parse_vlan;
        }
    }

    state parse_arp {
        packet.extract(hdr.arp);
        transition select(hdr.arp.op_code) {
            ARP_REQ: accept;
        }
    }

    state parse_ipv4 {
        packet.extract(hdr.ipv4);
        transition accept;
    }

    state parse_vlan {
        packet.extract(hdr.vlan);
        transition select(hdr.vlan.etherType) {
            TYPE_VLAN: parse_vlan;
            TYPE_IPV4: parse_ipv4;
            TYPE_ARP: parse_arp;
        }
    }
}

/*************************************************************************
************   C H E C K S U M    V E R I F I C A T I O N   *************
*************************************************************************/

control MyVerifyChecksum(inout headers hdr, inout metadata meta) {
    apply {}
}

/*************************************************************************
**************  I N G R E S S   P R O C E S S I N G   *******************
*************************************************************************/

control MyIngress(inout headers hdr,
                  inout metadata meta,
                  inout standard_metadata_t standard_metadata) {
    action drop() {
        mark_to_drop(standard_metadata);
    }

    // VLAN INCOMING ACTIONS AND TABLES
    action vlan_incoming_forward(egressSpec_t port) {
        standard_metadata.egress_spec = port;
    }

    table vlan_incoming_exact {
        key = {
            standard_metadata.ingress_port: exact;
            hdr.vlan.vid: exact;
        }

        actions = {
            vlan_incoming_forward;
            drop;
        }

        size = 1024;
        default_action = drop;
    }

    // PORT FORWARD ACTIONS AND RULES
    action portfwd(egressSpec_t port) {
        standard_metadata.egress_spec = port;
    }

    table port_exact {
        key = {
            standard_metadata.ingress_port: exact;
        }

        actions = {
            portfwd;
            drop;
        }
        size = 10;
        default_action = drop;
    }

    // ARP ACTIONS AND TABLES
    action arp_reply(macAddr_t request_mac) {
        // update operation code from request to reply
        hdr.arp.op_code = ARP_REPLY;

        // reply's dst_mac is the request's src mac
        hdr.arp.dst_mac = hdr.arp.src_mac;

        // reply's dst_ip is the request's src ip
        hdr.arp.src_mac = request_mac;

        // reply's src ip is the request's dst ip
        hdr.arp.src_ip = hdr.arp.dst_ip;

        // update ethernet header
        hdr.ethernet.dstAddr = hdr.ethernet.srcAddr;
        hdr.ethernet.srcAddr = request_mac;

        // send it back to the same port
        standard_metadata.egress_spec = standard_metadata.ingress_port;
    }

    table arp_exact {
        key = {
            hdr.arp.dst_ip: exact;
        }
        actions = {
            arp_reply;
            drop;
        }
        size = 1024;
        default_action = drop;
    }

    // IPV4 ACTIONS AND TABLES
    action ipv4_forward(macAddr_t dstAddr, egressSpec_t port) {
        standard_metadata.egress_spec = port;
        hdr.ethernet.srcAddr = hdr.ethernet.dstAddr;
        hdr.ethernet.dstAddr = dstAddr;
        hdr.ipv4.ttl = hdr.ipv4.ttl - 1;
    }
    table ipv4_lpm {
        key = {
            hdr.ipv4.dstAddr: lpm;
        }
        actions = {
            ipv4_forward;
            drop;
            NoAction;
        }
        size = 1024;
        default_action = NoAction();
    }

    apply {
        if (hdr.vlan.isValid()) {
            vlan_incoming_exact.apply();
        }
        else if (hdr.ethernet.isValid() && hdr.ipv4.isValid()) {
            if (!ipv4_lpm.apply().hit) {
                port_exact.apply();
            }
        }
        else if (hdr.ethernet.etherType == TYPE_ARP) {
            arp_exact.apply();
        }
        else {
            mark_to_drop(standard_metadata);
        }
    }
}

/*************************************************************************
****************  E G R E S S   P R O C E S S I N G   *******************
*************************************************************************/
control MyEgress(inout headers hdr,
                 inout metadata meta,
                 inout standard_metadata_t standard_metadata) {
    apply {}
}

/*************************************************************************
*************   C H E C K S U M    C O M P U T A T I O N   **************
*************************************************************************/
control MyComputeChecksum(inout headers hdr, inout metadata meta) {
    apply {
        update_checksum(
            hdr.ipv4.isValid(),
            {   hdr.ipv4.version,
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

/*************************************************************************
***********************  D E P A R S E R  *******************************
*************************************************************************/
control MyDeparser(packet_out packet, in headers hdr) {
    apply {
        /* TODO: add deparser logic */
        packet.emit(hdr.ethernet);
        packet.emit(hdr.vlan);  
        packet.emit(hdr.arp);
        packet.emit(hdr.ipv4);
    }
}

/*************************************************************************
***********************  S W I T C H  *******************************
*************************************************************************/
V1Switch (
    MyParser(),
    MyVerifyChecksum(),
    MyIngress(),
    MyEgress(),
    MyComputeChecksum(),
    MyDeparser()
) main;