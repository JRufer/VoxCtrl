use std::path::PathBuf;
use std::process::Command;

pub fn detect_pkg_manager() -> &'static str {
    #[cfg(test)]
    {
        if let Ok(mock) = std::env::var("VOXCTRL_PKG_MANAGER_MOCK") {
            return match mock.as_str() {
                "pacman" => "pacman",
                "apt" => "apt",
                "dnf" => "dnf",
                "zypper" => "zypper",
                "unknown" => "unknown",
                _ => "apt",
            };
        }
    }

    // Cached: the setup status is polled while the setup window is open, and
    // probing four package managers per poll is pure waste — the answer cannot
    // change while the app is running.
    static DETECTED: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    DETECTED.get_or_init(|| {
        if Command::new("pacman").arg("--version").output().is_ok() {
            "pacman"
        } else if Command::new("apt-get").arg("--version").output().is_ok() {
            "apt"
        } else if Command::new("dnf").arg("--version").output().is_ok() {
            "dnf"
        } else if Command::new("zypper").arg("--version").output().is_ok() {
            "zypper"
        } else {
            "unknown"
        }
    })
}

pub fn get_install_packages_command(pkg_mgr: &str) -> Option<String> {
    match pkg_mgr {
        "pacman" => Some("pacman -S --noconfirm --needed webkit2gtk-4.1 openssl libayatana-appindicator wtype xdotool wl-clipboard xclip portaudio espeak-ng".to_string()),
        "apt" => Some("apt-get update -y && apt-get install -y libwebkit2gtk-4.1-0 libssl3 libayatana-appindicator3-1 wtype xdotool wl-clipboard xclip libportaudio2 espeak-ng".to_string()),
        "dnf" => Some("dnf install -y webkit2gtk4.1 openssl libayatana-appindicator3 wtype xdotool wl-clipboard xclip portaudio espeak-ng".to_string()),
        "zypper" => Some("zypper install -y libwebkit2gtk-4_1-0 libopenssl3 libayatana-appindicator3-1 wtype xdotool wl-clipboard xclip libportaudio2 espeak-ng".to_string()),
        _ => None,
    }
}

/// Builds the privileged setup script.
///
/// This installs host packages only — the keystroke-injection helpers and
/// runtime libraries VoxCtrl needs to type a transcription into the focused
/// window. It grants no access to input devices, and deliberately so.
///
/// VoxCtrl used to write a udev rule here that tagged every `/dev/input/event*`
/// node with `uaccess`, plus `usermod -aG input`. Both made this process able to
/// read the keyboard — and with it every other process running as the user,
/// permanently, for every application on the machine. systemd's own defaults
/// grant `uaccess` on input devices to joysticks and nothing else, precisely to
/// keep unprivileged programs from reading keystrokes. Global shortcuts now go
/// through the desktop portal, which needs none of that, so the rule is gone
/// and is never written again.
pub fn build_privileged_setup_script(pkg_mgr: &str) -> String {
    let mut script = String::from("set -u\n");
    if let Some(cmd) = get_install_packages_command(pkg_mgr) {
        script.push_str(&format!(
            "{{ {cmd}; }} || echo 'VoxCtrl: host package installation failed (mirrors/network?). Install the packages manually later.' >&2\n"
        ));
    }
    script.push_str("exit 0\n");
    script
}

/// The commands a user can run by hand if the graphical setup cannot run
/// (no pkexec, no polkit agent, locked-down machine).
pub fn manual_setup_commands(pkg_mgr: &str) -> String {
    match get_install_packages_command(pkg_mgr) {
        Some(cmd) => format!("sudo sh -c '{cmd}'"),
        None => String::new(),
    }
}

pub fn command_exists(cmd: &str) -> bool {
    #[cfg(test)]
    {
        if let Ok(list) = std::env::var("VOXCTRL_FAKE_COMMANDS") {
            return list.split(',').any(|c| c == cmd);
        }
    }
    voxctrl_config::find_in_path(cmd).is_some()
}

fn run_command_status(runner: &str, args: &[&str]) -> Result<std::process::ExitStatus, String> {
    #[cfg(test)]
    {
        if let Ok(mock) = std::env::var("VOXCTRL_INSTALLER_TEST_MOCK") {
            if mock == "success" {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    return Ok(std::process::ExitStatus::from_raw(0));
                }
                #[cfg(not(unix))]
                {
                    return Ok(Command::new("cmd").args(&["/c", "exit 0"]).status().map_err(|e| e.to_string())?);
                }
            } else if mock == "failure" {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    return Ok(std::process::ExitStatus::from_raw(256)); // exit status 1
                }
                #[cfg(not(unix))]
                {
                    return Ok(Command::new("cmd").args(&["/c", "exit 1"]).status().map_err(|e| e.to_string())?);
                }
            } else if mock == "spawn_error" {
                return Err("Failed to spawn command".to_string());
            }
        }
    }

    Command::new(runner)
        .args(args)
        .status()
        .map_err(|e| format!("Failed to spawn {}: {}", runner, e))
}

/// Desktop entry filename. Must stay in step with the portal application id, so
/// the desktop can name and illustrate the shortcuts VoxCtrl registers.
pub const DESKTOP_FILE_NAME: &str = "ai.voxctrl.app.desktop";

/// What the entry was called before it had to match the application id.
pub const LEGACY_DESKTOP_FILE_NAME: &str = "voxctrl.desktop";

pub fn setup_desktop_integration() -> Result<(), String> {
    let home_dir = dirs::home_dir().ok_or("Could not find home directory")?;
    let launcher_dir = home_dir.join(".local/share/applications");
    let icon_dir = home_dir.join(".local/share/icons/hicolor/128x128/apps");

    std::fs::create_dir_all(&launcher_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&icon_dir).map_err(|e| e.to_string())?;

    // Copy high-res icon
    let icon_path = icon_dir.join("voxctrl.png");
    let icon_bytes = include_bytes!("../icons/128x128.png");
    std::fs::write(&icon_path, icon_bytes).map_err(|e| e.to_string())?;

    // Create desktop launcher
    let appimage_path = std::env::var("APPIMAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_exe().unwrap_or_default());
    
    let abs_path = std::fs::canonicalize(&appimage_path)
        .unwrap_or(appimage_path);

    let desktop_content = format!(
        r#"[Desktop Entry]
Name=VoxCtrl
Comment=Private Global Voice Dictation Gateway
Exec={}
Icon=voxctrl
Terminal=false
Type=Application
Categories=Utility;AudioVideo;
StartupNotify=false
StartupWMClass=ai.voxctrl.app
Keywords=whisper;voice;dictation;wayland;
"#,
        abs_path.to_string_lossy()
    );

    // Named after the application id VoxCtrl declares to the desktop portal
    // (`voxctrl_hotkeys::portal::APP_ID`). That is how a desktop resolves the id
    // it is handed for global shortcuts back to a name and icon — without the
    // match, KDE's shortcut settings list a bare identifier.
    let launcher_path = launcher_dir.join(DESKTOP_FILE_NAME);
    std::fs::write(&launcher_path, desktop_content).map_err(|e| e.to_string())?;

    // An install from before the rename leaves a second entry behind, which
    // shows up as a duplicate VoxCtrl in the application menu.
    let legacy = launcher_dir.join(LEGACY_DESKTOP_FILE_NAME);
    if legacy.exists() {
        let _ = std::fs::remove_file(&legacy);
    }
    
    // Make desktop entry executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(&launcher_path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&launcher_path, perms);
        }
    }

    register_mint_shortcut_if_needed();

    Ok(())
}

fn register_mint_shortcut_if_needed() {
    #[cfg(target_os = "linux")]
    {
        // Only worth doing where nothing better can serve shortcuts: on an X11
        // Mint session VoxCtrl reads the keys itself and every gesture works,
        // and a native shortcut registered alongside that would fire a second
        // time on the same keypress.
        if crate::mint_shortcuts::is_mint_desktop()
            && !crate::mint_shortcuts::is_mint_shortcut_registered()
        {
            let _ = crate::mint_shortcuts::register_mint_shortcut(None);
        }
    }
}

/// Where a distro package installs VoxCtrl's launcher.
///
/// Named after the portal application id, like the per-user one, so the
/// desktop resolves `ai.voxctrl.app` to a name and an icon either way.
pub const SYSTEM_DESKTOP_FILE: &str = "/usr/share/applications/ai.voxctrl.app.desktop";

/// Whether VoxCtrl should write its own launcher into `~/.local`.
///
/// It must not when a package manager installed it and that package ships the
/// launcher: a per-user entry shadows the system one, carries an absolute
/// `Exec=`, and is left behind as a dead menu item after `pacman -R`. A `.deb`
/// from before the launcher was renamed ships only `VoxCtrl.desktop`, which the
/// portal cannot match to `ai.voxctrl.app`, so that install still needs the
/// per-user entry — hence both conditions, not just the first.
pub fn wants_user_launcher(package_managed: bool, system_launcher_exists: bool) -> bool {
    !(package_managed && system_launcher_exists)
}

/// Remove a per-user launcher that a package's system launcher now replaces.
///
/// Only an entry that is safe to lose goes: one whose `Exec=` is this very
/// binary (VoxCtrl wrote it on an earlier launch, before the package shipped
/// its own launcher), or an AppImage that no longer exists. An entry for an
/// AppImage the user still keeps alongside the package is theirs, and stays.
/// Returns whether anything was removed.
pub fn remove_superseded_user_launcher(home: &std::path::Path, current_exe: &std::path::Path) -> bool {
    let launcher = home.join(".local/share/applications").join(DESKTOP_FILE_NAME);
    let Ok(content) = std::fs::read_to_string(&launcher) else {
        return false;
    };
    let Some(exec) = desktop_exec_path(&content) else {
        return false;
    };

    let points_at_us = std::fs::canonicalize(&exec).ok().as_deref()
        == std::fs::canonicalize(current_exe).ok().as_deref()
        && exec.exists();
    let dead_appimage = exec
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase().ends_with(".appimage"))
        .unwrap_or(false)
        && !exec.exists();

    if !(points_at_us || dead_appimage) {
        return false;
    }
    if std::fs::remove_file(&launcher).is_err() {
        return false;
    }
    // The icon was written alongside the launcher and only ever by VoxCtrl.
    // Left behind, it would shadow the package's icon of the same name.
    let _ = std::fs::remove_file(home.join(".local/share/icons/hicolor/128x128/apps/voxctrl.png"));
    true
}

/// The program path in a desktop entry's `Exec=` line, without arguments.
fn desktop_exec_path(content: &str) -> Option<PathBuf> {
    let exec = content
        .lines()
        .find_map(|l| l.trim().strip_prefix("Exec="))?
        .trim();
    // VoxCtrl writes the path unquoted and with no arguments; take the first
    // word in case someone added a field code such as %U by hand.
    let program = exec.split_whitespace().next()?;
    Some(PathBuf::from(program.trim_matches('"')))
}

/// Make sure the desktop can find VoxCtrl's launcher, the way this install needs.
///
/// A package that ships the launcher already provides it, so VoxCtrl writes
/// nothing into the user's home and tidies up the entry an earlier launch left;
/// anything else (an AppImage, a `.deb` without the launcher, a dev build) gets
/// the per-user entry as before.
pub fn ensure_desktop_integration() -> Result<(), String> {
    let managed = voxctrl_update::install::is_package_managed();
    let system_launcher = std::path::Path::new(SYSTEM_DESKTOP_FILE).exists();
    if wants_user_launcher(managed, system_launcher) {
        return setup_desktop_integration();
    }

    if let (Some(home), Ok(exe)) = (dirs::home_dir(), std::env::current_exe()) {
        if remove_superseded_user_launcher(&home, &exe) {
            tracing::info!("Removed a per-user launcher the package's own launcher replaces");
        }
    }
    register_mint_shortcut_if_needed();
    Ok(())
}

/// The command that installs one missing package, for an install a package
/// manager owns.
///
/// There VoxCtrl doesn't offer to run anything with administrator rights: the
/// typing helpers are optional dependencies of the package, so the user adds
/// the one their session needs the same way they installed VoxCtrl.
pub fn single_package_command(pkg_mgr: &str, package: &str) -> Option<String> {
    match pkg_mgr {
        "pacman" => Some(format!("sudo pacman -S --needed {package}")),
        "apt" => Some(format!("sudo apt-get install -y {package}")),
        "dnf" => Some(format!("sudo dnf install -y {package}")),
        "zypper" => Some(format!("sudo zypper install -y {package}")),
        _ => None,
    }
}

pub fn run_cli_installer() -> Result<(), String> {
    println!("=== VoxCtrl CLI Installer & Host Setup ===");
    let pkg_mgr = detect_pkg_manager();
    println!("Detected Package Manager: {}", pkg_mgr);

    if voxctrl_update::install::is_package_managed() {
        // The package already pulled in what VoxCtrl needs; the typing and
        // clipboard helpers are optional dependencies the user chooses.
        println!("VoxCtrl was installed by your package manager, so its dependencies");
        println!("are already handled and there is nothing to install with sudo.");
        if let Some(tool) = crate::commands::missing_injection_tool() {
            match single_package_command(pkg_mgr, tool) {
                Some(cmd) => println!("To type into other windows, install {tool}:\n    {cmd}"),
                None => println!("To type into other windows, install {tool} with your package manager."),
            }
        }
        return ensure_desktop_integration();
    }

    let full_script = build_privileged_setup_script(pkg_mgr);
    println!("Preparing to run system setup command via sudo...");
    println!("Executing: sudo sh -c \"{}\"", full_script);

    let status = run_command_status("sudo", &["sh", "-c", &full_script])?;

    if !status.success() {
        return Err("Host package installation failed.".to_string());
    }

    println!("System dependencies installed successfully!");

    // Setup desktop integration
    println!("Registering desktop entry and icon...");
    ensure_desktop_integration()?;
    println!("Desktop integration complete!");

    println!("\n==================================================");
    println!("  Setup & Integration Complete!");
    println!("==================================================");

    println!();
    println!("Global shortcuts are registered with your desktop through the XDG");
    println!("desktop portal, so there is nothing else to grant and no keyboard");
    println!("access to set up. Start VoxCtrl; it reports in-app if your desktop");
    println!("does not provide the portal.");
    Ok(())
}

pub async fn run_gui_installer() -> Result<(), String> {
    if voxctrl_update::install::is_package_managed() {
        // The setup window no longer offers this button on a packaged install;
        // this is the backstop if an older frontend or a script still asks.
        return Err("VoxCtrl was installed by your package manager, so it won't install \
                    packages itself. Install the missing helper with your package \
                    manager; the command is shown under \"Install it manually\"."
            .to_string());
    }
    let pkg_mgr = detect_pkg_manager();
    let full_script = build_privileged_setup_script(pkg_mgr);

    if get_install_packages_command(pkg_mgr).is_none() {
        // Nothing to install on this distro; the desktop entry is all that is
        // left, and it needs no privileges.
        return ensure_desktop_integration();
    }

    if !command_exists("pkexec") {
        return Err(
            "pkexec is not installed, so VoxCtrl cannot ask for administrator access. \
             Run the commands shown under \"Set it up manually\" in a terminal instead."
                .to_string(),
        );
    }

    let script_clone = full_script.clone();
    let status = tokio::task::spawn_blocking(move || {
        run_command_status("pkexec", &["sh", "-c", &script_clone])
    }).await.map_err(|e| format!("Spawn error: {}", e))??;

    if !status.success() {
        return Err("Installing the host packages failed or was canceled.".to_string());
    }

    // Desktop integration
    ensure_desktop_integration()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_get_install_packages_command() {
        let pacman_cmd = get_install_packages_command("pacman");
        assert!(pacman_cmd.is_some());
        assert!(pacman_cmd.unwrap().contains("pacman -S"));

        let apt_cmd = get_install_packages_command("apt");
        assert!(apt_cmd.is_some());
        assert!(apt_cmd.unwrap().contains("apt-get install"));

        let unknown_cmd = get_install_packages_command("unknown");
        assert!(unknown_cmd.is_none());
    }

    #[test]
    fn the_privileged_script_never_touches_input_permissions() {
        // The whole point of the change: VoxCtrl asks for administrator rights
        // to install packages and for nothing else. A udev rule tagging input
        // devices with `uaccess`, or `usermod -aG input`, would let every
        // process running as this user read every keystroke on the machine.
        for mgr in ["pacman", "apt", "dnf", "zypper", "unknown"] {
            let script = build_privileged_setup_script(mgr);
            for forbidden in [
                "udev",
                "uaccess",
                "usermod",
                "/dev/input",
                "99-voxctrl.rules",
                "udevadm",
            ] {
                assert!(
                    !script.contains(forbidden),
                    "{mgr} setup script must not mention `{forbidden}`:\n{script}"
                );
            }
        }
    }

    #[test]
    fn the_privileged_script_installs_packages_best_effort() {
        // A stale mirror is routine on rolling distros and must not read as a
        // failed setup — nothing else in the script depends on it.
        let script = build_privileged_setup_script("pacman");
        assert!(script.contains("pacman -S"));
        assert!(script.contains("||"), "package install must be best-effort");
        assert!(script.trim_end().ends_with("exit 0"));
    }

    #[test]
    fn the_privileged_script_is_empty_work_on_an_unknown_distro() {
        let script = build_privileged_setup_script("unknown");
        assert!(!script.contains("install"));
        assert!(script.trim_end().ends_with("exit 0"));
    }

    #[test]
    fn manual_commands_cover_the_packages_and_nothing_privileged() {
        let manual = manual_setup_commands("apt");
        assert!(manual.contains("apt-get install"));
        for forbidden in ["udev", "usermod", "uaccess", "/dev/input"] {
            assert!(
                !manual.contains(forbidden),
                "manual commands must not mention `{forbidden}`"
            );
        }
        assert!(manual_setup_commands("unknown").is_empty());
    }

    #[test]
    fn test_detect_pkg_manager_mocked() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "pacman");
        assert_eq!(detect_pkg_manager(), "pacman");

        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        assert_eq!(detect_pkg_manager(), "apt");

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
    }

    #[test]
    fn test_setup_desktop_integration_success() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        let temp_dir = tempdir().unwrap();
        let home_path = temp_dir.path().to_path_buf();
        
        // Mock HOME directory environment variable
        std::env::set_var("HOME", &home_path);
        // Mock APPIMAGE environment variable
        std::env::set_var("APPIMAGE", "/usr/bin/voxctrl-fake-appimage");

        let res = setup_desktop_integration();
        assert!(res.is_ok(), "desktop integration failed: {:?}", res);

        let desktop_file = home_path.join(".local/share/applications/ai.voxctrl.app.desktop");
        let icon_file = home_path.join(".local/share/icons/hicolor/128x128/apps/voxctrl.png");

        assert!(desktop_file.exists(), "desktop file was not created");
        assert!(icon_file.exists(), "icon file was not created");

        let content = fs::read_to_string(desktop_file).unwrap();
        assert!(content.contains("Name=VoxCtrl"));
        assert!(content.contains("Exec=/usr/bin/voxctrl-fake-appimage"));
        assert!(content.contains("Icon=voxctrl"));

        std::env::remove_var("HOME");
        std::env::remove_var("APPIMAGE");
    }

    #[test]
    fn test_setup_desktop_integration_failure_readonly() {
        // Root can create directories anywhere, so the "unwritable path" premise
        // doesn't hold — skip (e.g. containerized CI). Panicking here would also
        // poison the shared env lock and cascade failures into unrelated tests.
        #[cfg(unix)]
        {
            let uid = std::process::Command::new("id").arg("-u").output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            if uid == "0" {
                return;
            }
        }

        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        // Set HOME to a non-existent/readonly path
        std::env::set_var("HOME", "/nonexistent_directory_voxctrl_test");
        let res = setup_desktop_integration();
        assert!(res.is_err(), "Expected failure when writing to nonexistent/readonly path");
        std::env::remove_var("HOME");
    }

    #[test]
    fn test_run_cli_installer_success() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "success");
        
        let temp_dir = tempdir().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let res = run_cli_installer();
        assert!(res.is_ok(), "run_cli_installer failed: {:?}", res);

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("HOME");
    }

    #[test]
    fn test_run_cli_installer_failure() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "failure");
        
        let temp_dir = tempdir().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let res = run_cli_installer();
        assert!(res.is_err(), "Expected installer failure status");

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("HOME");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // see get_env_lock
    async fn test_run_gui_installer_success() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "success");
        std::env::set_var("VOXCTRL_FAKE_COMMANDS", "pkexec");

        let temp_dir = tempdir().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let res = run_gui_installer().await;
        assert!(res.is_ok(), "run_gui_installer failed: {:?}", res);

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
        std::env::remove_var("HOME");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // see get_env_lock
    async fn test_run_gui_installer_failure() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "failure");
        std::env::set_var("VOXCTRL_FAKE_COMMANDS", "pkexec");

        let temp_dir = tempdir().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let res = run_gui_installer().await;
        assert!(res.is_err(), "Expected GUI installer failure status");

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
        std::env::remove_var("HOME");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // see get_env_lock
    async fn test_run_gui_installer_without_pkexec_explains_itself() {
        // Without polkit the one-click setup cannot run at all. Failing with a
        // bare "setup failed" leaves the user with no way forward, so the error
        // has to point at the manual commands the setup window shows.
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_PKG_MANAGER_MOCK", "apt");
        std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "success");
        std::env::set_var("VOXCTRL_FAKE_COMMANDS", "");

        let err = run_gui_installer().await.unwrap_err();
        assert!(err.contains("pkexec"), "error must name the missing tool: {err}");
        assert!(err.contains("manually"), "error must offer a way forward: {err}");

        std::env::remove_var("VOXCTRL_PKG_MANAGER_MOCK");
        std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
        std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
    }

    #[test]
    fn a_package_with_its_own_launcher_gets_no_user_launcher() {
        assert!(!wants_user_launcher(true, true));
        // A `.deb` that ships only `VoxCtrl.desktop` still needs the user entry,
        // or the portal can't name the shortcuts.
        assert!(wants_user_launcher(true, false));
        // AppImages and dev builds always write their own.
        assert!(wants_user_launcher(false, true));
        assert!(wants_user_launcher(false, false));
    }

    fn write_user_launcher(home: &std::path::Path, exec: &str) -> std::path::PathBuf {
        let apps = home.join(".local/share/applications");
        let icons = home.join(".local/share/icons/hicolor/128x128/apps");
        fs::create_dir_all(&apps).unwrap();
        fs::create_dir_all(&icons).unwrap();
        fs::write(icons.join("voxctrl.png"), b"png").unwrap();
        let launcher = apps.join(DESKTOP_FILE_NAME);
        fs::write(&launcher, format!("[Desktop Entry]\nName=VoxCtrl\nExec={exec}\nIcon=voxctrl\n")).unwrap();
        launcher
    }

    #[test]
    fn a_launcher_this_binary_wrote_is_removed_with_its_icon() {
        let home = tempdir().unwrap();
        let exe = home.path().join("voxctrl");
        fs::write(&exe, b"").unwrap();
        let launcher = write_user_launcher(home.path(), &exe.to_string_lossy());

        assert!(remove_superseded_user_launcher(home.path(), &exe));
        assert!(!launcher.exists());
        assert!(!home.path().join(".local/share/icons/hicolor/128x128/apps/voxctrl.png").exists());
    }

    #[test]
    fn a_launcher_for_a_deleted_appimage_is_removed() {
        let home = tempdir().unwrap();
        let exe = home.path().join("voxctrl");
        fs::write(&exe, b"").unwrap();
        let gone = home.path().join("Apps/VoxCtrl-linux-x86_64-vulkan.AppImage");
        let launcher = write_user_launcher(home.path(), &gone.to_string_lossy());

        assert!(remove_superseded_user_launcher(home.path(), &exe));
        assert!(!launcher.exists());
    }

    #[test]
    fn a_launcher_for_an_appimage_still_on_disk_is_kept() {
        // The user runs an AppImage alongside the package; that entry is theirs.
        let home = tempdir().unwrap();
        let exe = home.path().join("voxctrl");
        fs::write(&exe, b"").unwrap();
        let appimage = home.path().join("VoxCtrl.AppImage");
        fs::write(&appimage, b"").unwrap();
        let launcher = write_user_launcher(home.path(), &appimage.to_string_lossy());

        assert!(!remove_superseded_user_launcher(home.path(), &exe));
        assert!(launcher.exists());
    }

    #[test]
    fn a_launcher_for_some_other_program_is_kept() {
        let home = tempdir().unwrap();
        let exe = home.path().join("voxctrl");
        fs::write(&exe, b"").unwrap();
        let launcher = write_user_launcher(home.path(), "/usr/local/bin/my-voxctrl-wrapper --flag");

        assert!(!remove_superseded_user_launcher(home.path(), &exe));
        assert!(launcher.exists());
    }

    #[test]
    fn no_user_launcher_means_nothing_to_remove() {
        let home = tempdir().unwrap();
        assert!(!remove_superseded_user_launcher(home.path(), std::path::Path::new("/usr/bin/voxctrl")));
    }

    #[test]
    fn a_packaged_install_is_offered_one_package_and_no_sudo_script() {
        assert_eq!(single_package_command("pacman", "wtype").as_deref(), Some("sudo pacman -S --needed wtype"));
        assert_eq!(single_package_command("apt", "xdotool").as_deref(), Some("sudo apt-get install -y xdotool"));
        assert_eq!(single_package_command("unknown", "wtype"), None);
    }
}
