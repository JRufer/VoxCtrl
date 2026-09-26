//! Proving a downloaded update was published by VoxCtrl's release workflow.
//!
//! The SHA-256 check in [`crate::apply::verify_digest`] compares the download
//! with the digest GitHub reports *for that same upload*. It catches a
//! corrupted or truncated transfer, but anyone able to replace a release asset
//! — a leaked token, a compromised account — replaces its digest along with
//! it. It proves integrity, not authorship.
//!
//! This module adds authorship. The release workflow signs every asset with a
//! [minisign] key whose secret half exists only as a repository secret, and
//! uploads the signature beside it as `<asset>.minisig`. Release builds carry
//! the public key (baked in at build time from the `VOXCTRL_UPDATE_PUBKEY`
//! repository variable — see `build.rs`), and refuse to install anything whose
//! signature does not verify against it.
//!
//! The signature's *trusted comment* — which the signature covers — names the
//! asset and the release tag it was published under. Checking both stops a
//! validly signed file from being replayed where it does not belong: an old,
//! vulnerable release re-uploaded under a new tag, or one platform's build
//! offered to another.
//!
//! A development build has no key baked in and keeps the digest check only,
//! as every build did before signing existed.
//!
//! [minisign]: https://jedisct1.github.io/minisign/

use std::io::Read;
use std::path::Path;

use minisign_verify::{PublicKey, Signature};

use crate::release::{ReleaseAsset, Result, UpdateError};

/// A signature file is a few hundred bytes; anything much larger is not one,
/// and is not worth downloading to find out.
const MAX_SIGNATURE_BYTES: usize = 4096;

/// The public key release signatures are checked against, or `None` in a
/// build that was not given one (a local or CI development build).
pub fn public_key() -> Option<&'static str> {
    Some(env!("VOXCTRL_UPDATE_PUBKEY_BAKED").trim()).filter(|key| !key.is_empty())
}

/// Whether this build refuses updates that are not signed.
pub fn required() -> bool {
    public_key().is_some()
}

/// The trusted comment the release workflow signs `asset_name` with when it
/// publishes it under `tag`. Must match `.github/workflows/release.yml`.
pub fn expected_trusted_comment(asset_name: &str, tag: &str) -> String {
    format!("voxctrl-update file:{asset_name} tag:{tag}")
}

/// The signature published alongside `asset`, if the release has one.
pub fn find<'a>(asset: &ReleaseAsset, assets: &'a [ReleaseAsset]) -> Option<&'a ReleaseAsset> {
    let name = format!("{}.minisig", asset.name);
    assets.iter().find(|a| a.name == name)
}

/// Check that the file at `path` carries a valid signature, `signature_text`,
/// from `public_key` (the base64 key line), made for `asset_name` in `tag`.
///
/// The file is read in chunks, never whole: the AppImage is around 100 MB.
pub fn verify_file(
    path: &Path,
    signature_text: &str,
    public_key: &str,
    asset_name: &str,
    tag: &str,
) -> Result<()> {
    let fail = |why: String| {
        UpdateError::Other(format!(
            "the download's signature did not verify ({why}), so it was not installed. \
             Download the release by hand only if you trust where it came from."
        ))
    };

    let key = PublicKey::from_base64(public_key)
        .map_err(|e| fail(format!("this build's update key is unusable: {e}")))?;
    let signature =
        Signature::decode(signature_text).map_err(|e| fail(format!("malformed signature: {e}")))?;

    let mut verifier = key
        .verify_stream(&signature)
        .map_err(|e| fail(format!("signed with a different key: {e}")))?;
    let mut file = std::fs::File::open(path)
        .map_err(|e| UpdateError::Other(format!("could not read {}: {e}", path.display())))?;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| UpdateError::Other(format!("could not read {}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        verifier.update(&buf[..n]);
    }
    verifier
        .finalize()
        .map_err(|_| fail("the file does not match its signature".to_string()))?;

    // Only meaningful once the signature is known good: it covers this text.
    let expected = expected_trusted_comment(asset_name, tag);
    if signature.trusted_comment() != expected {
        return Err(fail(format!(
            "it was signed as \"{}\", not \"{expected}\"",
            signature.trusted_comment()
        )));
    }
    Ok(())
}

/// [`verify_file`] off the async runtime.
pub async fn verify_file_blocking(
    path: &Path,
    signature_text: String,
    public_key: &'static str,
    asset_name: String,
    tag: String,
) -> Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        verify_file(&path, &signature_text, public_key, &asset_name, &tag)
    })
    .await
    .map_err(|e| UpdateError::Other(format!("signature check failed to run: {e}")))?
}

/// Download a signature file.
pub async fn fetch(client: &reqwest::Client, signature: &ReleaseAsset) -> Result<String> {
    let response = client
        .get(&signature.browser_download_url)
        .send()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    if !response.status().is_success() {
        return Err(UpdateError::Response(format!(
            "HTTP {} downloading {}",
            response.status(),
            signature.name
        )));
    }
    let body = response
        .bytes()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    if body.len() > MAX_SIGNATURE_BYTES {
        return Err(UpdateError::Response(format!(
            "{} is {} bytes, too large to be a signature",
            signature.name,
            body.len()
        )));
    }
    String::from_utf8(body.to_vec())
        .map_err(|_| UpdateError::Response(format!("{} is not a text signature", signature.name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures made with minisign 0.11 and a throwaway key generated for these
    // tests alone (its secret half was never kept):
    //   minisign -S -s test.key -m <payload> -t "<trusted comment>"
    const TEST_KEY: &str = "RWRZLGAIXDpu5MG9NzmjqSWG/OfGLOKmvA32pCRWOjlDxSLgz9I6kNI3";
    const OTHER_KEY: &str = "RWSql5P582w9cZKSJlQRZwjB07dDWRjtaBsZo83HBSszzKRlwMl/xFzX";
    const PAYLOAD: &[u8] = b"VoxCtrl test payload\n";
    const ASSET: &str = "VoxCtrl-linux-x86_64-vulkan.AppImage";

    /// Signed as `ASSET` in tag v9.9.9.
    const SIGNATURE: &str = "untrusted comment: signature from minisign secret key
RURZLGAIXDpu5IHGN71TkKIHinQMuHgBSvUyVgQFtw9lcr2rSKMjJ90E1gdKbJj0Nb71i15ZwZ90GdNBp4xdT9HnfNbgXfwTOQA=
trusted comment: voxctrl-update file:VoxCtrl-linux-x86_64-vulkan.AppImage tag:v9.9.9
MnJLEPKx03U5N8PhZ1liMAPEovKBV+8RUfmv8lfaMXeJQmxunSFpYswHq14Oh9Ic3bxA2bGMlqmKwAIXPZ25Dw==
";

    /// The same file, validly signed — but for an older release, v0.1.0.
    const OLD_RELEASE_SIGNATURE: &str = "untrusted comment: signature from minisign secret key
RURZLGAIXDpu5IHGN71TkKIHinQMuHgBSvUyVgQFtw9lcr2rSKMjJ90E1gdKbJj0Nb71i15ZwZ90GdNBp4xdT9HnfNbgXfwTOQA=
trusted comment: voxctrl-update file:VoxCtrl-linux-x86_64-vulkan.AppImage tag:v0.1.0
qh+WaDYh1GaRLL57aT0oQUGzXp+ji/SXtabAbzWnel485BlWWpXPTXVXEeBgPkjZIqgxJtxoz9bNjDZPUrHNCw==
";

    fn file_with(contents: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut f, contents).unwrap();
        f
    }

    fn verify(contents: &[u8], signature: &str, key: &str, asset: &str, tag: &str) -> Result<()> {
        let f = file_with(contents);
        verify_file(f.path(), signature, key, asset, tag)
    }

    #[test]
    fn a_genuine_release_asset_verifies() {
        verify(PAYLOAD, SIGNATURE, TEST_KEY, ASSET, "v9.9.9").expect("genuine signature rejected");
    }

    #[test]
    fn a_tampered_file_is_rejected() {
        let err = verify(b"VoxCtrl test payl0ad\n", SIGNATURE, TEST_KEY, ASSET, "v9.9.9")
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not match its signature"), "{err}");
    }

    /// A publisher with their own key — the attacker's — gets nowhere, however
    /// valid their signature is on its own terms.
    #[test]
    fn a_signature_from_another_key_is_rejected() {
        let err = verify(PAYLOAD, SIGNATURE, OTHER_KEY, ASSET, "v9.9.9").unwrap_err().to_string();
        assert!(err.contains("different key"), "{err}");
    }

    /// A real, signed old release re-uploaded as a new one must not install:
    /// that is how a fixed vulnerability would be brought back.
    #[test]
    fn a_signed_older_release_cannot_be_replayed_under_a_new_tag() {
        let err = verify(PAYLOAD, OLD_RELEASE_SIGNATURE, TEST_KEY, ASSET, "v9.9.9")
            .unwrap_err()
            .to_string();
        assert!(err.contains("tag:v0.1.0"), "{err}");
    }

    #[test]
    fn a_signature_made_for_another_asset_is_rejected() {
        let err = verify(PAYLOAD, SIGNATURE, TEST_KEY, "VoxCtrl-windows-x86_64-webgpu.exe", "v9.9.9")
            .unwrap_err()
            .to_string();
        assert!(err.contains("signed as"), "{err}");
    }

    #[test]
    fn garbage_is_not_a_signature() {
        assert!(verify(PAYLOAD, "not a signature", TEST_KEY, ASSET, "v9.9.9").is_err());
        assert!(verify(PAYLOAD, SIGNATURE, "not a key", ASSET, "v9.9.9").is_err());
    }

    /// Only meaningful in a build given a key (`VOXCTRL_UPDATE_PUBKEY`): the
    /// key `build.rs` extracted from a whole `.pub` file must be usable.
    #[test]
    fn a_baked_in_key_is_a_valid_key() {
        if let Some(key) = public_key() {
            PublicKey::from_base64(key).expect("the baked-in update key does not decode");
        }
    }

    #[test]
    fn the_signature_is_found_by_the_asset_name() {
        let asset = |name: &str| ReleaseAsset {
            name: name.into(),
            browser_download_url: format!("https://example.invalid/{name}"),
            size: 1,
            digest: None,
        };
        let assets = vec![asset(ASSET), asset(&format!("{ASSET}.minisig")), asset("other.minisig")];
        assert_eq!(find(&assets[0], &assets).unwrap().name, format!("{ASSET}.minisig"));
        assert!(find(&asset("VoxCtrl.exe"), &assets).is_none());
    }
}
