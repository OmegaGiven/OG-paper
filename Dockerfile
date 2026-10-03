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

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/og-paper /usr/local/bin/og-paper
ENV HOME=/data
VOLUME /data
EXPOSE 8991
# Add "--public", "wss://your.host" so the printed links name the address
# people really use (a reverse proxy or Tailscale serve gives wss).
CMD ["og-paper", "--serve-dir", "/data/pages", "--port", "8991"]
