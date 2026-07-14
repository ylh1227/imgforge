FROM rust:1.85-bookworm AS builder

WORKDIR /app
COPY . .
RUN cargo build --release --bin imgforge-server --features server

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg tesseract-ocr \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/imgforge-server /usr/local/bin/imgforge-server

ENV IMGFORGE_SERVER_BIND=0.0.0.0:8787 \
    IMGFORGE_SERVER_DATA_DIR=/var/lib/imgforge/server \
    IMGFORGE_PUBLIC_BASE=http://127.0.0.1:8787 \
    IMGFORGE_RATE_LIMIT_PER_MINUTE=120

VOLUME ["/var/lib/imgforge/server"]
EXPOSE 8787

CMD ["imgforge-server"]
