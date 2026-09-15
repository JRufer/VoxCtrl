use std::path::PathBuf;

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

/// Make sure the documented `Custom/` example exists, without ever touching
/// it once it does: writes `README.md` unconditionally (it's reference
/// documentation, not something meant to hold a user's own content), and
/// writes `Custom/index.html` + `style.css` from the template files under
/// `src-tauri/assets/custom-overlay-template/` **only when the `Custom/`
/// folder does not exist at all**.
///
/// This is a deliberate, simple existence check, not an attempt to detect
/// whether the files have been edited: the moment `Custom/` exists — freshly
/// seeded or hand-edited, doesn't matter which — this leaves it alone for
/// good, on every future launch, until the user deletes the folder
/// themselves. Deleting it is how you ask for the default example back.
/// Nothing outside `README.md` and `Custom/` is ever touched, so any
/// *other* style folder the user has created is always left alone too.
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
    if example_dir.exists() {
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&example_dir) {
        tracing::warn!("Could not create {}: {e}", example_dir.display());
        return;
    }
    let html_path = example_dir.join("index.html");
    if let Err(e) = std::fs::write(&html_path, EXAMPLE_INDEX_HTML) {
        tracing::warn!("Could not write {}: {e}", html_path.display());
    }
    let css_path = example_dir.join("style.css");
    if let Err(e) = std::fs::write(&css_path, EXAMPLE_STYLE_CSS) {
        tracing::warn!("Could not write {}: {e}", css_path.display());
    }

    tracing::info!("Created the example custom overlay at {}", example_dir.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_readme_and_custom_example_into_an_empty_directory() {
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
    fn never_touches_custom_once_it_exists_edited_or_not() {
        let _guard = crate::test_utils::get_env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        let dir = overlays_dir();
        let example_dir = dir.join("Custom");
        std::fs::create_dir_all(&example_dir).unwrap();
        std::fs::write(example_dir.join("index.html"), "<div>my own edit</div>").unwrap();
        std::fs::write(example_dir.join("style.css"), "/* my own edit */").unwrap();

        // Across several launches, including ones where the shipped
        // template has moved on (simulated by the fact that
        // EXAMPLE_INDEX_HTML never matches "my own edit").
        refresh_bundled_example();
        refresh_bundled_example();

        assert_eq!(
            std::fs::read_to_string(example_dir.join("index.html")).unwrap(),
            "<div>my own edit</div>"
        );
        assert_eq!(
            std::fs::read_to_string(example_dir.join("style.css")).unwrap(),
            "/* my own edit */"
        );

        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn recreates_the_default_example_once_the_folder_is_deleted() {
        let _guard = crate::test_utils::get_env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        let dir = overlays_dir();
        let example_dir = dir.join("Custom");
        std::fs::create_dir_all(&example_dir).unwrap();
        std::fs::write(example_dir.join("index.html"), "<div>my own edit</div>").unwrap();

        std::fs::remove_dir_all(&example_dir).unwrap();
        refresh_bundled_example();

        let html = std::fs::read_to_string(example_dir.join("index.html")).unwrap();
        assert!(html.contains("CUSTOM OVERLAY"));
        assert!(!html.contains("my own edit"));

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
