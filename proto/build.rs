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