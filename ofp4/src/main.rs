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

use clap::Parser;

use daemon::{Daemonize, Daemonizing};

use differential_datalog::api::HDDlog;
use differential_datalog::ddval::{DDValConvert, DDValue};
use differential_datalog::program::{IdxId, RelId, Update};
use differential_datalog::record::{Record, RelIdentifier, UpdCmd};
use differential_datalog::{DeltaMap, DDlog, DDlogDynamic, DDlogInventory};

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

use ovs::{
    self,
    latch::Latch,
    ofpbuf::Ofpbuf,
    ofp_bundle::*,
    ofp_flow::{FlowMod, FlowModCommand},
    ofp_msgs::OfpType,
    rconn::Rconn
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

use protobuf::{Message, well_known_types::Any};

use ofp4dl_ddlog::typedefs::ofp4lib::{flow_t, multicast_group_t};
use std::collections::{BTreeSet, HashMap};
use std::default::Default;
use std::convert::TryInto;
use std::fs::{File, OpenOptions};
use std::io::stderr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tracing::{event, error, info, instrument, Level, span, warn};

const OFP_PROTOCOL: ovs::ofp_protocol::Protocol = ovs::ofp_protocol::Protocol::OF15_OXM;
const OFP_VERSION: ovs::ofp_protocol::Version = ovs::ofp_protocol::Version::OFP15;

struct Config {
    p4info: P4Info,
    module: String,
    cookie: u64,
    table_schemas: HashMap<u32, Table>,
    flow_idxid: IdxId,
    flow_relid: RelId,
    multicast_group_relid: RelId,
}

impl Config {
    fn new(fpc: &ForwardingPipelineConfig, hddlog: &HDDlog) -> Result<Self> {
        let p4info = fpc.get_p4info();
        let module = p4info.get_pkg_info().name.clone();
        info!("Configuring for P4 module '{module}'");

        // Actions are referenced by id, so make a map.
        let action_by_id: HashMap<u32, p4ext::Action> = p4info
            .get_actions()
            .iter()
            .map(|a| (a.get_preamble().id, a.into()))
            .collect();
        let table_schemas = p4info.get_tables().iter()
            .map(|table| p4ext::Table::new_from_proto(table, &action_by_id))
            .map(|table| (table.preamble.id, table))
            .collect();

        let flow_relname = format!("{module}::Flow");
        let flow_relid = hddlog.inventory.get_table_id(&flow_relname).ddlog_map_error()?;
        let flow_idxid = hddlog.inventory.get_index_id(&flow_relname).ddlog_map_error()?;
        let multicast_group_relname = format!("{module}::MulticastGroup");
        let multicast_group_relid = hddlog.inventory.get_table_id(&multicast_group_relname).ddlog_map_error()?;

        Ok(Config {
            p4info: p4info.clone(),
            module,
            cookie: fpc.get_cookie().get_cookie(),
            table_schemas,
            flow_idxid,
            flow_relid,
            multicast_group_relid,
        })
    }
}

struct State {
    hddlog: HDDlog,
    latch: Latch,
    pending_flow_mods: Vec<Ofpbuf>,

    // Configuration state.
    device_id: u64,
    config: Option<Config>,
    config_seqno: u64,

    // Table state.
    multicast_groups: HashMap<MulticastGroupId, BTreeSet<Replica>>,
    table_entries: HashMap<TableKey, TableValue>
}

impl State {
    fn new(hddlog: HDDlog, device_id: u64)
           -> State {
        let (pending_flow_mods, config, config_seqno,
             multicast_groups, table_entries) = Default::default();
        State {
            latch: Latch::new(),
            hddlog, device_id,
            pending_flow_mods, config, config_seqno, multicast_groups, table_entries,
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
                warn!("bad TableEntry {target:?} for read operation ({error:?})");
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

fn unary_result<T>(ctx: &RpcContext, sink: UnarySink<T>, result: Result<T, grpcio::RpcStatus>) {
    match result {
        Ok(reply) => unary_success(ctx, sink, reply),
        Err(status) => unary_fail(ctx, sink, status)
    }
}

fn server_streaming_fail<T>(ctx: &RpcContext, sink: ServerStreamingSink<T>, status: grpcio::RpcStatus) {
    let f = sink.fail(status)
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

fn server_streaming_result<T: Send + 'static>(
    ctx: &RpcContext,
    sink: ServerStreamingSink<T>,
    result: Result<Vec<T>, grpcio::RpcStatus>) {
    match result {
        Ok(reply) => server_streaming_success(ctx, sink, reply),
        Err(status) => server_streaming_fail(ctx, sink, status)
    }
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
        let config = &state.config.as_ref().unwrap();
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
                        relid: config.multicast_group_relid,
                        v: multicast_group_t {
                            mcast_id: mge.multicast_group_id as u16,
                            port: insertion.egress_port as u16
                        }.into_ddvalue()
                    });
                }
                for deletion in old_value.difference(new_value) {
                    commands.push(Update::DeleteValue {
                        relid: config.multicast_group_relid,
                        v: multicast_group_t {
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
                delta_to_flow_mods(&delta, config.flow_relid, &mut state.pending_flow_mods);
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
                let table = match config.table_schemas.get(&te.key.table_id) {
                    Some(table) => table,
                    None => Err(Error(RpcStatusCode::NOT_FOUND)).context(format!("unknown table {}", te.key.table_id))?
                };
                let table_name = format!("{}::{}", config.module, table.preamble.name.replace('.', "_"));
                let relid = state.hddlog.inventory.get_table_id(&table_name).ddlog_map_error()? as RelId;

                // Validate the operation.
                let old_value = state.table_entries.get(&te.key);
                Self::validate_write(op, old_value.is_some())?;

                // Commit the operation to DDlog.
                let mut commands = Vec::with_capacity(2);
                if let Some(old_value) = old_value {
                    let old_te = TableEntry { key: te.key.clone(), value: old_value.clone() };
                    let old_record = old_te.to_record(table, &table_name).unwrap();
                    commands.push(UpdCmd::Delete(RelIdentifier::RelId(relid), old_record));
                }
                if op != Update_Type::DELETE {
                    let new_record = te.to_record(table, &table_name).unwrap();
                    commands.push(UpdCmd::Insert(RelIdentifier::RelId(relid), new_record));
                }
                let delta = {
                    let hddlog = &state.hddlog;

                    hddlog.transaction_start().ddlog_map_error()?;
                    hddlog.apply_updates_dynamic(&mut commands.into_iter()).ddlog_map_error()?;
                    hddlog.transaction_commit_dump_changes().ddlog_map_error()?
                };
                delta_to_flow_mods(&delta, config.flow_relid, &mut state.pending_flow_mods);
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

    #[instrument(name = "Read", err, skip(self))]
    fn do_read(&mut self, req: ReadRequest) -> Result<Vec<ReadResponse>, grpcio::RpcStatus> {
        let _span = span!(Level::INFO, "read").entered();
        let state = self.state.lock().unwrap();
        if req.device_id != state.device_id {
            return Err(grpcio::RpcStatus::new(RpcStatusCode::NOT_FOUND));
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
        Ok(responses)
    }

    #[instrument(name = "Write", err, skip(self))]
    fn do_write(&mut self, req: WriteRequest) -> Result<WriteResponse, grpcio::RpcStatus> {
        let _span = span!(Level::INFO, "write").entered();
        let mut state = self.state.lock().unwrap();
        if req.device_id != state.device_id {
            return Err(grpcio::RpcStatus::new(RpcStatusCode::NOT_FOUND));
        }
        if state.config.is_none() {
            return Err(grpcio::RpcStatus::new(RpcStatusCode::FAILED_PRECONDITION));
        }

        // XXX role
        // XXX election_id
        // XXX atomicity

        let mut errors = Vec::with_capacity(req.updates.len());
        for proto::p4runtime::Update { field_type: op, entity, .. } in req.updates {
            let code = match Self::write_entity(op, entity.as_ref(), &mut state) {
                Err(error) => {
                    warn!("{error:?}");
                    match error.downcast_ref::<Error>() {
                        Some(Error(code)) => *code,
                        _ => RpcStatusCode::UNKNOWN
                    }
                },
                Ok(()) => RpcStatusCode::OK,
            };
            errors.push(code);
        }
        if errors.iter().all(|&code| code != RpcStatusCode::OK) {
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
            return Err(grpcio::RpcStatus::with_details(RpcStatusCode::UNKNOWN, Default::default(),
                                                       details.write_to_bytes().unwrap()));
        }

        return Ok(WriteResponse::new());
    }

    #[instrument(name = "GetForwardingPipelineConfig", err, skip(self))]
    fn do_get_forwarding_pipeline_config(&mut self, req: GetForwardingPipelineConfigRequest)
        -> Result<GetForwardingPipelineConfigResponse, grpcio::RpcStatus>
    {
        let state = self.state.lock().unwrap();
        if req.device_id != state.device_id {
            return Err(grpcio::RpcStatus::new(RpcStatusCode::NOT_FOUND));
        }
        let config = match state.config {
            Some(ref config) => config,
            None => return Err(grpcio::RpcStatus::new(RpcStatusCode::FAILED_PRECONDITION))
        };

        Ok(GetForwardingPipelineConfigResponse {
            config: Some(ForwardingPipelineConfig {
                p4info: Some(config.p4info.clone()).into(),
                cookie: Some(ForwardingPipelineConfig_Cookie {
                    cookie: config.cookie, ..Default::default()}).into(),
                ..Default::default()}).into(),
            ..Default::default()})
    }

    #[instrument(name = "SetForwardingPipelineConfig", err, skip(self))]
    fn do_set_forwarding_pipeline_config(&mut self, req: SetForwardingPipelineConfigRequest)
        -> Result<SetForwardingPipelineConfigResponse, grpcio::RpcStatus>
    {
        // XXX check action, device_id, role, election_id

        let mut state = self.state.lock().unwrap();
        match Config::new(req.get_config(), &state.hddlog) {
            Ok(config) => {
                state.config = Some(config);
                state.config_seqno += 1;
                state.latch.set();
                Ok(SetForwardingPipelineConfigResponse::new())
            },
            Err(_) => Err(grpcio::RpcStatus::new(RpcStatusCode::INVALID_ARGUMENT))
        }
    }

        }

impl<'a> P4Runtime for P4RuntimeService {
    fn write(&mut self, ctx: RpcContext, req: WriteRequest, sink: UnarySink<WriteResponse>) {
        unary_result(&ctx, sink, self.do_write(req));
    }

    fn read(&mut self,
            ctx: RpcContext,
            req: ReadRequest,
            sink: ServerStreamingSink<ReadResponse>) {
        server_streaming_result(&ctx, sink, self.do_read(req));
    }

    fn set_forwarding_pipeline_config(
        &mut self,
        ctx: RpcContext,
        req: SetForwardingPipelineConfigRequest,
        sink: UnarySink<SetForwardingPipelineConfigResponse>) {
        unary_result(&ctx, sink, self.do_set_forwarding_pipeline_config(req))
    }

    fn get_forwarding_pipeline_config(
        &mut self,
        ctx: RpcContext,
        req: GetForwardingPipelineConfigRequest,
        sink: UnarySink<GetForwardingPipelineConfigResponse>) {
        unary_result(&ctx, sink, self.do_get_forwarding_pipeline_config(req));
    }

    fn stream_channel(
        &mut self,
        ctx: RpcContext,
        mut stream: RequestStream<StreamMessageRequest>,
        mut sink: DuplexSink<StreamMessageResponse>) {
        let f = async move {
            while let Some(n) = stream.try_next().await? {
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
                    ctx: RpcContext,
                    _req: CapabilitiesRequest,
                    sink: UnarySink<CapabilitiesResponse>) {
        let response = CapabilitiesResponse {
            p4runtime_api_version: String::from("1.3.0"),
            ..Default::default()
        };
        unary_success(&ctx, sink, response);
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

// This should be Record::as_string() but it's not there due to an oversight.
// (This will get fixed in a later DDlog version.)
fn record_as_string(r: &Record) -> Option<&String> {
    match r {
        Record::String(ref s) => Some(s),
        _ => None
    }
}

fn flow_record_to_string(record: &Record) -> Option<&String> {
    record_as_string(record.get_struct_field("flow")?)
}

fn flow_record_to_flow_mod(record: &Record) -> Result<FlowMod> {
    let flow = flow_record_to_string(&record).ok_or(anyhow!("Flow record {record} lacks 'flow' field"))?;
    match FlowMod::parse(flow, Some(FlowModCommand::Add)) {
        Ok((flow, _)) => Ok(flow),
        Err(s) => Err(anyhow!("{flow}: {s}"))
    }
}

#[derive(Parser, Debug)]
#[clap(version, about)]
struct Args {
    /// OVS remote to connect, e.g. "unix:/path/to/ovs/tutorial/sandbox/br0.mgmt"
    ovs_remote: String,

    /// P4Runtime connection listening port
    #[clap(long, default_value = "50051")]
    p4_port: u16,

    /// P4Runtime connection bind address
    #[clap(long, default_value = "127.0.0.1")]
    p4_addr: String,

    /// P4Runtime device ID
    #[clap(long, default_value = "1")]
    device_id: u64,

    #[clap(flatten)]
    daemonize: Daemonize,

    /// File to write logs to
    #[clap(long)]
    log_file: Option<PathBuf>,

    /// File to write DDlog replay log to
    #[clap(long)]
    ddlog_record: Option<PathBuf>
}

// Runs the server main loop, servicing P4Runtime requests from `state` and applying them to OVS
// via `rconn`.  After initialization completes, finishes daemonization using `daemonizing`, if it
// is not `None`.
fn run_server(state: Arc<Mutex<State>>, mut rconn: Rconn, mut daemonizing: Option<Daemonizing>) -> Result<()> {
    let mut last_connection_seqno = 0;
    let mut last_config_seqno = 0;
    let mut bundle_id = 0;
    loop {
        rconn.run();
        while let Some(msg) = rconn.recv() {
            match OfpType::decode(msg.as_slice()) {
                Ok(OfpType(ovs::sys::ofptype_OFPTYPE_BUNDLE_CONTROL)) => {
                    if let Ok(bcm) = BundleCtrlMsg::decode(msg.as_slice()) {
                        if bcm.type_ == OFPBCT_COMMIT_REPLY {
                            // Our request to commit a bundle succeeded or failed.  Either way,
                            // we've finished starting up, so it's time to detach from the
                            // foreground session.
                            if let Some(daemonizing) = daemonizing.take() {
                                daemonizing.finish();
                            }
                        }
                    }
                },
                _ => println!("received message {}", ovs::ofp_print::Printer(msg.as_slice()))
            }
        }

        state.lock().unwrap().latch.poll();
        if rconn.connected() {
            let mut state = state.lock().unwrap();

            let flags = ovs::ofp_bundle::OFPBF_ATOMIC | ovs::ofp_bundle::OFPBF_ORDERED;
            if rconn.connection_seqno() == last_connection_seqno &&
                state.config_seqno == last_config_seqno
            {
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
                let mut flow_mods = Vec::new();
                flow_mods.push(FlowMod::parse("", Some(FlowModCommand::Delete { strict: false })).unwrap().0);
                if let Some(ref config) = state.config {
                    flow_mods.extend(state.hddlog.dump_index_dynamic(config.flow_idxid).unwrap().into_iter()
                                     .filter_map(|record| match flow_record_to_flow_mod(&record) {
                                         Ok(fm) => Some(fm),
                                         Err(err) => { event!(Level::ERROR, "flow failed to parse: {err}"); None }
                                     }));
                };

                // Encode the flow_mods into OpenFlow and send them.
                let flow_mods = flow_mods.into_iter().map(|fm| fm.encode(OFP_PROTOCOL));
                bundle_id += 1;
                let bundle = ovs::ofp_bundle::BundleSequence::new(bundle_id, flags, OFP_VERSION, flow_mods);
                for msg in bundle {
                    rconn.send(msg).unwrap();
                }

                last_connection_seqno = rconn.connection_seqno();
                last_config_seqno = state.config_seqno;
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

fn main() -> Result<()> {
    log_panics::init();
    let Args { ovs_remote, p4_port, p4_addr, device_id,
               daemonize, log_file, ddlog_record } = Args::parse();
    if let Some(log_file) = log_file {
        let writer = OpenOptions::new().create(true).append(true).open(log_file)?;
        tracing_subscriber::fmt()
            .with_writer(writer)
            .with_ansi(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_writer(stderr)
            .with_ansi(unsafe { libc::isatty(libc::STDERR_FILENO) } == 1)
            .init();
    };
    grpcio::redirect_log();
    let (daemonizing, _cleanup) = unsafe { daemonize.start() };
    let daemonizing = Some(daemonizing);

    let env = Arc::new(Environment::new(1));
    let (mut hddlog, _init_state) = ofp4dl_ddlog::run(1, false).ddlog_map_error()?;
    if let Some(ref ddlog_record) = ddlog_record {
        let mut record = Some(File::create(ddlog_record).with_context(|| format!("{}: open failed", ddlog_record.display()))?);
        hddlog.record_commands(&mut record);
    }

    let state = Arc::new(Mutex::new(State::new(hddlog, device_id)));
    let service = create_p4_runtime(P4RuntimeService::new(state.clone()));
    let ch_builder = ChannelBuilder::new(env.clone());
    let mut server = ServerBuilder::new(env)
        .register_service(service)
        .bind(p4_addr, p4_port)
        .channel_args(ch_builder.build_args())
        .build()
        .unwrap();
    server.start();

    if p4_port == 0 {
        for (addr, port) in server.bind_addrs() {
            event!(Level::INFO, "Listening on {addr}:{port}");
        }
    }

    let mut rconn = Rconn::new(0, 0, ovs::rconn::DSCP_DEFAULT, OFP_VERSION.into());
    rconn.connect(&ovs_remote, None);

    run_server(state, rconn, daemonizing)
}

/// Converts the `delta` of changes to DDlog output relations (particularly `Flow`) into OpenFlow
/// [`FlowMod`] messages and appends those messages to `flow_mods`.
fn delta_to_flow_mods(delta: &DeltaMap<DDValue>,
                      flow_relid: RelId,
                      flow_mods: &mut Vec<Ofpbuf>) {
    for (&rel, changes) in delta.iter() {
        if rel == flow_relid {
            for (val, weight) in changes.iter() {
                let command = match weight {
                    1 => FlowModCommand::Add,
                    -1 => FlowModCommand::Delete { strict: true },
                    _ => unreachable!()
                };

                let flow = flow_t::from_ddvalue_ref(val);
                match FlowMod::parse(&flow.flow, Some(command)) {
                    Ok((flow_mod, _)) => flow_mods.push(flow_mod.encode(OFP_PROTOCOL)),
                    Err(s) => warn!("{flow}: {s}")
                };
            }
        }
    }
}
