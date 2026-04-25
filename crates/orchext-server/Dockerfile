# syntax=docker/dockerfile:1.7
#
# Multi-stage build for orchext-server.
# - Stage 1 (builder) compiles the workspace against a full Rust toolchain.
# - Stage 2 (runtime) is a debian-slim image with only the binary and
#   the CA bundle for outbound HTTPS (OAuth IdPs, etc.).
#
# The image is built from the repo root so `docker build -f
# crates/orchext-server/Dockerfile .` sees the entire Cargo workspace.

ARG RUST_VERSION=1.82

# --------------------------- builder ---------------------------
FROM rust:${RUST_VERSION}-slim-bookworm AS builder

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .

# `--locked` would reject any Cargo.lock drift; we don't enforce that
# at image-build time so the same Dockerfile can build from a branch
# that has lock updates pending. CI should run `cargo check --locked`
# separately.
RUN cargo build --release -p orchext-server

# --------------------------- runtime ---------------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --system --gid 1000 orchext \
 && useradd --system --uid 1000 --gid orchext --no-create-home orchext

COPY --from=builder /src/target/release/orchext-server /usr/local/bin/orchext-server

USER orchext
EXPOSE 8080

# DATABASE_URL is required. ORCHEXT_BIND defaults to 0.0.0.0:8080 inside
# the process itself; see src/config.rs.
ENV ORCHEXT_BIND=0.0.0.0:8080

# Healthcheck is defined in docker-compose.yml (HTTP GET /healthz) so
# we don't need to bake curl or an extra binary into the image.

ENTRYPOINT ["/usr/local/bin/orchext-server"]
