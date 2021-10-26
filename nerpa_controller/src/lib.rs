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

use differential_datalog::api::HDDlog;

use differential_datalog::{
    DDlog,
    DDlogDynamic,
    DeltaMap
}; 
use differential_datalog::ddval::DDValue;
use differential_datalog::program::Update;
use differential_datalog::record::{Record, IntoRecord};

use digest2ddlog::digest_to_ddlog;

use futures::{
    StreamExt,
};
use grpcio::{
    ClientDuplexReceiver,
    StreamingCallSink,
};

use p4ext::{
    ActionRef,
    Table
};

use proto::p4runtime::{
    StreamMessageRequest,
    StreamMessageResponse,
};
use proto::p4runtime_grpc::P4RuntimeClient;
use protobuf::Message;

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;

// Controller serves as a handle for the Tokio tasks.
// The Tokio task can either process DDlog inputs or push outputs to the switch.
#[derive(Clone)]
pub struct Controller {
    sender: mpsc::Sender<ControllerActorMessage>,
}

impl Controller {
    pub fn new(
        switch_client: SwitchClient,
        hddlog: HDDlog,
    ) -> Result<Controller, String> {
        let (sender, receiver) = mpsc::channel(1000);
        let program = ControllerProgram::new(hddlog);

        let mut actor = ControllerActor::new(receiver, switch_client, program);
        tokio::spawn(async move { actor.run().await });

        Ok(Self{sender})
    }

    pub async fn input_to_switch(
        &self,
        input: Vec<Update<DDValue>>
    ) -> Result<(), p4ext::P4Error> {
        let (send, recv) = oneshot::channel();
        let msg = ControllerActorMessage::UpdateMessage {
            respond_to: send,
            input: input,
        };

        let message_res = self.sender.send(msg).await;
        if message_res.is_err() {
            println!("could not send message to controller actor: {:#?}", message_res);
        }

        recv.await.expect("Actor task has been killed")
    }

    pub async fn stream_digests(&self) -> () {
        // The oneshot channel keeps an Actor running that processes digests.
        // It closes when the Actor task is killed.
        let (send, recv) = oneshot::channel();
        let msg = ControllerActorMessage::DigestMessage {
            _respond_to: send,
        };

        let message_res = self.sender.send(msg).await;
        if message_res.is_err() {
            println!("could not send message to controller actor: {:#?}", message_res);
        }

        recv.await.expect("Actor task has been killed");
    }
}

pub struct ControllerProgram {
    hddlog: HDDlog,
}

impl ControllerProgram {
    pub fn new(hddlog: HDDlog) -> Self {
        Self{hddlog}
    }

    pub fn add_input(
        &mut self,
        updates:Vec<Update<DDValue>>
    ) -> Result<DeltaMap<DDValue>, String> {
        self.hddlog.transaction_start()?;

        match self.hddlog.apply_updates(&mut updates.into_iter()) {
            Ok(_) => {},
            Err(_) => self.hddlog.transaction_rollback()?
        };

        self.hddlog.transaction_commit_dump_changes()
    }

    pub fn stop(&mut self) {
        self.hddlog.stop().unwrap();
    }
}

pub struct SwitchClient {
    pub client: P4RuntimeClient,
    p4info: String,
    device_id: u64,
    role_id: u64,
    target: String,
}

impl SwitchClient {
    pub fn new(
        client: P4RuntimeClient,
        p4info: String,
        opaque: String,
        cookie: String,
        action: String,
        device_id: u64,
        role_id: u64,
        target: String,
    ) -> Self {
        p4ext::set_pipeline(
            &p4info,
            &opaque,
            &cookie,
            &action,
            device_id,
            role_id,
            &target,
            &client
        );

        Self {
            client,
            p4info,
            device_id,
            role_id,
            target,
        }
    }

    pub async fn configure_digests(
        &mut self,
        max_timeout_ns: i64,
        max_list_size: i32,
        ack_timeout_ns: i64,
    ) -> Result<(), p4ext::P4Error> {
        // Read P4info from file.
        let p4info_str: &str = &self.p4info;
        let mut p4info_file = File::open(OsStr::new(p4info_str))
            .unwrap_or_else(|err| panic!("{}: could not open P4Info ({})", p4info_str, err));
        let p4info: proto::p4info::P4Info = Message::parse_from_reader(&mut p4info_file)
            .unwrap_or_else(|err| panic!("{}: could not read P4Info ({})", p4info_str, err));

        for d in p4info.get_digests().iter() {
            let config_res = p4ext::write_digest_config(
                d.get_preamble().get_id(),
                max_timeout_ns,
                max_list_size,
                ack_timeout_ns,
                self.device_id,
                self.role_id,
                &self.target,
                &self.client,
            ).await;

            if config_res.is_err() {
                return config_res;
            }
        }

        Ok(())
    }

    pub fn push_outputs(&mut self, delta: &DeltaMap<DDValue>) -> Result<(), p4ext::P4Error> {
        let mut updates = Vec::new();

        let pipeline = p4ext::get_pipeline_config(self.device_id, &self.target, &self.client);
        let switch: p4ext::Switch = pipeline.get_p4info().into();

        for (_rel_id, output_map) in (*delta).clone().into_iter() {
            for (value, _weight) in output_map {
                let record = value.clone().into_record();
                
                match record {
                    Record::NamedStruct(name, recs) => {
                        // Translate the record table name to the P4 table name.
                        let mut table: Table = Table::default();
                        let mut table_name: String = "".to_string();

                        match self.get_matching_table(name.to_string(), switch.tables.clone()) {
                            Some(t) => {
                                table = t;
                                table_name = table.preamble.name;
                            },
                            None => {},
                        };

                        // Iterate through fields in the record.
                        // Map all match keys to values.
                        // If the field is the action, extract the action, name, and parameters.
                        let mut action_name: String = "".to_string();
                        let matches = &mut HashMap::<std::string::String, u16>::new();
                        let params = &mut HashMap::<std::string::String, u16>::new();
                        let mut priority: i32 = 0;
                        for (_, (fname, v)) in recs.iter().enumerate() {
                            let match_name: String = fname.to_string();

                            match match_name.as_str() {
                                "action" => {
                                    match v {
                                        Record::NamedStruct(name, arecs) => {
                                            // Find matching action name from P4 table.
                                            action_name = match self.get_matching_action_name(name.to_string(), table.actions.clone()) {
                                                Some(an) => an,
                                                None => "".to_string()
                                            };
    
                                            // Extract param values from action's records.
                                            for (_, (afname, aval)) in arecs.iter().enumerate() {
                                                params.insert(afname.to_string(), self.extract_record_value(&aval));
                                            }
                                        },
                                        _ => println!("action was incorrectly formed!")
                                    }
                                },
                                "priority" => {
                                    priority = self.extract_record_value(&v).into();
                                },
                                _ => {
                                    matches.insert(match_name, self.extract_record_value(&v));
                                }
                            }
                        }

                        // If we found a table and action, construct a P4 table entry update.
                        if !(table_name.is_empty() || action_name.is_empty()) {
                            let update = p4ext::build_table_entry_update(
                                proto::p4runtime::Update_Type::INSERT,
                                table_name.as_str(),
                                action_name.as_str(),
                                params,
                                matches,
                                priority,
                                self.device_id,
                                &self.target,
                                &self.client,
                            ).unwrap_or_else(|err| panic!("could not build table update: {}", err));
                            updates.push(update);
                        }
                    }
                    _ => {
                        println!("record was not named struct");
                    }
                }
            }
        }

        p4ext::write(updates, self.device_id, self.role_id, &self.target, &self.client)
    }

    fn extract_record_value(&mut self, r: &Record) -> u16 {
        use num_traits::cast::ToPrimitive;
        match r {
            Record::Bool(true) => 1,
            Record::Bool(false) => 0,
            Record::Int(i) => i.to_u16().unwrap_or(0),
            // TODO: If required, handle other types.
            _ => 1,
        }
    }

    fn get_matching_table(&mut self, record_name: String, tables: Vec<Table>) -> Option<Table> {
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

    fn get_matching_action_name(&mut self, record_name: String, actions: Vec<ActionRef>) -> Option<String> {
        for action_ref in actions {
            let an = action_ref.action.preamble.name;
            let av: Vec<String> = an.split('.').map(|s| s.to_string()).collect();
            let asub = av.last().unwrap();

            if record_name.contains(asub) {
                return Some(an.to_string());
            }
        }

        None
    }
}

use tokio::sync::{oneshot, mpsc};

struct ControllerActor {
    receiver: mpsc::Receiver<ControllerActorMessage>,
    switch_client: SwitchClient,
    program: ControllerProgram,
}

#[derive(Debug)]
enum ControllerActorMessage {
    UpdateMessage {
        respond_to: oneshot::Sender<Result<(), p4ext::P4Error>>,
        input: Vec<Update<DDValue>>,
    },
    DigestMessage {
        _respond_to: oneshot::Sender<DeltaMap<DDValue>>,
    },
}

impl ControllerActor {
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

    async fn run(&mut self) {
        while let Some(msg) = self.receiver.recv().await {
            self.handle_message(msg).await;
        }
    }

    async fn handle_message(&mut self, msg: ControllerActorMessage) {        
        match msg {
            ControllerActorMessage::UpdateMessage {respond_to, input} => {
                let ddlog_res = self.program.add_input(input).unwrap();
                let message_res = respond_to.send(self.switch_client.push_outputs(&ddlog_res));
                if message_res.is_err() {
                    println!("could not send message from actor to controller: {:#?}", message_res.err());
                }
            },
            ControllerActorMessage::DigestMessage{ _respond_to } => {
                // This configuration sends a notification per-digest.
                let config_res = self.switch_client.configure_digests(0, 1, 1).await;
                if config_res.is_err() {
                    panic!("could not configure digests: {:#?}", config_res);
                }

                let (send, mut rx) = mpsc::channel::<Update<DDValue>>(1000);

                let (_sink, receiver) = self.switch_client.client.stream_channel().unwrap();

                let mut digest_actor = DigestActor::new(_sink, receiver, send);
                tokio::spawn(async move { digest_actor.run().await });

                while let Some(inp) = rx.recv().await {
                    let ddlog_res = self.program.add_input(vec![inp]);
                    if ddlog_res.is_ok() {
                        let p4_res = self.switch_client.push_outputs(&ddlog_res.unwrap());
                        if p4_res.is_err() {
                            println!("could not push digest output relation to switch: {:#?}", p4_res.err())
                        }
                    }
                };
            }
        }
    }
}

struct DigestActor {
    _sink: StreamingCallSink<StreamMessageRequest>,
    receiver: ClientDuplexReceiver<StreamMessageResponse>,
    respond_to: mpsc::Sender<Update<DDValue>>
}

impl DigestActor {
    fn new(
        _sink: StreamingCallSink<StreamMessageRequest>,
        receiver: ClientDuplexReceiver<StreamMessageResponse>,
        respond_to: mpsc::Sender<Update<DDValue>>
    ) -> Self {
        Self { _sink, receiver, respond_to }
    }

    async fn run(&mut self) {
        while let Some(result) = self.receiver.next().await {
            self.handle_digest(result).await;
        }
    }

    pub async fn handle_digest(&self, res: Result<StreamMessageResponse, grpcio::Error>) {
        match res {
            Err(e) => println!("received GRPC error from p4runtime streaming channel: {:#?}", e),
            Ok(r) => {
                let update_opt = r.update;
                if update_opt.is_none() {
                    println!("received empty response from p4runtime streaming channel");
                }

                use proto::p4runtime::StreamMessageResponse_oneof_update::*;

                // unwrap() is safe because of none check
                match update_opt.unwrap() {
                    digest(d) => {
                        for data in d.get_data().iter() {
                            let update = digest_to_ddlog(d.get_digest_id(), data);
                            
                            let channel_res = self.respond_to.send(update).await;
                            if channel_res.is_err() {
                                println!("could not send response over channel: {:#?}", channel_res);
                            }
                        }
                    },
                    error(e) => println!("received error from p4runtime streaming channel: {:#?}", e),
                    // no action for arbitration, packet, idle timeout, or other
                    _ => {},
                };
            }
        }
    }
}
