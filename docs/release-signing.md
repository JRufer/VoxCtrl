# Release signing

Every file a VoxCtrl release publishes is signed, and the built-in updater
refuses to install a download whose signature does not check out.

## Why a checksum was not enough

The updater always compared each download with the SHA-256 digest GitHub
reports for it. That catches a corrupted or truncated transfer. It cannot catch
a malicious one: anyone able to replace a release asset — through a leaked
token or a compromised account — replaces its digest along with it. A digest
proves the file is intact, not who published it.

A signature proves who published it. The secret signing key exists only as a
GitHub Actions secret, used by the release workflow's publish job. Replacing a
release asset without it produces a file no installed VoxCtrl will accept.

## How it works

- **Signing.** The `publish` job in `.github/workflows/release.yml` signs every
  release file with [minisign](https://jedisct1.github.io/minisign/) and
  uploads the signature beside it as `<file>.minisig`. Before anything is
  uploaded, it checks each signature against the public key the builds carry.
  That way a mismatched key pair fails the release instead of shipping updates
  no one can install.
- **What is signed.** The signature covers the file and a *trusted comment*,
  `voxctrl-update file:<file name> tag:<release tag>`. The updater requires
  that exact comment, so a validly signed file cannot be replayed under
  another name or another release. That rules out re-uploading an old,
  vulnerable build as the newest one.
- **Verifying.** Release builds have the public key baked in at build time
  (`crates/voxctrl-update/build.rs`). After the digest check, the updater
  downloads the `.minisig`, verifies the file against it, and only then puts
  the file in place (`crates/voxctrl-update/src/signature.rs`). A release with
  no signature for this platform is shown as available but not installable, so
  the user never downloads 100 MB for nothing.
- **Development builds** carry no key and keep the digest check only. The
  release workflow refuses to build a release without the key, so a published
  build can never quietly lack it.

The first signed version still has to be installed by the unsigned updater of
the version before it. Every update after that is verified.

## One-time setup

1. Generate a key pair without a password; the workflow cannot type one:

   ```sh
   minisign -G -W -p voxctrl-update.pub -s voxctrl-update.key
   ```

2. In the repository's **Settings → Secrets and variables → Actions**:
   - add the **variable** `VOXCTRL_UPDATE_PUBKEY`, set to the second line of
     `voxctrl-update.pub`, the one starting `RW`;
   - add the **secret** `VOXCTRL_UPDATE_SIGNING_KEY`, set to the whole contents
     of `voxctrl-update.key`.

3. Keep `voxctrl-update.key` somewhere safe and offline, such as a password
   manager, and delete the local copy. Anyone holding it can publish updates
   that installed copies of VoxCtrl will accept.

## Rotating or losing the key

Installed copies trust only the key they were built with. To change keys, ship
at least one release signed with the **old** key that carries the **new**
public key, then switch the secret. A copy that skipped that release will
refuse later updates, and its user has to download the new version by hand
from the releases page.

If the secret key leaks, rotate the same way, as soon as possible.

## Checking a download by hand

```sh
minisign -V -P <public key> -m VoxCtrl-linux-x86_64-vulkan.AppImage
```

This prints the trusted comment, which should name the file and the release
tag you downloaded.
