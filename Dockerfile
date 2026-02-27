### Stage 1: Build the client (WASM via Trunk)
FROM rust:1.88-bookworm AS client-build

RUN apt-get update && apt-get install -y --no-install-recommends brotli gzip && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown
RUN --mount=type=cache,id=sequoia-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=sequoia-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    cargo install trunk --locked

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY client/ client/
# Need a stub server crate so workspace resolves
COPY server/Cargo.toml server/Cargo.toml
RUN mkdir -p server/src && echo 'fn main() {}' > server/src/main.rs

WORKDIR /app/client
RUN --mount=type=cache,id=sequoia-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=sequoia-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=sequoia-client-target,target=/app/target,sharing=locked \
    --mount=type=cache,id=sequoia-trunk-cache,target=/app/.trunk,sharing=locked \
    trunk build --release
# Skip wasm-opt in Docker builds: Binaryen 108 (bookworm) can emit a broken
# externref-table export for this module, causing browser startup failure
# at __wbindgen_init_externref_table (WebAssembly.Table.grow).
RUN find dist -type f \( -name '*.wasm' -o -name '*.js' -o -name '*.css' -o -name '*.html' -o -name '*.json' -o -name '*.svg' \) -exec sh -c 'brotli -f -q 11 "$1" -o "$1.br"; gzip -f -k -9 "$1"' _ {} \;

### Stage 2: Build the server
FROM rust:1.88-bookworm AS server-build

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY server/ server/
# Need a stub client crate so workspace resolves
COPY client/Cargo.toml client/Cargo.toml
RUN mkdir -p client/src && echo 'fn main() {}' > client/src/main.rs

RUN --mount=type=cache,id=sequoia-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=sequoia-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=sequoia-server-target,target=/tmp/target-cache,sharing=locked \
    CARGO_TARGET_DIR=/tmp/target-cache cargo build --release --bin sequoia-server && \
    install -Dm755 /tmp/target-cache/release/sequoia-server /app/sequoia-server && \
    strip /app/sequoia-server

### Stage 3: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=server-build /app/sequoia-server /app/sequoia-server
COPY --from=server-build /app/server/migrations /app/server/migrations
COPY --from=client-build /app/client/dist /app/client/dist

ENV RUST_LOG=info
EXPOSE 3000

CMD ["/app/sequoia-server"]
