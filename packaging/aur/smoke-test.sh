#!/usr/bin/env bash
# Build and install the voxctrl-bin package from a given .deb, then check it.
#
#   packaging/aur/smoke-test.sh path/to/VoxCtrl-linux-x86_64-vulkan-webgpu.deb
#
# Meant for an archlinux container in CI, run as root: it creates a build user
# (makepkg refuses root), installs the package with pacman so every `depends`
# entry is resolved for real, and fails on anything a user would hit:
#   - namcap errors on the PKGBUILD or the package
#   - a library the binaries need that no dependency provides
#   - `voxctrl --version` not starting
#   - a file outside the layout the docs promise
# The .deb stands in for the release download, so a release can be checked
# while it is still a draft, before anyone can install it.
set -euo pipefail

deb="$(realpath "${1:?usage: smoke-test.sh <webgpu .deb>}")"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

pacman -Syu --noconfirm --needed base-devel namcap >/dev/null

id builder >/dev/null 2>&1 || useradd -m builder
work="$(mktemp -d)"
cp "$here/voxctrl-bin/PKGBUILD" "$work/"
pkgver="$(sed -n 's/^pkgver=//p' "$work/PKGBUILD")"

# Pre-place every source under the name the PKGBUILD downloads it as; makepkg
# uses a file that is already there instead of fetching it.
cp "$deb"                          "$work/voxctrl-$pkgver.deb"
cp "$repo/ai.voxctrl.app.desktop"  "$work/ai.voxctrl.app-$pkgver.desktop"
cp "$repo/LICENSE"                 "$work/LICENSE-$pkgver"
chown -R builder: "$work"

echo "── namcap PKGBUILD"
namcap "$work/PKGBUILD" | tee "$work/namcap-pkgbuild.txt"

echo "── makepkg"
# --skipchecksums: the PKGBUILD in the repo carries SKIP until release time,
# and this .deb is not the one a published release will hash anyway.
su builder -c "cd '$work' && makepkg --skipchecksums --nodeps -f"
pkgfile="$(ls "$work"/voxctrl-bin-*.pkg.tar.zst)"

echo "── namcap package"
namcap "$pkgfile" | tee "$work/namcap-pkg.txt"
if grep -E ' E: ' "$work/namcap-pkgbuild.txt" "$work/namcap-pkg.txt"; then
  echo "error: namcap reported errors (above)" >&2
  exit 1
fi

echo "── install"
pacman -U --noconfirm "$pkgfile"

echo "── file layout"
expected=(
  /usr/bin/voxctrl
  /usr/bin/voxctrl-llm-sidecar
  /usr/lib/voxctrl/libwebgpu_dawn.so
  /usr/share/applications/ai.voxctrl.app.desktop
  /usr/share/icons/hicolor/32x32/apps/voxctrl.png
  /usr/share/icons/hicolor/128x128/apps/voxctrl.png
  /usr/share/icons/hicolor/256x256/apps/voxctrl.png
  /usr/share/licenses/voxctrl-bin/LICENSE
)
for f in "${expected[@]}"; do
  [ -e "$f" ] || { echo "error: $f is missing" >&2; exit 1; }
done
unexpected="$(pacman -Qlq voxctrl-bin | grep -v '/$' | grep -vxF -f <(printf '%s\n' "${expected[@]}") || true)"
if [ -n "$unexpected" ]; then
  echo "error: files outside the documented layout:" >&2
  echo "$unexpected" >&2
  exit 1
fi

echo "── shared libraries"
# Releases up to 0.8.7 look for Dawn in /usr/lib only; from 0.8.8 the RUNPATH
# includes /usr/lib/voxctrl. ldd follows RUNPATH, so a new build is checked as
# users will run it. VOXCTRL_SMOKE_STRICT=1 (set by the release workflow) makes
# an old RUNPATH a failure; without it — a PR testing its PKGBUILD against the
# last published release — it is a warning, and Dawn is found by path instead.
if ! readelf -d /usr/bin/voxctrl | grep -q 'lib/voxctrl'; then
  if [ "${VOXCTRL_SMOKE_STRICT:-0}" = 1 ]; then
    echo "error: /usr/bin/voxctrl has no RUNPATH entry for /usr/lib/voxctrl (src-tauri/build.rs)" >&2
    exit 1
  fi
  echo "warning: this .deb predates the /usr/lib/voxctrl RUNPATH; checking with LD_LIBRARY_PATH" >&2
  export LD_LIBRARY_PATH=/usr/lib/voxctrl
fi
missing="$(ldd /usr/bin/voxctrl /usr/bin/voxctrl-llm-sidecar | grep 'not found' || true)"
if [ -n "$missing" ]; then
  echo "error: unresolved libraries:" >&2
  echo "$missing" >&2
  exit 1
fi

echo "── voxctrl --version"
voxctrl --version

echo "smoke test passed: $(pacman -Q voxctrl-bin)"
