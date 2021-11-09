#include <v1model.p4>

#define TABLE_CAPACITY 1024
#define MAC_LEARN_RCVR 1
#define BROADCAST_GRP 1

#define port_t bit<9>
#define mcast_group_t bit<16>
#define group_t bit<10>

#define BROADCAST_MAC 0xffffffffffff
#define MAX_FRAME_SIZE 16018

typedef bit<48> ethaddr_t;

header ethernet_t {
    ethaddr_t dstAddr;
    ethaddr_t srcAddr;
    bit<16> etherType;
}

struct headers_t {
    ethernet_t ethernet;
}

struct LearnDigest {
    port_t port;
    ethaddr_t src_mac;
}

struct metadata { }

error {
    BroadcastSrc,
    PacketTooLong
}

parser ParserImpl (
    packet_in buffer,
    out headers_t parsed_hdr,
    inout metadata meta,
    inout standard_metadata_t ostd
    )
{
    state start {
        // Reject too long frames. Compiler does not support that for v1model
        // bit<32> frame_len = buffer.length();
        // verify(frame_len > MAX_FRAME_SIZE, error.PacketTooLong);
        transition parse_eth;
    }

    state parse_eth {
        buffer.extract(parsed_hdr.ethernet);
        // Reject frames with broadcast src MAC. Switch emulator throws and terminates 
        // verify(parsed_hdr.ethernet.srcAddr == BROADCAST_MAC, error.BroadcastSrc);
        transition accept;
    }
}

control VerifyChecksumImpl (
    inout headers_t hdr,
    inout metadata meta
    )
{
    apply { }
}

control IngressImpl (
    inout headers_t hdr,
    inout metadata meta,
    inout standard_metadata_t ostd
    )
{
    /* Table source MAC */

    action Learn() {
        LearnDigest msg;
        msg.src_mac = hdr.ethernet.srcAddr;
        msg.port = ostd.ingress_port;
        digest(MAC_LEARN_RCVR, msg);
    }

    action Update() {
        // already implemented by target
    }

    table SrcMac {
        key = { hdr.ethernet.srcAddr: exact @name("src"); }
        actions = { Learn; Update; }
        default_action = Learn;
        support_timeout = true;
    }

    /* Table destination MAC */

    action Broadcast() {
        ostd.mcast_grp = BROADCAST_GRP;
    }

    action Multicast(mcast_group_t mcast_grp) {
        ostd.mcast_grp = mcast_grp;
    }

    action Forward(port_t port) {
        ostd.egress_spec = port;
    }

    table DstMac {
        key = { hdr.ethernet.dstAddr: exact @name("dst"); }
        actions = { Broadcast; Forward; Multicast; }
        default_action = Broadcast;

        size = TABLE_CAPACITY;
        support_timeout = true;
    }

    apply {
        SrcMac.apply();
        DstMac.apply();
    }
}

control EgressImpl (
    inout headers_t hdr,
    inout metadata meta,
    inout standard_metadata_t ostd
    )
{
    apply { }
}

control ComputeChecksumImpl (
    inout headers_t hdr,
    inout metadata meta)
{
    apply { }
}

control DeparserImpl (
    packet_out buffer,
    in headers_t hdr)
{
    apply {
        buffer.emit(hdr.ethernet);
    }
}

V1Switch(
    ParserImpl(),
    VerifyChecksumImpl(),
    IngressImpl(),
    EgressImpl(),
    ComputeChecksumImpl(),
    DeparserImpl()
) main;
