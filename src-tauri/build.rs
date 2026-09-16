use std::path::PathBuf;

fn main() {
    // `voxctrl-llm-sidecar` is shipped as a Tauri sidecar (see `externalBin` in
    // tauri.conf.json) so it lands next to the main binary in packaged builds.
    // Tauri validates that the sidecar file exists on *every* compile of this
    // crate, including a bare `cargo build`/`cargo check` or rust-analyzer —
    // none of which run `beforeBuildCommand`. Guarantee a placeholder exists
    // here, before tauri_build::build() runs its validation, so ordinary
    // development builds never fail. Packaging overwrites this with the real
    // binary via scripts/prepare-sidecar.mjs before Tauri bundles.
    ensure_sidecar_placeholder("voxctrl-llm-sidecar");

    // `VOXCTRL_BUILD_SHA` is baked into the binary by `option_env!` (see
    // `lib.rs`'s `BUILD_SHA`), which is resolved at compile time — so without
    // this, a rebuild after the SHA changes would silently keep the old
    // value. That is precisely the failure this is meant to prevent: an
    // AppImage that cannot say which commit it was built from led to three
    // separate rounds of reading results off a build that did not contain the
    // change being tested.
    println!("cargo:rerun-if-env-changed=VOXCTRL_BUILD_SHA");

    tauri_build::build()
}

fn ensure_sidecar_placeholder(name: &str) {
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => return,
    };
    // Tauri names sidecars with the target triple it is compiling for.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.is_empty() {
        return;
    }
    let exe_suffix = if target.contains("windows") { ".exe" } else { "" };

    let dir = manifest_dir.join("binaries");
    let sidecar = dir.join(format!("{name}-{target}{exe_suffix}"));
    if sidecar.exists() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        println!("cargo:warning=could not create {}: {e}", dir.display());
        return;
    }
    if let Err(e) = std::fs::write(&sidecar, []) {
        println!("cargo:warning=could not write placeholder sidecar {}: {e}", sidecar.display());
    }
}
