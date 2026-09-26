//! Finding executables.

use std::path::PathBuf;

/// Find the executable `name` the same way spawning it will.
///
/// This exists to answer one question — will `Command::new(name)` work? — so it
/// has to search what `Command` searches. On Unix that is `$PATH`.
///
/// On Windows it is not. `std`'s resolver appends `.exe` and then tries, in
/// order: the directory of the running executable, the system directory
/// (`System32`), the Windows directory, and only then `PATH`. Searching `PATH`
/// alone reported "not found" for anything living in the first three — every
/// tool in System32 among them — while spawning it worked. An Exec target
/// whose Test button contradicted the target actually running is how that
/// surfaced.
///
/// Note `PATHEXT` is deliberately *not* consulted: `CreateProcessW` appends
/// `.exe` and nothing else when searching, so a `.bat` or `.cmd` found by name
/// alone could not be spawned anyway, and reporting it as reachable would trade
/// one wrong answer for its mirror image.
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    // "If the file name does not contain an extension, .exe is appended."
    let search_name: std::borrow::Cow<str> = if cfg!(target_os = "windows") && !name.contains('.') {
        format!("{name}.exe").into()
    } else {
        name.into()
    };

    let mut dirs: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                dirs.push(parent.to_path_buf());
            }
        }
        // GetSystemDirectoryW / GetWindowsDirectoryW, without taking a Win32
        // dependency for two paths that are derived from one another.
        if let Some(root) = std::env::var_os("SystemRoot") {
            let root = PathBuf::from(root);
            dirs.push(root.join("System32"));
            dirs.push(root);
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }

    dirs.into_iter()
        .map(|dir| dir.join(search_name.as_ref()))
        .find(|p| p.is_file())
}
