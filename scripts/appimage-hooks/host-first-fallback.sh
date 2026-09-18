#! /usr/bin/env bash
# AppRun hook: host-first fallback libraries.
#
# Libraries under usr/lib/fallback are ones the bundled WebKitGTK needs but
# that must NOT shadow a host copy when the host has one: the host's own
# libraries link against them (Arch's libmount links libsystemd, and needs
# LIBSYSTEMD_251, newer than the copy the ubuntu-22.04 build host bundles),
# so a bundled copy on LD_LIBRARY_PATH aborts the host library at startup.
# They can't simply be stripped either: a host may not ship them at all, and
# WebKitGTK would then fail to load — non-systemd distributions have no
# libsystemd/libudev, and libgstgl-1.0.so.0 lives in libgstreamer-gl1.0-0,
# which gstreamer1.0-plugins-base does not pull in, so a desktop with no
# WebKit of its own can be missing it entirely.
#
# So for each fallback library, expose our copy only when the host has no
# library of that soname. The symlinks live in a per-user runtime directory
# because the AppImage mount is read-only.
#
# This file is installed into the AppDir by the AppImage slimming step in
# build_appimage.sh and .github/workflows/release.yml, and sourced by AppRun
# before it hands over to the linuxdeploy AppRun, which prepends the AppDir
# library directories to whatever LD_LIBRARY_PATH we export here.

fallback_src="${APPDIR:-$(dirname "$(realpath "$0")")}/usr/lib/fallback"
if [ -d "$fallback_src" ]; then
    fallback_dir="${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}/voxctrl-fallback-libs-$(id -u)"
    rm -rf "$fallback_dir" 2>/dev/null || true
    if mkdir -p "$fallback_dir" 2>/dev/null; then
        host_libs="$(ldconfig -p 2>/dev/null || /sbin/ldconfig -p 2>/dev/null || true)"
        bundled_webkit_exposed=false
        for lib in "$fallback_src"/*.so*; do
            [ -e "$lib" ] || continue
            name="$(basename "$lib")"
            host_has=false
            if printf '%s\n' "$host_libs" | grep -qF "$name"; then
                host_has=true
            else
                for dir in /usr/lib /usr/lib64 /lib /lib64 \
                           /usr/lib/x86_64-linux-gnu /lib/x86_64-linux-gnu; do
                    if [ -e "$dir/$name" ]; then host_has=true; break; fi
                done
            fi
            if [ "$host_has" = false ]; then
                ln -sf "$lib" "$fallback_dir/$name"
                case "$name" in
                    libwebkit2gtk-*) bundled_webkit_exposed=true ;;
                esac
            fi
        done
        export LD_LIBRARY_PATH="$fallback_dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

        # WebKitGTK is in that fallback set, so the host's copy wins wherever
        # there is one. That is the point: the WebKitGTK bundled from
        # ubuntu-22.04 renders a transparent window's compositing layers
        # without their alpha channel, so an overlay animating as it closes
        # leaves an opaque, blurry copy of its last frame on screen until the
        # window is destroyed. Every newer WebKitGTK tested is fine, which is
        # why a locally built AppImage — bundling the build machine's own,
        # much newer WebKitGTK — never showed it and released builds always
        # did.
        #
        # When the host's WebKitGTK is the one being used, the WEBKIT_*
        # variables AppRun exports must not go on pointing into this bundle:
        # the helper processes and the injected bundle are version-locked to
        # the library they shipped with, and handing the host's WebKit our
        # ubuntu-22.04 copies of them pairs a library with helpers it was
        # never built against. Unset, the host's WebKit uses the helpers
        # compiled into it, which are the matching ones.
        if [ "$bundled_webkit_exposed" = false ]; then
            unset WEBKIT_EXEC_PATH
            unset WEBKIT_INJECTED_BUNDLE_PATH
        else
            # The bundled WebKitGTK's WebProcess aborts (SIGABRT, an assertion
            # in Skia's colrv1_configure_skpaint) when it has to draw a glyph
            # from a COLRv1-only colour emoji font. Fedora 41+ ships Noto
            # Color Emoji only in that format (Noto-COLRv1.ttf) — no bitmap
            # fallback — so any window with emoji in it (the setup wizard,
            # settings) freezes the WebProcess the first time one is drawn.
            # Reported and root-caused in #130.
            #
            # This only matters when the bundled WebKitGTK is the one in use:
            # a host WebKitGTK new enough to handle COLRv1 is unaffected, and
            # this whole branch is already gated on the host having none.
            #
            # A fontconfig override that rejects that one file by name is
            # what the reporter's own workaround did, generalized: still
            # include every other rule the host would normally apply, and
            # only carve out this specific font so glyph fallback lands on
            # whatever monochrome emoji font (or none) is left.
            fontconfig_dir="${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}/voxctrl-fontconfig-$(id -u)"
            if mkdir -p "$fontconfig_dir" 2>/dev/null; then
                fontconfig_file="$fontconfig_dir/fonts.conf"
                cat > "$fontconfig_file" <<'FONTCONFIG_EOF'
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <include ignore_missing="yes">/etc/fonts/fonts.conf</include>
  <selectfont>
    <rejectfont>
      <glob>*/Noto-COLRv1.ttf</glob>
    </rejectfont>
  </selectfont>
</fontconfig>
FONTCONFIG_EOF
                if [ -f "$fontconfig_file" ] && [ -z "${FONTCONFIG_FILE:-}" ]; then
                    export FONTCONFIG_FILE="$fontconfig_file"
                fi
            fi
        fi
    fi
fi
