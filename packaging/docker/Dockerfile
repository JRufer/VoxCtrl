# syntax=docker/dockerfile:1
#
# voxctrl-server: VoxCtrl's headless speech server. Full guide: docs/server.md
#
# Build from the repository root:
#
#   docker build -f packaging/docker/Dockerfile -t voxctrl-server .
#
# FEATURES adds Cargo features on top of the defaults (whisper.cpp, Moonshine,
# Parakeet, all on the CPU). `--build-arg FEATURES=vulkan` builds whisper.cpp
# with Vulkan GPU offload; see "GPU acceleration" in docs/server.md before
# relying on it.

ARG DEBIAN_RELEASE=trixie

# ── Build ─────────────────────────────────────────────────────────────────────
FROM rust:1-${DEBIAN_RELEASE} AS build
ARG FEATURES=""

# cmake + clang build whisper.cpp and generate its bindings; libssl-dev is for
# the HTTPS client that downloads models. The Vulkan headers and shader
# compiler are only needed for a Vulkan build.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake clang libclang-dev pkg-config libssl-dev \
    && if echo ",${FEATURES}," | grep -q ",vulkan,"; then \
         apt-get install -y --no-install-recommends libvulkan-dev glslc; \
       fi \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .

# Only the server and the crates it depends on are compiled; the desktop app
# and its GTK/WebKit dependencies are never built here. The cache mounts keep
# rebuilds short, which is why the binary is copied out of the cached target
# directory in the same step.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p voxctrl-server ${FEATURES:+--features "${FEATURES}"} \
    && cp target/release/voxctrl-server /usr/local/bin/voxctrl-server

# ── Runtime ───────────────────────────────────────────────────────────────────
FROM debian:${DEBIAN_RELEASE}-slim
ARG FEATURES=""

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3t64 \
    && if echo ",${FEATURES}," | grep -q ",vulkan,"; then \
         apt-get install -y --no-install-recommends libvulkan1; \
       fi \
    && rm -rf /var/lib/apt/lists/*

# Everything the server writes lives under one directory, so one volume keeps
# the downloaded models and the generated API key across upgrades:
#   /var/lib/voxctrl/config/voxctrl/server.toml
#   /var/lib/voxctrl/data/voxctrl/models/
RUN useradd --system --uid 1000 --home-dir /var/lib/voxctrl --shell /usr/sbin/nologin voxctrl \
    && mkdir -p /var/lib/voxctrl/config /var/lib/voxctrl/data \
    && chown -R voxctrl:voxctrl /var/lib/voxctrl
ENV HOME=/var/lib/voxctrl \
    XDG_CONFIG_HOME=/var/lib/voxctrl/config \
    XDG_DATA_HOME=/var/lib/voxctrl/data \
    VOXCTRL_SERVER_BIND=0.0.0.0:8787

COPY --from=build /usr/local/bin/voxctrl-server /usr/local/bin/voxctrl-server

USER voxctrl
VOLUME /var/lib/voxctrl
EXPOSE 8787

# The server only starts listening once its model is downloaded and loaded,
# which on a first start can take several minutes for a large model.
HEALTHCHECK --interval=30s --timeout=10s --start-period=15m --retries=3 \
    CMD ["voxctrl-server", "--healthcheck"]

ENTRYPOINT ["voxctrl-server"]
