# OG Paper server: hosts many pages for OG Paper apps and browsers.
#   docker build -t og-paper .
#   docker run -d --name og-paper -p 8991:8991 -v og-paper:/data og-paper
# The server links (with their keys) are printed in the log:
#   docker logs og-paper
# Add one in the app under Pages > Add server. Pages, keys and the server
# key live in /data. Set OGP_SERVER_KEY to choose the server key yourself.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p og-paper

# The web app too, served by the server at /app/: browsers let a page
# reach the server it came from, but not a public site reach a private
# address (a tailnet or home network), so the server's own copy works where
# the public web app can't. Keep the version in step with Cargo.lock.
FROM rust:1-bookworm AS web
ARG WASM_BINDGEN=0.2.129
RUN rustup target add wasm32-unknown-unknown \
    && cargo install wasm-bindgen-cli --version ${WASM_BINDGEN} --locked
WORKDIR /src
COPY . .
RUN cargo build --release -p og-paper --lib --target wasm32-unknown-unknown \
    && wasm-bindgen --target web --no-typescript --out-dir web/app/pkg \
       target/wasm32-unknown-unknown/release/og_paper.wasm \
    && gzip -9 -k web/app/pkg/*.wasm web/app/pkg/*.js web/app/og-web.js

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/og-paper /usr/local/bin/og-paper
COPY --from=web /src/web/app /srv/web/app
ENV HOME=/data
VOLUME /data
EXPOSE 8991
# Add "--public", "wss://your.host" so the printed links name the address
# people really use (a reverse proxy or Tailscale serve gives wss).
CMD ["og-paper", "--serve-dir", "/data/pages", "--port", "8991"]
