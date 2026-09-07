//! The rule that keeps this feature honest as the config grows.
//!
//! `redact::CONFIG_HANDLING` classifies every string-valued setting by hand.
//! Nothing stops someone adding a setting to `voxctrl-config` and forgetting
//! this file exists — except this test, which walks the *real* `AppConfig` and
//! fails naming any string, list or map that nobody has classified.
//!
//! The failure mode it prevents is not "a report is missing a field". It is a
//! new free-text setting — another prompt, another vocabulary — going out in a
//! public issue because the redactor had never heard of it.

use serde_json::Value;
use voxctrl_bugreport::redact::{redact_config, CONFIG_HANDLING};
use voxctrl_bugreport::scrub::Scrubber;
use voxctrl_config::AppConfig;

/// A config with every optional field populated, so `skip_serializing_if` and
/// `None` cannot hide a setting from the walk below.
fn fully_populated() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.audio.evdev_device = Some("/dev/input/event4".into());
    cfg.openai.api_key = Some("sk-test-not-a-real-key".into());
    cfg.openai.custom_prompt = Some("a prompt the user wrote".into());
    cfg.tts.hf_token = Some("hf_test_not_a_real_token".into());
    cfg.tts.pocket_tts.legacy_hf_token = Some("hf_legacy_not_a_real_token".into());
    cfg.tts.breeze_tts_2.legacy_hf_token = Some("hf_legacy_not_a_real_token".into());
    cfg.updates.skipped_version = Some("0.4.0".into());
    cfg.features.show_notification = Some(true);
    cfg.features
        .custom_vocabulary
        .push("a word the user added".into());
    cfg.features
        .snippets
        .insert("trigger".into(), "expansion the user wrote".into());
    cfg
}

/// Collect the dotted paths of every value a string could hide in.
fn string_bearing_paths(prefix: &str, value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                // A map keyed by user input (snippets) is itself the leaf: its
                // keys are as much free text as its values.
                if CONFIG_HANDLING.iter().any(|(p, _)| *p == path) {
                    out.push(path);
                    continue;
                }
                string_bearing_paths(&path, child, out);
            }
        }
        Value::String(_) | Value::Array(_) => out.push(prefix.to_string()),
        _ => {}
    }
}

#[test]
fn every_config_string_is_classified() {
    let json = serde_json::to_value(fully_populated()).expect("AppConfig serialises");
    let mut paths = Vec::new();
    string_bearing_paths("", &json, &mut paths);

    let unclassified: Vec<&String> = paths
        .iter()
        .filter(|path| !CONFIG_HANDLING.iter().any(|(p, _)| *p == path.as_str()))
        .collect();

    assert!(
        unclassified.is_empty(),
        "these config settings can carry text and nobody has said what a bug \
         report may do with them: {unclassified:#?}\n\nAdd each to \
         `CONFIG_HANDLING` in crates/voxctrl-bugreport/src/redact.rs. Until \
         then they are omitted from reports, which is safe but unhelpful."
    );
}

#[test]
fn no_classified_path_has_gone_stale() {
    // The mirror of the test above: a rule for a setting that no longer exists
    // is dead weight, and worse, it reads as protection that is not there.
    let json = serde_json::to_value(fully_populated()).expect("AppConfig serialises");
    let mut paths = Vec::new();
    string_bearing_paths("", &json, &mut paths);

    let stale: Vec<&str> = CONFIG_HANDLING
        .iter()
        .map(|(p, _)| *p)
        .filter(|p| !paths.iter().any(|found| found == p))
        .collect();

    assert!(
        stale.is_empty(),
        "these paths are classified in CONFIG_HANDLING but no longer exist in \
         AppConfig: {stale:#?}"
    );
}

#[test]
fn a_real_config_full_of_secrets_leaks_none_of_them() {
    // The end-to-end version: take a config carrying every kind of thing we
    // promise not to send, redact it, and search the output for all of it.
    let mut cfg = fully_populated();
    cfg.openai.api_key = Some("sk-live-SECRETKEY".into());
    cfg.openai.system_prompt = "my manager is called SECRETNAME".into();
    cfg.openai.endpoint = "https://SECRETHOST.internal.example/v1".into();
    cfg.tts.hf_token = Some("hf_SECRETTOKEN".into());
    cfg.tts.voice_dir = "/home/SECRETUSER/voices".into();
    cfg.features.custom_vocabulary = vec!["SECRETWORD".into()];
    cfg.features
        .snippets
        .insert("addr".into(), "SECRETADDRESS".into());
    cfg.tts.breeze_tts_2.speaker_prompt = "sounds like SECRETPERSON".into();

    let scrubber = Scrubber::new(Some("/home/SECRETUSER".into()), Some("SECRETUSER".into()));
    let redacted = redact_config(
        &serde_json::to_value(&cfg).expect("AppConfig serialises"),
        &scrubber,
    );
    let text = serde_json::to_string_pretty(&redacted).expect("redacted config serialises");

    for secret in [
        "SECRETKEY",
        "SECRETNAME",
        "SECRETHOST",
        "SECRETTOKEN",
        "SECRETUSER",
        "SECRETWORD",
        "SECRETADDRESS",
        "SECRETPERSON",
    ] {
        assert!(
            !text.contains(secret),
            "{secret} survived redaction:\n{text}"
        );
    }

    // And the settings that explain bugs did survive.
    assert_eq!(redacted["engine"]["whisper_cpp"]["model_size"], "tiny");
    assert_eq!(redacted["audio"]["noise_suppression"], false);
}
