FROM rust:bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    clang \
    llvm \
    libclang-dev \
    cmake \
    make \
    g++ \
    librocksdb-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

ENV SQLX_OFFLINE=true

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && cp /app/target/release/ThisCrow /app/ThisCrow

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    librocksdb7.8 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/ThisCrow /usr/local/bin/ThisCrow

WORKDIR /
ENTRYPOINT ["ThisCrow"]
EXPOSE 8080
EXPOSE 8081