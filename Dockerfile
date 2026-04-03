FROM rustlang/rust:nightly-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    clang \
    llvm \
    libclang-dev \
    cmake \
    make \
    g++ \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

ENV SQLX_OFFLINE=true
ENV ROCKSDB_STATIC=1
ENV LIBROCKSDB_SYS_STATIC=1

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && cp /app/target/release/ThisCrow /app/ThisCrow

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/ThisCrow /usr/local/bin/ThisCrow

ENV RUST_LOG=warn
WORKDIR /
ENTRYPOINT ["ThisCrow"]
EXPOSE 8080
EXPOSE 8081