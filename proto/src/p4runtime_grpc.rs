// This file is generated. Do not edit
// @generated

// https://github.com/Manishearth/rust-clippy/issues/702
#![allow(unknown_lints)]
#![allow(clippy::all)]

#![allow(box_pointers)]
#![allow(dead_code)]
#![allow(missing_docs)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(trivial_casts)]
#![allow(unsafe_code)]
#![allow(unused_imports)]
#![allow(unused_results)]

const METHOD_P4_RUNTIME_WRITE: ::grpcio::Method<super::p4runtime::WriteRequest, super::p4runtime::WriteResponse> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/p4.v1.P4Runtime/Write",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_P4_RUNTIME_READ: ::grpcio::Method<super::p4runtime::ReadRequest, super::p4runtime::ReadResponse> = ::grpcio::Method {
    ty: ::grpcio::MethodType::ServerStreaming,
    name: "/p4.v1.P4Runtime/Read",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_P4_RUNTIME_SET_FORWARDING_PIPELINE_CONFIG: ::grpcio::Method<super::p4runtime::SetForwardingPipelineConfigRequest, super::p4runtime::SetForwardingPipelineConfigResponse> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/p4.v1.P4Runtime/SetForwardingPipelineConfig",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_P4_RUNTIME_GET_FORWARDING_PIPELINE_CONFIG: ::grpcio::Method<super::p4runtime::GetForwardingPipelineConfigRequest, super::p4runtime::GetForwardingPipelineConfigResponse> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/p4.v1.P4Runtime/GetForwardingPipelineConfig",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_P4_RUNTIME_STREAM_CHANNEL: ::grpcio::Method<super::p4runtime::StreamMessageRequest, super::p4runtime::StreamMessageResponse> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Duplex,
    name: "/p4.v1.P4Runtime/StreamChannel",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_P4_RUNTIME_CAPABILITIES: ::grpcio::Method<super::p4runtime::CapabilitiesRequest, super::p4runtime::CapabilitiesResponse> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/p4.v1.P4Runtime/Capabilities",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

#[derive(Clone)]
pub struct P4RuntimeClient {
    client: ::grpcio::Client,
}

impl P4RuntimeClient {
    pub fn new(channel: ::grpcio::Channel) -> Self {
        P4RuntimeClient {
            client: ::grpcio::Client::new(channel),
        }
    }

    pub fn write_opt(&self, req: &super::p4runtime::WriteRequest, opt: ::grpcio::CallOption) -> ::grpcio::Result<super::p4runtime::WriteResponse> {
        self.client.unary_call(&METHOD_P4_RUNTIME_WRITE, req, opt)
    }

    pub fn write(&self, req: &super::p4runtime::WriteRequest) -> ::grpcio::Result<super::p4runtime::WriteResponse> {
        self.write_opt(req, ::grpcio::CallOption::default())
    }

    pub fn write_async_opt(&self, req: &super::p4runtime::WriteRequest, opt: ::grpcio::CallOption) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::p4runtime::WriteResponse>> {
        self.client.unary_call_async(&METHOD_P4_RUNTIME_WRITE, req, opt)
    }

    pub fn write_async(&self, req: &super::p4runtime::WriteRequest) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::p4runtime::WriteResponse>> {
        self.write_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn read_opt(&self, req: &super::p4runtime::ReadRequest, opt: ::grpcio::CallOption) -> ::grpcio::Result<::grpcio::ClientSStreamReceiver<super::p4runtime::ReadResponse>> {
        self.client.server_streaming(&METHOD_P4_RUNTIME_READ, req, opt)
    }

    pub fn read(&self, req: &super::p4runtime::ReadRequest) -> ::grpcio::Result<::grpcio::ClientSStreamReceiver<super::p4runtime::ReadResponse>> {
        self.read_opt(req, ::grpcio::CallOption::default())
    }

    pub fn set_forwarding_pipeline_config_opt(&self, req: &super::p4runtime::SetForwardingPipelineConfigRequest, opt: ::grpcio::CallOption) -> ::grpcio::Result<super::p4runtime::SetForwardingPipelineConfigResponse> {
        self.client.unary_call(&METHOD_P4_RUNTIME_SET_FORWARDING_PIPELINE_CONFIG, req, opt)
    }

    pub fn set_forwarding_pipeline_config(&self, req: &super::p4runtime::SetForwardingPipelineConfigRequest) -> ::grpcio::Result<super::p4runtime::SetForwardingPipelineConfigResponse> {
        self.set_forwarding_pipeline_config_opt(req, ::grpcio::CallOption::default())
    }

    pub fn set_forwarding_pipeline_config_async_opt(&self, req: &super::p4runtime::SetForwardingPipelineConfigRequest, opt: ::grpcio::CallOption) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::p4runtime::SetForwardingPipelineConfigResponse>> {
        self.client.unary_call_async(&METHOD_P4_RUNTIME_SET_FORWARDING_PIPELINE_CONFIG, req, opt)
    }

    pub fn set_forwarding_pipeline_config_async(&self, req: &super::p4runtime::SetForwardingPipelineConfigRequest) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::p4runtime::SetForwardingPipelineConfigResponse>> {
        self.set_forwarding_pipeline_config_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn get_forwarding_pipeline_config_opt(&self, req: &super::p4runtime::GetForwardingPipelineConfigRequest, opt: ::grpcio::CallOption) -> ::grpcio::Result<super::p4runtime::GetForwardingPipelineConfigResponse> {
        self.client.unary_call(&METHOD_P4_RUNTIME_GET_FORWARDING_PIPELINE_CONFIG, req, opt)
    }

    pub fn get_forwarding_pipeline_config(&self, req: &super::p4runtime::GetForwardingPipelineConfigRequest) -> ::grpcio::Result<super::p4runtime::GetForwardingPipelineConfigResponse> {
        self.get_forwarding_pipeline_config_opt(req, ::grpcio::CallOption::default())
    }

    pub fn get_forwarding_pipeline_config_async_opt(&self, req: &super::p4runtime::GetForwardingPipelineConfigRequest, opt: ::grpcio::CallOption) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::p4runtime::GetForwardingPipelineConfigResponse>> {
        self.client.unary_call_async(&METHOD_P4_RUNTIME_GET_FORWARDING_PIPELINE_CONFIG, req, opt)
    }

    pub fn get_forwarding_pipeline_config_async(&self, req: &super::p4runtime::GetForwardingPipelineConfigRequest) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::p4runtime::GetForwardingPipelineConfigResponse>> {
        self.get_forwarding_pipeline_config_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn stream_channel_opt(&self, opt: ::grpcio::CallOption) -> ::grpcio::Result<(::grpcio::ClientDuplexSender<super::p4runtime::StreamMessageRequest>, ::grpcio::ClientDuplexReceiver<super::p4runtime::StreamMessageResponse>)> {
        self.client.duplex_streaming(&METHOD_P4_RUNTIME_STREAM_CHANNEL, opt)
    }

    pub fn stream_channel(&self) -> ::grpcio::Result<(::grpcio::ClientDuplexSender<super::p4runtime::StreamMessageRequest>, ::grpcio::ClientDuplexReceiver<super::p4runtime::StreamMessageResponse>)> {
        self.stream_channel_opt(::grpcio::CallOption::default())
    }

    pub fn capabilities_opt(&self, req: &super::p4runtime::CapabilitiesRequest, opt: ::grpcio::CallOption) -> ::grpcio::Result<super::p4runtime::CapabilitiesResponse> {
        self.client.unary_call(&METHOD_P4_RUNTIME_CAPABILITIES, req, opt)
    }

    pub fn capabilities(&self, req: &super::p4runtime::CapabilitiesRequest) -> ::grpcio::Result<super::p4runtime::CapabilitiesResponse> {
        self.capabilities_opt(req, ::grpcio::CallOption::default())
    }

    pub fn capabilities_async_opt(&self, req: &super::p4runtime::CapabilitiesRequest, opt: ::grpcio::CallOption) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::p4runtime::CapabilitiesResponse>> {
        self.client.unary_call_async(&METHOD_P4_RUNTIME_CAPABILITIES, req, opt)
    }

    pub fn capabilities_async(&self, req: &super::p4runtime::CapabilitiesRequest) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::p4runtime::CapabilitiesResponse>> {
        self.capabilities_async_opt(req, ::grpcio::CallOption::default())
    }
    pub fn spawn<F>(&self, f: F) where F: ::futures::Future<Output = ()> + Send + 'static {
        self.client.spawn(f)
    }
}

pub trait P4Runtime {
    fn write(&mut self, ctx: ::grpcio::RpcContext, req: super::p4runtime::WriteRequest, sink: ::grpcio::UnarySink<super::p4runtime::WriteResponse>);
    fn read(&mut self, ctx: ::grpcio::RpcContext, req: super::p4runtime::ReadRequest, sink: ::grpcio::ServerStreamingSink<super::p4runtime::ReadResponse>);
    fn set_forwarding_pipeline_config(&mut self, ctx: ::grpcio::RpcContext, req: super::p4runtime::SetForwardingPipelineConfigRequest, sink: ::grpcio::UnarySink<super::p4runtime::SetForwardingPipelineConfigResponse>);
    fn get_forwarding_pipeline_config(&mut self, ctx: ::grpcio::RpcContext, req: super::p4runtime::GetForwardingPipelineConfigRequest, sink: ::grpcio::UnarySink<super::p4runtime::GetForwardingPipelineConfigResponse>);
    fn stream_channel(&mut self, ctx: ::grpcio::RpcContext, stream: ::grpcio::RequestStream<super::p4runtime::StreamMessageRequest>, sink: ::grpcio::DuplexSink<super::p4runtime::StreamMessageResponse>);
    fn capabilities(&mut self, ctx: ::grpcio::RpcContext, req: super::p4runtime::CapabilitiesRequest, sink: ::grpcio::UnarySink<super::p4runtime::CapabilitiesResponse>);
}

pub fn create_p4_runtime<S: P4Runtime + Send + Clone + 'static>(s: S) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_P4_RUNTIME_WRITE, move |ctx, req, resp| {
        instance.write(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder.add_server_streaming_handler(&METHOD_P4_RUNTIME_READ, move |ctx, req, resp| {
        instance.read(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_P4_RUNTIME_SET_FORWARDING_PIPELINE_CONFIG, move |ctx, req, resp| {
        instance.set_forwarding_pipeline_config(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_P4_RUNTIME_GET_FORWARDING_PIPELINE_CONFIG, move |ctx, req, resp| {
        instance.get_forwarding_pipeline_config(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder.add_duplex_streaming_handler(&METHOD_P4_RUNTIME_STREAM_CHANNEL, move |ctx, req, resp| {
        instance.stream_channel(ctx, req, resp)
    });
    let mut instance = s;
    builder = builder.add_unary_handler(&METHOD_P4_RUNTIME_CAPABILITIES, move |ctx, req, resp| {
        instance.capabilities(ctx, req, resp)
    });
    builder.build()
}
