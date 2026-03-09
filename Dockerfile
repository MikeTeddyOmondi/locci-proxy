# Multi-stage build for smallest Debian image
FROM rust:1.88-slim AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    cmake \
    build-essential \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Build dependencies first (cached layer)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --release \
    && strip target/release/locci-proxy

# Use distroless for smallest secure Debian-based runtime
FROM gcr.io/distroless/cc-debian12:latest AS release
WORKDIR /app
COPY --from=build /app/target/release/locci-proxy ./locci-proxy
# Proxy port | Control API port
EXPOSE 8484 8485
ENTRYPOINT ["./locci-proxy"]
