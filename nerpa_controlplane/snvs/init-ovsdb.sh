#!/bin/bash

echo "Running init ovsdb commands!"
ovsdb-client -v transact tcp:127.0.0.1:6640 '["snvs", {"op": "insert", "table": "Port", "row": {"id": 0, "vlan_mode": "access", "tag": 1, "trunks": 0, "priority_tagging": "no"}}]'
ovsdb-client -v transact tcp:127.0.0.1:6640 '["snvs", {"op": "insert", "table": "Port", "row": {"id": 1, "vlan_mode": "access", "tag": 1, "trunks": 0, "priority_tagging": "no"}}]'
ovsdb-client -v transact tcp:127.0.0.1:6640 '["snvs", {"op": "insert", "table": "Port", "row": {"id": 2, "vlan_mode": "access", "tag": 1, "trunks": 0, "priority_tagging": "no"}}]'
ovsdb-client -v transact tcp:127.0.0.1:6640 '["snvs", {"op": "insert", "table": "Port", "row": {"id": 3, "vlan_mode": "access", "tag": 1, "trunks": 0, "priority_tagging": "no"}}]'
