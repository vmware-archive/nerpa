#! /usr/bin/python3

from scapy.all import *
import sys

if sys.argv[1] == "0":
    sendp(Ether(src="00:11:11:00:00:00",dst="00:22:22:00:00:00")/IP(dst="1.2.3.4")/UDP(sport=1234,dport=2345), iface="veth0")
elif sys.argv[1] == "1":
    sendp(Ether(src="00:22:22:00:00:00",dst="00:11:11:00:00:00")/IP(dst="1.2.3.4")/UDP(sport=1234,dport=2345), iface="veth2")
else:
    raise False
