mkdir -p gen/csharp
mkdir -p gen/go
protoc --proto_path=src/proto --csharp_out=gen/csharp src/proto/queue.proto
protoc --proto_path=src/proto --go_out=gen/go src/proto/queue.proto