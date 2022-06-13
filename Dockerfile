FROM rust:latest as builder
WORKDIR /usr/src/quiet-proxy
COPY . .
ENV RUSTFLAGS "-C target-cpu=native"
RUN cargo build --release

FROM debian:latest
COPY --from=builder /usr/src/quiet-proxy/target/release/quiet-proxy /usr/local/bin/quiet-proxy
CMD ["quiet-proxy"]