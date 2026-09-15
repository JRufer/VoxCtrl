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

/// Seed the overlays directory with a documented example the first time it
/// doesn't exist yet: a `Custom/` style (a copy of the built-in Voice Card
/// style with one line changed, so the diff is obvious) plus a README
/// explaining the folder format, template placeholders, and the events a
/// custom overlay can listen for.
///
/// Only acts when the directory itself doesn't exist — if it's already
/// there (including a bare empty folder), the user has already set this up
/// or deliberately cleared it, and this leaves it alone rather than
/// resurrecting `Custom/` every launch.
pub fn seed_example_if_missing() {
    let dir = overlays_dir();
    if dir.exists() {
        return;
    }

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

    tracing::info!("Seeded example custom overlay at {}", example_dir.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_readme_and_custom_example_into_an_empty_directory() {
        let _guard = crate::test_utils::get_env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        // dirs::data_local_dir() resolves via XDG_DATA_HOME on Linux; this
        // keeps the test from touching the real user's home directory.
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        seed_example_if_missing();

        let dir = overlays_dir();
        assert!(dir.join("README.md").exists());
        assert!(dir.join("Custom").join("index.html").exists());
        assert!(dir.join("Custom").join("style.css").exists());
        let html = std::fs::read_to_string(dir.join("Custom").join("index.html")).unwrap();
        assert!(html.contains("USER CUSTOM"));

        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn does_not_touch_an_already_existing_directory() {
        let _guard = crate::test_utils::get_env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        let dir = overlays_dir();
        std::fs::create_dir_all(&dir).unwrap();
        // Simulates a user who deleted the seeded example: the directory
        // exists, but Custom/ does not, and it should stay that way.
        seed_example_if_missing();

        assert!(!dir.join("Custom").exists());
        assert!(!dir.join("README.md").exists());

        std::env::remove_var("XDG_DATA_HOME");
    }
}
