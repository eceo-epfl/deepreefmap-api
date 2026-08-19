FROM rust:1.97.1-trixie AS builder

WORKDIR /app

# Manifests first, so the dependency build is cached independently of source edits.
COPY Cargo.toml Cargo.lock ./
COPY migration/Cargo.toml migration/Cargo.toml
RUN mkdir -p src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs \
    && mkdir -p migration/src && echo "fn main() {}" > migration/src/main.rs \
    && echo "" > migration/src/lib.rs
RUN cargo build --release && rm -rf src migration/src

COPY src src
COPY migration/src migration/src
# The stub build above leaves fingerprints newer than the real sources, so cargo
# would consider the binary current and ship a stub that serves nothing.
RUN touch src/main.rs src/lib.rs migration/src/lib.rs && cargo build --release

FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# Unprivileged: the process needs nothing but a socket and a database connection.
RUN useradd --system --create-home --uid 10001 deepreefmap
USER deepreefmap

COPY --from=builder /app/target/release/deepreefmap-api /usr/local/bin/deepreefmap-api

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:3000/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/deepreefmap-api"]
