//! Bake the relay endpoint in, in a way cargo actually tracks.
//!
//! The obvious spelling — `option_env!("VOXCTRL_BUGREPORT_ENDPOINT")` in the
//! source — does not work, and fails silently, which is worse than not working.
//! `rerun-if-env-changed` reruns *this script* when the variable changes, but
//! cargo only recompiles the crate if the script's **output** changes. A script
//! that merely declared the dependency produced identical output either way, so
//! the crate kept whatever value it was first compiled with: deploying a relay
//! and rebuilding appeared to do nothing, and the only way out was `cargo
//! clean` and a long afternoon.
//!
//! Emitting the value as `cargo:rustc-env` is what cargo does track — the
//! output differs, so the crate is rebuilt — and the source reads it back with
//! `env!`, which is then always in step with the environment.
fn main() {
    println!("cargo:rerun-if-env-changed=VOXCTRL_BUGREPORT_ENDPOINT");
    let endpoint = std::env::var("VOXCTRL_BUGREPORT_ENDPOINT").unwrap_or_default();
    println!("cargo:rustc-env=VOXCTRL_BUGREPORT_ENDPOINT_BAKED={endpoint}");
}
