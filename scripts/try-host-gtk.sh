#!/usr/bin/env bash
# Run a CI-built AppImage with its bundled GTK stack removed, so GTK comes
# from the host like WebKit already does.
#
# Why: `diff-appimage-stacks.sh` showed that libwebkit2gtk is in neither
# bundle — linuxdeploy's excludelist drops it — so every build already uses
# the *host's* WebKitGTK. GLib and Mesa are stripped on purpose, so those are
# the host's too. GTK3/GDK3 (and Cairo/Pango/gdk-pixbuf with them) are the one
# part of that stack still coming out of the bundle, which on a CI build means
# ubuntu-22.04's versions sitting underneath the host's much newer WebKit.
#
# WebKit asks GDK for the window's visual, surface and frame clock — the exact
# machinery behind alpha compositing and repaint scheduling — so that seam is
# a plausible cause of "transparency is right when static, smeared when
# animating". A locally built AppImage bundles the build machine's own GTK,
# which matches the WebKit it runs against, and shows no symptom.
#
# This tests that in place, without a rebuild or a release: it removes the
# bundled GTK stack from an extracted copy and runs it. Nothing is installed
# and the original AppImage file is not modified.
#
# Usage:
#   ./scripts/try-host-gtk.sh VoxCtrl-linux-x86_64-vulkan.AppImage
#   ./scripts/try-host-gtk.sh --keep VoxCtrl-...AppImage   # prepare, don't run
#
# If the overlay's closing animation is clean under this, the fix is to stop
# bundling GTK in build_appimage.sh and .github/workflows/release.yml.

set -uo pipefail

KEEP_ONLY=false
STRIP_GTK=true
STRIP_WEBKIT=false

while [ $# -gt 1 ]; do
    case "$1" in
        --keep)    KEEP_ONLY=true ;;
        --gtk)     STRIP_GTK=true;  STRIP_WEBKIT=false ;;
        # The bundled WebKit helper processes and injected bundle come from
        # the build host, while libwebkit2gtk itself is excluded from the
        # bundle by linuxdeploy and therefore comes from the user's system.
        # On a CI build that pairs ubuntu-22.04 helpers with whatever
        # WebKitGTK the host has — and the web process is the one that
        # actually renders and composites the page.
        --webkit)  STRIP_GTK=false; STRIP_WEBKIT=true ;;
        --all)     STRIP_GTK=true;  STRIP_WEBKIT=true ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
    shift
done

if [ $# -ne 1 ]; then
    echo "Usage: $0 [--keep] [--gtk|--webkit|--all] <AppImage>" >&2
    echo "  --gtk     (default) run against the host's GTK stack" >&2
    echo "  --webkit  run against the host's WebKit helper processes" >&2
    echo "  --all     both" >&2
    exit 1
fi

if [ ! -f "$1" ]; then
    echo "Not a file: $1" >&2
    exit 1
fi
IMAGE=$(readlink -f "$1")
if [ ! -x "$IMAGE" ]; then
    echo "Not executable (run: chmod +x '$IMAGE')" >&2
    exit 1
fi

WORK=$(mktemp -d -t voxctrl-hostgtk-XXXXXX)
if [ "$KEEP_ONLY" = false ]; then
    trap 'rm -rf "$WORK"' EXIT
fi

echo "Extracting to $WORK ..."
( cd "$WORK" && "$IMAGE" --appimage-extract >/dev/null 2>&1 )
ROOT="$WORK/squashfs-root"
if [ ! -d "$ROOT" ]; then
    echo "Could not extract $IMAGE" >&2
    exit 1
fi

# The GTK stack, and everything GTK links that would otherwise be resolved
# against the bundle's older copies. The rule the build scripts already state
# for the libraries they strip applies here too: once one of these comes from
# the host, they all have to, or the host's copy resolves its symbols against
# a stale bundled one.
STACK=(
    'libgtk-3.so*' 'libgdk-3.so*'
    'libcairo.so*' 'libcairo-gobject.so*' 'libcairo-script-interpreter.so*'
    'libpango-1.0.so*' 'libpangocairo-1.0.so*' 'libpangoft2-1.0.so*'
    'libgdk_pixbuf-2.0.so*'
    'libatk-1.0.so*' 'libatk-bridge-2.0.so*' 'libatspi.so*'
    'libepoxy.so*' 'libharfbuzz.so*' 'libfribidi.so*'
    'librsvg-2.so*'
)

# Report which WebKitGTK a bundle actually carries. Both builds name the file
# libwebkit2gtk-4.1.so.0, so the filename says nothing — the version has to
# come out of the binary itself. Reading the filename is exactly how an
# earlier pass concluded WebKit was not bundled at all.
webkit_version() {
    local lib="$1" ver
    ver=$(basename "$(readlink -f "$lib")")
    if [ "$ver" = "$(basename "$lib")" ]; then
        # Not a versioned symlink; ask the binary.
        ver=$(strings "$lib" 2>/dev/null | grep -oE 'WebKitGTK/[0-9]+\.[0-9]+\.[0-9]+' | head -1)
        [ -n "$ver" ] || ver="(version not readable from the binary)"
    fi
    echo "$ver"
}

if [ "$STRIP_WEBKIT" = true ]; then
    echo
    echo "Bundled WebKitGTK in this AppImage:"
    mapfile -t WK_LIBS < <(find -L "$ROOT" \( -name 'libwebkit2gtk-4.*.so*' \
                                          -o -name 'libjavascriptcoregtk-4.*.so*' \) 2>/dev/null | sort -u)
    for lib in "${WK_LIBS[@]}"; do
        printf '    %-44s %s\n' "${lib#$ROOT/}" "$(webkit_version "$lib")"
    done
    [ "${#WK_LIBS[@]}" -eq 0 ] && echo "    (none — already using the host's)"

    echo
    echo "Removing the bundled WebKit entirely (library, helper processes and"
    echo "injected bundle) so the host's WebKitGTK is used instead:"

    # The library and its helpers are one unit: the bundled libwebkit2gtk has
    # the bundled helper path compiled into it, so removing only the helpers
    # leaves it unable to spawn its web process and the app aborts at startup.
    # Both go, and the host's matching pair is used.
    wk_removed=0
    for lib in "${WK_LIBS[@]}"; do
        echo "    ${lib#$ROOT/}"
        chmod u+w "$lib" 2>/dev/null
        rm -f "$lib"
        wk_removed=$((wk_removed + 1))
    done
    while IFS= read -r dir; do
        [ -n "$dir" ] || continue
        echo "    ${dir#$ROOT/}/"
        chmod -R u+w "$dir" 2>/dev/null
        rm -rf "$dir"
        wk_removed=$((wk_removed + 1))
    done < <(find -L "$ROOT" -type d -name 'webkit2gtk-4.*' 2>/dev/null | sort -u)

    if [ "$wk_removed" -eq 0 ]; then
        echo "    (nothing found)"
        echo >&2
        echo "ERROR: no bundled WebKit to remove." >&2
        echo "Refusing to launch: a run with nothing removed proves nothing." >&2
        exit 1
    fi

    # AppRun points WebKit at the bundled helpers. With them gone the exports
    # must go too, or the host's WebKit is sent to paths that no longer exist
    # instead of using the helpers its own library ships with.
    for script in "$ROOT/AppRun" "$ROOT/apprun-hooks/linuxdeploy-plugin-gtk.sh"; do
        [ -f "$script" ] || continue
        sed -i -E 's/^([[:space:]]*export[[:space:]]+(WEBKIT_EXEC_PATH|WEBKIT_INJECTED_BUNDLE_PATH)=)/# \1/' "$script"
    done
    echo "  ($wk_removed removed; WEBKIT_EXEC_PATH / WEBKIT_INJECTED_BUNDLE_PATH neutralised)"

    if ! ldconfig -p 2>/dev/null | grep -q 'libwebkit2gtk-4\.1\.so'; then
        echo >&2
        echo "WARNING: this host has no libwebkit2gtk-4.1 installed, so there is" >&2
        echo "nothing to fall back to and the app will not start. Install it first" >&2
        echo "(e.g. pacman -S webkit2gtk-4.1 / apt install libwebkit2gtk-4.1-0)." >&2
    fi
fi

if [ "$STRIP_GTK" = false ]; then
    echo
    echo "── Launching (GTK left bundled). Watch the overlay CLOSE. ──"
    echo "   (Ctrl-C here when you're done.)"
    echo
    if [ "$KEEP_ONLY" = true ]; then
        echo "Prepared (not run): $ROOT/AppRun"
        echo "Remove it afterwards with:  rm -rf $WORK"
        exit 0
    fi
    "$ROOT/AppRun"
    exit $?
fi

echo
echo "Removing the bundled GTK stack (the host's will be used instead):"

# `-L` matters: AppDir library directories are often reached through a
# symlink, and plain `find` will not descend into one. An earlier version of
# this script omitted it, removed nothing, and still launched — which looked
# like a clean test of the hypothesis while testing nothing at all. Hence both
# the `-L` here and the hard stop below.
collect_stack() {
    local pat
    for pat in "${STACK[@]}"; do
        find -L "$ROOT" -name "$pat" 2>/dev/null
    done | sort -u
}

mapfile -t FOUND < <(collect_stack)

if [ "${#FOUND[@]}" -eq 0 ]; then
    echo "    (none found)"
    echo
    echo "ERROR: none of the bundled GTK libraries could be found to remove." >&2
    echo "Refusing to launch: a run with nothing removed proves nothing." >&2
    echo >&2
    echo "Libraries that ARE in this bundle, for diagnosis:" >&2
    find -L "$ROOT" -name '*.so*' 2>/dev/null \
        | sed "s|$ROOT/||" | sort | head -40 >&2
    exit 1
fi

for lib in "${FOUND[@]}"; do
    echo "    ${lib#$ROOT/}"
    chmod u+w "$lib" 2>/dev/null
    rm -f "$lib"
done
echo "  (${#FOUND[@]} files removed)"

# Prove the removal actually took, rather than trusting that it did.
mapfile -t STILL_THERE < <(collect_stack)
if [ "${#STILL_THERE[@]}" -ne 0 ]; then
    echo >&2
    echo "ERROR: ${#STILL_THERE[@]} GTK libraries survived removal:" >&2
    printf '    %s\n' "${STILL_THERE[@]#$ROOT/}" >&2
    echo "Refusing to launch: the bundle would still shadow the host's GTK." >&2
    exit 1
fi

# Loadable modules built against the bundled libraries. Left in place, the
# host's gdk-pixbuf/GTK would try to load ubuntu-22.04 modules and fail.
echo
echo "Removing bundled loadable modules:"
for dir in "$ROOT/usr/lib/gdk-pixbuf-2.0" "$ROOT/usr/lib/gtk-3.0" \
           "$ROOT/usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0" \
           "$ROOT/usr/lib/x86_64-linux-gnu/gtk-3.0"; do
    if [ -d "$dir" ]; then
        echo "    ${dir#$ROOT/}"
        rm -rf "$dir"
    fi
done

# linuxdeploy's GTK hook points GTK at those bundled modules. With the modules
# gone the variables have to go too, or the host's GTK follows them to paths
# that no longer exist. These are the same variables `host_env.rs` already
# clears when VoxCtrl runs a program belonging to the desktop.
HOOK="$ROOT/apprun-hooks/linuxdeploy-plugin-gtk.sh"
if [ -f "$HOOK" ]; then
    echo
    echo "Neutralising the bundled-GTK exports in $(basename "$HOOK")"
    sed -i -E 's/^([[:space:]]*export[[:space:]]+(GTK_PATH|GTK_IM_MODULE_FILE|GTK_EXE_PREFIX|GTK_DATA_PREFIX|GDK_PIXBUF_MODULE_FILE|GDK_PIXBUF_MODULEDIR)=)/# \1/' "$HOOK"
fi

if [ "$KEEP_ONLY" = true ]; then
    echo
    echo "Prepared (not run). Launch it yourself with:"
    echo "    $ROOT/AppRun"
    echo "Remove it afterwards with:  rm -rf $WORK"
    exit 0
fi

echo
echo "── Launching. Trigger a dictation and watch the overlay CLOSE. ──"
echo "   Clean close  → the bundled GTK was the cause; the fix is to stop"
echo "                  bundling GTK in both build scripts."
echo "   Still smears → GTK is not it, and this costs us nothing further."
echo "   (Ctrl-C here when you're done.)"
echo
"$ROOT/AppRun"
