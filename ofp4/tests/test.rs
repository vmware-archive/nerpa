use anyhow::{anyhow, Result};
use daemon::Cleanup;
use futures_util::{sink::SinkExt, stream::StreamExt};
use grpcio::{ChannelBuilder, EnvBuilder};
use p4ext::{MatchField, Table};
use proto::p4info::P4Info;
use proto::p4runtime::{
    Action,
    Entity,
    Entity_oneof_entity,
    FieldMatch,
    FieldMatch_Exact,
    FieldMatch_oneof_field_match_type,
    ForwardingPipelineConfig,
    MasterArbitrationUpdate,
    MulticastGroupEntry,
    PacketReplicationEngineEntry,
    PacketReplicationEngineEntry_oneof_type,
    Replica,
    SetForwardingPipelineConfigRequest,
    SetForwardingPipelineConfigRequest_Action,
    StreamMessageRequest,
    StreamMessageRequest_oneof_update,
    StreamMessageResponse,
    StreamMessageResponse_oneof_update,
    TableAction,
    TableAction_oneof_type,
    TableEntry,
    Uint128,
    Update,
    Update_Type,
    WriteRequest,
    WriteRequest_Atomicity,
};
use proto::p4runtime_grpc::P4RuntimeClient;
use protobuf::{Message, RepeatedField};
use regex::Regex;
use std::collections::HashMap;
use std::default::Default;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use tracing::{debug, info};
use tracing_test::traced_test;

enum Completion<T> {
    Incomplete,
    Complete(T)
}
use Completion::*;

/// Repeatedly evaluates `condition`, sleeping a bit between calls, until it yields
/// Complete(value), then returns Ok(value).  After a while, however, give up and return an error
/// instead.
fn wait_until<T, F>(mut condition: F) -> Result<T>
    where F: FnMut() -> Completion<T>
{
    for i in 0..10 {
        if let Complete(result) = condition() {
            return Ok(result)
        }

        // Delay only a little bit on the first few tries, because we assume that in many cases the
        // condition will become true quickly.
        let ms = match i {
            0 => 10,
            1 => 100,
            _ => 1000,
        };
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    Err(anyhow!("wait_until timed out"))
}

/// Waits for `child` to die, and returns:
///    - `Ok(Ok(status))`: Child exited with `status`.
///    - `Ok(Err(e))`: System reported error waiting for `child` (e.g. we already waited for it).
///    - `Err(e)`: Timeout.
fn wait_for_child_to_die(child: &mut Child) -> Result<Result<ExitStatus>> {
    match wait_until(|| match child.try_wait() {
        Ok(Some(status)) => Complete(Ok(status)),
        Ok(None) => Incomplete,
        Err(e) => Complete(Err(e)),
    }) {
        Ok(Ok(result)) => Ok(Ok(result)),
        Ok(Err(error)) => Ok(Err(error.into())),
        Err(error) => Err(error),
    }
}

fn ovs_command<S: AsRef<OsStr>, P: AsRef<Path>>(program: S, tmp_dir: P) -> Command {
    let mut command = Command::new(program);
    command.current_dir(tmp_dir.as_ref());

    // We could add OVS_PKGDATADIR here, but that's where vswitchd.ovsschema lives and we want
    // ovsdb-tool to be able to find it.
    for dir_var in ["OVS_SYSCONFDIR", "OVS_RUNDIR", "OVS_LOGDIR", "OVS_DBDIR"] {
        command.env(dir_var, tmp_dir.as_ref());
    }
    command
}

fn read_to_string<R: std::io::Read>(r: &mut R) -> Result<String> {
    let mut s = String::new();
    r.read_to_string(&mut s)?;
    Ok(s)
}
struct RunOutput {
    stdout: String,
    stderr: String
}

trait Run {
    fn command_string(&self) -> String;
    fn start(&mut self, cleanup: &mut Cleanup) -> Result<Child>;
    fn run(&mut self) -> Result<RunOutput>;
    fn run_nocapture(&mut self) -> Result<()>;
}

impl Run for Command {
    /// Returns a string with the program name followed by arguments, separated by spaces.  This is
    /// suitable for diagnostic messages; it's not properly escaped or encoded for other use.
    fn command_string(&self) -> String {
        let mut command: String = self.get_program().to_string_lossy().into_owned();
        for arg in self.get_args() {
            command.push(' ');
            command.push_str(&arg.to_string_lossy());
        }
        command
    }

    /// Logs this command, starts it, and returns its `Child`, arranging for it to be killed if
    /// `Cleanup` is dropped.
    fn start(&mut self, cleanup: &mut Cleanup) -> Result<Child> {
        info!("running command: {}", self.command_string());
        Ok(cleanup.spawn(self)?)
    }

    /// Log this command and runs it to completion (but no more than about 10 seconds), ensuring
    /// that it gets killed if we do.  Logs its output and, if it fails, its exit status, and
    /// returns its output.
    fn run(&mut self) -> Result<RunOutput> {
        self.stdout(Stdio::piped());
        self.stderr(Stdio::piped());
        
        let program = self.get_program().to_string_lossy().into_owned();
        let mut cleanup = Cleanup::new()?;
        let mut child = self.start(&mut cleanup)?;
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let status = wait_for_child_to_die(&mut child)??;
        cleanup.cancel();

        info!("{program} exited ({})", status);
        let output = RunOutput {
            stdout: read_to_string(&mut stdout)?,
            stderr: read_to_string(&mut stderr)?
        };
        for (sink, string) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
            if string.len() > 0 {
                info!("{program} output to {sink}:\n{string}");
            }
        }
        if !status.success() {
            Err(anyhow!("{program} failed ({status})"))?;
        }
        Ok(output)
    }

    /// Log this command and runs it to completion (but no more than about 10 seconds), ensuring
    /// that it gets killed if we do, and logs its exit status.
    fn run_nocapture(&mut self) -> Result<()> {
        let program = self.get_program().to_string_lossy().into_owned();
        let mut cleanup = Cleanup::new()?;
        let mut child = self.start(&mut cleanup)?;
        let status = wait_for_child_to_die(&mut child)??;
        cleanup.cancel();

        info!("{program} exited ({})", status);
        if !status.success() {
            Err(anyhow!("{program} failed ({status})"))?;
        }
        Ok(())
    }
}

/// Passes `args` to `ovs-appctl ofproto/trace` and returns a tuple of (complete output from
/// `ofproto/trace`, just datapath actions).
fn trace_flow<'a, P, I, S>(tmp_dir: P, args: I) -> Result<(String, String)>
where P: AsRef<Path>,
      I: IntoIterator<Item = S>,
      S: AsRef<OsStr>
{
    // Run the trace.
    let mut command = ovs_command("ovs-appctl", &tmp_dir);
    command.arg("ofproto/trace").arg("br0").args(args);
    let command_string = command.command_string();
    info!("Running {command_string}...");
    let output = String::from_utf8(Cleanup::output(&mut command)?.stdout)?;

    // ofproto/trace yields lots of output.  It might look like this:
    //
    //     Flow: in_port=2,vlan_tci=0x0000,dl_src=00:00:00:00:00:00,dl_dst=00:00:00:00:00:00,dl_type=0x0000
    //
    //     bridge("br0")
    //     -------------
    //      0. priority 32768
    //         resubmit(,1)
    //      1. priority 32768
    //         resubmit(,2)
    //      2. in_port=2, priority 32768
    //         set_field:0x1/0xffff->reg0
    //         resubmit(,3)
    //      3. priority 32768
    //         resubmit(,4)
    //      4. reg0=0/0xffff0000, priority 32768
    //         resubmit(,5)
    //      5. priority 32768
    //         resubmit(,6)
    //      6. priority 32768
    //         output:NXM_NX_REG0[0..15]
    //          -> output port is 1
    //
    //     Final flow: reg0=0x1,in_port=2,vlan_tci=0x0000,dl_src=00:00:00:00:00:00,dl_dst=00:00:00:00:00:00,dl_type=0x0000
    //     Megaflow: recirc_id=0,eth,in_port=2,dl_type=0x0000
    //     Datapath actions: 1
    //
    // It would be hard to check the entire output for correctness, since it is so extensive and
    // not designed to be machine-parsable, but the final "Datapath actions:" line says what it
    // going to happen to the packet in the end.  It's easy enough to check that summary.
    debug!("{output}");
    let last_line = String::from(output.lines().nth_back(0).unwrap_or(""));
    info!("...{last_line}");
    if let Some(rest) = last_line.strip_prefix("Datapath actions: ") {
        Ok((output, rest.into()))
    } else {
        Err(anyhow!("Trace command returned unexpected output:\n{command_string}\n{output}"))
    }
}

const DEVICE_ID: u64 = 1;

fn election_id() -> Uint128 {
    Uint128 { high: 0, low: 1, ..Default::default() }
}

async fn start_ofp4(p4info: P4Info) -> Result<(Cleanup, PathBuf, P4RuntimeClient)> {
    grpcio::redirect_log();
    
    let mut cleanup = Cleanup::new()?;
    if let Ok(_) = std::env::var("KEEP_TMPDIR") {
        cleanup.keep_temp_dirs();
    }
    let tmp_dir = cleanup.create_temp_dir(".")?;

    // Create OVS configuration database.
    ovs_command("ovsdb-tool", &tmp_dir).arg("create").arg("ovsdb.conf.db").run()?;

    // Start ovsdb-server to serve the configuration database.
    cleanup.register_pidfile(tmp_dir.join("ovsdb-server.pid"))?;
    ovs_command("ovsdb-server", &tmp_dir).arg("ovsdb.conf.db").arg("--remote=punix:db.sock").arg("--pidfile").arg("--detach").run()?;

    // Use ovs-vsctl to configure OVS.
    let mut command = ovs_command("ovs-vsctl", &tmp_dir);
    command.args(["--no-wait", "--", "add-br", "br0"]);
    for port in 1..=4 {
        let portname = format!("p{port}");
        command.args(["--", "add-port", "br0", &portname,
                      "--", "set", "Interface", &portname, &format!("ofport_request={port}")]);
    }
    command.run()?;

    // Start ovs-vswitchd.
    cleanup.register_pidfile(tmp_dir.join("ovs-vswitchd.pid"))?;
    ovs_command("ovs-vswitchd", &tmp_dir)
        .arg("--log-file").arg("-vvconn").arg("-vconsole:off")
        .arg("--enable-dummy=override").arg("--disable-system").arg("--disable-system-route")
        .arg("--detach").arg("--pidfile")
        .arg("unix:db.sock")
        .run()?;

    // Start ovs-ofctl monitoring flows and writing to `flow-log.txt` in the temporary directory.
    // This can be useful for debugging if anything goes wrong.
    cleanup.register_pidfile(tmp_dir.join("ovs-ofctl.pid"))?;
    let flow_log_stdout = File::create(tmp_dir.join("flow-log.txt"))?;
    let flow_log_stderr = flow_log_stdout.try_clone()?;
    ovs_command("ovs-ofctl", &tmp_dir).stdout(flow_log_stdout).stderr(flow_log_stderr).arg("monitor").arg("br0").arg("watch:!initial").arg("--pidfile").arg("--detach").run_nocapture()?;

    // Start ofp4.
    let mut remote_arg = OsString::from("unix:");
    remote_arg.push(tmp_dir.join("br0.mgmt"));
    cleanup.register_pidfile(tmp_dir.join("ofp4.pid"))?;
    Command::new(env!("CARGO_BIN_EXE_ofp4"))
        .arg("--log-file=ofp4.log")
        .arg("--ddlog-record=ddlog.txt")
        .current_dir(&tmp_dir)
        .arg(remote_arg)
        .arg("--p4-port=0")
        .arg("--detach").arg("--pidfile=ofp4.pid")
        .arg(&format!("--device-id={DEVICE_ID}"))
        .run()?;

    // ofp4 printed to its log the P4Runtime port where it's listening.  Read this out and parse it
    // as `p4_port`, so we can connect back to it.
    //
    // (We could tell it a port to listen, but in practice that prevents reliably running tests in
    // parallel, even choosing a random port.  The address space is not big enough.)
    let ofp4_log = String::from_utf8(std::fs::read(tmp_dir.join("ofp4.log"))?)?;
    let re = Regex::new("(?m)Listening on (.*):([0-9]+)$").unwrap();
    let (p4_addr, p4_port) = match re.captures(&ofp4_log) {
        None => Err(anyhow!("ofp4 failed to log its listening address and port"))?,
        Some(c) => (c.get(1).unwrap().as_str(), c.get(2).unwrap().as_str())
    };
    let p4_port: u16 = p4_port.parse().unwrap();
    info!("ofp4 is listening on port {p4_port}");

    // Connect to ofp4.
    info!("Connect to ofp4");
    let env = Arc::new(EnvBuilder::new().build());
    let ch = ChannelBuilder::new(env).connect(&format!("{}:{}", p4_addr, p4_port));
    let client = P4RuntimeClient::new(ch);

    // Start a StreamChannel.
    let (mut tx, mut rx) = client.stream_channel()?;

    // Send MasterArbitrationUpdate, which is required to work with the device, and ensure that we
    // get it back unchanged.  (It's unchanged because gRPC considers 0 to be the same as empty and
    // the reply should give us a `status` of `GRPC_STATUS_OK`, which has value 0.)
    info!("Send master arbitration update");
    let mau = MasterArbitrationUpdate {
        device_id: DEVICE_ID,
        election_id: Some(election_id()).into(),
        ..Default::default()
    };
    let smr = StreamMessageRequest {
        update: Some(StreamMessageRequest_oneof_update::arbitration(mau.clone())),
        ..Default::default()
    };
    tx.send((smr, grpcio::WriteFlags::default())).await?;
    assert_eq!(rx.next().await.unwrap()?,
               StreamMessageResponse {
                   update: Some(StreamMessageResponse_oneof_update::arbitration(mau)),
                   ..Default::default()
               });

    // Grab and parse P4Info for the P4 program we want to test.

    // Install the P4 program into ofp4.
    //
    // If this fails, it probably means that the program we're testing wasn't compiled in.  That
    // might mean that it needs to be added to `ofp4dl.dl` or that `make` needs to be rerun.
    info!("Install P4 program into ofp4");
    let sfpcr = SetForwardingPipelineConfigRequest {
        device_id: DEVICE_ID,
        action: SetForwardingPipelineConfigRequest_Action::VERIFY_AND_SAVE,
        config: Some(ForwardingPipelineConfig {
            p4info: Some(p4info).into(),
            ..Default::default()
        }).into(),
        ..Default::default()
    };
    client.set_forwarding_pipeline_config(&sfpcr)?;

    Ok((cleanup, tmp_dir, client))
}

#[tokio::test]
#[traced_test]
async fn wire() -> Result<()> {
    let p4info: P4Info = Message::parse_from_bytes(include_bytes!("../wire.p4info.bin"))?;
    let (_cleanup, tmp_dir, _client) = start_ofp4(p4info).await?;

    assert_eq!(trace_flow(&tmp_dir, ["in_port=p1"])?.1, "2");
    assert_eq!(trace_flow(&tmp_dir, ["in_port=p2"])?.1, "1");

    Ok(())
}

#[tokio::test]
#[traced_test]
async fn snvs() -> Result<()> {
    let p4info: P4Info = Message::parse_from_bytes(include_bytes!("../snvs.p4info.bin"))?;
    let actions: HashMap<String, u32> = p4info.get_actions().iter()
        .map(|action| { let p = action.get_preamble(); (p.name.clone(), p.id) })
        .collect();
    let action_by_id: HashMap<u32, p4ext::Action> = p4info
        .get_actions()
        .iter()
        .map(|a| (a.get_preamble().id, a.into()))
        .collect();
    let tables: HashMap<String, Table> = p4info.get_tables().iter()
        .map(|table| p4ext::Table::new_from_proto(table, &action_by_id))
        .map(|table| (table.preamble.name.clone(), table))
        .collect();

    let (_cleanup, tmp_dir, client) = start_ofp4(p4info).await?;

    // Add a multicast group entry, with ID 1, that contains ports 1, 2, 3, and 4.
    fn replica(egress_port: u32, instance: u32) -> Replica {
        Replica { egress_port, instance, ..Default::default() }
    }
    let mge = MulticastGroupEntry {
        multicast_group_id: 1,
        replicas: RepeatedField::from_vec(vec![
            replica(1, 1),
            replica(2, 1),
            replica(3, 1),
            replica(4, 1),
        ]),
        ..Default::default()
    };
    let pree = PacketReplicationEngineEntry {
        field_type: Some(PacketReplicationEngineEntry_oneof_type::multicast_group_entry(mge)),
        ..Default::default()
    };
    let entity = Entity {
        entity: Some(Entity_oneof_entity::packet_replication_engine_entry(pree)).into(),
        ..Default::default()
    };
    let mge_update = Update {
        field_type: Update_Type::INSERT,
        entity: Some(entity).into(),
        ..Default::default()
    };

    // Add tagged VLAN with ID 1.
    let table = &tables["SnvsIngress.InputVlan"];
    let mfs = &table.match_fields;
    fn exact_fm(mf: &MatchField, value: Vec<u8>) -> FieldMatch {
        let exact = FieldMatch_Exact { value, ..Default::default() };
        FieldMatch {
            field_id: mf.preamble.id,
            field_match_type: Some(FieldMatch_oneof_field_match_type::exact(exact)).into(),
            ..Default::default()
        }
    }
    let fms = vec![exact_fm(&mfs[0], vec![0, 1]),
                   exact_fm(&mfs[1], vec![1])];
    let action = Action {
        action_id: actions["SnvsIngress.UseTaggedVlan"],
        ..Default::default()
    };
    let table_action = TableAction {
        field_type: Some(TableAction_oneof_type::action(action)),
        ..Default::default()
    };
    let te = TableEntry {
        table_id: table.preamble.id,
        field_match: RepeatedField::from_vec(fms),
        action: Some(table_action).into(),
        priority: 50,
        ..Default::default()
    };
    let entity = Entity {
        entity: Some(Entity_oneof_entity::table_entry(te)).into(),
        ..Default::default()
    };
    let te_update = Update {
        field_type: Update_Type::INSERT,
        entity: Some(entity).into(),
        ..Default::default()
    };
    let updates = vec![te_update, mge_update];
    let wr = WriteRequest {
        device_id: DEVICE_ID,
        election_id: Some(election_id()).into(),
        updates: RepeatedField::from_vec(updates),
        atomicity: WriteRequest_Atomicity::DATAPLANE_ATOMIC,
        ..Default::default()
    };
    client.write(&wr)?;

    // XXX This should not be necessary, but ofp4 does not yet wait for OpenFlow flow table changes
    // to commit before returning success.  See https://github.com/vmware/nerpa/issues/86.
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Check that a packet received on port 1, in VLAN 1, will get broadcast to the other ports in
    // the VLAN.
    assert_eq!(trace_flow(&tmp_dir, ["in_port=p1,dl_vlan=1"])?.1, "2,3,4");

    Ok(())
}

