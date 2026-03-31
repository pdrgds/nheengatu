FROM rust:1.82-slim AS builder

WORKDIR /app

# Build dependencies first (better layer caching)
COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml core/
COPY cli/Cargo.toml cli/
COPY web/Cargo.toml web/

# Stub source files to cache dependency compilation
RUN mkdir -p core/src cli/src web/src && \
    echo "fn main() {}" > cli/src/main.rs && \
    echo "fn main() {}" > web/src/main.rs && \
    touch core/src/lib.rs && \
    cargo build --release -p nheengatu-web && \
    rm -rf core/src cli/src web/src

# Build the real source
COPY core core
COPY web web
RUN touch core/src/lib.rs web/src/main.rs && \
    cargo build --release -p nheengatu-web

# Runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/nheengatu-web .
COPY web/templates templates

EXPOSE 3000
CMD ["./nheengatu-web"]
