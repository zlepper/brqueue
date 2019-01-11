FROM rust:1.31.1 AS builder
# Install protoc
RUN apt-get update && apt-get install -y unzip \
    && PROTOC_ZIP=protoc-3.3.0-linux-x86_64.zip \
    && curl -OL https://github.com/google/protobuf/releases/download/v3.3.0/$PROTOC_ZIP \
    && unzip -o $PROTOC_ZIP -d /usr/local bin/protoc \
    && rm -f $PROTOC_ZIP

COPY . /root/brqueue
WORKDIR /root/brqueue
# Run normal tests
RUN cargo test
# Run all tests in release mode, so they can run faster, and so we can
# Ensure that everything also works in release mode.
RUN cargo test --release
RUN cargo test --release -- --ignored
RUN cargo build --release

FROM debian
COPY --from=builder /root/brqueue/target/release/brqueue /bin/brqueue
RUN chmod +x /bin/brqueue
CMD ["/bin/brqueue"]