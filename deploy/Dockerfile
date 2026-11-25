# Compila o call-server e o client. O último estágio é a imagem do Render (ver render.yaml); o
# estágio "binario" é só o executável, para a VM (ver scripts/deploy.ps1).
# bookworm tem uma glibc mais velha que a do Ubuntu 24.04 da VM, então o binário roda lá.
# O nome fixo do toolchain ignora o rust-toolchain.toml, que baixaria os extras de desenvolvimento.
FROM rust:1-bookworm AS base
WORKDIR /src
RUN echo "export RUSTUP_TOOLCHAIN=$(rustup toolchain list | head -n1 | cut -d' ' -f1)" >/toolchain
COPY . .

FROM base AS server
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/src/target \
    . /toolchain && cargo build --locked --release -p call-server && cp target/release/call-server /call-server

FROM scratch AS binario
COPY --from=server /call-server /

FROM base AS web
# A soma SHA-256 garante que o executável baixado é o esperado.
ADD --checksum=sha256:26fca8a9952e46cd7d8b8b9a2e2c1786c090fc14b8e10b6678ab4d0f9f1a385f \
    https://github.com/trunk-rs/trunk/releases/download/v0.22.2/trunk-x86_64-unknown-linux-gnu.tar.gz /trunk.tar.gz
RUN tar -xzf /trunk.tar.gz -C /usr/local/bin
# CALL_API_URL vazia: o client usa a origem da própria página.
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/src/target \
    . /toolchain && rustup target add wasm32-unknown-unknown \
    && cd web && CALL_API_URL= trunk build --release --locked --dist /web \
    && find /web -type f -exec gzip -9 --keep {} +

FROM debian:bookworm-slim
COPY --from=server /call-server /usr/local/bin/
COPY --from=web /web /web
ENV WEB_DIR=/web BIND_ADDR=0.0.0.0:10000 LOG_FORMAT=json
EXPOSE 10000
USER nobody
CMD ["call-server"]
