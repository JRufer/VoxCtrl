#!/usr/bin/env bash
# Compare the runtime stacks of two VoxCtrl AppImages.
#
# The overlay's closing animation renders correctly from a locally built
# AppImage and smears on a CI-built one, from identical source. Identical
# source means the cause is in the *bundle*, not the code — so the useful
# question is not "which library do we think matters" but "which libraries
# actually differ", which is what this answers.
#
# Usage:
#   ./scripts/diff-appimage-stacks.sh GOOD.AppImage BAD.AppImage
#
# Prints, for each bundle: the app version, whether the binary carries the
# current rendering-defaults code (so a build predating it is never mistaken
# for a failed fix), every bundled shared library with its real version, and
# which of the libraries the build scripts deliberately strip are resolving to
# the host instead. Then the diff between the two.
#
# Needs no FUSE (uses --appimage-extract) and changes nothing on the system.

set -uo pipefail

if [ $# -ne 2 ]; then
    echo "Usage: $0 <known-good.AppImage> <known-bad.AppImage>" >&2
    echo "  e.g. $0 VoxCtrl-local.AppImage VoxCtrl-linux-x86_64-vulkan.AppImage" >&2
    exit 1
fi

for arg in "$1" "$2"; do
    if [ ! -f "$arg" ]; then
        echo "Not a file: $arg" >&2
        exit 1
    fi
done

# Resolved only after the existence check, so a bad path is reported as the
# user typed it rather than as an empty string.
GOOD_IMAGE=$(readlink -f "$1")
BAD_IMAGE=$(readlink -f "$2")

for img in "$GOOD_IMAGE" "$BAD_IMAGE"; do
    if [ ! -x "$img" ]; then
        echo "Not executable (run: chmod +x '$img'): $img" >&2
        exit 1
    fi
done

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# Libraries the build scripts delete from the bundle on purpose, so the app
# falls through to the host's copies. Listed here to report which host
# versions are actually being used at runtime — a mismatch between the
# bundled half of the stack and these is the seam worth looking at.
HOST_FIRST=(libEGL libGL libGLX libGLdispatch libOpenGL libglapi libgbm libdrm
            libglib-2.0 libgobject-2.0 libgio-2.0 libwayland-client libxkbcommon)

# The bundled half of the graphics stack, reported first because these are the
# ones that differ by build host.
INTEREST=(libwebkit2gtk libjavascriptcoregtk libgtk-3 libgdk-3 libcairo
          libpango libgdk_pixbuf libharfbuzz libepoxy libsoup)

extract() {
    local image="$1" dest="$2"
    mkdir -p "$dest"
    ( cd "$dest" && "$image" --appimage-extract >/dev/null 2>&1 )
    if [ ! -d "$dest/squashfs-root" ]; then
        echo "Could not extract $image" >&2
        return 1
    fi
}

# Resolve a library file to the most specific version string available: the
# real filename behind any symlinks (libfoo.so.1.2.3 tells you more than
# libfoo.so.1), falling back to the soname.
lib_version() {
    local path="$1"
    basename "$(readlink -f "$path" 2>/dev/null || echo "$path")"
}

inventory() {
    local root="$1" out="$2"
    : > "$out"
    # -L so symlinked library directories are followed; some bundles nest
    # libraries under usr/lib/x86_64-linux-gnu as a link.
    find -L "$root" -name '*.so*' -type f 2>/dev/null | while read -r lib; do
        local base soname
        base=$(basename "$lib")
        # Reduce libfoo.so.1.2.3 to the stem "libfoo" so the two bundles'
        # entries line up even when their versions differ.
        soname=$(echo "$base" | sed -E 's/\.so.*$//')
        printf '%s\t%s\n' "$soname" "$(lib_version "$lib")"
    done | sort -u > "$out"
}

app_fingerprint() {
    local root="$1"
    local bin
    bin=$(find -L "$root/usr/bin" -maxdepth 1 -type f -executable 2>/dev/null \
          | grep -iE 'voxctrl' | grep -v sidecar | head -1)

    if [ -z "$bin" ]; then
        echo "  (could not locate the main binary)"
        return
    fi

    # Does this build contain the rendering-defaults code? If this says no,
    # the build predates that change and nothing about it tests the fix.
    if strings "$bin" 2>/dev/null | grep -q 'LIBGL_ALWAYS_SOFTWARE'; then
        echo "  rendering-defaults code: PRESENT"
    else
        echo "  rendering-defaults code: ABSENT (build predates it)"
    fi

    local desktop version
    desktop=$(find -L "$root" -maxdepth 2 -name '*.desktop' 2>/dev/null | head -1)
    if [ -n "$desktop" ]; then
        version=$(grep -oP '^X-AppImage-Version=\K.*' "$desktop" 2>/dev/null)
        [ -n "${version:-}" ] && echo "  version: $version"
    fi
}

report_bundle() {
    local label="$1" root="$2" inv="$3"

    echo "── $label ──────────────────────────────────────────────"
    app_fingerprint "$root"
    echo
    echo "  Bundled graphics/UI libraries (these differ by build host):"
    local found=0
    for name in "${INTEREST[@]}"; do
        local hit
        hit=$(awk -F'\t' -v n="$name" '$1 == n {print $2; exit}' "$inv")
        if [ -n "$hit" ]; then
            printf '    %-24s %s\n' "$name" "$hit"
            found=1
        fi
    done
    [ "$found" -eq 0 ] && echo "    (none found)"
    echo
    echo "  Stripped from the bundle — resolving to the host at runtime:"
    for name in "${HOST_FIRST[@]}"; do
        local hit
        hit=$(awk -F'\t' -v n="$name" '$1 == n {print $2; exit}' "$inv")
        if [ -n "$hit" ]; then
            printf '    %-24s %s  <-- STILL BUNDLED (expected stripped)\n' "$name" "$hit"
        fi
    done
    echo
}

echo "Extracting (no FUSE needed, nothing is installed or modified)..."
extract "$GOOD_IMAGE" "$WORK/good" || exit 1
extract "$BAD_IMAGE" "$WORK/bad" || exit 1

inventory "$WORK/good/squashfs-root" "$WORK/good.inv"
inventory "$WORK/bad/squashfs-root" "$WORK/bad.inv"

echo
report_bundle "GOOD  $(basename "$GOOD_IMAGE")" "$WORK/good/squashfs-root" "$WORK/good.inv"
report_bundle "BAD   $(basename "$BAD_IMAGE")" "$WORK/bad/squashfs-root" "$WORK/bad.inv"

echo "── Version differences (same library, different build) ──────────"
join -t$'\t' "$WORK/good.inv" "$WORK/bad.inv" 2>/dev/null \
  | awk -F'\t' '$2 != $3 {printf "    %-28s good: %-28s bad: %s\n", $1, $2, $3}' \
  | sort | head -60
echo

echo "── Only in the GOOD bundle ──────────────────────────────────────"
comm -23 <(cut -f1 "$WORK/good.inv" | sort -u) <(cut -f1 "$WORK/bad.inv" | sort -u) \
  | sed 's/^/    /' | head -40
echo

echo "── Only in the BAD bundle ───────────────────────────────────────"
comm -13 <(cut -f1 "$WORK/good.inv" | sort -u) <(cut -f1 "$WORK/bad.inv" | sort -u) \
  | sed 's/^/    /' | head -40
echo

echo "── Host (what the stripped libraries actually resolve to) ───────"
for name in libEGL libGL libgbm libdrm libglib-2.0 libgtk-3 libwebkit2gtk-4.1; do
    hit=$(ldconfig -p 2>/dev/null | grep -oP "^\s*${name}\.so[^ ]*\s.*=>\s*\K.*" | head -1)
    if [ -n "${hit:-}" ]; then
        printf '    %-24s %s\n' "$name" "$(lib_version "$hit")"
    else
        printf '    %-24s (not installed on this host)\n' "$name"
    fi
done
echo
echo "Done. Paste this whole output back."
