FROM messense/rust-musl-cross:x86_64-musl as builder

# Gerekli paketleri yükle
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    musl-tools \
    musl-dev \
    && rm -rf /var/lib/apt/lists/*

# Çevresel değişkenleri ayarla
ENV SQLX_OFFLINE=true
ENV OPENSSL_STATIC=1

# Ring crate için gerekli environment variables
ENV CC_x86_64_unknown_linux_musl=x86_64-linux-musl-gcc
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-musl-gcc

WORKDIR /app
COPY . .

# Rust target'ı ekle
RUN rustup target add x86_64-unknown-linux-musl

# Build et
RUN cargo build --release --target x86_64-unknown-linux-musl

# --- Runtime image ---
FROM scratch

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/ThisCrow /usr/local/bin/ThisCrow
ENV RUST_LOG=warn
WORKDIR /
ENTRYPOINT ["ThisCrow"]
EXPOSE 8080