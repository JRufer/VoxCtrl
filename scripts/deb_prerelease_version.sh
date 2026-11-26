#!/usr/bin/env bash
# Give a .deb of a pre-release the version dpkg sorts correctly.
#
# VoxCtrl's versions are SemVer: 1.0.0-rc.1 comes before 1.0.0. dpkg reads a
# `-` as the start of the Debian revision, so it takes 1.0.0-rc.1 as version
# 1.0.0, revision rc.1, and sorts it *after* 1.0.0: apt would then call the
# real release a downgrade. Debian's convention for "comes before" is `~`, so
# the package's Version becomes 1.0.0~rc.1. Only the control file changes.
#
# Usage: scripts/deb_prerelease_version.sh path/to/package.deb
# The release workflow runs it on each .deb when the app version has a `-`.

set -euo pipefail

deb="$1"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

dpkg-deb --raw-extract "$deb" "$work/pkg"
control="$work/pkg/DEBIAN/control"
old=$(sed -n 's/^Version: //p' "$control")
if [[ "$old" != *-* ]]; then
    echo "$deb: version $old has no pre-release part; left as it is"
    exit 0
fi
new="${old/-/\~}"
sed -i "s/^Version: .*/Version: ${new}/" "$control"
dpkg-deb --root-owner-group --build "$work/pkg" "$deb" >/dev/null
echo "$deb: Version ${old} -> $(dpkg-deb --field "$deb" Version)"
