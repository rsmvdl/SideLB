FROM debian:bookworm-slim AS rust_builder

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get -y install --no-install-recommends \
        git \
        curl \
        cmake \
        autoconf \
        automake \
        build-essential \
        libfontconfig1-dev \
        pkg-config \
        ca-certificates && \
    rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --default-toolchain stable -y

WORKDIR /app
ADD . .

RUN cargo build --release --locked

RUN if [ ! -f /app/target/release/sidelb ]; then \
        echo "Build failed: sidelb binary not found!" >&2; \
        exit 1; \
    fi

FROM debian:bookworm-slim AS final_image

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=rust_builder /app/target/release/sidelb /usr/local/bin/sidelb
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN chmod +x /usr/local/bin/sidelb && \
    chmod +x /usr/local/bin/docker-entrypoint.sh

HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
  CMD /usr/local/bin/sidelb --health-check-uds || exit 1

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["sidelb-daemon"]
