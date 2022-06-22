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

#ifndef __DEFINE__
#define __DEFINE__

#ifndef IP_VER_LENGTH
#define IP_VER_LENGTH 4
#endif

#ifndef IP_VERSION_4
#define IP_VERSION_4 4
#endif

#ifndef IP_VERSION_6
#define IP_VERSION_6 6
#endif

#define UDP_PORT_GTPU 2152
#define GTP_V1 0x01
#define GTP_GPDU 0xff

typedef bit<32> next_id_t;
typedef bit<20> mpls_label_t;
typedef bit<12> vlan_id_t;

typedef bit<32> teid_t;


// PORT types. Set by the control plane.
typedef bit<2> port_type_t;

const bit<9> CPU_PORT = 255;

const bit<16> ETHERTYPE_QINQ = 0x88A8;
const bit<16> ETHERTYPE_QINQ_NON_STD = 0x9100;
const bit<16> ETHERTYPE_VLAN = 0x8100;
const bit<16> ETHERTYPE_MPLS = 0x8847;
const bit<16> ETHERTYPE_IPV4 = 0x0800;

const bit<8> PROTO_ICMP = 1;
const bit<8> PROTO_TCP = 6;
const bit<8> PROTO_UDP = 17;

const vlan_id_t DEFAULT_VLAN_ID = 12w4094;

#endif
