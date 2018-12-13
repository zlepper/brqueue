extern crate futures;
extern crate futures_cpupool;
extern crate grpc;
extern crate protobuf;

use std::thread;

use grpc::RequestOptions;
use grpc::SingleResponse;

mod rpc;

//use crate::rpc;

struct Te;

impl rpc::Test for Te {
    fn hello(&self, _o: RequestOptions, p: rpc::TestRequest) -> SingleResponse<rpc::TestResponse> {
        let message = format!("Hello {}", p.get_name());
        let mut res = rpc::TestResponse::new();
        res.set_message(message.to_string());
        grpc::SingleResponse::completed(res)
    }
}

fn main() {
    let mut server: grpc::ServerBuilder<tls_api_native_tls::TlsAcceptor> = grpc::ServerBuilder::new();

    server.http.set_port(21000);
    server.add_service(rpc::TestServer::new_service_def(Te));
    server.http.set_cpu_pool_threads(4);

    let _server = server.build().expect("Failed to start server");

    println!("Test server started on port {}", 21000);

    loop {
        thread::park();
    }
}
