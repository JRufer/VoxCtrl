//! Checking GitHub for a newer VoxCtrl, and replacing this one with it.
//!
//! The flow, end to end:
//!
//! 1. [`check`] asks the public GitHub releases API what the latest published
//!    release is, and compares its tag with the running version.
//! 2. If it is newer, the release asset matching *this* installation is
//!    resolved — CPU or Vulkan AppImage, Windows installer — and returned as a
//!    [`PendingUpdate`].
//! 3. [`install`] downloads that asset beside the current one, verifies it
//!    against GitHub's published checksum and — in a release build — against
//!    the release signature (see [`signature`]), and moves it into place.
//! 4. The caller relaunches via [`apply::spawn_relaunch`] and exits.
//!
//! Nothing here decides *when* to check — VoxCtrl never checks on its own.
//! [`check`] only runs when the user presses "Check for updates" in Settings,
//! so this crate never reaches the network unasked.

pub mod apply;
pub mod install;
pub mod release;
pub mod signature;
pub mod version;

use std::path::PathBuf;

pub use apply::Progress;
pub use install::InstallKind;
pub use release::{Release, ReleaseAsset, Result, UpdateError, RELEASES_PAGE_URL};
pub use version::Version;

/// How much of the release notes the update dialog is given. Enough for the
/// headline and the first few points; the full notes are one click away on the
/// release page.
const NOTES_BUDGET: usize = 1200;

/// What a check found, in the shape the UI needs it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateInfo {
    /// The new version, normalised without its leading `v` (`0.4.0`).
    pub version: String,
    /// The release tag as GitHub has it (`v0.4.0`).
    pub tag: String,
    /// The version currently running.
    pub current_version: String,
    /// Release notes, trimmed to something a dialog can show.
    pub notes: String,
    /// The release page, for "see all the details" and for the cases VoxCtrl
    /// cannot update itself.
    pub release_url: String,
    /// File name of the asset that would be installed, when there is one.
    pub asset_name: Option<String>,
    /// Its size in bytes, so the dialog can say how large the download is.
    pub download_size: u64,
    /// Whether "Update and restart" can do anything on this installation.
    pub can_self_update: bool,
    /// Why not, when it cannot.
    pub unsupported_reason: Option<String>,
}

/// A resolved update, ready to install. Holds the pieces [`install`] needs so
/// the release does not have to be fetched a second time.
#[derive(Debug, Clone)]
pub struct PendingUpdate {
    pub info: UpdateInfo,
    pub asset: Option<ReleaseAsset>,
    /// The `<asset>.minisig` published beside `asset`, when there is one.
    pub signature: Option<ReleaseAsset>,
    pub kind: InstallKind,
}

/// The result of a check.
#[derive(Debug, Clone)]
pub enum CheckOutcome {
    /// Nothing newer is published.
    UpToDate { current: String },
    /// A newer release exists.
    Available(Box<PendingUpdate>),
}

impl CheckOutcome {
    pub fn available(&self) -> Option<&PendingUpdate> {
        match self {
            Self::Available(p) => Some(p),
            Self::UpToDate { .. } => None,
        }
    }
}

/// Ask GitHub whether there is anything newer than `current_version`.
///
/// `gpu_build` says whether this binary is the GPU variant of its platform's
/// release artifacts; see [`install::detect`] for why it cannot be worked out
/// from the installation alone on Windows.
pub async fn check(current_version: &str, gpu_build: bool) -> Result<CheckOutcome> {
    let client = release::client()?;
    let latest = release::fetch_latest(&client).await?;
    Ok(evaluate(&latest, current_version, install::detect(gpu_build)))
}

/// The decision half of [`check`], with the network and the machine both passed
/// in. Everything that can be got wrong lives here, and none of it needs either.
pub fn evaluate(latest: &Release, current_version: &str, kind: InstallKind) -> CheckOutcome {
    evaluate_with(latest, current_version, kind, signature::required())
}

/// [`evaluate`], with whether signatures are required passed in rather than
/// taken from this build.
pub fn evaluate_with(
    latest: &Release,
    current_version: &str,
    kind: InstallKind,
    signature_required: bool,
) -> CheckOutcome {
    let up_to_date = CheckOutcome::UpToDate { current: current_version.to_string() };

    // A draft is an unfinished release the workflow has not published yet;
    // `/releases/latest` already excludes them, but a caller pointed at another
    // endpoint must not offer one either.
    if latest.draft {
        return up_to_date;
    }

    if !version::is_newer(&latest.tag_name, current_version) {
        return up_to_date;
    }

    let version = version::Version::parse(&latest.tag_name)
        .map(|v| v.to_string())
        .unwrap_or_else(|| latest.tag_name.clone());

    let asset = install::select_asset(&kind, &latest.assets).cloned();
    let signature = asset
        .as_ref()
        .and_then(|a| signature::find(a, &latest.assets))
        .cloned();

    // Self-updating needs both a supported installation *and* a file to
    // install. A release that shipped without this platform's artifact — a
    // build that failed in the matrix — is still worth telling the user about,
    // but "Update and restart" would have nothing to download.
    let (can_self_update, unsupported_reason) = match (kind.can_self_update(), asset.is_some()) {
        // Said at check time, not after a 100 MB download: this build will not
        // install an unsigned file, so the button must not pretend it can.
        (true, true) if signature_required && signature.is_none() => (
            false,
            Some(format!(
                "Release {} is not signed, so VoxCtrl will not install it automatically. \
                 Download it from the release page only if you trust where it came from.",
                latest.tag_name
            )),
        ),
        (true, true) => (true, None),
        (true, false) => (
            false,
            Some(format!(
                "Release {} does not include a download for this platform yet. \
                 It may still be uploading.",
                latest.tag_name
            )),
        ),
        (false, _) => (false, kind.unsupported_reason().map(str::to_string)),
    };

    let release_url = if latest.html_url.is_empty() {
        release::RELEASES_PAGE_URL.to_string()
    } else {
        latest.html_url.clone()
    };

    CheckOutcome::Available(Box::new(PendingUpdate {
        info: UpdateInfo {
            version,
            tag: latest.tag_name.clone(),
            current_version: current_version.to_string(),
            notes: release::summarize_notes(latest.body.as_deref().unwrap_or_default(), NOTES_BUDGET),
            release_url,
            asset_name: asset.as_ref().map(|a| a.name.clone()),
            download_size: asset.as_ref().map(|a| a.size).unwrap_or(0),
            can_self_update,
            unsupported_reason,
        },
        asset,
        signature,
        kind,
    }))
}

/// Check the downloaded file at `path` against its release signature, when
/// this build requires one. Runs after the digest check, before anything is
/// put in place.
async fn verify_signature(
    pending: &PendingUpdate,
    asset: &ReleaseAsset,
    client: &reqwest::Client,
    path: &std::path::Path,
) -> Result<()> {
    let Some(public_key) = signature::public_key() else {
        tracing::warn!("this build has no update-signing key; checking the digest only");
        return Ok(());
    };
    let sig_asset = pending.signature.as_ref().ok_or_else(|| {
        UpdateError::Other(format!(
            "{} is not signed, so it was not installed.",
            asset.name
        ))
    })?;
    let text = signature::fetch(client, sig_asset).await?;
    signature::verify_file_blocking(path, text, public_key, asset.name.clone(), pending.info.tag.clone())
        .await
}

/// Download and install a pending update, returning the path to launch
/// afterwards.
///
/// On Linux that is the replaced AppImage; on Windows it is the downloaded
/// installer, which does the replacing itself.
pub async fn install(
    pending: &PendingUpdate,
    mut on_progress: impl FnMut(Progress),
) -> Result<PathBuf> {
    let asset = pending.asset.as_ref().ok_or_else(|| {
        UpdateError::Other(
            "This release has no download for your platform, so there is nothing to install."
                .to_string(),
        )
    })?;

    match &pending.kind {
        InstallKind::AppImage { path, .. } => {
            // Checked first: a target we cannot replace turns a 100 MB download
            // into wasted bandwidth and a confusing failure at the very end.
            apply::check_writable(path)?;

            let staged = apply::staging_path(path);
            let client = release::client()?;

            let result = async {
                apply::download(&client, asset, &staged, &mut on_progress).await?;
                apply::verify_digest_blocking(&staged, asset.digest.as_deref()).await?;
                verify_signature(pending, asset, &client, &staged).await?;
                apply::install_appimage(&staged, path)
            }
            .await;

            if result.is_err() {
                // Leaving a 100 MB hidden file next to the app after a failed
                // update is its own small bug report.
                let _ = std::fs::remove_file(&staged);
            }
            result?;

            tracing::info!("updated {} to {}", path.display(), pending.info.version);
            Ok(path.clone())
        }
        InstallKind::WindowsInstaller { .. } => {
            // The installer is a throwaway: it is run once and replaces the
            // real installation itself, so it goes to the temp directory
            // rather than beside anything.
            let dest = std::env::temp_dir().join(&asset.name);
            let client = release::client()?;

            let result = async {
                apply::download(&client, asset, &dest, &mut on_progress).await?;
                apply::verify_digest_blocking(&dest, asset.digest.as_deref()).await?;
                verify_signature(pending, asset, &client, &dest).await
            }
            .await;

            if result.is_err() {
                let _ = std::fs::remove_file(&dest);
            }
            result?;

            Ok(dest)
        }
        InstallKind::ManagedPackage | InstallKind::Unmanaged => Err(UpdateError::Other(
            pending
                .kind
                .unsupported_reason()
                .unwrap_or("This installation cannot update itself.")
                .to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shadows the crate's `evaluate`, so these tests describe the decision
    /// itself and pass the same whether or not this build has a signing key.
    fn evaluate(latest: &Release, current_version: &str, kind: InstallKind) -> CheckOutcome {
        evaluate_with(latest, current_version, kind, false)
    }

    fn release_with(tag: &str, assets: Vec<ReleaseAsset>) -> Release {
        Release {
            tag_name: tag.to_string(),
            name: Some(format!("VoxCtrl {tag}")),
            body: Some("## What's new\n\nThings.".to_string()),
            html_url: format!("https://github.com/JRufer/VoxCtrl/releases/tag/{tag}"),
            draft: false,
            prerelease: false,
            assets,
        }
    }

    fn appimage_asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            browser_download_url: format!("https://example.invalid/{name}"),
            size: 98_000_000,
            digest: Some("sha256:abc".to_string()),
        }
    }

    fn appimage_kind() -> InstallKind {
        InstallKind::AppImage { path: "/home/u/VoxCtrl.AppImage".into(), vulkan: false }
    }

    #[test]
    fn a_newer_release_is_offered_with_the_matching_asset() {
        let rel = release_with(
            "v0.4.0",
            vec![appimage_asset("VoxCtrl_0.4.0_amd64-linux-x86_64.AppImage")],
        );
        let pending = match evaluate(&rel, "0.3.10", appimage_kind()) {
            CheckOutcome::Available(p) => p,
            other => panic!("expected an update, got {other:?}"),
        };
        assert_eq!(pending.info.version, "0.4.0");
        assert_eq!(pending.info.tag, "v0.4.0");
        assert_eq!(pending.info.current_version, "0.3.10");
        assert!(pending.info.can_self_update);
        assert_eq!(
            pending.info.asset_name.as_deref(),
            Some("VoxCtrl_0.4.0_amd64-linux-x86_64.AppImage")
        );
        assert_eq!(pending.info.download_size, 98_000_000);
    }

    #[test]
    fn the_same_version_reports_up_to_date() {
        let rel = release_with("v0.3.10", vec![]);
        assert!(matches!(
            evaluate(&rel, "0.3.10", appimage_kind()),
            CheckOutcome::UpToDate { .. }
        ));
    }

    #[test]
    fn a_draft_release_is_never_offered() {
        // The release workflow publishes drafts first. Offering one would push
        // users onto a build nobody has finished checking.
        let mut rel = release_with("v0.9.0", vec![appimage_asset("VoxCtrl_0.9.0_amd64-linux-x86_64.AppImage")]);
        rel.draft = true;
        assert!(matches!(
            evaluate(&rel, "0.3.10", appimage_kind()),
            CheckOutcome::UpToDate { .. }
        ));
    }

    #[test]
    fn a_release_missing_this_platforms_build_is_reported_but_not_installable() {
        let rel = release_with("v0.4.0", vec![appimage_asset("VoxCtrl_0.4.0_x64-setup-windows-x86_64.exe")]);
        let pending = evaluate(&rel, "0.3.10", appimage_kind());
        let pending = pending.available().expect("still worth telling the user");
        assert!(!pending.info.can_self_update);
        assert!(pending.info.unsupported_reason.as_ref().unwrap().contains("does not include"));
        assert!(pending.asset.is_none());
    }

    #[test]
    fn a_package_install_is_told_to_use_its_package_manager() {
        let rel = release_with(
            "v0.4.0",
            vec![appimage_asset("VoxCtrl_0.4.0_amd64-linux-x86_64.AppImage")],
        );
        let pending = evaluate(&rel, "0.3.10", InstallKind::ManagedPackage);
        let pending = pending.available().unwrap();
        assert!(!pending.info.can_self_update);
        assert!(pending.info.unsupported_reason.as_ref().unwrap().contains("package manager"));
    }

    #[test]
    fn a_release_without_a_page_url_still_links_somewhere() {
        let mut rel = release_with("v0.4.0", vec![]);
        rel.html_url = String::new();
        let pending = evaluate(&rel, "0.3.10", appimage_kind());
        assert_eq!(pending.available().unwrap().info.release_url, RELEASES_PAGE_URL);
    }

    #[tokio::test]
    async fn installing_without_an_asset_explains_itself_instead_of_panicking() {
        let rel = release_with("v0.4.0", vec![]);
        let pending = match evaluate(&rel, "0.3.10", appimage_kind()) {
            CheckOutcome::Available(p) => p,
            other => panic!("expected an update, got {other:?}"),
        };
        let err = install(&pending, |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("nothing to install"), "{err}");
    }

    #[tokio::test]
    async fn a_development_build_refuses_to_overwrite_itself() {
        let rel = release_with("v0.4.0", vec![appimage_asset("VoxCtrl_0.4.0_amd64-linux-x86_64.AppImage")]);
        let pending = PendingUpdate {
            info: evaluate(&rel, "0.3.10", appimage_kind()).available().unwrap().info.clone(),
            asset: Some(appimage_asset("VoxCtrl_0.4.0_amd64-linux-x86_64.AppImage")),
            signature: None,
            kind: InstallKind::Unmanaged,
        };
        let err = install(&pending, |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("build directory"), "{err}");
    }

    const SIGNED_ASSET: &str = "VoxCtrl_0.9.0_amd64-linux-x86_64.AppImage";

    /// A build that requires signatures says so when the update is found —
    /// before anyone downloads 100 MB — rather than failing at the end.
    #[test]
    fn an_unsigned_release_is_not_offered_for_install_when_signatures_are_required() {
        let rel = release_with("v0.9.0", vec![appimage_asset(SIGNED_ASSET)]);
        let outcome = evaluate_with(&rel, "0.3.10", appimage_kind(), true);
        let pending = outcome.available().expect("still worth telling the user about");
        assert!(!pending.info.can_self_update);
        assert!(pending.info.unsupported_reason.as_ref().unwrap().contains("not signed"));
    }

    #[test]
    fn a_signed_release_is_installable_and_carries_its_signature() {
        let rel = release_with(
            "v0.9.0",
            vec![appimage_asset(SIGNED_ASSET), appimage_asset(&format!("{SIGNED_ASSET}.minisig"))],
        );
        let outcome = evaluate_with(&rel, "0.3.10", appimage_kind(), true);
        let pending = outcome.available().unwrap();
        assert!(pending.info.can_self_update, "{:?}", pending.info.unsupported_reason);
        assert_eq!(pending.asset.as_ref().unwrap().name, SIGNED_ASSET, "the signature was picked as the update");
        assert_eq!(pending.signature.as_ref().unwrap().name, format!("{SIGNED_ASSET}.minisig"));
    }

    /// Development builds carry no key and keep today's digest-only behavior.
    #[test]
    fn an_unsigned_release_stays_installable_where_signatures_are_not_required() {
        let rel = release_with("v0.9.0", vec![appimage_asset(SIGNED_ASSET)]);
        assert!(evaluate_with(&rel, "0.3.10", appimage_kind(), false).available().unwrap().info.can_self_update);
    }
}
