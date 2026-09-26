//! Loading and saving `config.json`.

use std::path::{Path, PathBuf};

use super::*;

/// Read a config file, keeping every section that parses.
///
/// serde is all-or-nothing: one unreadable value anywhere in the file — a
/// backend spelled `whisper_cpp` instead of `whisper-cpp`, a hand-edit, a
/// section written by a newer VoxCtrl with a stricter type — used to fail the
/// whole deserialize, and the app started on wholesale defaults. Every unrelated
/// setting in the file silently reverted for that run, and the next save wrote
/// the defaults back over the user's file for good.
///
/// So a failure is retried section by section: each top-level key is applied to
/// the defaults on its own, and one that will not deserialize is dropped with a
/// warning naming it. A bad `engine` block costs the engine settings and nothing
/// else. Only a file that is not a JSON object at all falls back entirely.
pub(crate) fn parse_tolerant(text: &str) -> AppConfig {
    let whole_file_error = match serde_json::from_str::<AppConfig>(text) {
        Ok(cfg) => return cfg,
        Err(e) => e,
    };

    let Ok(serde_json::Value::Object(file)) = serde_json::from_str::<serde_json::Value>(text)
    else {
        tracing::warn!("Failed to load config, using defaults: {whole_file_error}");
        return AppConfig::default();
    };

    tracing::warn!(
        "Config did not load as a whole ({whole_file_error}); \
         recovering it section by section"
    );

    let Ok(serde_json::Value::Object(mut merged)) = serde_json::to_value(AppConfig::default())
    else {
        return AppConfig::default();
    };

    for (key, value) in file {
        // A key with no counterpart in the defaults is one serde would ignore
        // anyway — a setting from a newer or older VoxCtrl. Leave it be.
        if !merged.contains_key(&key) {
            continue;
        }
        let mut candidate = merged.clone();
        candidate.insert(key.clone(), value);
        match serde_json::from_value::<AppConfig>(serde_json::Value::Object(candidate.clone())) {
            Ok(_) => merged = candidate,
            Err(e) => tracing::warn!(
                "Config section '{key}' could not be read ({e}); it falls back to \
                 defaults, and the rest of the file is kept"
            ),
        }
    }

    serde_json::from_value(serde_json::Value::Object(merged)).unwrap_or_default()
}

pub struct Config {
    pub data: AppConfig,
    pub(crate) path: PathBuf,
}

impl Config {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctrl")
            .join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let mut data = if path.exists() {
            match std::fs::read_to_string(&path).map_err(ConfigError::Io) {
                Ok(text) => parse_tolerant(&text),
                Err(e) => {
                    tracing::warn!("Failed to read config, using defaults: {e}");
                    AppConfig::default()
                }
            }
        } else {
            AppConfig::default()
        };

        // Every migration below rewrites the file so it runs only once; they
        // share a single save at the end rather than each writing it again.
        let mut migrated = false;

        // Migrate show_notification from legacy features to ui struct if present
        if let Some(legacy_notif) = data.features.show_notification.take() {
            data.ui.show_notification = legacy_notif;
            migrated = true;
        }

        // Legacy key names in the stop key ("KEY_ESCAPE", or a punctuation key
        // saved as e.g. "KEY_.") → the evdev names the backends report.
        migrated |= canonicalize_key_names(&mut data.tts.stop_key);

        // Migrate legacy default OpenAI timeout (8s) to the new default (30s) to prevent timeouts
        if data.openai.timeout_secs == 8 {
            data.openai.timeout_secs = 30;
            migrated = true;
        }

        // Migrate the legacy single `custom_prompt` (used when mode == Custom) into the
        // new `user_prompt` field, then clear it so this runs only once.
        if let Some(legacy_prompt) = data.openai.custom_prompt.take() {
            if !legacy_prompt.trim().is_empty() {
                data.openai.user_prompt = legacy_prompt;
                // The legacy custom prompt carried the full instruction, so drop the
                // default grammar-fix system prompt to preserve the old behavior.
                data.openai.system_prompt = String::new();
            }
            migrated = true;
        }

        // One token now serves every gated model; older configs carry a copy
        // per engine. Rewrite the file so the duplicates go away for good.
        migrated |= migrate_hf_token(&mut data);

        let config = Self { data, path };
        if migrated {
            if let Err(e) = config.save() {
                tracing::error!("Failed to save migrated config: {e}");
            }
        }

        // The shared voice-clip folder used to be named after Pocket-TTS even
        // though every cloning engine uses it; rename it on disk once.
        migrate_cloned_voices_dir();

        config
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.data)?;
        write_private(&self.path, &json)?;
        Ok(())
    }

    pub fn reload(&mut self) {
        *self = Self::load();
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::load()
    }
}

/// Replace `path` with `content`, readable only by the owner on Unix (the
/// config holds API keys and tokens).
///
/// Written to a sibling temporary file and renamed into place, so a crash or a
/// full disk mid-write leaves the previous file intact rather than a truncated
/// one — which the tolerant loader would read back as "all defaults", silently
/// discarding every setting.
pub fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    // A leftover from an interrupted write would keep its old permissions
    // through the truncating open below.
    let _ = std::fs::remove_file(&tmp);
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}
