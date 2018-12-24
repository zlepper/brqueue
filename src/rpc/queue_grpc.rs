// This file is generated. Do not edit
// @generated

// https://github.com/Manishearth/rust-clippy/issues/702
#![allow(unknown_lints)]
#![allow(clippy)]

#![cfg_attr(rustfmt, rustfmt_skip)]

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


// interface

pub trait Queue {
    fn enqueue(&self, o: ::grpc::RequestOptions, p: super::queue::EnqueueRequest) -> ::grpc::SingleResponse<super::queue::EnqueueResponse>;

    fn pop(&self, o: ::grpc::RequestOptions, p: super::queue::GetRequest) -> ::grpc::SingleResponse<super::queue::GetResponse>;

    fn get_all(&self, o: ::grpc::RequestOptions, p: super::queue::GetAllRequest) -> ::grpc::SingleResponse<super::queue::GetAllResponse>;

    fn subscribe(&self, o: ::grpc::RequestOptions, p: super::queue::SubscribeRequest) -> ::grpc::StreamingResponse<super::queue::SubscribeResponse>;

    fn acknowledge_work(&self, o: ::grpc::RequestOptions, p: super::queue::AcknowledgeWorkRequest) -> ::grpc::SingleResponse<super::queue::AcknowledgeWorkResponse>;
}

// client

pub struct QueueClient {
    grpc_client: ::std::sync::Arc<::grpc::Client>,
    method_Enqueue: ::std::sync::Arc<::grpc::rt::MethodDescriptor<super::queue::EnqueueRequest, super::queue::EnqueueResponse>>,
    method_Pop: ::std::sync::Arc<::grpc::rt::MethodDescriptor<super::queue::GetRequest, super::queue::GetResponse>>,
    method_GetAll: ::std::sync::Arc<::grpc::rt::MethodDescriptor<super::queue::GetAllRequest, super::queue::GetAllResponse>>,
    method_Subscribe: ::std::sync::Arc<::grpc::rt::MethodDescriptor<super::queue::SubscribeRequest, super::queue::SubscribeResponse>>,
    method_AcknowledgeWork: ::std::sync::Arc<::grpc::rt::MethodDescriptor<super::queue::AcknowledgeWorkRequest, super::queue::AcknowledgeWorkResponse>>,
}

impl ::grpc::ClientStub for QueueClient {
    fn with_client(grpc_client: ::std::sync::Arc<::grpc::Client>) -> Self {
        QueueClient {
            grpc_client: grpc_client,
            method_Enqueue: ::std::sync::Arc::new(::grpc::rt::MethodDescriptor {
                name: "/Queue/Enqueue".to_string(),
                streaming: ::grpc::rt::GrpcStreaming::Unary,
                req_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                resp_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
            }),
            method_Pop: ::std::sync::Arc::new(::grpc::rt::MethodDescriptor {
                name: "/Queue/Pop".to_string(),
                streaming: ::grpc::rt::GrpcStreaming::Unary,
                req_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                resp_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
            }),
            method_GetAll: ::std::sync::Arc::new(::grpc::rt::MethodDescriptor {
                name: "/Queue/GetAll".to_string(),
                streaming: ::grpc::rt::GrpcStreaming::Unary,
                req_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                resp_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
            }),
            method_Subscribe: ::std::sync::Arc::new(::grpc::rt::MethodDescriptor {
                name: "/Queue/Subscribe".to_string(),
                streaming: ::grpc::rt::GrpcStreaming::ServerStreaming,
                req_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                resp_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
            }),
            method_AcknowledgeWork: ::std::sync::Arc::new(::grpc::rt::MethodDescriptor {
                name: "/Queue/AcknowledgeWork".to_string(),
                streaming: ::grpc::rt::GrpcStreaming::Unary,
                req_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                resp_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
            }),
        }
    }
}

impl Queue for QueueClient {
    fn enqueue(&self, o: ::grpc::RequestOptions, p: super::queue::EnqueueRequest) -> ::grpc::SingleResponse<super::queue::EnqueueResponse> {
        self.grpc_client.call_unary(o, p, self.method_Enqueue.clone())
    }

    fn pop(&self, o: ::grpc::RequestOptions, p: super::queue::GetRequest) -> ::grpc::SingleResponse<super::queue::GetResponse> {
        self.grpc_client.call_unary(o, p, self.method_Pop.clone())
    }

    fn get_all(&self, o: ::grpc::RequestOptions, p: super::queue::GetAllRequest) -> ::grpc::SingleResponse<super::queue::GetAllResponse> {
        self.grpc_client.call_unary(o, p, self.method_GetAll.clone())
    }

    fn subscribe(&self, o: ::grpc::RequestOptions, p: super::queue::SubscribeRequest) -> ::grpc::StreamingResponse<super::queue::SubscribeResponse> {
        self.grpc_client.call_server_streaming(o, p, self.method_Subscribe.clone())
    }

    fn acknowledge_work(&self, o: ::grpc::RequestOptions, p: super::queue::AcknowledgeWorkRequest) -> ::grpc::SingleResponse<super::queue::AcknowledgeWorkResponse> {
        self.grpc_client.call_unary(o, p, self.method_AcknowledgeWork.clone())
    }
}

// server

pub struct QueueServer;


impl QueueServer {
    pub fn new_service_def<H : Queue + 'static + Sync + Send + 'static>(handler: H) -> ::grpc::rt::ServerServiceDefinition {
        let handler_arc = ::std::sync::Arc::new(handler);
        ::grpc::rt::ServerServiceDefinition::new("/Queue",
            vec![
                ::grpc::rt::ServerMethod::new(
                    ::std::sync::Arc::new(::grpc::rt::MethodDescriptor {
                        name: "/Queue/Enqueue".to_string(),
                        streaming: ::grpc::rt::GrpcStreaming::Unary,
                        req_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                        resp_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                    }),
                    {
                        let handler_copy = handler_arc.clone();
                        ::grpc::rt::MethodHandlerUnary::new(move |o, p| handler_copy.enqueue(o, p))
                    },
                ),
                ::grpc::rt::ServerMethod::new(
                    ::std::sync::Arc::new(::grpc::rt::MethodDescriptor {
                        name: "/Queue/Pop".to_string(),
                        streaming: ::grpc::rt::GrpcStreaming::Unary,
                        req_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                        resp_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                    }),
                    {
                        let handler_copy = handler_arc.clone();
                        ::grpc::rt::MethodHandlerUnary::new(move |o, p| handler_copy.pop(o, p))
                    },
                ),
                ::grpc::rt::ServerMethod::new(
                    ::std::sync::Arc::new(::grpc::rt::MethodDescriptor {
                        name: "/Queue/GetAll".to_string(),
                        streaming: ::grpc::rt::GrpcStreaming::Unary,
                        req_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                        resp_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                    }),
                    {
                        let handler_copy = handler_arc.clone();
                        ::grpc::rt::MethodHandlerUnary::new(move |o, p| handler_copy.get_all(o, p))
                    },
                ),
                ::grpc::rt::ServerMethod::new(
                    ::std::sync::Arc::new(::grpc::rt::MethodDescriptor {
                        name: "/Queue/Subscribe".to_string(),
                        streaming: ::grpc::rt::GrpcStreaming::ServerStreaming,
                        req_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                        resp_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                    }),
                    {
                        let handler_copy = handler_arc.clone();
                        ::grpc::rt::MethodHandlerServerStreaming::new(move |o, p| handler_copy.subscribe(o, p))
                    },
                ),
                ::grpc::rt::ServerMethod::new(
                    ::std::sync::Arc::new(::grpc::rt::MethodDescriptor {
                        name: "/Queue/AcknowledgeWork".to_string(),
                        streaming: ::grpc::rt::GrpcStreaming::Unary,
                        req_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                        resp_marshaller: Box::new(::grpc::protobuf::MarshallerProtobuf),
                    }),
                    {
                        let handler_copy = handler_arc.clone();
                        ::grpc::rt::MethodHandlerUnary::new(move |o, p| handler_copy.acknowledge_work(o, p))
                    },
                ),
            ],
        )
    }
}
