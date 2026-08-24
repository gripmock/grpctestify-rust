# syntax=docker/dockerfile:1

FROM rust:1.98-alpine3.24 AS builder

COPY . /grpctestify-src

WORKDIR /grpctestify-src

# hadolint ignore=DL3018
RUN apk add --no-cache musl-dev \
    && cargo build --release --bin grpctestify \
    && cp /grpctestify-src/target/release/grpctestify /usr/local/bin/grpctestify \
    && strip /usr/local/bin/grpctestify \
    && chmod +x /usr/local/bin/grpctestify \
    && rm -rf /root/.cargo /root/.rustup /grpctestify-src/target /grpctestify-src/node_modules /var/cache/*

FROM alpine:3.24

LABEL org.opencontainers.image.title="gRPC Testify"
LABEL org.opencontainers.image.description="Native CLI for gRPC testing with .gctf files — zero runtime dependencies"
LABEL org.opencontainers.image.source="https://github.com/gripmock/grpctestify-rust"
LABEL org.opencontainers.image.documentation="https://gripmock.github.io/grpctestify-rust/"
LABEL org.opencontainers.image.authors="Babichev Maxim <info@babichev.net>"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.vendor="gripmock"

# hadolint ignore=DL3018
RUN apk add --no-cache ca-certificates tzdata \
    && adduser -D -h /home/grpctestify -u 1000 grpctestify

COPY --from=builder /usr/local/bin/grpctestify /usr/local/bin/grpctestify

USER 1000

ENTRYPOINT ["/usr/local/bin/grpctestify"]