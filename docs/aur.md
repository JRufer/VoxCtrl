# AUR packages: how they're built and published

For installing from the AUR, see [Installation → Arch Linux](installation.md#arch-linux-aur).
This page is for maintaining the packages.

## The three packages

| Package | Source | Updated |
|---|---|---|
| `voxctrl-bin` | The release's `VoxCtrl-linux-x86_64-vulkan-webgpu.deb`, repackaged | Automatically, when a release is published |
| `voxctrl` | The release tag's source tarball, built on the user's machine | Automatically, when a release is published |
| `voxctrl-git` | `master`, built on the user's machine | By hand, only when its build steps change |

All three `provide` `voxctrl` and conflict with each other. They install the
same files:

| Path | What |
|---|---|
| `/usr/bin/voxctrl` | The app |
| `/usr/bin/voxctrl-llm-sidecar` | S1-mini sidecar; found next to the app by `find_llm_sidecar_binary()` |
| `/usr/lib/voxctrl/libwebgpu_dawn.so` | Dawn, private to VoxCtrl; on the binary's RUNPATH (`src-tauri/build.rs`) |
| `/usr/share/applications/ai.voxctrl.app.desktop` | Launcher, named after the portal app id |
| `/usr/share/icons/hicolor/{32x32,128x128,256x256}/apps/voxctrl.png` | Icons |
| `/usr/share/licenses/<pkgname>/LICENSE` | MIT licence |

The `.deb`'s own `VoxCtrl.desktop` is deliberately not installed: the desktop
portal can't match that name to `ai.voxctrl.app`, so KDE's shortcut settings
would show a bare identifier.

## Where things live

| File | Role |
|---|---|
| `packaging/aur/<pkg>/PKGBUILD` | Source of truth. Edit here, never on the AUR |
| `packaging/aur/update-pkgbuilds.sh` | Sets `pkgver`/`pkgrel`, fills real checksums, writes `.SRCINFO` (needs Arch) |
| `packaging/aur/smoke-test.sh` | Builds `voxctrl-bin` from a given `.deb`, installs it, checks it |
| `.github/workflows/aur.yml` | On PRs touching `packaging/aur`: smoke test against the latest release. Weekly: full source build from `master` |
| `.github/workflows/release.yml`, job `aur-smoke` | Smoke test against each new build's `.deb`, before the draft is published |
| `.github/workflows/aur-publish.yml` | Pushes `voxctrl-bin` and `voxctrl` to the AUR when a release is published |

The repo's PKGBUILDs carry `SKIP` checksums; real ones are computed at publish
time from the actual downloads, and the `.deb`'s is cross-checked against the
release's `SHA256SUMS`. `./scripts/bump_version.sh` keeps `pkgver` in step with
the app version.

## What the app does differently on a package install

`voxctrl_update::install::is_package_managed()` is true when the binary lives
under `/usr/`, `/opt/` or `/nix/store/` (and isn't an AppImage). Then:

- The updater doesn't self-update or check automatically.
- If `/usr/share/applications/ai.voxctrl.app.desktop` exists, VoxCtrl writes no
  launcher into `~/.local/share/applications`, and removes one there that points
  at itself or at an AppImage that no longer exists
  (`installer::ensure_desktop_integration`).
- The setup window offers no `pkexec` install, only the one command that adds
  the missing typing helper.
- Bug reports say `package (pacman)` or `package (dpkg)`.

## Releasing

Nothing extra: bump, write the notes, merge, publish the draft as usual. Then:

1. Check that the release run's **AUR package smoke test** job is green before
   publishing the draft. Red means AUR users would get a broken package.
2. Publishing the draft starts **Publish to AUR**. Within a few minutes
   `paru -Syu` offers the new version.

Never push changes at the version of an already-published release.
`release.yml` would rebuild its assets, their hashes would stop matching the
AUR's, and every `paru`/`yay` install would fail with a checksum error.

To fix only the packaging for a release that's already out: change the
PKGBUILD on `master`, then run **Actions → Publish to AUR → Run workflow**
with the tag and `pkgrel` 2 (3, …). Tick *dry run* first to see the result
without pushing.

`voxctrl-git` isn't touched by the workflow. When its build steps change, push
it by hand (below), with `.SRCINFO` from `makepkg --printsrcinfo`.

## One-time setup

Done once by the maintainer, on their own machine:

1. Create an account at <https://aur.archlinux.org/register>.
2. Make a key used for nothing else:
   `ssh-keygen -t ed25519 -f ~/.ssh/aur_voxctrl -C voxctrl-aur`.
   Paste `~/.ssh/aur_voxctrl.pub` into **My Account → SSH Public Key**.
3. In `~/.ssh/config`:
   ```
   Host aur.archlinux.org
     IdentityFile ~/.ssh/aur_voxctrl
     User aur
   ```
4. Create each package by pushing its first commit. On Arch, from a clone of
   this repo at the release tag:
   ```bash
   packaging/aur/update-pkgbuilds.sh 0.8.8
   for pkg in voxctrl-bin voxctrl voxctrl-git; do
     git clone ssh://aur@aur.archlinux.org/$pkg.git /tmp/aur-$pkg
     cp packaging/aur/$pkg/PKGBUILD /tmp/aur-$pkg/
     (cd /tmp/aur-$pkg && makepkg --printsrcinfo > .SRCINFO \
        && git add PKGBUILD .SRCINFO && git commit -m "Initial import" && git push)
   done
   git checkout -- packaging/aur   # keep the repo copies at SKIP
   ```
   The AUR creates a package on its first push.
5. On GitHub, **Settings → Secrets and variables → Actions**: secret
   `AUR_SSH_PRIVATE_KEY` (contents of `~/.ssh/aur_voxctrl`), variables
   `AUR_USERNAME` and `AUR_EMAIL` (the AUR commit author).

## Known limits

- The source packages need network access during `build()`: the `ort` crate
  downloads its prebuilt ONNX Runtime and Dawn there, as it does in the release
  workflow. `makepkg` and AUR helpers allow that; a network-isolated chroot
  doesn't. Building against Arch's `onnxruntime` package instead
  (`ORT_LIB_LOCATION`, `ORT_PREFER_DYNAMIC_LINK=1`) is possible only if its
  version matches what `ort` expects; not done yet.
- `voxctrl-bin` runs a binary linked on Ubuntu 24.04. Arch's libraries are
  always newer, which is the safe direction; a soname change (say
  `libwebkit2gtk-4.1.so.0`) would break it, and the weekly job and `aur-smoke`
  are there to notice.
