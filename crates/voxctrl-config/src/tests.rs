#[test]
fn a_name_on_path_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let extension = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let name = format!("voxctrl_path_probe{extension}");
    std::fs::write(dir.path().join(&name), b"").unwrap();

    let _guard = PathGuard::prepending(dir.path());
    assert_eq!(
        find_in_path("voxctrl_path_probe").as_deref(),
        Some(dir.path().join(&name).as_path())
    );
}

#[test]
fn a_name_that_is_on_no_searched_directory_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = PathGuard::prepending(dir.path());
    assert_eq!(find_in_path("voxctrl_definitely_not_here_9f3a"), None);
}

/// The directories Windows searches before `PATH` are exactly the gap this
/// function used to have: `Command::new` finds a System32 tool, and looking
/// only at `PATH` said it did not exist.
#[cfg(target_os = "windows")]
#[test]
fn a_system_directory_tool_is_found_even_when_path_is_empty() {
    let _guard = PathGuard::replacing_with_nothing();
    let found = find_in_path("where").expect("where.exe lives in System32");
    assert!(found.is_file());
}

static PATH_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Puts a directory at the front of `PATH` and restores it on drop.
#[allow(dead_code)]
struct PathGuard<'a>(Option<std::ffi::OsString>, std::sync::MutexGuard<'a, ()>);

impl<'a> PathGuard<'a> {
    #[cfg(target_os = "windows")]
    fn replacing_with_nothing() -> Self {
        let guard = PATH_MUTEX.lock().unwrap();
        let previous = std::env::var_os("PATH");
        std::env::set_var("PATH", "");
        Self(previous, guard)
    }

    fn prepending(dir: &std::path::Path) -> Self {
        let guard = PATH_MUTEX.lock().unwrap();
        let previous = std::env::var_os("PATH");
        let mut entries = vec![dir.to_path_buf()];
        if let Some(existing) = &previous {
            entries.extend(std::env::split_paths(existing));
        }
        std::env::set_var("PATH", std::env::join_paths(entries).unwrap());
        Self(previous, guard)
    }
}

impl Drop for PathGuard<'_> {
    fn drop(&mut self) {
        match self.0.take() {
            Some(previous) => std::env::set_var("PATH", previous),
            None => std::env::remove_var("PATH"),
        }
    }
}

use super::*;

// ── parse_tolerant ────────────────────────────────────────────────────────

/// A file with nothing wrong with it must take the ordinary path and come
/// back exactly as written.
#[test]
fn a_valid_config_parses_whole() {
    let cfg = parse_tolerant(
        r#"{"engine": {"backend": "moonshine",
                       "whisper_cpp": {"model_dir": "", "model_size": "small",
                                       "device": "auto", "threads": 0},
                       "moonshine": {"model_size": "base", "language": "en"}},
            "audio": {"vad_threshold": 0.65, "input_device_index": null,
                      "evdev_device": null, "noise_suppression": true,
                      "gain": 1.6, "dynamic_stream": true}}"#,
    );
    assert_eq!(cfg.engine.backend, BackendChoice::Moonshine);
    assert_eq!(cfg.engine.whisper_cpp.model_size, "small");
    assert_eq!(cfg.audio.gain, 1.6);
}

/// The failure this exists for: `whisper_cpp` where the enum spells it
/// `whisper-cpp`. The engine section is lost, and every unrelated setting
/// in the file survives — which is the opposite of what used to happen.
#[test]
fn one_bad_section_does_not_take_the_rest_of_the_file_with_it() {
    let cfg = parse_tolerant(
        r#"{"engine": {"backend": "whisper_cpp"},
            "audio": {"vad_threshold": 0.65, "input_device_index": null,
                      "evdev_device": null, "noise_suppression": true,
                      "gain": 1.6, "dynamic_stream": true},
            "ui": {"show_overlay": false, "overlay_style": "pulse",
                   "overlay_position": "top", "overlay_monitor": "primary",
                   "auto_show_settings": false, "show_notification": false,
                   "show_command_overlay": true, "command_overlay_duration_secs": 3,
                   "setup_completed": true}}"#,
    );

    // Kept.
    assert_eq!(cfg.audio.gain, 1.6);
    assert!(cfg.audio.noise_suppression);
    assert_eq!(cfg.ui.overlay_style, "pulse");
    assert!(!cfg.ui.show_overlay);

    // Lost, because it is the section that would not read.
    assert_eq!(cfg.engine.backend, BackendChoice::default());
}

/// A key the running build knows nothing about is left alone rather than
/// counted as a failed section — it is how a config survives a downgrade.
#[test]
fn an_unknown_top_level_key_is_ignored() {
    let cfg = parse_tolerant(
        r#"{"engine": {"backend": "whisper_cpp"},
            "some_future_section": {"whatever": 1},
            "features": {"remove_fillers": false, "custom_vocabulary": [],
                         "spoken_punctuation": true, "auto_format_lists": true,
                         "snippets": {}}}"#,
    );
    assert!(!cfg.features.remove_fillers);
}

/// Not a JSON object at all — there are no sections to recover, so this is
/// the one case that still falls back wholesale.
#[test]
fn a_file_that_is_not_an_object_falls_back_to_defaults() {
    let cfg = parse_tolerant("[1, 2, 3]");
    assert_eq!(cfg.engine.backend, BackendChoice::default());
    assert_eq!(cfg.audio.gain, AudioConfig::default().gain);
}

fn tts_json(body: &str) -> TtsConfig {
    serde_json::from_str(body).expect("tts config should parse")
}

/// A config written when each engine carried its own copy of the token
/// must come back with one token on `tts`, and the file it writes next
/// must hold that token exactly once.
#[test]
fn migrates_per_engine_hf_tokens_onto_one_key() {
    let mut data = AppConfig {
        tts: tts_json(
            r#"{"enabled": true, "engine": "pocket_tts", "voice": "v",
                "stop_key": ["KEY_ESC"], "response_overlay": true,
                "pocket_tts": {"voice": "alba", "hf_token": "hf_from_pocket"},
                "breeze_tts_2": {"hf_token": "hf_from_pocket"}}"#,
        ),
        ..Default::default()
    };
    assert_eq!(
        data.tts.pocket_tts.legacy_hf_token.as_deref(),
        Some("hf_from_pocket"),
        "the old location must still parse"
    );

    assert!(migrate_hf_token(&mut data));

    assert_eq!(data.tts.hf_token.as_deref(), Some("hf_from_pocket"));
    assert!(data.tts.pocket_tts.legacy_hf_token.is_none());
    assert!(data.tts.breeze_tts_2.legacy_hf_token.is_none());

    let written = serde_json::to_string(&data.tts).unwrap();
    assert_eq!(
        written.matches("hf_token").count(),
        1,
        "the token must be stored once, not per engine: {written}"
    );
}

/// A token set only on Breeze is lifted too — either copy will do.
#[test]
fn migrates_a_breeze_only_token() {
    let mut data = AppConfig {
        tts: tts_json(
            r#"{"enabled": false, "engine": "espeak", "voice": "v", "stop_key": [],
                "response_overlay": true, "breeze_tts_2": {"hf_token": "hf_from_breeze"}}"#,
        ),
        ..Default::default()
    };

    assert!(migrate_hf_token(&mut data));
    assert_eq!(data.tts.hf_token.as_deref(), Some("hf_from_breeze"));
}

/// A config that already has the single key keeps it, and needs no rewrite.
#[test]
fn a_config_with_one_token_is_left_alone() {
    let mut data = AppConfig {
        tts: tts_json(
            r#"{"enabled": false, "engine": "espeak", "voice": "v", "stop_key": [],
                "response_overlay": true, "hf_token": "hf_single"}"#,
        ),
        ..Default::default()
    };

    assert!(!migrate_hf_token(&mut data), "nothing to migrate");
    assert_eq!(data.tts.hf_token.as_deref(), Some("hf_single"));
    assert_eq!(
        serde_json::to_string(&data.tts).unwrap().matches("hf_token").count(),
        1
    );
}

/// The single key wins over a stale per-engine copy rather than being
/// overwritten by it.
#[test]
fn the_single_token_wins_over_a_legacy_copy() {
    let mut data = AppConfig {
        tts: tts_json(
            r#"{"enabled": false, "engine": "espeak", "voice": "v", "stop_key": [],
                "response_overlay": true, "hf_token": "hf_current",
                "pocket_tts": {"hf_token": "hf_stale"}}"#,
        ),
        ..Default::default()
    };

    assert!(migrate_hf_token(&mut data));
    assert_eq!(data.tts.hf_token.as_deref(), Some("hf_current"));
    assert!(data.tts.pocket_tts.legacy_hf_token.is_none());
}

/// The shared voice-clip folder is renamed from its old Pocket-TTS-only
/// name to the new engine-neutral one, preserving its contents.
#[test]
fn migrates_the_cloned_voices_folder() {
    let base = tempfile::tempdir().unwrap();
    let old_dir = base.path().join("voxctrl").join("pocket-tts-voices");
    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::write(old_dir.join("narrator.wav"), b"fake wav data").unwrap();

    assert!(migrate_cloned_voices_dir_at(base.path()));

    let new_dir = base.path().join("voxctrl").join("cloned-tts-voices");
    assert!(!old_dir.exists());
    assert!(new_dir.join("narrator.wav").exists());
}

/// With no old folder there is nothing to do, and an existing new folder
/// is never overwritten.
#[test]
fn cloned_voices_migration_is_a_noop_without_the_old_folder() {
    let base = tempfile::tempdir().unwrap();
    assert!(!migrate_cloned_voices_dir_at(base.path()));

    let old_dir = base.path().join("voxctrl").join("pocket-tts-voices");
    let new_dir = base.path().join("voxctrl").join("cloned-tts-voices");
    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::write(old_dir.join("a.wav"), b"old").unwrap();
    std::fs::create_dir_all(&new_dir).unwrap();
    std::fs::write(new_dir.join("b.wav"), b"new").unwrap();

    assert!(
        !migrate_cloned_voices_dir_at(base.path()),
        "must not clobber an existing new folder"
    );
    assert!(new_dir.join("b.wav").exists());
    assert!(old_dir.join("a.wav").exists());
}

/// Configs written before the Backend dropdown lost its "Auto-detect"
/// entry still say `"auto"`. They must keep loading, on whisper.cpp —
/// which is what auto-selection resolved to in every case — rather than
/// failing the whole config back to defaults.
#[test]
fn legacy_auto_backend_loads_as_whisper_cpp() {
    let parsed: BackendChoice = serde_json::from_str(r#""auto""#).unwrap();
    assert_eq!(parsed, BackendChoice::WhisperCpp);
    assert_eq!(BackendChoice::default(), BackendChoice::WhisperCpp);
}

#[test]
fn backend_choice_serializes_kebab_case() {
    assert_eq!(
        serde_json::to_string(&BackendChoice::WhisperCpp).unwrap(),
        r#""whisper-cpp""#
    );
    assert_eq!(
        serde_json::to_string(&BackendChoice::Moonshine).unwrap(),
        r#""moonshine""#
    );
    assert_eq!(
        serde_json::to_string(&BackendChoice::Parakeet).unwrap(),
        r#""parakeet""#
    );
    assert_eq!(
        serde_json::to_string(&BackendChoice::RemoteOpenAi).unwrap(),
        r#""remote-openai""#
    );
    let parsed: BackendChoice = serde_json::from_str(r#""remote-openai""#).unwrap();
    assert_eq!(parsed, BackendChoice::RemoteOpenAi);
    let parsed_alias: BackendChoice = serde_json::from_str(r#""openai-compatible""#).unwrap();
    assert_eq!(parsed_alias, BackendChoice::RemoteOpenAi);
}

#[test]
fn test_default_config_values() {
    let cfg = AppConfig::default();
    assert!(!cfg.ui.auto_show_settings);
    assert!(!cfg.ui.show_notification);
    assert_eq!(cfg.ui.overlay_style, "mono_bars");
    assert_eq!(cfg.ui.overlay_position, "center");
    assert_eq!(cfg.ui.overlay_monitor, "primary");
    assert!(cfg.features.show_notification.is_none());
}

#[test]
fn test_legacy_notification_migration() {
    let legacy_json = r#"{
        "engine": {
            "backend": "auto",
            "whisper_cpp": {
                "model_dir": "",
                "model_size": "large-v3",
                "device": "auto",
                "threads": 0
            },
            "moonshine": {
                "model_size": "base",
                "language": "en"
            }
        },
        "audio": {
            "vad_threshold": 0.5,
            "input_device_index": null,
            "evdev_device": null,
            "noise_suppression": false,
            "gain": 1.0,
            "dynamic_stream": true
        },
        "ui": {
            "show_overlay": true,
            "overlay_style": "voice_card"
        },
        "features": {
            "remove_fillers": true,
            "custom_vocabulary": [],
            "spoken_punctuation": true,
            "auto_format_lists": true,
            "show_notification": true,
            "snippets": {}
        },
        "openai": {
            "enabled": false,
            "model": "llama3.2:1b",
            "mode": "clean",
            "custom_prompt": null,
            "endpoint": "http://localhost:11434",
            "timeout_secs": 8
        },
        "tts": {
            "enabled": false,
            "engine": "piper",
            "voice": "en-us-lessac-medium",
            "stop_key": ["KEY_ESC"],
            "response_overlay": true
        },
        "mcp": {
            "server_enabled": false,
            "record_timeout": 15.0
        }
    }"#;

    let parsed: AppConfig = serde_json::from_str(legacy_json).unwrap();
    assert!(parsed.features.show_notification.is_some());
    assert_eq!(parsed.features.show_notification, Some(true));

    // Create a temporary config path to test Config::load migration logic
    let temp_dir = tempfile::tempdir().unwrap();
    let config_file_path = temp_dir.path().join("config.json");
    std::fs::write(&config_file_path, legacy_json).unwrap();

    let config = Config {
        data: parsed,
        path: config_file_path.clone(),
    };

    // Trigger load which executes the migration
    let _migrated_config = Config::load();
    
    // Assertions on the loaded instance
    let mut custom_config = Config {
        data: config.data.clone(),
        path: config_file_path.clone(),
    };
    if let Some(legacy_notif) = custom_config.data.features.show_notification {
        custom_config.data.ui.show_notification = legacy_notif;
        custom_config.data.features.show_notification = None;
        custom_config.save().unwrap();
    }

    assert!(custom_config.data.ui.show_notification);
    assert!(custom_config.data.features.show_notification.is_none());

    // Re-read file to verify the JSON content no longer has features.show_notification
    let re_read_content = std::fs::read_to_string(&config_file_path).unwrap();
    assert!(re_read_content.contains(r#""show_notification": true"#));
    assert!(!re_read_content.contains(r#""features": {
"remove_fillers": true,
"custom_vocabulary": [],
"spoken_punctuation": true,
"auto_format_lists": true,
"show_notification": true"#));
}

#[test]
fn test_fresh_install_starts_with_the_wizard_pending() {
    // No config file on disk means a machine that has never run VoxCtrl,
    // so the first-run wizard has to be pending.
    let cfg = AppConfig::default();
    assert!(!cfg.ui.setup_completed);
}

#[test]
fn test_existing_config_file_never_reopens_the_wizard() {
    // A config written by an earlier VoxCtrl has no `setup_completed` key.
    // Its owner has plainly already set the app up, so deserializing must
    // treat the missing field as "done" rather than ambushing them with a
    // setup wizard on an upgrade.
    let legacy_json = r#"{
        "show_overlay": true,
        "overlay_style": "waveform",
        "auto_show_settings": true,
        "show_notification": false
    }"#;

    let parsed: UiConfig = serde_json::from_str(legacy_json).unwrap();
    assert!(parsed.setup_completed);
}

#[test]
fn test_setup_completed_round_trips() {
    let mut cfg = AppConfig::default();
    assert!(!cfg.ui.setup_completed);

    cfg.ui.setup_completed = true;
    let json = serde_json::to_string(&cfg).unwrap();
    let back: AppConfig = serde_json::from_str(&json).unwrap();
    assert!(back.ui.setup_completed);

    cfg.ui.setup_completed = false;
    let json = serde_json::to_string(&cfg).unwrap();
    let back: AppConfig = serde_json::from_str(&json).unwrap();
    assert!(
        !back.ui.setup_completed,
        "an explicit false must survive a save/load cycle, or a user who \
         quits the wizard would never see it again"
    );
}

#[test]
fn test_ui_config_position_monitor_defaults() {
    let partial_json = r#"{
        "show_overlay": true,
        "overlay_style": "waveform",
        "auto_show_settings": true,
        "show_notification": false
    }"#;

    let parsed: UiConfig = serde_json::from_str(partial_json).unwrap();
    assert_eq!(parsed.overlay_position, "center");
    assert_eq!(parsed.overlay_monitor, "primary");
}

#[test]
fn test_openai_prompt_defaults_for_legacy_config() {
    // Legacy config without system_prompt / user_prompt keys must deserialize
    // with the new prompt defaults applied via serde defaults.
    let legacy_openai = r#"{
        "enabled": true,
        "model": "llama3.2:1b",
        "mode": "clean",
        "custom_prompt": null,
        "endpoint": "http://localhost:11434",
        "timeout_secs": 30
    }"#;

    let parsed: OpenAiConfig = serde_json::from_str(legacy_openai).unwrap();
    assert_eq!(parsed.user_prompt, "{text}");
    assert!(parsed.system_prompt.contains("Fix grammar"));
    assert_eq!(parsed.api_key, None);
}

#[test]
fn test_openai_timeout_migration() {
    let mut default_cfg = AppConfig::default();
    default_cfg.openai.timeout_secs = 8;

    let legacy_json = serde_json::to_string(&default_cfg).unwrap();

    let parsed: AppConfig = serde_json::from_str(&legacy_json).unwrap();
    assert_eq!(parsed.openai.timeout_secs, 8);

    let temp_dir = tempfile::tempdir().unwrap();
    let config_file_path = temp_dir.path().join("config.json");
    std::fs::write(&config_file_path, &legacy_json).unwrap();

    let mut config = Config {
        data: parsed,
        path: config_file_path.clone(),
    };

    if config.data.openai.timeout_secs == 8 {
        config.data.openai.timeout_secs = 30;
        config.save().unwrap();
    }

    assert_eq!(config.data.openai.timeout_secs, 30);

    let re_read_content = std::fs::read_to_string(&config_file_path).unwrap();
    assert!(re_read_content.contains(r#""timeout_secs": 30"#));
}

#[test]
fn test_breeze_tts_2_serde() {
    let engine = TtsEngine::BreezeTts2;
    let json = serde_json::to_string(&engine).unwrap();
    assert_eq!(json, r#""breeze_tts_2""#);

    let parsed1: TtsEngine = serde_json::from_str(r#""breeze_tts_2""#).unwrap();
    assert_eq!(parsed1, TtsEngine::BreezeTts2);

    let parsed2: TtsEngine = serde_json::from_str(r#""breeze_tts2""#).unwrap();
    assert_eq!(parsed2, TtsEngine::BreezeTts2);
}

#[test]
fn test_vox_cpm_2_serde() {
    let engine = TtsEngine::VoxCpm2;
    let json = serde_json::to_string(&engine).unwrap();
    assert_eq!(json, r#""vox_cpm_2""#);

    let parsed1: TtsEngine = serde_json::from_str(r#""vox_cpm_2""#).unwrap();
    assert_eq!(parsed1, TtsEngine::VoxCpm2);

    let parsed2: TtsEngine = serde_json::from_str(r#""voxcpm2""#).unwrap();
    assert_eq!(parsed2, TtsEngine::VoxCpm2);

    let parsed3: TtsEngine = serde_json::from_str(r#""vox_cpm2""#).unwrap();
    assert_eq!(parsed3, TtsEngine::VoxCpm2);
}

/// A config written before the `updates` section was removed must still
/// load. Without unknown fields being ignored by default, an old config
/// carrying a leftover `"updates": {...}` key would fail to parse and the
/// user would silently lose every setting they ever chose.
#[test]
fn a_config_with_a_leftover_updates_section_still_loads() {
    let json = serde_json::to_string(&AppConfig::default()).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value.as_object_mut().unwrap().insert(
        "updates".to_string(),
        serde_json::json!({ "auto_check": false, "skipped_version": "0.4.0" }),
    );
    let with_leftover = serde_json::to_string(&value).unwrap();

    serde_json::from_str::<AppConfig>(&with_leftover).expect("older configs must still load");
}

#[test]
fn early_command_detection_defaults_on_for_older_configs() {
    use super::FeaturesConfig;
    let features: FeaturesConfig = serde_json::from_str(
        r#"{"remove_fillers": true, "custom_vocabulary": [], "spoken_punctuation": true,
            "auto_format_lists": true, "snippets": {}}"#,
    )
    .unwrap();
    assert!(features.early_command_detection);
}

#[test]
fn legacy_key_names_become_the_names_the_backends_report() {
    assert_eq!(canonical_key_name("KEY_ESCAPE"), Some("KEY_ESC"));
    assert_eq!(canonical_key_name("KEY_."), Some("KEY_DOT"));
    assert_eq!(canonical_key_name("KEY_>"), Some("KEY_DOT"), "shifted period");
    assert_eq!(canonical_key_name("KEY_\\"), Some("KEY_BACKSLASH"));
    assert_eq!(canonical_key_name("KEY_DOT"), None, "already canonical");
    assert_eq!(canonical_key_name("KEY_1"), None, "a real key is left alone");

    let mut keys = vec!["KEY_LEFTCTRL".to_string(), "KEY_/".to_string()];
    assert!(canonicalize_key_names(&mut keys));
    assert_eq!(keys, vec!["KEY_LEFTCTRL", "KEY_SLASH"]);
    assert!(!canonicalize_key_names(&mut keys), "a second pass changes nothing");
}
