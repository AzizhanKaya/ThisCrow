FROM messense/rust-musl-cross:x86_64-musl as builder
ENV SQLX_OFFLINE=true
WORKDIR /app
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl

# --- Runtime image ---
FROM scratch

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/ThisCrow /usr/local/bin/ThisCrow
ENTRYPOINT ["ThisCrow"]
EXPOSE 8080