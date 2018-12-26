FROM rust:1.31.0 AS builder
COPY . .
RUN cargo build --release

FROM debian
COPY --from=builder /target/release/brqueue /bin/brqueue
RUN chmod +x /bin/brqueue
CMD ["/bin/brqueue"]