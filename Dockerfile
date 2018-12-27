FROM rust:1.31.1 AS builder
# Install protoc
RUN apt-get update && apt-get install -y unzip \
    && PROTOC_ZIP=protoc-3.3.0-linux-x86_64.zip \
    && curl -OL https://github.com/google/protobuf/releases/download/v3.3.0/$PROTOC_ZIP \
    && unzip -o $PROTOC_ZIP -d /usr/local bin/protoc \
    && rm -f $PROTOC_ZIP

COPY . .

RUN cargo build --release

FROM debian
COPY --from=builder /target/release/brqueue /bin/brqueue
RUN chmod +x /bin/brqueue
CMD ["/bin/brqueue"]