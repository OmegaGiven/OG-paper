# Headless OG Paper host: serves one canvas to OG Paper apps and browsers.
#   docker build -t og-paper .
#   docker run -d -p 8991:8991 -v og-paper:/data og-paper
# Links (with their keys) are printed in the container log; keys are kept
# in /data so they stay the same across restarts.
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
# Add "--public", "wss://your.host" to the command so the links name the
# address guests really use (a reverse proxy or Tailscale serve gives wss).
CMD ["og-paper", "--serve", "/data/canvas.ogp", "--port", "8991"]
