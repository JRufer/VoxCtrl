#!/usr/bin/env bash
# Fail the AppImage build if a library only WebKitGTK needs is still bundled
# on the normal library path.
#
# The bundled libwebkit2gtk / libjavascriptcoregtk live in usr/lib/fallback
# and are host-first (see scripts/appimage-hooks/host-first-fallback.sh): on
# any desktop with its own WebKitGTK, the host's copy is loaded. That host
# copy then resolves its own dependencies against LD_LIBRARY_PATH, where the
# AppImage's usr/lib comes first. Any WebKit dependency bundled there from the
# older build host shadows the host's newer copy, and the host's WebKitGTK
# aborts at launch, e.g. on Arch (v0.9.20):
#
#   voxctrl: symbol lookup error: /usr/lib/libwebkit2gtk-4.1.so.0:
#     undefined symbol: manette_device_get_mapping
#
# So every direct dependency of the bundled WebKitGTK that nothing else in the
# bundle needs must be host-first too (moved to usr/lib/fallback). This check
# finds any that are not, so a new WebKitGTK dependency cannot quietly
# reintroduce the crash.
#
# Libraries WebKit shares with the app itself (GTK, cairo, pango, libsoup's
# users, ...) are not flagged: the app needs them bundled regardless, and they
# are covered by the general rules in docs/appimage_build_process.md.
#
# Usage: check-appimage-webkit-deps.sh <AppDir/squashfs-root>

set -euo pipefail

root="${1:?usage: check-appimage-webkit-deps.sh <AppDir>}"
lib="$root/usr/lib"
fallback="$lib/fallback"

# Deliberately left bundled on the normal path:
#   libxml2 / libicu* — their sonames differ across distributions (libxml2 is
#   .so.16 on Arch, ICU's major version is in its soname), so a host WebKitGTK
#   links a different soname and the bundled copy cannot shadow it.
allow_re='^(libxml2\.so\.|libicu(data|i18n|uc)\.so\.)'

webkit_libs=()
for f in "$fallback"/libwebkit2gtk-4.*.so* "$fallback"/libjavascriptcoregtk-4.*.so*; do
    [ -f "$f" ] && [ ! -L "$f" ] && webkit_libs+=("$f")
done
if [ "${#webkit_libs[@]}" -eq 0 ]; then
    echo "check-appimage-webkit-deps: no bundled WebKitGTK in $fallback, nothing to check"
    exit 0
fi

needed() { readelf -d "$1" 2>/dev/null | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'; }

# Every ELF that stays on the normal path, other than WebKit's own helper
# processes and MiniBrowser: those are version-locked to the bundled WebKitGTK
# and only ever run when it is the one in use.
others=()
while IFS= read -r -d '' f; do
    case "$f" in
        "$fallback"/*|*/webkit2gtk-4.*/*|*/MiniBrowser) continue ;;
    esac
    head -c 4 "$f" 2>/dev/null | grep -q 'ELF' && others+=("$f")
done < <(find "$root" -type f \( -name '*.so*' -o -perm -u+x \) -print0)

other_needs="$(for f in "${others[@]}"; do needed "$f"; done | sort -u)"

bad=()
for dep in $(for f in "${webkit_libs[@]}"; do needed "$f"; done | sort -u); do
    [ -e "$lib/$dep" ] || continue                   # not bundled on the normal path
    [[ "$dep" =~ $allow_re ]] && continue
    grep -qxF "$dep" <<<"$other_needs" && continue   # the app needs it too
    bad+=("$dep")
done

if [ "${#bad[@]}" -gt 0 ]; then
    echo "check-appimage-webkit-deps: these WebKitGTK-only libraries are bundled on" >&2
    echo "the normal library path and would shadow a host WebKitGTK's own copies:" >&2
    printf '  %s\n' "${bad[@]}" >&2
    echo "Add them to the usr/lib/fallback list in build_appimage.sh and" >&2
    echo ".github/workflows/release.yml (see docs/appimage_build_process.md, rule 4)." >&2
    exit 1
fi
echo "check-appimage-webkit-deps: OK (no WebKitGTK-only libraries shadow the host)"
