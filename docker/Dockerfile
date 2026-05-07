# ──────────────────────────────────────────────────────────
#  PersistHotStuff Docker image
#  Multi-stage build: build with Rust, run a minimal image
# ──────────────────────────────────────────────────────────
FROM rust:latest AS builder

# protoc is required by prost-build (gRPC codegen at compile time)
RUN apt-get update && \
    apt-get install -y --no-install-recommends protobuf-compiler && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency layer (build a stub main first so deps are cached)
COPY Cargo.toml Cargo.lock* build.rs* ./
COPY proto/ proto/
RUN mkdir -p src && echo "fn main() {}" > src/main.rs && \
    cargo build --release 2>/dev/null || true && \
    rm -rf src

# Full build
COPY . .
RUN cargo build --release --examples

# ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates iproute2 iputils-ping procps && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy all evaluation binaries + the main binary
COPY --from=builder /build/target/release/persisthotstuff-rst   /app/persisthotstuff-rst
COPY --from=builder /build/target/release/examples/eval_crash_recovery      /app/eval_crash_recovery
COPY --from=builder /build/target/release/examples/eval_membership_cycling  /app/eval_membership_cycling
COPY --from=builder /build/target/release/examples/eval_commit_latency      /app/eval_commit_latency

# Data volume for WAL & snapshots
VOLUME /app/data

# Default: print usage
CMD ["./persisthotstuff-rst"]
