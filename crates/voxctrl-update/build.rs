//! Bake the release-signing public key in, the way cargo actually tracks.
//!
//! Same mechanism, and same reason, as `voxctrl-bugreport/build.rs`: a bare
//! `option_env!` is not rebuilt when the variable changes, so the key is passed
//! through `cargo:rustc-env` and read back with `env!`.
//!
//! The release workflow sets `VOXCTRL_UPDATE_PUBKEY` from the repository
//! variable of the same name, and refuses to build a release without it. Any
//! other build leaves it empty, and its updater falls back to the digest check.
fn main() {
    println!("cargo:rerun-if-env-changed=VOXCTRL_UPDATE_PUBKEY");
    let key = std::env::var("VOXCTRL_UPDATE_PUBKEY").unwrap_or_default();
    // Accept the whole `.pub` file as well as its key line: keep only the line
    // that is not minisign's "untrusted comment:" header.
    let key = key
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))
        .unwrap_or_default();
    println!("cargo:rustc-env=VOXCTRL_UPDATE_PUBKEY_BAKED={key}");
}
