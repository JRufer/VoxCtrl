use std::path::PathBuf;

/// Where user-authored overlay styles live: each subfolder with an
/// `index.html` + `style.css` becomes a selectable "Overlay style" in
/// Settings → Visual & Feedback (see `commands::get_custom_overlays`).
pub fn overlays_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("overlays")
}

/// Keep the documented example in sync with what this build ships: writes
/// `README.md` and the `Custom/` style (a copy of the built-in Voice Card
/// style with one line changed, so the diff is obvious) from the template
/// files under `src-tauri/assets/custom-overlay-template/`, every launch.
///
/// This unconditionally overwrites those two specific paths — nothing else
/// in the overlays directory, and any *other* folder the user has created,
/// is ever touched. `Custom/` is documented (in the README this writes, and
/// in `Custom/index.html`'s own comments) as an app-maintained reference
/// that gets reset on every launch, precisely so that fixes and template
/// improvements in a new build actually reach it: earlier versions of this
/// function only wrote once, "if the folder looked unseeded," which meant
/// an already-seeded `Custom/` from a previous run silently never picked up
/// a newer template. Anyone who wants to keep their own edits is told to
/// duplicate `Custom/` under a new name rather than edit it in place.
pub fn refresh_bundled_example() {
    let dir = overlays_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("Could not create custom overlays directory {}: {e}", dir.display());
        return;
    }

    let readme = dir.join("README.md");
    if let Err(e) = std::fs::write(&readme, include_str!("../assets/custom-overlay-template/overlays-readme.md")) {
        tracing::warn!("Could not write {}: {e}", readme.display());
    }

    let example_dir = dir.join("Custom");
    if let Err(e) = std::fs::create_dir_all(&example_dir) {
        tracing::warn!("Could not create {}: {e}", example_dir.display());
        return;
    }
    let html_path = example_dir.join("index.html");
    if let Err(e) = std::fs::write(&html_path, include_str!("../assets/custom-overlay-template/index.html")) {
        tracing::warn!("Could not write {}: {e}", html_path.display());
    }
    let css_path = example_dir.join("style.css");
    if let Err(e) = std::fs::write(&css_path, include_str!("../assets/custom-overlay-template/style.css")) {
        tracing::warn!("Could not write {}: {e}", css_path.display());
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
    fn overwrites_a_stale_custom_example_left_by_an_older_build() {
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
