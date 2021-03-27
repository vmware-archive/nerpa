extern crate protoc_grpcio;

fn main() {
    let protos = [
        ("p4runtime/proto", "p4/v1/p4runtime.proto"),
        ("p4runtime/proto", "p4/v1/p4data.proto"),
        ("p4runtime/proto", "p4/config/v1/p4info.proto"),
        ("p4runtime/proto", "p4/config/v1/p4types.proto"),
        ("googleapis", "google/rpc/status.proto"),
        ("googleapis", "google/rpc/code.proto"),
    ];
    for proto in &protos {
        println!("cargo:rerun-if-changed={}/{}", proto.0, proto.1);
    }
    protoc_grpcio::compile_grpc_protos(
        &protos.iter().map(|x| x.1).collect::<Vec<&str>>(),
        &protos.iter().map(|x| x.0).collect::<Vec<&str>>(),
        "src/",
        None,
    )
    .expect("Failed to compile gRPC definitions!");
}