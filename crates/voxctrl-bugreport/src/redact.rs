//! Turning a live VoxCtrl configuration into something safe to publish.
//!
//! A bug report is going to end up in a public issue tracker, so the question
//! this module answers is not "what is worth redacting?" but "what has earned
//! the right to be included?". Every string-valued setting is classified by
//! hand below; anything not on the list is omitted, and a test fails until
//! whoever added the setting says which kind it is. That way a new field can
//! never leak by being forgotten — the worst it can do is go missing from
//! reports until someone classifies it.
//!
//! Booleans and numbers are kept as they are. They are the settings that
//! actually explain most bugs, and a number cannot carry a name, a path or a
//! key.

use serde_json::{Map, Value};

use crate::scrub::Scrubber;

/// What a string-valued setting is allowed to become in a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handling {
    /// A value from a fixed vocabulary — an engine name, a model size, a key
    /// name. Kept, after scrubbing.
    Safe,
    /// A filesystem path. Only whether it is the platform default or a custom
    /// location is kept; the path itself never is.
    Path,
    /// An API key or token. Only whether one is set is kept.
    Secret,
    /// Something the user typed: a prompt, a vocabulary word, a snippet. Only
    /// how much of it there is is kept.
    FreeText,
    /// A URL. Reduced to scheme, host class and port — enough to tell a local
    /// server from a remote one, with no path, query or credentials.
    Endpoint,
}

use Handling::*;

/// Every string-, array- and map-valued path in `AppConfig`, and what happens
/// to it. Paths are dotted, matching the JSON the config serialises to.
///
/// Note what is *not* here: `features.snippets`, `features.custom_vocabulary`
/// and `tts.snippets` are free text the user typed, and the sample values in
/// the app's own UI are family names. They are counted, never quoted.
pub const CONFIG_HANDLING: &[(&str, Handling)] = &[
    ("engine.backend", Safe),
    ("engine.whisper_cpp.model_dir", Path),
    ("engine.whisper_cpp.model_size", Safe),
    ("engine.whisper_cpp.device", Safe),
    ("engine.moonshine.model_size", Safe),
    ("engine.moonshine.language", Safe),
    // A device node such as /dev/input/event4. No account name, no home
    // directory, and which node was picked is the whole question when a
    // hotkey does not fire.
    ("audio.evdev_device", Safe),
    ("ui.overlay_style", Safe),
    ("ui.overlay_position", Safe),
    ("ui.overlay_monitor", Safe),
    ("features.custom_vocabulary", FreeText),
    ("features.snippets", FreeText),
    ("openai.mode", Safe),
    ("openai.custom_prompt", FreeText),
    ("openai.system_prompt", FreeText),
    ("openai.user_prompt", FreeText),
    ("openai.endpoint", Endpoint),
    ("openai.model", Safe),
    ("openai.api_key", Secret),
    ("tts.engine", Safe),
    ("tts.voice", Safe),
    ("tts.voice_dir", Path),
    ("tts.stop_key", Safe),
    ("tts.hf_token", Secret),
    ("tts.snippets", FreeText),
    ("tts.pocket_tts.voice", Safe),
    ("tts.pocket_tts.voice_dir", Path),
    ("tts.pocket_tts.hf_token", Secret),
    ("tts.inflect_micro.model_dir", Path),
    ("tts.breeze_tts_2.voice_mode", Safe),
    ("tts.breeze_tts_2.cloned_voice", Safe),
    ("tts.breeze_tts_2.voice_dir", Path),
    ("tts.breeze_tts_2.speaker_prompt", FreeText),
    ("tts.breeze_tts_2.model_dir", Path),
    ("tts.breeze_tts_2.hf_token", Secret),
    ("updates.skipped_version", Safe),
];

fn handling_for(path: &str) -> Option<Handling> {
    CONFIG_HANDLING
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, h)| *h)
}

/// Redact a serialised `AppConfig`.
pub fn redact_config(config: &Value, scrubber: &Scrubber) -> Value {
    walk("", config, scrubber)
}

fn walk(path: &str, value: &Value, scrubber: &Scrubber) -> Value {
    if let Some(handling) = handling_for(path) {
        return apply(handling, value, scrubber);
    }

    match value {
        // An object with no rule of its own is descended into, so its children
        // get judged on their own paths.
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, child) in map {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                out.insert(key.clone(), walk(&child_path, child, scrubber));
            }
            Value::Object(out)
        }
        // Numbers, booleans and nulls carry nothing about the person running
        // the app, and they are most of what explains a bug.
        Value::Bool(_) | Value::Number(_) | Value::Null => value.clone(),
        // A string or a list that nobody classified. Omitting it is the safe
        // default; `every_config_string_is_classified` fails until someone
        // decides what it should be.
        Value::String(_) | Value::Array(_) => Value::String("<unclassified, omitted>".into()),
    }
}

fn apply(handling: Handling, value: &Value, scrubber: &Scrubber) -> Value {
    match handling {
        Safe => scrub_value(value, scrubber),
        Path => match value {
            Value::Null => Value::Null,
            Value::String(s) if s.is_empty() => Value::String("<platform default>".into()),
            _ => Value::String("<custom path>".into()),
        },
        Secret => match value {
            Value::Null => Value::String("<not set>".into()),
            Value::String(s) if s.is_empty() => Value::String("<not set>".into()),
            _ => Value::String("<set, not included>".into()),
        },
        FreeText => Value::String(free_text_summary(value)),
        Endpoint => match value.as_str() {
            Some(url) => Value::String(summarize_endpoint(url)),
            None => Value::Null,
        },
    }
}

fn scrub_value(value: &Value, scrubber: &Scrubber) -> Value {
    match value {
        Value::String(s) => Value::String(scrubber.scrub(s)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| scrub_value(item, scrubber))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// How much free text there is, and nothing about what it says.
fn free_text_summary(value: &Value) -> String {
    match value {
        Value::Null => "<not set>".into(),
        Value::String(s) if s.is_empty() => "<empty>".into(),
        Value::String(s) => format!("<{} characters, not included>", s.chars().count()),
        Value::Array(items) => format!("<{} entries, not included>", items.len()),
        Value::Object(map) => format!("<{} entries, not included>", map.len()),
        _ => "<not included>".into(),
    }
}

/// Reduce a URL to what distinguishes one deployment from another.
///
/// "Is it talking to something on this machine, on the LAN, or out on the
/// internet, and on what port?" is the whole diagnostic value of an endpoint,
/// and it can be answered without carrying a private hostname, a path, a query
/// string or the `user:password@` some people put in a URL.
pub fn summarize_endpoint(url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return "<not set>".into();
    }
    let (scheme, rest) = match url.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("<no scheme>".to_string(), url),
    };
    // Drop credentials, then the path and query.
    let rest = rest.rsplit_once('@').map(|(_, host)| host).unwrap_or(rest);
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    let (host, port) = split_host_port(&authority);
    let class = classify_host(host);
    match port {
        Some(port) => format!("{scheme}://{class}:{port}"),
        None => format!("{scheme}://{class}"),
    }
}

fn split_host_port(authority: &str) -> (&str, Option<&str>) {
    // An IPv6 literal is bracketed, so the last colon only means "port" when
    // it comes after the closing bracket.
    if let Some(end) = authority.find(']') {
        let (host, tail) = authority.split_at(end + 1);
        return (host, tail.strip_prefix(':').filter(|p| !p.is_empty()));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (authority, None),
    }
}

fn classify_host(host: &str) -> &'static str {
    if host.is_empty() {
        return "<no host>";
    }
    if host == "localhost"
        || host == "[::1]"
        || host.starts_with("127.")
        || host.ends_with(".localhost")
    {
        return "localhost";
    }
    if is_private_ipv4(host) || host.starts_with("[fd") || host.starts_with("[fc") {
        return "<private address>";
    }
    if host.parse::<std::net::Ipv4Addr>().is_ok() || host.starts_with('[') {
        return "<public address>";
    }
    // A hostname. Even a public one can be an employer's internal domain, so
    // the name itself does not travel — only the fact that it is off-machine.
    "<remote host>"
}

fn is_private_ipv4(host: &str) -> bool {
    let Ok(addr) = host.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    addr.is_private() || addr.is_link_local()
}

// ── Targets and hotkey bindings ──────────────────────────────────────────────

/// Keys in `targets.json` whose string values are from a fixed vocabulary.
const TARGET_SAFE_STRINGS: &[&str] = &[
    "delivery",
    "http_method",
    "file_mode",
    "chat_reply_mode",
    "processing_mode",
];

/// Keys in `bindings.json` whose string values are from a fixed vocabulary.
const BINDING_SAFE_STRINGS: &[&str] = &["gesture", "openai_mode"];

/// Keys that name a thing the user typed, and are dropped rather than summarised
/// because their length says nothing useful.
const DROPPED_KEYS: &[&str] = &["id", "label", "target_id", "target_ids"];

/// Summarise the output targets.
///
/// The shape of someone's routing is what explains a routing bug — how many
/// targets, of which kinds, with which options set. The contents are not: a
/// target's label is a name they chose, its command is a shell line from their
/// machine, its webhook URL is a secret in its own right. So every string that
/// is not from a fixed vocabulary becomes `<set>`, and identifiers become
/// stable pseudonyms so the bindings below can still be matched to them.
pub fn summarize_targets(targets: &Value, ids: &mut IdMap) -> Value {
    let Some(list) = targets.as_array() else {
        return Value::Array(vec![]);
    };
    Value::Array(
        list.iter()
            .map(|target| {
                let mut out = redact_record(target, TARGET_SAFE_STRINGS);
                if let Some(id) = target.get("id").and_then(Value::as_str) {
                    out.insert("id".into(), Value::String(ids.pseudonym(id)));
                }
                Value::Object(out)
            })
            .collect(),
    )
}

/// Summarise the hotkey bindings, under the same rule as the targets.
///
/// Key names (`["KEY_LEFTCTRL", "KEY_SPACE"]`) are kept: which combination was
/// registered is exactly what a hotkey bug is about, and a key name is not
/// personal. Nothing here reports keys that were *pressed* — VoxCtrl never
/// receives those; see `docs/privacy.md`.
pub fn summarize_bindings(bindings: &Value, ids: &mut IdMap) -> Value {
    let Some(list) = bindings.as_array() else {
        return Value::Array(vec![]);
    };
    Value::Array(
        list.iter()
            .map(|binding| {
                let mut out = redact_record(binding, BINDING_SAFE_STRINGS);
                if let Some(keys) = binding.get("keys") {
                    out.insert("keys".into(), keys.clone());
                }
                let targets: Vec<Value> = binding
                    .get("target_ids")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                    .filter(|v| !v.is_empty())
                    .or_else(|| binding.get("target_id").and_then(Value::as_str).map(|t| vec![t]))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| Value::String(ids.pseudonym(t)))
                    .collect();
                out.insert("targets".into(), Value::Array(targets));
                Value::Object(out)
            })
            .collect(),
    )
}

fn redact_record(record: &Value, safe_strings: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(map) = record.as_object() else {
        return out;
    };
    for (key, value) in map {
        if DROPPED_KEYS.contains(&key.as_str()) || key == "keys" {
            continue;
        }
        let redacted = match value {
            Value::Null => continue,
            Value::Bool(_) | Value::Number(_) => value.clone(),
            Value::String(_) if safe_strings.contains(&key.as_str()) => value.clone(),
            // Every other string is something typed on the user's machine. That
            // it is set is the diagnostic fact; what it says is theirs.
            Value::String(_) => Value::String("<set>".into()),
            Value::Array(items) => Value::String(format!("<{} entries>", items.len())),
            Value::Object(_) => Value::String("<set>".into()),
        };
        out.insert(key.clone(), redacted);
    }
    out
}

/// Replaces target identifiers with `target-1`, `target-2`, … keeping the
/// mapping stable so a binding still points at a recognisable target.
#[derive(Debug, Default)]
pub struct IdMap {
    seen: Vec<String>,
}

impl IdMap {
    pub fn pseudonym(&mut self, id: &str) -> String {
        // "default" is not a name anyone chose — it is the built-in target
        // meaning "the focused window" — so it stays legible.
        if id == "default" || id.is_empty() {
            return "default".into();
        }
        if let Some(index) = self.seen.iter().position(|seen| seen == id) {
            return format!("target-{}", index + 1);
        }
        self.seen.push(id.to_string());
        format!("target-{}", self.seen.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scrubber() -> Scrubber {
        Scrubber::new(Some("/home/jane".into()), Some("jane".into()))
    }

    #[test]
    fn secrets_are_reported_as_present_but_never_carried() {
        let cfg = json!({
            "openai": {"api_key": "sk-live-abcdef", "endpoint": "https://api.example.com/v1"},
            "tts": {"hf_token": null},
        });
        let out = redact_config(&cfg, &scrubber());
        assert_eq!(out["openai"]["api_key"], json!("<set, not included>"));
        assert_eq!(out["tts"]["hf_token"], json!("<not set>"));
        assert!(!serde_json::to_string(&out).unwrap().contains("sk-live"));
    }

    #[test]
    fn free_text_is_counted_not_quoted() {
        let cfg = json!({
            "features": {
                "custom_vocabulary": ["Waylin", "Enola"],
                "snippets": {"addr": "123 Main St"},
            },
            "openai": {"system_prompt": "Write like my boss Dana"},
        });
        let out = redact_config(&cfg, &scrubber());
        let text = serde_json::to_string(&out).unwrap();
        assert_eq!(out["features"]["custom_vocabulary"], json!("<2 entries, not included>"));
        assert_eq!(out["features"]["snippets"], json!("<1 entries, not included>"));
        for leaked in ["Waylin", "Enola", "123 Main St", "Dana"] {
            assert!(!text.contains(leaked), "{leaked} survived redaction: {text}");
        }
    }

    #[test]
    fn a_path_is_reduced_to_default_or_custom() {
        let cfg = json!({"tts": {"voice_dir": "/home/jane/voices"}, "engine": {"whisper_cpp": {"model_dir": ""}}});
        let out = redact_config(&cfg, &scrubber());
        assert_eq!(out["tts"]["voice_dir"], json!("<custom path>"));
        assert_eq!(out["engine"]["whisper_cpp"]["model_dir"], json!("<platform default>"));
    }

    #[test]
    fn numbers_and_booleans_survive_untouched() {
        // These are what actually explain most bugs, and none of them can
        // carry a name, a path or a key.
        let cfg = json!({"audio": {"gain": 1.5, "noise_suppression": true, "vad_threshold": 0.42}});
        let out = redact_config(&cfg, &scrubber());
        assert_eq!(out["audio"], json!({"gain": 1.5, "noise_suppression": true, "vad_threshold": 0.42}));
    }

    #[test]
    fn an_unclassified_string_is_omitted_rather_than_guessed() {
        let cfg = json!({"engine": {"some_future_setting": "whatever it holds"}});
        let out = redact_config(&cfg, &scrubber());
        assert_eq!(out["engine"]["some_future_setting"], json!("<unclassified, omitted>"));
    }

    #[test]
    fn endpoints_keep_only_where_and_which_port() {
        assert_eq!(summarize_endpoint("http://localhost:11434"), "http://localhost:11434");
        assert_eq!(summarize_endpoint("http://127.0.0.1:8080/v1"), "http://localhost:8080");
        assert_eq!(summarize_endpoint("http://192.168.1.40:1234/v1"), "http://<private address>:1234");
        assert_eq!(
            summarize_endpoint("https://user:pw@llm.corp.example.com/v1/chat"),
            "https://<remote host>"
        );
        assert_eq!(summarize_endpoint("http://[::1]:8000"), "http://localhost:8000");
        assert_eq!(summarize_endpoint(""), "<not set>");
    }

    #[test]
    fn a_targets_file_keeps_its_shape_and_loses_its_contents() {
        let targets = json!([
            {
                "id": "webhook-home", "label": "Message Mom", "delivery": "webhook",
                "webhook_url": "https://hooks.example.com/T0/B1/xyzsecret",
                "webhook_secret": "s3cret", "http_method": "POST",
                "http_headers": {"Authorization": "Bearer abc"},
                "chat_timeout_secs": 30, "file_timestamp": true
            }
        ]);
        let mut ids = IdMap::default();
        let out = summarize_targets(&targets, &mut ids);
        let text = serde_json::to_string(&out).unwrap();

        assert_eq!(out[0]["delivery"], json!("webhook"));
        assert_eq!(out[0]["http_method"], json!("POST"));
        assert_eq!(out[0]["chat_timeout_secs"], json!(30));
        assert_eq!(out[0]["file_timestamp"], json!(true));
        assert_eq!(out[0]["webhook_url"], json!("<set>"));
        assert_eq!(out[0]["id"], json!("target-1"));
        for leaked in ["Message Mom", "xyzsecret", "s3cret", "Bearer abc", "hooks.example.com"] {
            assert!(!text.contains(leaked), "{leaked} survived: {text}");
        }
    }

    #[test]
    fn bindings_keep_their_keys_and_point_at_pseudonymous_targets() {
        let mut ids = IdMap::default();
        let targets = json!([{"id": "obs-notes", "label": "Obsidian notes", "delivery": "file"}]);
        let redacted_targets = summarize_targets(&targets, &mut ids);
        let bindings = json!([
            {"id": "b1", "label": "Dictate to notes", "keys": ["KEY_LEFTCTRL", "KEY_SPACE"],
             "gesture": "hold", "target_id": "obs-notes", "disabled": false, "tap_ms": 200,
             "openai_prompt": "rewrite in the voice of my therapist"}
        ]);
        let out = summarize_bindings(&bindings, &mut ids);

        assert_eq!(redacted_targets[0]["id"], json!("target-1"));
        assert_eq!(out[0]["targets"], json!(["target-1"]));
        assert_eq!(out[0]["keys"], json!(["KEY_LEFTCTRL", "KEY_SPACE"]));
        assert_eq!(out[0]["gesture"], json!("hold"));
        assert_eq!(out[0]["tap_ms"], json!(200));
        assert_eq!(out[0]["openai_prompt"], json!("<set>"));
        let text = serde_json::to_string(&out).unwrap();
        assert!(!text.contains("therapist"));
        assert!(!text.contains("Dictate to notes"));
    }

    #[test]
    fn the_focused_window_target_keeps_its_name() {
        let mut ids = IdMap::default();
        assert_eq!(ids.pseudonym("default"), "default");
        assert_eq!(ids.pseudonym("mine"), "target-1");
        assert_eq!(ids.pseudonym("mine"), "target-1", "the mapping must be stable");
    }
}
