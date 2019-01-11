extern crate protoc_rust;

use std::fs;

use protoc_rust::Customize;

fn main() {
    protoc_rust::run(protoc_rust::Args {
        out_dir: "src/rpc",
        includes: &[],
        input: &["src/proto/queue.proto"],
        customize: Customize {
            ..Default::default()
        },
    })
        .expect("protoc generation failed");

    // Remove test storage as these will be regenerated every time we
    // run the tests
    match fs::remove_dir_all("test_storage") {
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!("Failed to remove test storage: {}", e),
        _ => {},
    }
}
