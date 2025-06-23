FROM messense/rust-musl-cross:x86_64-musl as builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev

ENV SQLX_OFFLINE=true
ENV OPENSSL_STATIC=1
ENV OPENSSL_DIR=/usr

WORKDIR /app
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl

# --- Runtime image ---
FROM scratch

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/ThisCrow /usr/local/bin/ThisCrow
ENV RUST_LOG=warn
WORKDIR /
ENTRYPOINT ["ThisCrow"]
EXPOSE 8080