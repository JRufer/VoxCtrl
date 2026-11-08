#!/usr/bin/env bash
# Fill pkgver, pkgrel and real checksums into the release PKGBUILDs and
# regenerate their .SRCINFO files.
#
#   packaging/aur/update-pkgbuilds.sh 0.8.8        # pkgrel=1
#   packaging/aur/update-pkgbuilds.sh 0.8.8 2      # a packaging-only fix
#
# Runs on Arch (or in an archlinux container) as a normal user: it needs
# makepkg and updpkgsums (pacman-contrib), and makepkg refuses to run as root.
# The release must already be published, because updpkgsums downloads every
# source to hash it. voxctrl-git is left alone: it has no fixed version.
set -euo pipefail

version="${1:?usage: update-pkgbuilds.sh <version> [pkgrel]}"
pkgrel="${2:-1}"
version="${version#v}"

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: '$version' is not a plain X.Y.Z version; pre-releases don't go to the AUR" >&2
  exit 1
fi
if ! [[ "$pkgrel" =~ ^[1-9][0-9]*$ ]]; then
  echo "error: pkgrel must be a positive integer, got '$pkgrel'" >&2
  exit 1
fi
for tool in makepkg updpkgsums; do
  command -v "$tool" >/dev/null || { echo "error: $tool not found (pacman -S base-devel pacman-contrib)" >&2; exit 1; }
done

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

for pkg in voxctrl-bin voxctrl; do
  dir="$here/$pkg"
  echo "── $pkg $version-$pkgrel"
  sed -i -e "s/^pkgver=.*/pkgver=$version/" -e "s/^pkgrel=.*/pkgrel=$pkgrel/" "$dir/PKGBUILD"
  (
    cd "$dir"
    updpkgsums
    if grep -q "'SKIP'" PKGBUILD; then
      echo "error: $pkg still has a SKIP checksum after updpkgsums" >&2
      exit 1
    fi
    makepkg --printsrcinfo > .SRCINFO
    # updpkgsums leaves the downloaded sources here; they are not part of the package repo.
    find . -maxdepth 1 -type f ! -name PKGBUILD ! -name .SRCINFO -delete
  )
done

echo "Done. Review with: git diff -- packaging/aur"
