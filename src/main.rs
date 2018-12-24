extern crate bincode;
extern crate env_logger;
extern crate futures;
extern crate futures_cpupool;
extern crate grpc;
extern crate protobuf;
extern crate uuid;

use std::thread;

use grpc::{RequestOptions, SingleResponse, StreamingResponse};
use std::sync::Arc;

mod rpc;
mod queue;
mod queue_server;

#[derive(Clone)]
struct QueueServer {
    server: queue_server::QueueServer,
}

impl QueueServer {
    fn new() -> QueueServer {
        let server = queue_server::QueueServer::new().expect("Could not create queue server");

        QueueServer {
            server,
        }
    }
}

fn rpc_priority_to_queue_priority(p: rpc::Priority) -> queue_server::Priority {
    match p {
        rpc::Priority::LOW => queue_server::Priority::Low,
        rpc::Priority::HIGH => queue_server::Priority::High,
    }
}

impl rpc::Queue for QueueServer {
    fn enqueue(&self, _o: RequestOptions, p: rpc::EnqueueRequest) -> SingleResponse<rpc::EnqueueResponse> {
        let priority = rpc_priority_to_queue_priority(p.get_priority());
        let mut s = self.to_owned();
        match s.server.enqueue(p.get_message().into(), priority, p.get_requiredCapabilities().into()) {
            Ok(queue_server::CreatedMessage{id}) => {
                let mut response = rpc::EnqueueResponse::new();
                response.set_id(id);
                response.set_success(true);
                SingleResponse::completed(response)
            },
            Err(err) => {
                let mut response = rpc::EnqueueResponse::new();
                response.set_success(false);
                SingleResponse::completed(response)
            },
        }
    }

    fn pop(&self, _o: RequestOptions, p: rpc::GetRequest) -> SingleResponse<rpc::GetResponse> {
        _o.
        let mut s = self.to_owned();
        unimplemented!()
    }

    fn get_all(&self, _o: RequestOptions, p: rpc::GetAllRequest) -> SingleResponse<rpc::GetAllResponse> {
        unimplemented!()
    }

    fn subscribe(&self, _o: RequestOptions, p: rpc::SubscribeRequest) -> StreamingResponse<rpc::SubscribeResponse> {
        unimplemented!()
    }

    fn acknowledge_work(&self, _o: RequestOptions, p: rpc::AcknowledgeWorkRequest) -> SingleResponse<rpc::AcknowledgeWorkResponse> {
        unimplemented!()
    }
}

fn main() {
    let mut server: grpc::ServerBuilder<tls_api_native_tls::TlsAcceptor> = grpc::ServerBuilder::new();

    server.http.set_port(21000);
    server.add_service(rpc::QueueServer::new_service_def(QueueServer::new()));
    server.http.set_cpu_pool_threads(4);

    let _server = server.build().expect("Failed to start server");

    println!("Test server started on port {}", 21000);

    loop {
        thread::park();
    }
}
