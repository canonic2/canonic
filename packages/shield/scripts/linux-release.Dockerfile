ARG BASE_IMAGE
FROM ${BASE_IMAGE} AS verifier
RUN apt-get update \
    && apt-get install -y --no-install-recommends binutils python3 libstdc++6 libatomic1 \
    && rm -rf /var/lib/apt/lists/*
ENV XDG_CACHE_HOME=/tmp/cache XDG_CONFIG_HOME=/tmp/config XDG_DATA_HOME=/tmp/data XDG_STATE_HOME=/tmp/state

FROM verifier AS builder
ARG TARGETARCH
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential ca-certificates curl pkg-config perl \
        gcc-x86-64-linux-gnu g++-x86-64-linux-gnu gcc-aarch64-linux-gnu g++-aarch64-linux-gnu \
        openjdk-17-jdk-headless \
    && rm -rf /var/lib/apt/lists/* \
    && case "${TARGETARCH}" in amd64) rust_host=x86_64 ;; arm64) rust_host=aarch64 ;; *) exit 1 ;; esac \
    && url="https://static.rust-lang.org/rustup/archive/1.28.2/${rust_host}-unknown-linux-gnu/rustup-init" \
    && curl --fail --silent --show-error --location "$url" -o /usr/local/bin/rustup-init \
    && curl --fail --silent --show-error --location "$url.sha256" -o /tmp/rustup-init.sha256 \
    && cd /usr/local/bin && sha256sum --check /tmp/rustup-init.sha256 \
    && chmod 0755 rustup-init
ENV CARGO_HOME=/cache/cargo RUSTUP_HOME=/cache/rustup CARGO_TARGET_DIR=/cache/target
ENV PATH=/cache/cargo/bin:$PATH
