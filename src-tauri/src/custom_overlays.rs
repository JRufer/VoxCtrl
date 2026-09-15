use std::path::{Path, PathBuf};

const EXAMPLE_INDEX_HTML: &str = include_str!("../assets/custom-overlay-template/index.html");
const EXAMPLE_STYLE_CSS: &str = include_str!("../assets/custom-overlay-template/style.css");
const EXAMPLE_README: &str = include_str!("../assets/custom-overlay-template/overlays-readme.md");

/// Where user-authored overlay styles live: each subfolder with an
/// `index.html` + `style.css` becomes a selectable "Overlay style" in
/// Settings → Visual & Feedback (see `commands::get_custom_overlays`).
pub fn overlays_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("overlays")
}

/// Records the exact `Custom/index.html` + `style.css` content this function
/// last wrote, at the overlays root rather than inside `Custom/` itself so
/// it isn't dragged along when someone duplicates that folder to start their
/// own style. Stores full contents rather than a hash: both files are a few
/// KB, and a direct string comparison sidesteps needing a hash function
/// whose stability across Rust/std versions isn't guaranteed.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct ExampleMarker {
    index_html: String,
    style_css: String,
}

fn marker_path(dir: &Path) -> PathBuf {
    dir.join(".voxctrl-custom-example.json")
}

/// Keep the documented example in sync with what this build ships, without
/// clobbering anyone who has started editing it: writes `README.md`
/// unconditionally, and writes `Custom/index.html` + `style.css` from the
/// template files under `src-tauri/assets/custom-overlay-template/` only
/// when they still match what this function itself wrote last time (or
/// don't exist yet, or predate this version tracking entirely).
///
/// `Custom/` is documented (in the README this writes, and in
/// `Custom/index.html`'s own comments) as an app-maintained reference that
/// stays in sync with the installed build — but "stays in sync" has to mean
/// "picks up template fixes the user hasn't touched," not "silently
/// overwrites whatever someone is in the middle of editing there," which an
/// earlier, unconditional version of this function did. Anyone who wants to
/// experiment freely without needing to think about any of this is still
/// told to duplicate `Custom/` under a new name — a differently-named
/// folder is never touched by this function, edited or not.
pub fn refresh_bundled_example() {
    let dir = overlays_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("Could not create custom overlays directory {}: {e}", dir.display());
        return;
    }

    let readme = dir.join("README.md");
    if let Err(e) = std::fs::write(&readme, EXAMPLE_README) {
        tracing::warn!("Could not write {}: {e}", readme.display());
    }

    let example_dir = dir.join("Custom");
    if let Err(e) = std::fs::create_dir_all(&example_dir) {
        tracing::warn!("Could not create {}: {e}", example_dir.display());
        return;
    }

    let html_path = example_dir.join("index.html");
    let css_path = example_dir.join("style.css");
    let marker_path = marker_path(&dir);

    let on_disk_html = std::fs::read_to_string(&html_path).ok();
    let on_disk_css = std::fs::read_to_string(&css_path).ok();
    let marker: Option<ExampleMarker> = std::fs::read_to_string(&marker_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    // Safe to (re)write when: there's a marker and the files on disk still
    // match what it recorded (nothing has touched them since); or there's
    // nothing on disk yet at all (first run); or there's content but no
    // marker (an install from before this tracking existed — overwrite this
    // one more time so it stops being permanently stale, same as a fresh
    // seed, and start tracking from here on).
    let safe_to_write = match (&marker, &on_disk_html, &on_disk_css) {
        (Some(m), Some(h), Some(c)) => &m.index_html == h && &m.style_css == c,
        (_, None, _) | (_, _, None) => true,
        (None, Some(_), Some(_)) => true,
    };

    if !safe_to_write {
        tracing::info!(
            "Custom/ has been edited since it was last written; leaving it alone ({})",
            example_dir.display()
        );
        return;
    }

    if let Err(e) = std::fs::write(&html_path, EXAMPLE_INDEX_HTML) {
        tracing::warn!("Could not write {}: {e}", html_path.display());
    }
    if let Err(e) = std::fs::write(&css_path, EXAMPLE_STYLE_CSS) {
        tracing::warn!("Could not write {}: {e}", css_path.display());
    }
    let marker = ExampleMarker {
        index_html: EXAMPLE_INDEX_HTML.to_string(),
        style_css: EXAMPLE_STYLE_CSS.to_string(),
    };
    if let Ok(json) = serde_json::to_string(&marker) {
        let _ = std::fs::write(&marker_path, json);
    }

    tracing::info!("Refreshed the example custom overlay at {}", example_dir.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_readme_and_custom_example_into_an_empty_directory() {
        let _guard = crate::test_utils::get_env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        // dirs::data_local_dir() resolves via XDG_DATA_HOME on Linux; this
        // keeps the test from touching the real user's home directory.
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        refresh_bundled_example();

        let dir = overlays_dir();
        assert!(dir.join("README.md").exists());
        assert!(dir.join("Custom").join("index.html").exists());
        assert!(dir.join("Custom").join("style.css").exists());
        let html = std::fs::read_to_string(dir.join("Custom").join("index.html")).unwrap();
        assert!(html.contains("CUSTOM OVERLAY"));

        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn upgrades_an_untouched_example_across_two_launches() {
        let _guard = crate::test_utils::get_env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        refresh_bundled_example();
        // A second launch of the same build should be a no-op that leaves
        // everything matching — this mainly checks it doesn't error or
        // mistake its own just-written marker for a user edit.
        refresh_bundled_example();

        let dir = overlays_dir();
        let html = std::fs::read_to_string(dir.join("Custom").join("index.html")).unwrap();
        assert!(html.contains("CUSTOM OVERLAY"));

        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn migrates_a_legacy_example_with_no_marker_exactly_once() {
        let _guard = crate::test_utils::get_env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        let dir = overlays_dir();
        let example_dir = dir.join("Custom");
        std::fs::create_dir_all(&example_dir).unwrap();
        std::fs::write(example_dir.join("index.html"), "<div>a stale, older example</div>").unwrap();
        std::fs::write(example_dir.join("style.css"), "/* stale */").unwrap();

        refresh_bundled_example();

        let html = std::fs::read_to_string(example_dir.join("index.html")).unwrap();
        assert!(html.contains("CUSTOM OVERLAY"));
        assert!(!html.contains("a stale, older example"));

        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn never_overwrites_an_edit_made_since_the_last_write() {
        let _guard = crate::test_utils::get_env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        // First launch: seeds normally, and records the marker.
        refresh_bundled_example();

        // The user edits it by hand, exactly like the reported scenario.
        let dir = overlays_dir();
        let example_dir = dir.join("Custom");
        std::fs::write(example_dir.join("index.html"), "<div>my test edit</div>").unwrap();

        // A later launch must not clobber that edit.
        refresh_bundled_example();

        let html = std::fs::read_to_string(example_dir.join("index.html")).unwrap();
        assert_eq!(html, "<div>my test edit</div>");

        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn leaves_the_users_own_overlay_folders_alone() {
        let _guard = crate::test_utils::get_env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        let dir = overlays_dir();
        let mine = dir.join("MyOwnStyle");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::write(mine.join("index.html"), "<div>mine</div>").unwrap();

        refresh_bundled_example();

        assert_eq!(std::fs::read_to_string(mine.join("index.html")).unwrap(), "<div>mine</div>");

        std::env::remove_var("XDG_DATA_HOME");
    }
}
