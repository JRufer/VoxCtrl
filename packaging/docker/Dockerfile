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
#
# AUDIOCPP_REF builds audio.cpp's audiocpp_server from source into the image,
# for TTS engines no audio.cpp release has yet: `--build-arg
# AUDIOCPP_REF=reverb-tts` gives Reverb-TTS. Empty (the default) builds nothing
# and the server downloads the released runtime as before.

ARG DEBIAN_RELEASE=trixie
# The image the server runs in. It has to provide glibc 2.43 or newer: the
# audio.cpp runtime that the neural text-to-speech engines download (v0.9.4)
# is built against it, and on anything older every one of those engines fails
# to start with "GLIBC_2.43 not found". Debian 13 (trixie) has 2.41; Ubuntu
# 26.04 has 2.43. The server binary is built on trixie and runs on either.
ARG RUNTIME_IMAGE=ubuntu:26.04

# ── Build ─────────────────────────────────────────────────────────────────────
FROM rust:1-${DEBIAN_RELEASE} AS build
ARG FEATURES=""

# cmake + clang build whisper.cpp and llama.cpp and generate their bindings; libssl-dev is for
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
# rebuilds short, which is why the binaries are copied out of the cached target
# directory in the same step.
#
# voxctrl-llm-sidecar is the llama.cpp helper that runs the dictation cleanup
# model. The server looks for it beside its own binary; without it cleanup
# still works, on a much slower built-in engine. It is a separate build
# because the two packages do not share feature names: of FEATURES, only
# `vulkan` applies to it.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p voxctrl-server ${FEATURES:+--features "${FEATURES}"} \
    && if echo ",${FEATURES}," | grep -q ",vulkan,"; then \
         cargo build --release --locked -p voxctrl-llm-sidecar --features vulkan; \
       else \
         cargo build --release --locked -p voxctrl-llm-sidecar; \
       fi \
    && cp target/release/voxctrl-server target/release/voxctrl-llm-sidecar /usr/local/bin/

# ── audio.cpp server (optional) ───────────────────────────────────────────────
# Built on the runtime image itself, so it links the same glibc and libstdc++
# it will run with. Every model family is built, so the released engines keep
# working on it. A Vulkan build when FEATURES has `vulkan`, else CPU only.
# AUDIOCPP_JOBS bounds the compile: each job can take 4-5 GB of memory, and
# Vulkan's shader generators start up to 16 glslc processes each, so they get
# one make job of their own.
FROM ${RUNTIME_IMAGE} AS audiocpp
ARG FEATURES=""
ARG AUDIOCPP_REPO=https://github.com/JRufer/audio.cpp
ARG AUDIOCPP_REF=""
ARG AUDIOCPP_JOBS=2
RUN mkdir -p /out \
    && if [ -n "${AUDIOCPP_REF}" ]; then \
         set -e; \
         vulkan=OFF; \
         if echo ",${FEATURES}," | grep -q ",vulkan,"; then vulkan=ON; fi; \
         apt-get update; \
         apt-get install -y --no-install-recommends ca-certificates git cmake g++ make; \
         if [ "$vulkan" = ON ]; then apt-get install -y --no-install-recommends libvulkan-dev glslc; fi; \
         git clone --depth 1 --branch "${AUDIOCPP_REF}" "${AUDIOCPP_REPO}" /audio.cpp; \
         cmake -S /audio.cpp -B /audio.cpp/build -DCMAKE_BUILD_TYPE=Release -DENGINE_ENABLE_VULKAN=$vulkan; \
         if [ "$vulkan" = ON ]; then cmake --build /audio.cpp/build --target ggml-vulkan --parallel 1; fi; \
         cmake --build /audio.cpp/build --target audiocpp_server --parallel "${AUDIOCPP_JOBS}"; \
         install -m 755 /audio.cpp/build/bin/audiocpp_server /out/audiocpp_server; \
         rm -rf /audio.cpp /var/lib/apt/lists/*; \
       fi

# ── Runtime ───────────────────────────────────────────────────────────────────
FROM ${RUNTIME_IMAGE}
ARG FEATURES=""

# espeak-ng is the eSpeak text-to-speech engine. libvulkan1 and libgomp1 are
# for the audio.cpp runtime the neural TTS engines download at first start: it
# is a Vulkan build and links the loader even when it runs on the CPU.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3t64 \
       espeak-ng libvulkan1 libgomp1 \
    && rm -rf /var/lib/apt/lists/*

# Everything the server writes lives under one directory, so one volume keeps
# the downloaded models and the generated API key across upgrades:
#   /var/lib/voxctrl/config/voxctrl/server.toml
#   /var/lib/voxctrl/data/voxctrl/models/
# Ubuntu's image comes with an `ubuntu` user at uid 1000. The server keeps
# that uid, so an existing volume's files still belong to it.
RUN (userdel --remove ubuntu 2>/dev/null || true) \
    && useradd --system --uid 1000 --home-dir /var/lib/voxctrl --shell /usr/sbin/nologin voxctrl \
    && mkdir -p /var/lib/voxctrl/config /var/lib/voxctrl/data \
    && chown -R voxctrl:voxctrl /var/lib/voxctrl
ENV HOME=/var/lib/voxctrl \
    XDG_CONFIG_HOME=/var/lib/voxctrl/config \
    XDG_DATA_HOME=/var/lib/voxctrl/data \
    VOXCTRL_SERVER_BIND=0.0.0.0:8787

COPY --from=build /usr/local/bin/voxctrl-server /usr/local/bin/voxctrl-llm-sidecar /usr/local/bin/
# The server looks for a bundled audiocpp_server in ../lib/voxctrl beside its
# own binary and prefers it to the runtime it would download. Empty unless
# AUDIOCPP_REF was set.
COPY --from=audiocpp /out/ /usr/local/lib/voxctrl/

USER voxctrl
VOLUME /var/lib/voxctrl
EXPOSE 8787

# The server only starts listening once its model is downloaded and loaded,
# which on a first start can take several minutes for a large model.
HEALTHCHECK --interval=30s --timeout=10s --start-period=15m --retries=3 \
    CMD ["voxctrl-server", "--healthcheck"]

ENTRYPOINT ["voxctrl-server"]
