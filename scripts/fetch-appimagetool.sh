#!/usr/bin/env bash
# Fetch appimagetool, the reference AppImage packaging tool.
#
# Used to build the AppImage bundle from an AppDir (see appimage-pack.sh).
# It used to be vendored directly in the repo as `appimagetool.bin`; that put a
# 15 MB third-party binary in every clone and in git history forever. Fetching
# it here, the same way fetch-uruntime.sh fetches the AppImage runtime, keeps
# the repo free of committed binaries without changing what build_appimage.sh
# or CI actually run.
#
# Usage: fetch-appimagetool.sh <destination-path>
#
# Exits non-zero (without leaving a usable file behind) if the tool could not
# be fetched or does not look like a working appimagetool.
set -euo pipefail

dest="${1:?usage: fetch-appimagetool.sh <destination-path>}"
url="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"

mkdir -p "$(dirname "$dest")"

if ! curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 30 -o "$dest" "$url"; then
    echo "fetch-appimagetool: download failed ($url)" >&2
    rm -f "$dest"
    exit 1
fi
chmod +x "$dest"

# It must be a self-contained x86-64 ELF: anything else (an HTML error page, a
# redirect stub, a wrong-architecture asset) would silently break every build
# that depends on it.
if ! file -b "$dest" | grep -q 'ELF 64-bit.*x86-64'; then
    echo "fetch-appimagetool: downloaded file is not an x86-64 ELF binary" >&2
    rm -f "$dest"
    exit 1
fi

# And it must answer as appimagetool itself, extracting itself to run since the
# build environment may have no FUSE at all (see appimage-pack.sh).
if ! APPIMAGE_EXTRACT_AND_RUN=1 "$dest" --version >/dev/null 2>&1; then
    echo "fetch-appimagetool: downloaded tool did not answer --version" >&2
    rm -f "$dest"
    exit 1
fi

echo "fetch-appimagetool: using $url" >&2
