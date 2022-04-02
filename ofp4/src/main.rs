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

//! `ofp4` provides a P4Runtime interface to Open vSwitch.  It accepts P4Runtime connections from a
//! controller and connects to an Open vSwitch instance over OpenFlow and OVSDB.

use anyhow::{anyhow, Context, Result};

use clap::{App, Arg};

use differential_datalog::DeltaMap;
use differential_datalog::api::HDDlog;
use differential_datalog::ddval::{DDValConvert, DDValue};
use differential_datalog::program::{IdxId, RelId, Update};
use differential_datalog::record::{RelIdentifier, UpdCmd};
use differential_datalog::{DDlog, DDlogDynamic, DDlogInventory};

use futures_util::{FutureExt, SinkExt, TryFutureExt, TryStreamExt};

use grpcio::{
    ChannelBuilder,
    DuplexSink,
    Environment,
    RequestStream,
    RpcContext,
    RpcStatusCode,
    ServerBuilder,
    ServerStreamingSink,
    UnarySink,
};

use log::error;

use ovs::{
    self,
    latch::Latch,
    ofpbuf::Ofpbuf,
    ofp_flow::{FlowMod, FlowModCommand}
};

use p4ext::*;

use proto::p4info::P4Info;
use proto::p4runtime::{
    CapabilitiesRequest,
    CapabilitiesResponse,
    Entity,
    Entity_oneof_entity,
    ForwardingPipelineConfig,
    ForwardingPipelineConfig_Cookie,
    GetForwardingPipelineConfigRequest,
    GetForwardingPipelineConfigResponse,
    PacketReplicationEngineEntry,
    PacketReplicationEngineEntry_oneof_type,
    ReadRequest,
    ReadResponse,
    SetForwardingPipelineConfigRequest,
    SetForwardingPipelineConfigResponse,
    StreamMessageRequest,
    StreamMessageResponse,
    Update_Type,
    WriteRequest,
    WriteResponse,
};
use proto::p4runtime_grpc::{P4Runtime, create_p4_runtime};

use protobuf::{Message, SingularPtrField, well_known_types::Any};

use snvs_ddlog::typedefs::{Flow, MulticastGroup};
use snvs_ddlog::{Indexes, Relations};

use std::collections::{BTreeSet, HashMap};
use std::default::Default;
use std::convert::TryInto;
use std::sync::{Arc, Mutex};

const OFP_PROTOCOL: ovs::ofp_protocol::Protocol = ovs::ofp_protocol::Protocol::OF15_OXM;
const OFP_VERSION: ovs::ofp_protocol::Version = ovs::ofp_protocol::Version::OFP15;

struct State {
    hddlog: HDDlog,
    latch: Latch,
    pending_flow_mods: Vec<Ofpbuf>,

    // Configuration state.
    device_id: u64,
    p4info: P4Info,
    cookie: u64,
    table_schemas: HashMap<u32, Table>,

    // Table state.
    multicast_groups: HashMap<MulticastGroupId, BTreeSet<Replica>>,
    table_entries: HashMap<TableKey, TableValue>
}

impl State {
    fn new(hddlog: HDDlog) -> State {
        let (device_id, pending_flow_mods, p4info, cookie, table_schemas, multicast_groups,
             table_entries)
            = Default::default();
        let latch = Latch::new(); 
        State {
            hddlog, latch, pending_flow_mods, device_id, p4info, cookie, table_schemas,
            multicast_groups, table_entries,
        }
    }

    /// Implements the P4Runtime `read` operation for the specified `multicast_group_id`, including
    /// the P4Runtime behavior that a zero or missing multicast group acts as a wildcard.  Returns
    /// the entities to send back to the P4Runtime client.
    fn read_multicast_groups(&self, multicast_group_id: u32) -> Vec<Entity> {
        let group_ids: Vec<MulticastGroupId> = if multicast_group_id == 0 {
            self.multicast_groups.keys().cloned().collect()
        } else {
            vec![multicast_group_id]
        };

        let mut entities = Vec::new();
        for multicast_group_id in group_ids {
            if let Some(mg_replicas) = self.multicast_groups.get(&multicast_group_id) {
                let p_mge: proto::p4runtime::MulticastGroupEntry = (&MulticastGroupEntry {
                    multicast_group_id, replicas: mg_replicas.clone()
                }).into();
                let entity = Entity {
                    entity: Some(Entity_oneof_entity::packet_replication_engine_entry(PacketReplicationEngineEntry {
                        field_type: Some(PacketReplicationEngineEntry_oneof_type::multicast_group_entry(p_mge)), ..Default::default()})), ..Default::default()};
                entities.push(entity);
            }
        }
        entities
    }

    /// Implements the P4Runtime `read` operation for table entries that match all of the fields in
    /// `target`.  As P4Runtime specifies, any field in `target` that is zero or missing acts as a
    /// wildcard.  Returns the entities to send back to the P4Runtime client.
    fn read_table_entries(&self, target: &proto::p4runtime::TableEntry) -> Vec<Entity> {
        let target: TableEntry = match target.try_into() {
            Ok(target) => target,
            Err(error) => {
                eprintln!("bad TableEntry {:?} for read operation ({:?})", target, error);
                return Vec::new();
            }
        };
            
        let mut entities = Vec::new();
        for (key, value) in &self.table_entries {
            if target.key.table_id != 0 && target.key.table_id != key.table_id {
                continue;
            }
            if !target.key.matches.is_empty() && target.key.matches != key.matches {
                continue;
            }
            if target.key.priority != 0 && target.key.priority != key.priority {
                continue;
            }
            if target.key.is_default_action && !key.is_default_action {
                continue;
            }
            if target.value.controller_metadata != 0 && target.value.controller_metadata != value.controller_metadata {
                continue;
            }
            if !target.value.metadata.is_empty() && target.value.metadata != value.metadata {
                continue;
            }
            // XXX meter_config
            // XXX counter_data
            // XXX idle_timeout_ns?
            // XXX time_since_last_hit?
            let (unknown_fields, cached_size) = Default::default();
            let te = TableEntry { key: key.clone(), value: value.clone() }; 
            entities.push(Entity {
                entity: Some(Entity_oneof_entity::table_entry((&te).into())),
                unknown_fields, cached_size });
        }
        entities
    }
}

#[derive(Clone)]
struct P4RuntimeService {
    state: Arc<Mutex<State>>,
}

impl P4RuntimeService {
    fn new(state: Arc<Mutex<State>>) -> P4RuntimeService {
        P4RuntimeService { state }
    }
}

fn unary_fail<T>(ctx: &RpcContext, sink: UnarySink<T>, status: grpcio::RpcStatus) {
    let f = sink.fail(status)
        .map_err(|e| error!("failed to send error: {:?}", e))
        .map(|_| ());
    ctx.spawn(f);
}

fn unary_success<T>(ctx: &RpcContext, sink: UnarySink<T>, reply: T) {
    let f = sink
        .success(reply)
        .map_err(|e: grpcio::Error| error!("write failed: {:?}", e))
        .map(|_| ());
    ctx.spawn(f);
}

fn server_streaming_fail<T>(ctx: &RpcContext, sink: ServerStreamingSink<T>, code: RpcStatusCode) {
    let f = sink.fail(grpcio::RpcStatus::new(code))
        .map_err(|e| error!("failed to send error: {:?}", e))
        .map(|_| ());
    ctx.spawn(f);
}

fn server_streaming_success<T: Send + 'static>(ctx: &RpcContext, mut sink: ServerStreamingSink<T>,
                               reply: Vec<T>) {
    let f = async move {
        for msg in reply {
            sink.send((msg, Default::default())).await?;
        }
        sink.close().await?;
        Ok(())
    }
    .map_err(|e: grpcio::Error| error!("failed to stream response: {:?}", e))
        .map(|_| ());
    ctx.spawn(f);
}

impl P4RuntimeService {
    fn validate_write(op: Update_Type, entity_exists: bool) -> Result<()> {
        match (op, entity_exists) {
            (Update_Type::UNSPECIFIED, _) => Err(Error(RpcStatusCode::INVALID_ARGUMENT))?,
            (Update_Type::INSERT, true) => Err(Error(RpcStatusCode::ALREADY_EXISTS))?,
            (Update_Type::MODIFY, false) => Err(Error(RpcStatusCode::NOT_FOUND))?,
            (Update_Type::DELETE, false) => Err(Error(RpcStatusCode::NOT_FOUND))?,
            _ => Ok(()),
        }
    }

    fn write_entity(op: Update_Type, entity: Option<&Entity>, state: &mut State) -> Result<()> {
        match entity {
            None => Err(Error(RpcStatusCode::INVALID_ARGUMENT))?,
            Some(Entity {
                entity: Some(Entity_oneof_entity::packet_replication_engine_entry(
                    PacketReplicationEngineEntry {
                        field_type: Some(PacketReplicationEngineEntry_oneof_type::multicast_group_entry(
                            mge)), ..})), ..
            }) => {
                let mge: MulticastGroupEntry = mge.into();
                if mge.multicast_group_id == 0 {
                    Err(Error(RpcStatusCode::INVALID_ARGUMENT)).context(format!("multicast_group_id must not be zero"))?;
                }

                // Validate the operation.
                let no_values = BTreeSet::new();
                let old_value = state.multicast_groups.get(&mge.multicast_group_id).unwrap_or(&no_values);
                Self::validate_write(op, !old_value.is_empty())?;

                let new_value = match op {
                    Update_Type::UNSPECIFIED => unreachable!(),
                    Update_Type::INSERT | Update_Type::MODIFY => &mge.replicas,
                    Update_Type::DELETE => &no_values,
                };

                // Commit the operation to DDlog.
                let mut commands = Vec::with_capacity(2);
                for insertion in new_value.difference(old_value) {
                    commands.push(Update::Insert {
                        relid: Relations::MulticastGroup as RelId,
                        v: MulticastGroup {
                            mcast_id: mge.multicast_group_id as u16,
                            port: insertion.egress_port as u16
                        }.into_ddvalue()
                    });
                }
                for deletion in old_value.difference(new_value) {
                    commands.push(Update::DeleteValue {
                        relid: Relations::MulticastGroup as RelId,
                        v: MulticastGroup {
                            mcast_id: mge.multicast_group_id as u16,
                            port: deletion.egress_port as u16
                        }.into_ddvalue()
                    });
                }
                let delta = {
                    let hddlog = &state.hddlog;

                    hddlog.transaction_start().ddlog_map_error()?;
                    hddlog.apply_updates(&mut commands.into_iter()).ddlog_map_error()?;
                    hddlog.transaction_commit_dump_changes().ddlog_map_error()?
                };
                delta_to_flow_mods(&delta, &mut state.pending_flow_mods);
                state.latch.set();

                // Commit the operation to our internal representation.
                if new_value.is_empty() {
                    state.multicast_groups.remove(&mge.multicast_group_id);
                } else {
                    state.multicast_groups.insert(mge.multicast_group_id, mge.replicas);
                }
                Ok(())
            },
            Some(Entity { entity: Some(Entity_oneof_entity::table_entry(te)), .. }) => {
                let te: TableEntry = te.try_into()?;

                // Look up the table schema and get its DDlog relation ID.
                let table = match state.table_schemas.get(&te.key.table_id) {
                    Some(table) => table,
                    None => Err(Error(RpcStatusCode::NOT_FOUND)).context(format!("unknown table {}", te.key.table_id))?
                };
                let relid = state.hddlog.inventory.get_table_id(table.base_name()).ddlog_map_error()? as RelId;

                // Validate the operation.
                let old_value = state.table_entries.get(&te.key);
                Self::validate_write(op, old_value.is_some())?;

                // Commit the operation to DDlog.
                let mut commands = Vec::with_capacity(2);
                if let Some(old_value) = old_value {
                    let old_te = TableEntry { key: te.key.clone(), value: old_value.clone() };
                    let old_record = old_te.to_record(table).unwrap();
                    commands.push(UpdCmd::Delete(RelIdentifier::RelId(relid), old_record));
                }
                if op != Update_Type::DELETE {
                    let new_record = te.to_record(table).unwrap();
                    commands.push(UpdCmd::Insert(RelIdentifier::RelId(relid), new_record));
                }
                eprintln!("len={} {:?}", commands.len(), commands);
                let delta = {
                    let hddlog = &state.hddlog;

                    hddlog.transaction_start().ddlog_map_error()?;
                    hddlog.apply_updates_dynamic(&mut commands.into_iter()).ddlog_map_error()?;
                    hddlog.transaction_commit_dump_changes().ddlog_map_error()?
                };
                delta_to_flow_mods(&delta, &mut state.pending_flow_mods);
                state.latch.set();

                // Commit the operation to our internal representation.
                if op == Update_Type::DELETE {
                    state.table_entries.remove(&te.key);
                } else {
                    state.table_entries.insert(te.key, te.value);
                }

                Ok(())
            },
            _ => Err(Error(RpcStatusCode::UNIMPLEMENTED))?
        }
    }
}

impl<'a> P4Runtime for P4RuntimeService {
    fn write(&mut self,
             ctx: RpcContext,
             req: WriteRequest,
             sink: UnarySink<WriteResponse>) {
        println!("write {:?}", req);
        let mut state = self.state.lock().unwrap();
        if req.device_id != state.device_id {
            unary_fail(&ctx, sink, grpcio::RpcStatus::new(RpcStatusCode::NOT_FOUND));
            return;
        }

        // XXX role
        // XXX election_id
        // XXX atomicity

        let mut errors = Vec::with_capacity(req.updates.len());
        for proto::p4runtime::Update { field_type: op, entity, .. } in req.updates {
            let code = match Self::write_entity(op, entity.as_ref(), &mut state) {
                Err(error) => {
                    eprintln!("{:?}", error);
                    match error.downcast_ref::<Error>() {
                        Some(Error(code)) => *code,
                        _ => RpcStatusCode::UNKNOWN
                    }
                },
                Ok(()) => RpcStatusCode::OK,
            };
            errors.push(code);
        }
        if errors.iter().all(|&code| code == RpcStatusCode::OK) {
            unary_success(&ctx, sink, WriteResponse::new());
        } else {
            let (message, unknown_fields, cached_size) = Default::default();
            let details = proto::status::Status {
                code: RpcStatusCode::UNKNOWN.into(),
                details: errors.iter().map(|&canonical_code| {
                    let (message, space, code, details, unknown_fields, cached_size) = Default::default();
                    Any::pack(&proto::p4runtime::Error {
                        canonical_code: canonical_code.into(),
                        message, space, code, details, unknown_fields, cached_size
                    }).unwrap()}).collect(),
                message, unknown_fields, cached_size
            };
            unary_fail(&ctx, sink, grpcio::RpcStatus::with_details(RpcStatusCode::UNKNOWN, Default::default(), details.write_to_bytes().unwrap()));
        }
    }

    fn read(&mut self,
            ctx: RpcContext,
            req: ReadRequest,
            sink: ServerStreamingSink<ReadResponse>) {
        println!("read {:?}", req);
        let state = self.state.lock().unwrap();
        if req.device_id != state.device_id {
            server_streaming_fail(&ctx, sink, RpcStatusCode::NOT_FOUND);
            return;
        }

        let mut responses = Vec::new();
        for rq_entity in req.entities {
            let rpy_entities = match rq_entity {
                Entity {
                    entity: Some(Entity_oneof_entity::packet_replication_engine_entry(PacketReplicationEngineEntry {
                        field_type: Some(PacketReplicationEngineEntry_oneof_type::multicast_group_entry(mge)), ..})), ..}
                => state.read_multicast_groups(mge.multicast_group_id),

                Entity { entity: Some(Entity_oneof_entity::table_entry(te)), .. }
                => state.read_table_entries(&te),

                _ => Vec::new(),
            };
            responses.push(ReadResponse { entities: rpy_entities.into(), ..Default::default() });
        }

        server_streaming_success(&ctx, sink, responses);
    }

    fn set_forwarding_pipeline_config(
        &mut self,
        ctx: RpcContext,
        req: SetForwardingPipelineConfigRequest,
        sink: UnarySink<SetForwardingPipelineConfigResponse>) {
        println!("set_forwarding_pipeline_config");
        let config = req.get_config();

        let mut state = self.state.lock().unwrap();
        state.p4info = config.get_p4info().clone();
        state.cookie = config.get_cookie().get_cookie();

        // Actions are referenced by id, so make a map.
        let action_by_id: HashMap<u32, p4ext::Action> = state.p4info
            .get_actions()
            .iter()
            .map(|a| (a.get_preamble().id, a.into()))
            .collect();
        state.table_schemas = state.p4info.get_tables().iter()
            .map(|table| p4ext::Table::new_from_proto(table, &action_by_id))
            .map(|table| (table.preamble.id, table))
            .collect();
        unary_success(&ctx, sink, SetForwardingPipelineConfigResponse::new());
    }

    fn get_forwarding_pipeline_config(&mut self, ctx: RpcContext, req: GetForwardingPipelineConfigRequest, sink: UnarySink<GetForwardingPipelineConfigResponse>) {
        println!("get_forwarding_pipeline_config");
        let state = self.state.lock().unwrap();
        if req.device_id != state.device_id {
            unary_fail(&ctx, sink, grpcio::RpcStatus::new(RpcStatusCode::NOT_FOUND));
            return;
        }
        let reply = GetForwardingPipelineConfigResponse {
            config: SingularPtrField::some(ForwardingPipelineConfig {
                p4info: SingularPtrField::some(state.p4info.clone()),
                cookie: SingularPtrField::some(ForwardingPipelineConfig_Cookie {
                    cookie: state.cookie, ..Default::default()}),
                ..Default::default()}),
            ..Default::default()};
        unary_success(&ctx, sink, reply);
    }

    fn stream_channel(
        &mut self,
        ctx: RpcContext,
        mut stream: RequestStream<StreamMessageRequest>,
        mut sink: DuplexSink<StreamMessageResponse>) {
        let f = async move {
            while let Some(n) = stream.try_next().await? {
                println!("stream_channel");
                let mut reply = StreamMessageResponse::new();
                reply.set_arbitration(n.get_arbitration().clone());
                sink.send((reply, grpcio::WriteFlags::default())).await?;
            }
            sink.close().await?;
            Ok(())
        }
        .map_err(|e: grpcio::Error| error!("stream_channel failed: {:?}", e))
        .map(|_| ());
        ctx.spawn(f)
    }

    fn capabilities(&mut self,
                    _ctx: RpcContext,
                    _req: CapabilitiesRequest,
                    _sink: UnarySink<CapabilitiesResponse>) {
        println!("capabilities");
    }
}

trait DdlogMapError<T> {
    fn ddlog_map_error(self) -> Result<T>;
}
impl<T> DdlogMapError<T> for std::result::Result<T, String> {
    fn ddlog_map_error(self) -> Result<T> {
        self.map_err(|s| anyhow!("DDlog error: {}", s))
    }
}

fn main() -> Result<()> {
    const OVS_REMOTE: &str = "ovs-remote";
    const P4_PORT: &str = "p4-port";
    const P4_ADDR: &str = "p4-addr";

    let matches = App::new("ofp4")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(Arg::with_name(OVS_REMOTE)
             .help("OVS remote to connect, e.g. \"unix:/path/to/ovs/tutorial/sandbox/br0.mgmt\"")
             .required(true)
             .index(1))
        .arg(Arg::with_name(P4_PORT)
             .long(P4_PORT)
             .help("P4Runtime connection listening port")
             .takes_value(true)
             .default_value("50051"))
        .arg(Arg::with_name(P4_ADDR)
             .long(P4_ADDR)
             .help("P4Runtime connection bind address")
             .takes_value(true)
             .default_value("127.0.0.1"))
        .get_matches();

    let ovs_remote = matches.value_of(OVS_REMOTE).unwrap();
    let p4_port = matches.value_of(P4_PORT).unwrap().parse::<u16>().unwrap();
    let p4_addr = matches.value_of(P4_ADDR).unwrap();

    let env = Arc::new(Environment::new(1));
    let (mut hddlog, _init_state) = snvs_ddlog::run(1, false).ddlog_map_error()?;
    let mut record = Some(std::fs::File::create("replay.txt")?);
    hddlog.record_commands(&mut record);
    let state = Arc::new(Mutex::new(State::new(hddlog)));
    let service = create_p4_runtime(P4RuntimeService::new(state.clone()));
    let ch_builder = ChannelBuilder::new(env.clone());
    let mut server = ServerBuilder::new(env)
        .register_service(service)
        .bind(p4_addr, p4_port)
        .channel_args(ch_builder.build_args())
        .build()
        .unwrap();
    server.start();

    let mut rconn = ovs::rconn::Rconn::new(0, 0, ovs::rconn::DSCP_DEFAULT, OFP_VERSION.into());

    rconn.connect(ovs_remote, None);
    let mut last_seqno = 0;
    let mut bundle_id = 0;
    loop {
        rconn.run();
        loop {
            let p = rconn.recv();
            match p {
                None => break,
                Some(message) => println!("received message {}", ovs::ofp_print::Printer(message.as_slice()))
            }
        }

        state.lock().unwrap().latch.poll();
        if rconn.connected() {
            let mut state = state.lock().unwrap();

            let flags = ovs::ofp_bundle::OFPBF_ATOMIC | ovs::ofp_bundle::OFPBF_ORDERED;
            if rconn.connection_seqno() == last_seqno {
                // Send pending flow mods, if any.
                if !state.pending_flow_mods.is_empty() {
                    bundle_id += 1;
                    let bundle = ovs::ofp_bundle::BundleSequence::new(bundle_id, flags, OFP_VERSION,
                                                                      state.pending_flow_mods.drain(..));
                    for msg in bundle {
                        rconn.send(msg).unwrap();
                    }
                }
            } else {
                // We just reconnected.  Send all the flows.  Discard pending flow mods, if any,
                // because the full collection of flows includes them.
                state.pending_flow_mods.clear();

                // Compose a sequence of flow_mods starting with one to delete all the existing
                // flows, then add in all the flows we do want.  We're going to put all of these
                // together into an atomic bundle, so we shouldn't change the treatment of all the
                // packets in the middle.
                let del_flows = FlowMod::parse("", Some(FlowModCommand::Delete { strict: false })).unwrap().0;
                let add_flows = state.hddlog.dump_index(Indexes::Flow as IdxId).unwrap().into_iter()
                    .filter_map(|record| {
                        let flow: &Flow = Flow::from_ddvalue_ref(&record);
                        match FlowMod::parse(&flow.s, Some(FlowModCommand::Add)) {
                            Ok((flow, _)) => Some(flow),
                            Err(s) => {
                                eprintln!("{}: {}", flow.s, s);
                                None
                            }
                        }
                    });
                let flow_mods = std::iter::once(del_flows).chain(add_flows).map(|fm| fm.encode(OFP_PROTOCOL));

                bundle_id += 1;
                let bundle = ovs::ofp_bundle::BundleSequence::new(bundle_id, flags, OFP_VERSION, flow_mods);
                for msg in bundle {
                    rconn.send(msg).unwrap();
                }

                last_seqno = rconn.connection_seqno();
            }
        } else {
            // We're disconnected.  We can't send pending flow mods.  When we reconnect, we'll send
            // everything.
            let mut state = state.lock().unwrap();
            state.pending_flow_mods.clear();
        }

        state.lock().unwrap().latch.wait();
        rconn.run_wait();
        rconn.recv_wait();
        ovs::poll_loop::block();
    }
}

/// Converts the `delta` of changes to DDlog output relations (particularly `Flow`) into OpenFlow
/// [`FlowMod`] messages and appends those messages to `flow_mods`.
fn delta_to_flow_mods(delta: &DeltaMap<DDValue>, flow_mods: &mut Vec<Ofpbuf>) {
    for (&rel, changes) in delta.iter() {
        if rel == Relations::Flow as RelId {
            for (val, &weight) in changes.iter() {
                let command = match weight {
                    1 => FlowModCommand::Add,
                    -1 => FlowModCommand::Delete { strict: true },
                    _ => unreachable!()
                };

                let flow0: &Flow = Flow::from_ddvalue_ref(val);
                match FlowMod::parse(&flow0.s, Some(command)) {
                    Ok((flow_mod, _)) => flow_mods.push(flow_mod.encode(OFP_PROTOCOL)),
                    Err(s) => eprintln!("{}: {}", flow0.s, s)
                };
            }
        }
    }
}
