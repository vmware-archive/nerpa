/*!
Converter between the DDlog control plane and P4 data plane.

In the Nerpa programming framework, the control plane is incremental
and uses a [Differential Datalog](https://github.com/vmware/differential-datalog)
program. The data plane is programmed in [P4](https://p4.org/) and uses
the [P4 Runtime API](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html).
This crate interfaces between the two. It converts DDlog output relations
into the appropriate type of P4 entry, and sends that to the switch.
*/
#![warn(missing_docs)]
/*
Copyright (c) 2021 VMware, Inc.
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

extern crate grpcio;
extern crate proto;
extern crate protobuf;

use num_traits::cast::ToPrimitive;

use differential_datalog::api::HDDlog;

use differential_datalog::{
    DDlog,
    DDlogDynamic,
    DeltaMap
}; 
use differential_datalog::ddval::DDValue;
use differential_datalog::program::Update;
use differential_datalog::record::{
    CollectionKind,
    IntoRecord,
    Name,
    Record,
};

use dp2ddlog::{digest_to_ddlog, packet_in_to_ddlog};

use futures::{SinkExt, StreamExt};
use grpcio::{ClientDuplexReceiver, StreamingCallSink};

use p4ext::{
    ActionRef,
    MatchField,
    MatchType,
    Table
};

use proto::p4runtime::{
    Action,
    Action_Param,
    FieldMatch,
    MasterArbitrationUpdate,
    StreamMessageRequest,
    StreamMessageResponse,
    TableAction,
};
use proto::p4runtime_grpc::P4RuntimeClient;
use protobuf::Message;

use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::OsStr,
    fmt,
    fs::File,
    sync::Arc,
};
use tokio::sync::{oneshot, mpsc};
use tokio::time::{Duration, sleep};
use tracing::{debug, error, instrument};

/// Public handle for the Tokio tasks.
/// The Tokio task either processes DDlog inputs or pushes outputs to the switch.
#[derive(Clone, Debug)]
pub struct Controller {
    /// Sends messages to an asynchronously running actor.
    sender: mpsc::Sender<ControllerActorMessage>,
}

impl Controller {
    /// Create a new handle for Tokio tasks.
    ///
    /// Passes `switch_client` and `hddlog` to a `ControllerActor`, which allows interaction with the P4 switch and DDlog program, respectively. Runs the actor asynchronously.
    ///
    /// # Arguments
    /// * `switch_client` - P4 Runtime client with extra information.
    /// * `hddlog` - DDlog program.
    pub fn new(
        switch_client: SwitchClient,
        hddlog: Arc<HDDlog>,
    ) -> Result<Controller, String> {
        let (sender, receiver) = mpsc::channel(1000);
        let program = ControllerProgram::new(hddlog);

        let mut actor = ControllerActor::new(receiver, switch_client, program);
        tokio::spawn(async move { actor.run().await });

        Ok(Self{sender})
    }

    /// Stream inputs from OVSDB and from the data plane.
    ///
    /// Send a message to the `ControllerActor`. On receipt, the actor starts streaming inputs.
    ///
    /// # Arguments
    /// * `hddlog` - DDlog program.
    /// * `server` - Filepath for OVSDB server.
    /// * `database` - Name of OVSDB.
    #[instrument]
    pub async fn stream_inputs(
        &self,
        hddlog: Arc<HDDlog>,
        server: String,
        database: String,
    ) {
        // The oneshot channel keeps the Actor running that processes inputs.
        // It closes when the Actor task is killed.
        let (send, recv) = oneshot::channel();
        let msg = ControllerActorMessage::InputMessage {
            _respond_to: send,
            hddlog,
            server,
            database,
        };

        let message_res = self.sender.send(msg).await;
        if message_res.is_err() {
            error!("could not send message to controller actor: {:#?}", message_res);
        }

        recv.await.expect("Actor task has been killed");
    }
}

/// Handle to the running DDlog program.
#[derive(Debug)]
pub struct ControllerProgram {
    hddlog: Arc<HDDlog>,
}

impl ControllerProgram {
    /// Create a handle to a DDlog program.
    ///
    /// # Arguments
    /// * `hddlog` - DDlog program.
    pub fn new(hddlog: Arc<HDDlog>) -> Self {
        Self{hddlog}
    }

    /// Apply `updates` to the DDlog program.
    ///
    /// This starts a new transaction and attempts to apply updates. If successful, it commits the transaction.
    /// Else, it rolls the transaction back and returns an error.
    ///
    /// # Arguments
    /// * `updates` - vector of Updates to apply to the DDlog program.
    #[tracing::instrument]
    pub fn apply_updates(
        &mut self,
        updates:Vec<Update<DDValue>>
    ) -> Result<DeltaMap<DDValue>, String> {
        self.hddlog.transaction_start()?;

        match self.hddlog.apply_updates(&mut updates.into_iter()) {
            Ok(_) => {},
            Err(e) => {
                error!("applying updates had following error: {:#?}", e);
                self.hddlog.transaction_rollback()?;
                return Err(e);
            }
        };

        self.hddlog.transaction_commit_dump_changes()
    }
}

/// P4 Runtime client.
//
// This is a "newtype" style struct, so we can define `Debug` on it.
pub struct P4RC(P4RuntimeClient);

impl fmt::Debug for P4RC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("P4RuntimeClient")
         .finish()
    }
}

/// Sink to send messages using the P4 Runtime API over StreamChannel.
//
//  This is a "newtype" style struct, so we can define `Debug` on it.
pub struct PacketSink(StreamingCallSink<StreamMessageRequest>);

impl fmt::Debug for PacketSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PacketSink")
         .finish()
    }
}

/// Sends messages to the P4 Runtime switch.
#[derive(Debug)]
pub struct SwitchClient {
    // Includes necessary information to configure the switch and to send packets to the switch without unnecessary extra computation.
    //
    /// The P4 Runtime Client as a newtype for debugging.
    pub client: P4RC,
    p4info: String,
    device_id: u64,
    role_id: u64,
    target: String,
    // Using P4 Info, map each PacketMetadata field to its id.
    // This is used as a cache for metadata for P4 Runtime PacketOuts.
    packet_meta_field_to_id: HashMap<String, u32>,
    packet_sink: PacketSink,
}

impl SwitchClient {
    /// Return a P4 Runtime switch client, with extra information for easier communication.
    ///
    /// # Arguments
    /// * `client` - P4 Runtime client.
    /// * `p4info` - Filepath for P4info binary file.
    /// * `json` - Filepath for JSON representation of compiled P4 program.
    /// * `cookie` - Metadata used by the control plane to identify a forwarding pipeline configuration.
    /// * `action` - Configuration action for the forwarding pipeline.
    /// * `device_id` - ID of the P4-enabled device.
    /// * `role_id` - the desired role ID for the controller
    /// * `target` - hardware/software entity hosting P4 Runtime (e.g., "localhost:50051"). Used for logging.
    pub async fn new(
        client: P4RuntimeClient,
        p4info: String,
        json: String,
        cookie: String,
        action: String,
        device_id: u64,
        role_id: u64,
        target: String,
    ) -> Self {
        p4ext::set_pipeline_config(
            &p4info,
            &json,
            &cookie,
            &action,
            device_id,
            role_id,
            &target,
            &client
        );

        // Load a P4info struct from file to cache any necessary data structures.
        let mut p4info_file = File::open(OsStr::new(&p4info))
            .unwrap_or_else(|err| panic!("{}: could not open P4Info ({})", p4info, err));
        let p4info_struct: proto::p4info::P4Info = Message::parse_from_reader(&mut p4info_file)
            .unwrap_or_else(|err| panic!("{}: could not read P4Info ({})", p4info, err));

        // Map packet metadata field names to packet_ids.
        // We do this in the constructor, to avoid computation per packet sent to the dataplane.
        let mut packet_meta_field_to_id = HashMap::new();
        for cm in p4info_struct.get_controller_packet_metadata().iter() {
            if cm.get_preamble().get_name().eq("packet_out") {
                for m in cm.get_metadata().iter() {
                    packet_meta_field_to_id.insert(
                        m.get_name().to_string(),
                        m.get_id()
                    );
                }
            }
        }

        // Establish a connection to the switch to send packets.
        let (mut sink, _receiver) = client.stream_channel().unwrap();
        // Send a master arbitration update to establish this as backup with election id 1.
        // The Tokio actor handling messages from the dataplane has a StreamChannel with election id 0.
        use proto::p4runtime::Uint128;
        let mut election_id = Uint128::new();
        election_id.set_high(0);
        election_id.set_low(1);

        let mut upd = MasterArbitrationUpdate::new();
        upd.set_device_id(device_id);
        upd.set_election_id(election_id);

        let mut req = StreamMessageRequest::new();
        req.set_arbitration(upd);

        // Send the master arbitration update request to the switch.
        // Retry using exponential backoff.
        // TODO: Decompose this retry into a separate function.
        let mut retries = 5;
        let mut wait = 1000; // milliseconds
        loop {
            match sink.send((req.clone(), grpcio::WriteFlags::default())).await {
                Err(e) => {
                    if retries > 0 {
                        error!("failed to configure backup stream through master arbitration: {:#?}", e);

                        retries -= 1;
                        sleep(Duration::from_secs(wait)).await;
                        wait *= 2;
                    }
                },
                Ok(_) => break,
            }
        };

        // Wrap types from external crates in newtypes.
        let p4rc = P4RC(client);
        let packet_sink = PacketSink(sink);


        Self {
            client: p4rc,
            p4info,
            device_id,
            role_id,
            target,
            packet_meta_field_to_id,
            packet_sink,
        }
    }

    /// Configure the digest notification level on the switch.
    ///
    /// The `DigestEntry` configuration is described [here](https://p4.org/p4-spec/p4runtime/main/P4Runtime-Spec.html#sec-digestentry).
    ///
    /// # Arguments
    /// * `max_timeout_ns`: maximum server buffering delay in nanoseconds for an outstanding digest.
    /// * `max_list_size`: maximum number of digest messages in a single Protobuf message from the server.
    /// * `ack_timeout_ns`: time in nanoseconds for the server to wait for acknowledgement.
    pub async fn configure_digests(
        &mut self,
        max_timeout_ns: i64,
        max_list_size: i32,
        ack_timeout_ns: i64,
    ) -> Result<(), p4ext::P4Error> {
        // Read P4Info from file.
        let p4info_str: &str = &self.p4info;
        let mut p4info_file = File::open(OsStr::new(p4info_str))
            .unwrap_or_else(|err| panic!("{}: could not open P4Info ({})", p4info_str, err));
        let p4info: proto::p4info::P4Info = Message::parse_from_reader(&mut p4info_file)
            .unwrap_or_else(|err| panic!("{}: could not read P4Info ({})", p4info_str, err));

        // Write updates for each digest.
        let mut digest_updates = Vec::new();
        for d in p4info.get_digests().iter() {
            digest_updates.push(
                p4ext::build_digest_entry_update(
                    d.get_preamble().get_id(),
                    max_timeout_ns,
                    max_list_size,
                    ack_timeout_ns
                )
            );
        }

        let digest_res = p4ext::write(
            digest_updates,
            self.device_id,
            self.role_id,
            &self.target,
            &self.client.0
        );

        if digest_res.is_err() {
            let e = digest_res.err().unwrap(); // safe because of `is_err` check
            error!("writing digest updates failed with error: {:#?}", e);
            return Err(e);
        }

        Ok(())
    }

    /// Push DDlog outputs as table entries in the P4-enabled switch.
    ///
    /// # Arguments
    /// * `delta` - DDlog output relations.
    #[instrument]
    pub async fn push_outputs(&mut self, delta: &DeltaMap<DDValue>) -> Result<(), p4ext::P4Error> {
        let mut updates = Vec::new();
        let mut packet_outs = Vec::new();

        let pipeline = p4ext::get_pipeline_config(self.device_id, &self.target, &self.client.0);
        let switch: p4ext::Switch = pipeline.get_p4info().into();

        for (_, output_map) in (*delta).clone().into_iter() {
            for (value, weight) in output_map {
                let record = value.clone().into_record();
                
                match record {
                    Record::NamedStruct(output_name, output_records) => {
                        // Check if the record corresponds to the multicast group.
                        // We assume that there a relevant DDlog relation's name includes "multicast".
                        // A DDlog relation that does not update multicast should not include "multicast" in its name.
                        if output_name.as_ref().to_lowercase().contains("multicast") {
                            self.update_multicast(output_records.clone(), weight).await;
                        }

                        // Check for output relations that contain packets as Records.
                        // Convert those packets to byte-vectors, and add them to the packet queue.
                        // This queue is sent after updates are pushed to the switch.
                        if output_name.as_ref().to_lowercase().contains("packet") {
                            // The output record corresponding to a packet should be a Record::Array.
                            // Any other output records correspond to fields in the 'packet_out' header.
                            // These are stored as PacketMetadata.
                            let mut payload = Vec::new();
                            let mut metadata_vec = Vec::new();

                            for output_record in output_records.iter() {
                                // One output record must be a NamedStruct corresponding to the packet.
                                // We convert its Array to a Vec<u8> and use it as the payload in a P4 Runtime PacketOut.
                                if let (output_record_name, Record::Array(array_kind, array_records)) = output_record {
                                    if output_record_name
                                        .as_ref()
                                        .to_lowercase()
                                        .contains("packet")
                                    && array_kind == &CollectionKind::Vector {
                                        for array_record in array_records.iter() {
                                            payload.append(&mut Self::record_to_bytestring(array_record));
                                        }
                                    }
                                }
                                // All other records correspond to PacketMetadata fields.
                                // These are fields in the P4 struct with the "packet_out" header.
                                else {
                                    let (meta_record_name, meta_record) = output_record;
                                    let meta_record_key = meta_record_name.to_string();
                                    let metadata_id = self.packet_meta_field_to_id[&meta_record_key];
                                    let metadata_value = Self::record_to_bytestring(meta_record);

                                    let mut metadata = proto::p4runtime::PacketMetadata::new();
                                    metadata.set_metadata_id(metadata_id);
                                    metadata.set_value(metadata_value);

                                    metadata_vec.push(metadata);
                                }
                            }

                            // If a non-zero payload was found, construct and append a PacketOut to the packet queue.
                            if !payload.is_empty() {
                                let mut packet_out = proto::p4runtime::PacketOut::new();
                                packet_out.set_payload(payload);
                                packet_out.set_metadata(protobuf::RepeatedField::from_vec(metadata_vec));

                                packet_outs.push(packet_out);
                            }
                        }

                        // Translate the record table name to the P4 table name.
                        let table = match Self::get_matching_table(output_name.to_string(), switch.tables.clone()) {
                            Some(t) => t,
                            None => continue,
                        };
                        let table_id = table.preamble.id;

                        let mut action_opt: Option<TableAction> = None;
                        let mut field_match_vec = Vec::<FieldMatch>::new();
                        let mut priority: i32 = 0;

                        // Iterate over all output records, processing action, priority, and match fields.
                        for (rec_name, record) in output_records.iter() {
                            let match_name = rec_name.to_string();

                            match match_name.as_str() {
                                "action" => {
                                    match record {
                                        Record::NamedStruct(name, action_recs) => {
                                            action_opt = Self::record_to_action(
                                                name,
                                                action_recs.to_vec(),
                                                table.actions.clone(),
                                            );
                                        },
                                        _ => debug!("action relation was not NamedStruct")
                                    }
                                },
                                "priority" => {
                                    priority = Self::record_to_u128(record) as i32
                                },
                                _ => {
                                    // Find a match field with the matching name.
                                    let matching_mfs: Vec<MatchField> = table.match_fields
                                        .iter()
                                        .filter(|m| m.preamble.name == match_name)
                                        .cloned()
                                        .collect();
                                    if matching_mfs.len() != 1 {
                                        continue;
                                    }
                                    let mf = &matching_mfs[0];

                                    let fm_opt = Self::record_to_match(record, mf);
                                    if fm_opt.is_some() {
                                        field_match_vec.push(fm_opt.unwrap());
                                    }
                                },
                            }
                        }

                        // If we found a table and action, construct a P4 table entry update.
                        if let Some(table_action) = action_opt {
                            let update = p4ext::build_table_entry_update(
                                proto::p4runtime::Update_Type::INSERT,
                                table_id,
                                table_action,
                                field_match_vec,
                                priority,
                            );
                            updates.push(update);
                        }
                    },
                    _ => {
                        debug!("output record was not NamedStruct");
                        continue;
                    }
                }
            }
        }

        let write_res = p4ext::write(
            updates,
            self.device_id,
            self.role_id,
            &self.target,
            &self.client.0,
        );
        if write_res.is_err() {
            error!("could not write updates to P4 Runtime: {:#?}",  write_res.as_ref().err());
            return write_res;
        }

        // Send packets found in output relations to the switch.
        if !packet_outs.is_empty() {
            // Send packets to the switch.
            for packet_out in packet_outs {
                let mut req = StreamMessageRequest::new();
                req.set_packet(packet_out);

                let req_res = self.packet_sink.0.send((req, grpcio::WriteFlags::default())).await;
                if req_res.is_err() {
                    error!("failed to send request over stream channel: {:#?}", req_res.err());
                }
            }
        }

        Ok(())
    }

    /// Update the multicast group entry using P4 Runtime.
    ///
    /// # Arguments
    /// * `recs` - Vector of tuples of (Name, Record). The second element in a NamedStruct.
    /// Expected to have length 2, one record representing the ID and the other the port.
    /// The ID record should be an Int. Its name should include "id" (not case-sensitive).
    /// The port record name should be an Int. Its name should include "port" (not case-sensitive).
    ///
    /// * `weight` - The weight from the DDlog output record.
    /// A positive weight represents an insert/modify. A negative weight represents a delete.
    async fn update_multicast(
        &mut self,
        recs: Vec<(Cow<'static, str>, Record)>,
        weight: isize,
    ) {
        if recs.len() != 2 {
            error!("multicast relation should include exactly 2 fields!");
            return;
        }

        // P4 Runtime requires multicast ID greater than 0 for a valid write,
        // so it can be used as a sentinel value.
        let mut mcast_id: u32 = 0;

        // Since port is 16-bit, the maximum u32 can be used as a sentinel for the port.
        let mut mcast_port: u32 = u32::MAX;

        for (k, v) in recs.iter() {
            let rec_name = k.as_ref().to_lowercase();
            if rec_name.contains("id") {
                mcast_id = Self::record_to_u128(v) as u32
            } else if rec_name.contains("port") {
                mcast_port = Self::record_to_u128(v) as u32
            } else {
                error!("multicast relation field named {} did not include port or id", rec_name);
            }
        }

        if mcast_id == 0 {
            error!("multicast relation does not contain an 'id' field");
            return;
        }

        if mcast_port == u32::MAX {
            error!("multicast relation does not contain a 'port' field");
            return;
        }

        // We read all current multicast entities using group id 0.
        // We then find the replicas for the desired multicast group.
        // Since this search is wild-carded, we can safely unwrap the result.
        let mcast_entries = p4ext::read(
            vec![p4ext::build_multicast_read(0)],
            self.device_id,
            &self.client.0,
        ).await.unwrap();

        // We find the replicas for the current multicast group.
        let mut replicas = Vec::new();
        for mcast_ent in mcast_entries.iter() {
            let mge = mcast_ent
                .get_packet_replication_engine_entry()
                .get_multicast_group_entry();
            if mge.get_multicast_group_id() == mcast_id {
                replicas = mge.get_replicas().to_vec();
            }
        }

        // No replicas means this is a new multicast group.
        // In this case, the update type is an INSERT.
        // Else, it is a MODIFY.
        let mcast_update_type = if replicas.is_empty() {
            proto::p4runtime::Update_Type::INSERT
        } else {
            proto::p4runtime::Update_Type::MODIFY
        };

        // A non-negative weight means we insert this port in the multicast group.
        // Else, we delete this port from the multicast group.
        if weight >= 0 {
            let mut new_replica = proto::p4runtime::Replica::new();
            new_replica.set_egress_port(mcast_port);

            let new_replica_instance: u32 = replicas.len() as u32 + 1;
            new_replica.set_instance(new_replica_instance);

            replicas.push(new_replica);
        } else {
            // Sort the replicas in increasing order of instance.
            replicas.sort_by(|a, b| a.instance.cmp(&b.instance));

            // Adjust the instance for replicas with different port.
            // This avoids gaps in the ordering of replicas.
            let mut num_deleted = 0;
            for r in replicas.iter_mut() {
                if r.egress_port == mcast_port {
                    num_deleted += 1;
                } else {
                    r.instance -= num_deleted;
                }
            }

            // Remove replicas with matching port.
            replicas.retain(|r| r.egress_port != mcast_port);
        }

        // Push the multicast update to the switch.
        let mcast_update = p4ext::build_multicast_write(
            mcast_update_type,
            mcast_id,
            replicas,
        );

        let write_res = p4ext::write(
            vec![mcast_update],
            self.device_id,
            self.role_id,
            &self.target,
            &self.client.0,
        );
        if write_res.is_err() {
            error!("could not push multicast update to switch: {:#?}", write_res.err());
        }
    }

    /// Convert a DDlog Record and P4Info Actions to a P4Runtime TableAction.
    ///
    /// `record_name` and `record_actions` are a destructured `Record::NamedStruct` and represent P4 actions.
    /// Each item in `record_actions` represents a P4 action's name and arguments as a `Record::NamedStruct`.
    ///
    /// # Arguments
    /// * `record_name` - the top-level Name of a `NamedStruct`.
    /// * `record_actions` - the Records of a `NamedStruct`, corresponding to P4 Actions.
    /// * `action_refs` - the actions in a P4 table. 
    fn record_to_action(
        record_name: &Name,
        record_actions: Vec<(Name, Record)>,
        action_refs: Vec<ActionRef>,
    ) -> Option<TableAction> {
        // Find the matching action reference in the actions, formatted as per P4Info.
        // If no match exists, early-return None.
        let action_ref_opt: Option<ActionRef> = {
            let mut action_ref_opt: Option<ActionRef> = None;

            for action_ref in action_refs.iter() {
                let action_name = &action_ref.action.preamble.name;
                let action_vec = action_name
                    .split('.')
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>();
                
                let action_substr_opt = action_vec.last();
                if let Some(action_substr) = action_substr_opt {
                    if record_name.contains(action_substr) {
                        action_ref_opt = Some(action_ref.clone());
                    }
                }
            }

            action_ref_opt
        };

        if action_ref_opt.is_none() {
            debug!("could not find action matching record name: {:#?}", record_name.as_ref());
        }

        let action_ref = action_ref_opt.as_ref()?;

        // Store the values corresponding to each parameter in the action.
        // Iterate through action param records, and map each name to a value as a byte-vector.
        let mut action_params_map = HashMap::<String, Vec<u8>>::new();
        for (ra_name, ra_record) in record_actions.iter() {
            action_params_map.insert(
                ra_name.to_string(),
                Self::record_to_bytestring(ra_record)
            );
        }

        let action = &action_ref.action;
        let action_id = action.preamble.id;

        // Convert the DDlog Record to P4Runtime Action_Params.
        let mut params_vec = Vec::<Action_Param>::new();
        for param in &action.params {
            let mut action_param = Action_Param::new();
            action_param.set_param_id(param.preamble.id);

            let param_value = action_params_map[&param.preamble.name].clone();
            action_param.set_value(param_value);

            params_vec.push(action_param);
        }

        // Define the Action and TableAction.
        let mut action = Action::new();
        action.set_action_id(action_id);
        action.set_params(protobuf::RepeatedField::from_vec(params_vec));

        let mut table_action = TableAction::new();
        table_action.set_action(action);
        
        Some(table_action)
    }

    /// Convert a DDlog Record, using P4Info MatchFields, to a P4Runtime FieldMatch.
    /// Returns None for a "don't-care", because FieldMatch must be omitted in this case.
    ///
    /// # Arguments
    /// * `r` - the Record representing a field match.
    /// * `match_field` - the P4 Info for the field match.
    fn record_to_match(
        r: &Record,
        match_field: &MatchField,
    ) -> Option<FieldMatch> {
        let mut field_match = FieldMatch::new();
        field_match.set_field_id(match_field.preamble.id);

        match match_field.match_type {
            MatchType::Exact => {
                // In an Exact match, we convert the record value to a byte-vector.
                let mut exact_match = proto::p4runtime::FieldMatch_Exact::new();
                exact_match.set_value(Self::record_to_bytestring(r));
                field_match.set_exact(exact_match);
            },
            MatchType::Lpm => {
                let mut lpm_match = proto::p4runtime::FieldMatch_LPM::new();

                // The value for an LPM match should be a Tuple.
                // If not, return None.
                if let Record::Tuple(t) = r {
                    if t.len() != 2 {
                        error!("match field LPM Tuple had len {}, expected 2", t.len());
                        return None;
                    }

                    let prefix_len = Self::record_to_u128(&t[1]);
                    if prefix_len == 0 {
                        return None;
                    }
                    
                    let value = Self::record_to_bytestring(&t[0]);
                    lpm_match.set_value(value);

                    lpm_match.set_prefix_len(prefix_len as i32);

                    field_match.set_lpm(lpm_match);
                } else {
                    error!("Record for a Field Match of type LPM must be a Tuple");
                    return None;
                }
            },
            MatchType::Ternary => {
                let mut ternary_match = proto::p4runtime::FieldMatch_Ternary::new();

                // The value for a Ternary match should be a Tuple.
                // If not, return None.
                if let Record::Tuple(t) = r {
                    if t.len() != 2 {
                        error!("match field Ternary Tuple had len {}, expected 2", t.len());
                        return None;
                    }

                    // TODO: Check if we need to left-pad value/mask.
                    let value = Self::record_to_bytestring(&t[0]);
                    ternary_match.set_value(value);

                    let mask =Self::record_to_u128(&t[1]);
                    if mask == 0 {
                        return None
                    }
                    ternary_match.set_mask(Self::u128_to_bytestring(mask));
                } else {
                    error!("Record for a Field Match of type Ternary must be a Tuple");
                    return None;
                }

                field_match.set_ternary(ternary_match);
            },
            MatchType::Range => {
                let mut range_match = proto::p4runtime::FieldMatch_Range::new();

                // The value for a Range match should be a Tuple. If not, return None.
                if let Record::Tuple(t) = r {
                    if t.len() != 2 {
                        error!("match field Range Tuple had len {}, expected 2", t.len());
                        return None;
                    }

                    // XXX check for don't-care

                    let low = Self::record_to_bytestring(&t[0]);
                    range_match.set_low(low);

                    let high = Self::record_to_bytestring(&t[1]);
                    range_match.set_high(high);
                } else {
                    error!("Record for a Field Match of type Range must be a Tuple");
                    return None;
                }

                field_match.set_range(range_match);
            },
            MatchType::Optional => {
                let mut optional_match = proto::p4runtime::FieldMatch_Optional::new();

                // The value for an Optional match should be a NamedStruct. If not, return None.
                if let Record::NamedStruct(record_name, record_value) = r {
                    let value = match record_name.to_string().as_str() {
                        "ddlog_std::Some" => Self::record_to_bytestring(&record_value[0].1),
                        "ddlog_std::None" => return None,
                        _ => {
                            return None; // XXX
                        }
                    };

                    optional_match.set_value(value);
                } else {
                    error!("Record for a Field Match of type Optional must be a NamedStruct");
                    return None;
                }

                field_match.set_optional(optional_match);
            },
            // Includes unspecified and other types.
            _ => {
                let mut other = protobuf::well_known_types::Any::new();

                let value = Self::record_to_bytestring(r);
                other.set_value(value);
                field_match.set_other(other);
            }
        }
        
        Some(field_match)
    }

    /// Extracts and returns a numerical value from a DDlog record.  Only properly supports numeric
    /// types (like boolean and integer), and returns 0 for everything else.
    ///
    /// # Arguments
    /// * `r` - the record to convert.
    fn record_to_u128(r: &Record) -> u128 {
        // TODO: Handle additional possible Record values.
        match r {
            Record::Bool(b) => return if *b { 1 } else { 0 },
            Record::Int(i) => match (*i).to_u128() {
                Some(value) => return value,
                None => error!("attempted to extract out-of-range field value {}", i)
            }
            _ => error!("attempted to extract value from unsupported record type: {:#?}", r),
        }
        0
    }

    /// Converts a `u128` into a bytestring as specified in P4Runtime 1.3.0 section 8.4
    /// "Bytestrings".  This representation uses the minimum number of bytes to represent a given
    /// number in big-endian order.  (As an exception to the minimum-length rule, zero is
    /// represented by a single 0-byte).
    ///
    /// # Arguments
    /// * `r` - the value to convert.
    fn u128_to_bytestring(mut value: u128) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        loop {
            v.push((value & 0xff) as u8);
            value >>= 8;
            if value == 0 {
                v.reverse();
                return v
            }
        }
    }

    /// Convert a DDlog record's value into a bytestring as specified in P4Runtime 1.3.0 section
    /// 8.4 "Bytestrings".  This representation uses the minimum number of bytes to represent a
    /// given number in big-endian order.  (As an exception to the minimum-length rule, zero is
    /// represented by a single 0-byte).
    ///
    /// Only supports numeric types (like boolean and integer).
    /// This returns an empty byte vector for an unsupported type.
    ///
    /// # Arguments
    /// * `r` - the record to convert.
    fn record_to_bytestring(r: &Record) -> Vec<u8> {
        Self::u128_to_bytestring(Self::record_to_u128(r))
    }

    /// Retrieve a P4 table with the provided name.
    ///
    /// # Arguments
    /// * `record_name` - DDlog record name, which is the P4 table alias.
    /// * `tables` - all P4 tables, from P4info.
    fn get_matching_table(record_name: String, tables: Vec<Table>) -> Option<Table> {
        // TODO: Compare the record name with table alias. That should simplify this function.
        for t in tables {
            let tn = &t.preamble.name;
            let tv: Vec<String> = tn.split('.').map(|s| s.to_string()).collect();
            let ts = tv.last().unwrap();

            if record_name.contains(ts) {
                return Some(t);
            }
        }

        None
    }
}

/// Processes DDlog input relations and pushes them to the P4 switch.
struct ControllerActor {
    /// Receives messages from the public-facing handle.
    receiver: mpsc::Receiver<ControllerActorMessage>,
    /// Client for the P4-enabled switch.
    switch_client: SwitchClient,
    /// Handle to the running DDlog program.
    program: ControllerProgram,
}

/// Message from the controller actor.
#[derive(Debug)]
enum ControllerActorMessage {
    InputMessage {
        /// Channel used to keep the actor running.
        _respond_to: oneshot::Sender<DeltaMap<DDValue>>,
        /// Running DDlog program.
        hddlog: Arc<HDDlog>,
        /// Filepath to OVSDB server.
        server: String,
        /// Name of OVS database.
        database: String,
    },
}

impl ControllerActor {
    /// Create a new actor that processes DDlog inputs and pushes them to the P4 switch.
    ///
    /// # Arguments
    /// * `receiver` - receives messages from the public controller handle.
    /// * `switch_client` - client for the P4 switch.
    /// * `program` - handle for the DDlog program.
    fn new(
        receiver: mpsc::Receiver<ControllerActorMessage>,
        switch_client: SwitchClient,
        program: ControllerProgram,
    ) -> Self {
        ControllerActor {
            receiver,
            switch_client,
            program,
        }
    }

    /// Run the actor indefinitely. Handle each received message.
    async fn run(&mut self) {
        while let Some(msg) = self.receiver.recv().await {
            self.handle_message(msg).await;
        }
    }

    /// Handle message to the controller actor.
    /// 
    /// # Arguments
    /// * `msg` - message from the public controller actor.
    async fn handle_message(&mut self, msg: ControllerActorMessage) {        
        match msg {
            ControllerActorMessage::InputMessage {_respond_to, hddlog, server, database} => {
                let (digest_tx, mut rx) = mpsc::channel::<Option<Update<DDValue>>>(1);
                let ovsdb_tx = mpsc::Sender::clone(&digest_tx);

                // Start streaming messages from the dataplane.
                // Set the configuration as a notification per-digest.
                // TODO: Retry the configuration if it errors.
                let config_res = self.switch_client.configure_digests(0, 1, 1).await;
                if config_res.is_err() {
                    error!("could not configure digests: {:#?}", config_res);
                }

                // Start the dataplane response actor.
                let (sink, receiver) = self.switch_client.client.0.stream_channel().unwrap();
                let mut digest_actor = DataplaneResponseActor::new(sink, receiver, digest_tx);
                tokio::spawn(async move { digest_actor.run().await });

                // Start processing inputs from OVSDB.
                let ctx = ovsdb_client::context::OvsdbContext::new(
                    hddlog,
                    DeltaMap::<DDValue>::new(),
                    database.clone(),
                );

                tokio::spawn(async move {
                    ovsdb_client::process_ovsdb_inputs(
                        ctx,
                        server,
                        database,
                        ovsdb_tx,
                    ).await
                });

                // Process each input.
                while let Some(inp_opt) = rx.recv().await {
                    if inp_opt.is_none() {
                        continue;
                    }

                    let ddlog_res = self.program.apply_updates(vec![inp_opt.unwrap()]);
                    if ddlog_res.is_ok() {
                        let p4_res = self.switch_client.push_outputs(&ddlog_res.unwrap()).await;
                        if p4_res.is_err() {
                            error!("could not push digest output relation to switch: {:#?}", p4_res.err());
                        }
                    } else {
                        error!("could not apply changes to ddlog input relation: {:#?}", ddlog_res.err());
                    }
                };
            },
        }
    }
}

/// Actor that processes responses from the dataplane.
struct DataplaneResponseActor {
    /// Sends message to the data plane.
    to_data_plane: StreamingCallSink<StreamMessageRequest>,
    /// Receives messages from the data plane.
    receiver: ClientDuplexReceiver<StreamMessageResponse>,
    /// Sends DDlog updates to the controller actor.
    to_controller: mpsc::Sender<Option<Update<DDValue>>>
}

impl DataplaneResponseActor {
    /// Return actor that processes responses from the data plane.
    ///
    /// # Arguments
    /// * `to_data_plane` - sends messages to the data plane.
    /// * `receiver` - receives messages from the data plane.
    /// * `to_controller` - sends DDlog updates to the controller actor.
    fn new(
        to_data_plane: StreamingCallSink<StreamMessageRequest>,
        receiver: ClientDuplexReceiver<StreamMessageResponse>,
        to_controller: mpsc::Sender<Option<Update<DDValue>>>
    ) -> Self {
        Self { to_data_plane, receiver, to_controller }
    }

    /// Run the actor indefinitely. Handle each received message. 
    async fn run(&mut self) {
        // Send a master arbitration update. This lets the actor properly stream responses from the dataplane.
        let mut update = MasterArbitrationUpdate::new();
        update.set_device_id(0);
        let mut smr = StreamMessageRequest::new();
        smr.set_arbitration(update);
        let req_result = self.to_data_plane.send((smr, grpcio::WriteFlags::default())).await;
        if req_result.is_err() {
            panic!("failed to configure stream channel with master arbitration update: {:#?}", req_result.err());
        }

        while let Some(result) = self.receiver.next().await {
            self.handle_dataplane_message(result).await;
        }
    }

    /// Handle dataplane messages. Convert received digests into DDlog inputs. Send inputs to the controller.
    ///
    /// # Arguments
    /// * `res` - result from the dataplane.
    pub async fn handle_dataplane_message(&self, res: Result<StreamMessageResponse, grpcio::Error>) {
        match res {
            Err(e) => error!("received GRPC error from p4runtime streaming channel: {:#?}", e),
            Ok(r) => {
                let p4_update_opt = r.update;
                if p4_update_opt.is_none() {
                    debug!("received empty response from p4runtime streaming channel");
                    return;
                }

                use proto::p4runtime::StreamMessageResponse_oneof_update::*;

                // unwrap() is safe because of none check
                match p4_update_opt.unwrap() {
                    digest(d) => {
                        for data in d.get_data().iter() {
                            let dd_update_opt = digest_to_ddlog(d.get_digest_id(), data);
                            
                            let channel_res = self.to_controller.send(dd_update_opt).await;
                            if channel_res.is_err() {
                                error!("could not send response over channel: {:#?}", channel_res);
                            }
                        }
                    },
                    packet(p) => {
                        let dd_update_opt = packet_in_to_ddlog(p);
                        debug!("received packetin update: {:#?}", dd_update_opt);

                        let channel_res = self.to_controller.send(dd_update_opt).await;
                        if channel_res.is_err() {
                            error!("could not send response over channel: {:#?}", channel_res);
                        }
                    }
                    error(e) => error!("received error from p4runtime streaming channel: {:#?}", e),
                    // no action for arbitration, idle timeout, or other
                    m => debug!("received message from p4runtime streaming channel: {:#?}", m),
                };
            }
        }
    }
}
