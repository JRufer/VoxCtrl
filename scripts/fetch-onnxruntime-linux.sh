#!/usr/bin/env bash
# Fetch Microsoft's official ONNX Runtime shared library for Linux x86_64 and
# point `ort` at it, for builds that must run on Ubuntu 22.04 / Debian 12.
#
# Why: `ort` 2.0.0-rc.15 (ONNX Runtime 1.31) is needed so the Windows GPU
# build works without Microsoft's closed-source dxil.dll (see
# crates/voxctrl-inference/Cargo.toml). Its prebuilt Linux static library is
# built against glibc 2.38 and GCC 13's libstdc++ (it calls __isoc23_strtol and
# std::string::_M_replace_cold), so nothing linked from it starts on, or even
# links on, Ubuntu 22.04 (glibc 2.35, GCC 12). Microsoft's own
# libonnxruntime.so.1 is built on manylinux_2_28 and needs only glibc 2.28 and
# GLIBCXX_3.4.22. Builds on newer distributions (the WebGPU AppImage on
# ubuntu-24.04, Docker on Debian trixie, Arch) keep the static prebuilt.
#
# The library is taken from the onnxruntime-node npm package, which is where
# Microsoft publishes the same build with a registry-checked hash. Both the
# package and the extracted library are verified against pinned hashes.
#
# Usage: scripts/fetch-onnxruntime-linux.sh <dir>
#   Leaves <dir>/libonnxruntime.so.1 and a libonnxruntime.so link to it. When
#   run under GitHub Actions it also exports, through $GITHUB_ENV:
#     ORT_LIB_PATH=<dir> ORT_PREFER_DYNAMIC_LINK=1   so ort links it dynamically
#     ORT_SHARED_LIB=<dir>/libonnxruntime.so.1      for the packaging steps
#   and prepends <dir> to LD_LIBRARY_PATH, so tests and linuxdeploy find it.
#
# The binary's rpath ($ORIGIN, $ORIGIN/../lib/voxctrl, $ORIGIN/../lib; see
# src-tauri/build.rs) finds the library where the .deb and AppImage put it.
set -euo pipefail

VERSION="1.31.1"
TARBALL_URL="https://registry.npmjs.org/onnxruntime-node/-/onnxruntime-node-${VERSION}.tgz"
# npm's dist.integrity for onnxruntime-node@1.31.1 (sha512, base64).
TARBALL_SHA512="ijnSghbDHVWaQ2nDPmVccPlCHI8kH1Mx1aJ4a/qzRP16bBcVs4E3EqZQKZCkhWnbqN1tuQWxagimFtDpbPYYQw=="
MEMBER="package/bin/napi-v6/linux/x64/libonnxruntime.so.1"
LIB_SHA256="f19a4da48c4c65e0cdccff82354482f657fa91a3455b66174195b94ca55b0e6f"

dest="${1:?usage: $0 <dir>}"
mkdir -p "$dest"
dest="$(cd "$dest" && pwd)"
lib="$dest/libonnxruntime.so.1"

if [ -f "$lib" ] && echo "$LIB_SHA256  $lib" | sha256sum -c --status; then
  echo "ONNX Runtime ${VERSION} already in $dest"
else
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  echo "Downloading onnxruntime-node ${VERSION} (Microsoft's ONNX Runtime build)"
  curl -fsSL --retry 4 --retry-delay 5 -o "$tmp/ort.tgz" "$TARBALL_URL"
  actual="$(openssl dgst -sha512 -binary "$tmp/ort.tgz" | base64 -w0)"
  if [ "$actual" != "$TARBALL_SHA512" ]; then
    echo "::error::onnxruntime-node ${VERSION} does not match its pinned sha512 (got $actual)" >&2
    exit 1
  fi
  tar -xzf "$tmp/ort.tgz" -C "$tmp" "$MEMBER"
  if ! echo "$LIB_SHA256  $tmp/$MEMBER" | sha256sum -c --status; then
    echo "::error::libonnxruntime.so.1 from onnxruntime-node ${VERSION} does not match its pinned sha256" >&2
    exit 1
  fi
  install -m 0755 "$tmp/$MEMBER" "$lib"
fi
ln -sfn libonnxruntime.so.1 "$dest/libonnxruntime.so"
echo "ONNX Runtime ${VERSION}: $lib"

if [ -n "${GITHUB_ENV:-}" ]; then
  {
    echo "ORT_LIB_PATH=$dest"
    echo "ORT_PREFER_DYNAMIC_LINK=1"
    echo "ORT_SHARED_LIB=$lib"
    echo "LD_LIBRARY_PATH=$dest${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  } >> "$GITHUB_ENV"
fi
