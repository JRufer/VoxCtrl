//! The build-time switch that decides whether a build can send reports itself.
//!
//! `option_env!` is resolved at compile time, so this cannot exercise both
//! states in one run. Instead the test asserts the contract against whatever
//! this build was compiled with — and the CI matrix runs it with the variable
//! unset, so the shipped-by-default case is the one that is verified.
//!
//! Verified by hand for the other direction:
//!
//! ```sh
//! cargo test -p voxctrl-bugreport --test endpoint            # unset  → None
//! VOXCTRL_BUGREPORT_ENDPOINT=https://relay.example \
//!   cargo test -p voxctrl-bugreport --test endpoint          # set    → Some
//! VOXCTRL_BUGREPORT_ENDPOINT= \
//!   cargo test -p voxctrl-bugreport --test endpoint          # empty  → None
//! ```

use voxctrl_bugreport::submit::{relay_endpoint, submit};

#[test]
fn the_endpoint_matches_what_this_build_was_compiled_with() {
    // Read from the environment at *test run* time, not compile time: the
    // point of this test is that a rebuild picks the change up, and a
    // compile-time read in the test would go stale in exactly the same way the
    // bug in build.rs did — and pass regardless.
    let compiled = std::env::var("VOXCTRL_BUGREPORT_ENDPOINT").ok();
    let compiled = compiled.as_deref();
    match compiled {
        // An unset secret in a CI workflow arrives as an empty string, not as
        // an absent variable — so an empty value has to mean "no relay", or
        // every build would advertise a Send button pointing at nothing.
        None | Some("") => assert_eq!(
            relay_endpoint(),
            None,
            "a build with no endpoint must not offer to send reports"
        ),
        Some(url) => assert_eq!(relay_endpoint(), Some(url)),
    }
}

#[tokio::test]
async fn a_build_with_no_relay_refuses_before_touching_the_network() {
    if relay_endpoint().is_some() {
        return; // This build has one; the case under test does not apply.
    }
    let report = serde_json::from_str::<voxctrl_bugreport::BugReport>(
        r#"{"schema":1,"created_at":"2026-01-01T00:00:00Z","install_id":"x",
            "fingerprint":"abc","statement":{"summary":"s","description":"d","area":"a","frequency":"always"},
            "system":{"app_version":"0.5.1","install_kind":"test","build_features":[],
                      "os":"linux","arch":"x86_64","gpus":[],"collection_notes":[]},
            "config":{},"targets":[],"bindings":[],
            "log":{"text":"","lines":0,"truncated":false}}"#,
    )
    .expect("the fixture matches BugReport");

    let err = submit(&report).await.unwrap_err();
    assert!(
        matches!(err, voxctrl_bugreport::SubmitError::NotConfigured),
        "got {err:?}"
    );
}
