#include "ovs/include/openvswitch/hmap.h"
#include "ovs/include/openvswitch/json.h"
#include "ovs/include/openvswitch/ofp-bundle.h"
#include "ovs/include/openvswitch/ofp-errors.h"
#include "ovs/include/openvswitch/ofp-flow.h"
#include "ovs/include/openvswitch/ofp-msgs.h"
#include "ovs/include/openvswitch/ofp-print.h"
#include "ovs/include/openvswitch/ofpbuf.h"
#include "ovs/include/openvswitch/poll-loop.h"
#include "ovs/include/openvswitch/rconn.h"
#include "ovs/include/openvswitch/shash.h"
#include "ovs/lib/daemon.h"
#include "ovs/lib/jsonrpc.h"
#include "ovs/lib/latch.h"
#include "ovs/lib/ovsdb-cs.h"
#include "ovs/lib/reconnect.h"
#include "ovs/lib/sset.h"
#include "ovs/lib/stream.h"
#include "ovs/lib/svec.h"

/**
 * <div rustbindgen replaces="minimatch"></div>
 */
struct minimatch_rust {
    struct miniflow *flow;
    struct miniflow *mask;
    struct tun_metadata_allocation *tun_md;
};
