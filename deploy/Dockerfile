# Só compila o call-server para Linux; o resultado é o binário (ver scripts/deploy.ps1).
# bookworm tem uma glibc mais velha que a do Ubuntu 24.04 da VM, então o binário roda lá.
# O nome fixo do toolchain ignora o rust-toolchain.toml, que baixaria os extras de desenvolvimento.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/src/target \
    RUSTUP_TOOLCHAIN="$(rustup toolchain list | head -n1 | cut -d' ' -f1)" \
    cargo build --locked --release -p call-server && cp target/release/call-server /call-server

FROM scratch
COPY --from=build /call-server /
