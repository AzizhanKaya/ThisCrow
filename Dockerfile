FROM rust:1.86 as builder

WORKDIR /app
COPY . .
RUN cargo build --release

# --- Runtime image ---
FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ThisCrow /usr/local/bin/ThisCrow

CMD ["ThisCrow"]

