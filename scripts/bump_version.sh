#!/usr/bin/env bash
# Bumps VoxCtrl's version number in every file that declares one, in a single
# atomic step. There is no single source of truth for the app version:
#
#   - src-tauri/tauri.conf.json  ("version")      — what Tauri's getVersion()
#     returns, what the About tab shows, and what build_appimage.sh /
#     release.yml derive the release tag and filenames from.
#   - package.json                ("version")      — npm/frontend package
#     version; cosmetic today, but easy to let drift (it did once: v0.1.8
#     shipped with package.json still at 0.1.7).
#   - Cargo.toml                  ([workspace.package] version) — the Cargo
#     version inherited by every crate via `version.workspace = true`.
#   - packaging/aur/{voxctrl-bin,voxctrl,voxctrl-server}/PKGBUILD (pkgver) — the AUR
#     packages' version; aur-publish.yml sets it again from the tag.
#   - docs/release-notes/<version>.md — not a version field, but the release
#     page's "What's new" text; a stub is created here and CI requires it be
#     written before a release for that version will build.
#
# Nothing enforces these match each other, and nothing enforces the release
# tag matches them either (release.yml lets a manual workflow_dispatch
# override the tag with an arbitrary string). Use this script instead of
# hand-editing version fields so all three stay in lockstep; CI additionally
# verifies they agree before every release build (see release.yml).
#
# Usage:
#   ./scripts/bump_version.sh 0.2.8
#   ./scripts/bump_version.sh 1.0.0-rc.1     # a release candidate
#
# A pre-release version (`1.0.0-rc.1`) is a SemVer pre-release: it sorts below
# `1.0.0`, which is what the updater, Cargo and npm all expect. Use the dotted
# form, `rc.1`, so `rc.10` sorts after `rc.9`. The release workflow publishes
# it as a GitHub pre-release, which the in-app updater never offers, and
# rewrites the .deb's version to `1.0.0~rc.1` so apt also sorts it below
# `1.0.0` (see scripts/deb_prerelease_version.sh). The AUR PKGBUILDs are left
# alone: pkgver may not contain `-`.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [ $# -ne 1 ]; then
    echo "Usage: $0 <new-version>   (e.g. $0 0.2.8)" >&2
    exit 1
fi

NEW_VERSION="$1"
if ! [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+((\.[0-9]+)|(-(alpha|beta|rc)\.[0-9]+))?$ ]]; then
    echo "Error: '$NEW_VERSION' doesn't look like a version number (expected e.g. 1.2.3 or 1.0.0-rc.1)" >&2
    exit 1
fi

if ! command -v jq &>/dev/null; then
    echo "Error: this script requires 'jq'. Install it (e.g. sudo pacman -S jq / sudo apt install jq)." >&2
    exit 1
fi

update_json() {
    local file="$1"
    local tmp
    tmp=$(mktemp)
    # jq's default output already ends with exactly one trailing newline
    # (regardless of whether the input file had one) — don't add another.
    jq --indent 2 --arg v "$NEW_VERSION" '.version = $v' "$file" > "$tmp"
    mv "$tmp" "$file"
    echo "  Updated $file"
}

echo "Bumping VoxCtrl version to $NEW_VERSION..."

update_json "package.json"
update_json "src-tauri/tauri.conf.json"

# Cargo.toml is TOML, not JSON: patch the version line under [workspace.package].
if ! grep -q '^\[workspace\.package\]' Cargo.toml; then
    echo "Error: could not find [workspace.package] in Cargo.toml" >&2
    exit 1
fi
sed -i "/^\[workspace\.package\]/,/^\[/ s/^version = \".*\"/version = \"${NEW_VERSION}\"/" Cargo.toml
echo "  Updated Cargo.toml"

# The AUR PKGBUILDs' pkgver. The AUR copies get it (and real checksums) from
# the release tag when .github/workflows/aur-publish.yml runs; this keeps the
# repo's copies, and the smoke test that builds from them, on the same version.
# Four-part versions aren't valid AUR versions here, so those are left alone.
if [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    for pkgbuild in packaging/aur/voxctrl-bin/PKGBUILD packaging/aur/voxctrl/PKGBUILD \
                    packaging/aur/voxctrl-server/PKGBUILD; do
        [ -f "$pkgbuild" ] || continue
        sed -i -e "s/^pkgver=.*/pkgver=${NEW_VERSION}/" -e "s/^pkgrel=.*/pkgrel=1/" "$pkgbuild"
        echo "  Updated $pkgbuild"
    done
fi

# The release page's "What's new" is built from docs/release-notes/<version>.md
# (see release.yml). Create a stub so the notes are written in the same commit
# as the bump; CI refuses to build a release while the stub is still in place.
NOTES_FILE="docs/release-notes/${NEW_VERSION}.md"
mkdir -p docs/release-notes
if [ ! -e "$NOTES_FILE" ]; then
    cat > "$NOTES_FILE" <<'STUB'
<!-- TODO: write release notes. Replace this file's contents with what changed
     in this version, in the same style as the earlier files in this folder.
     The "What's new in <version>" heading is added automatically. -->
STUB
    echo "  Created $NOTES_FILE (stub — write the release notes before pushing)"
else
    echo "  Release notes already exist: $NOTES_FILE"
fi

echo ""
echo "Done. Verify with:"
echo "  jq -r .version package.json src-tauri/tauri.conf.json"
echo "  grep -A1 '\\[workspace.package\\]' Cargo.toml"
echo ""
# Cargo.lock records each workspace crate's version too. Builds that pass
# `--locked` (the Docker image, the AUR source packages) refuse a lockfile that
# disagrees with the manifests, so it is brought up to date here rather than
# left to "the next cargo build", which is how a stale entry once shipped.
# `--workspace` touches only the workspace's own entries. `--offline` first,
# as that needs nothing from the network; without it if the local index
# cannot answer.
if command -v cargo &>/dev/null; then
    if cargo update --workspace --offline --quiet 2>/dev/null || cargo update --workspace --quiet; then
        echo "  Cargo.lock -> $NEW_VERSION (workspace crates)"
    else
        echo "  WARNING: could not refresh Cargo.lock. Run 'cargo update --workspace'" >&2
        echo "           and commit the result: '--locked' builds fail without it." >&2
    fi
else
    echo "  WARNING: cargo not found, so Cargo.lock still has the old version." >&2
    echo "           Run 'cargo update --workspace' and commit the result." >&2
fi
