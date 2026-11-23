# Container boot test for the treeterm daemon.
#
# Stage 1 compiles the daemon as a static musl binary, which needs no distro
# libraries at run time.
# Each distro stage then copies that single binary onto a stock ubuntu/fedora
# image and runs the boot smoke test as a build step — if the daemon fails to
# start, the `RUN` fails and the image build fails.
#
# Build context is the repo root; the build needs only daemon-rs/ (the proto it
# compiles is in daemon-rs/crates/treeterm-proto/proto/).

# ---------------------------------------------------------------------------
# Builder: static musl binary
#
# Compile on a glibc host with the musl target added via rustup — exactly how
# `cargo run -p xtask -- build-daemons` (in app-rs/) produces the shipped binary.
# (Building on a musl-native host instead fails: proc-macro crates such as
# async-trait cannot be produced for a static-only musl target.)
# ---------------------------------------------------------------------------
FROM rust:slim AS builder
RUN apt-get update \
 && apt-get install -y --no-install-recommends protobuf-compiler musl-tools \
 && rm -rf /var/lib/apt/lists/*
ENV PROTOC=/usr/bin/protoc
WORKDIR /work
COPY daemon-rs/ daemon-rs/
WORKDIR /work/daemon-rs
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/work/daemon-rs/target \
    set -eux; \
    TARGET="$(uname -m)-unknown-linux-musl"; \
    rustup target add "$TARGET"; \
    cargo build --release --target "$TARGET" --bin treeterm-daemon; \
    cp "target/$TARGET/release/treeterm-daemon" /treeterm-daemon

# ---------------------------------------------------------------------------
# Ubuntu boot test
# ---------------------------------------------------------------------------
FROM ubuntu:24.04 AS ubuntu-test
COPY --from=builder /treeterm-daemon /usr/local/bin/treeterm-daemon
COPY daemon-rs/tests/container/verify-daemon-boot.sh /verify-daemon-boot.sh
RUN chmod +x /usr/local/bin/treeterm-daemon /verify-daemon-boot.sh \
 && /verify-daemon-boot.sh

# ---------------------------------------------------------------------------
# Fedora boot test
# ---------------------------------------------------------------------------
FROM fedora:latest AS fedora-test
COPY --from=builder /treeterm-daemon /usr/local/bin/treeterm-daemon
COPY daemon-rs/tests/container/verify-daemon-boot.sh /verify-daemon-boot.sh
RUN chmod +x /usr/local/bin/treeterm-daemon /verify-daemon-boot.sh \
 && /verify-daemon-boot.sh
