//! `server.toml`: where the server listens, the key clients present, and
//! which speech model it runs.
//!
//! The file is TOML rather than the desktop app's JSON because a person edits
//! it by hand, over SSH, on a machine with no Settings window. Every value can
//! also be set through a `VOXCTRL_SERVER_*` environment variable, which is how
//! a container is configured without mounting a file at all.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use voxctrl_config::{AppConfig, BackendChoice, MoonshineConfig, ParakeetConfig, WhisperCppConfig};

/// Port chosen to stay clear of the ones a speech box usually already has in
/// use (8000 and 8080 for other model servers, 11434 for Ollama).
pub const DEFAULT_BIND: &str = "0.0.0.0:8787";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ServerConfig {
    /// Address and port to listen on.
    pub bind: String,
    /// The key clients send as `Authorization: Bearer <key>`. Left empty, one
    /// is generated on first start and written back to this file.
    pub api_key: String,
    /// `false` accepts requests with no key. Only for a trusted network: the
    /// requests are recordings of someone's microphone.
    pub require_api_key: bool,
    /// Largest upload accepted, in megabytes. 16 kHz mono 16-bit audio is
    /// about 2 MB a minute.
    pub max_upload_mb: u64,
    pub engine: ServerEngineConfig,
}

/// The speech engine section. Same keys and values as the desktop app's
/// `engine` settings, minus the parts that only make sense on a client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ServerEngineConfig {
    pub backend: BackendChoice,
    pub whisper_cpp: WhisperCppConfig,
    pub moonshine: MoonshineConfig,
    pub parakeet: ParakeetConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND.into(),
            api_key: String::new(),
            require_api_key: true,
            max_upload_mb: 256,
            engine: ServerEngineConfig::default(),
        }
    }
}

impl ServerConfig {
    /// `<config dir>/voxctrl/server.toml`, beside the desktop app's own config.
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctrl")
            .join("server.toml")
    }

    pub fn from_toml_str(text: &str) -> Result<Self> {
        toml::from_str(text).context("server.toml is not valid")
    }

    /// Read `path`, or return the defaults when it does not exist yet.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Could not read {}", path.display()))?;
        Self::from_toml_str(&text).with_context(|| format!("In {}", path.display()))
    }

    /// Write the config to `path`, readable by its owner only: it holds the key.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("Could not create {}", dir.display()))?;
        }
        let text = toml::to_string_pretty(self).context("Could not serialize the config")?;
        write_private(path, &text).with_context(|| format!("Could not write {}", path.display()))
    }

    /// Apply `VOXCTRL_SERVER_*` overrides on top of the file. `get` looks a
    /// variable up; it is a parameter so tests need not touch the process
    /// environment. Empty values are ignored, so a compose file can leave a
    /// variable blank to mean "not set".
    pub fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) -> Result<()> {
        let var = |name: &str| get(name).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());

        if let Some(v) = var("VOXCTRL_SERVER_BIND") {
            self.bind = v;
        }
        if let Some(v) = var("VOXCTRL_SERVER_API_KEY") {
            self.api_key = v;
        }
        if let Some(v) = var("VOXCTRL_SERVER_REQUIRE_API_KEY") {
            self.require_api_key = parse_bool("VOXCTRL_SERVER_REQUIRE_API_KEY", &v)?;
        }
        if let Some(v) = var("VOXCTRL_SERVER_MAX_UPLOAD_MB") {
            self.max_upload_mb = v
                .parse()
                .with_context(|| format!("VOXCTRL_SERVER_MAX_UPLOAD_MB must be a number, got '{v}'"))?;
        }
        // Backend first: the three below apply to whichever backend is selected.
        if let Some(v) = var("VOXCTRL_SERVER_BACKEND") {
            self.engine.backend = parse_backend(&v)?;
        }
        if let Some(v) = var("VOXCTRL_SERVER_MODEL") {
            match self.engine.backend {
                BackendChoice::WhisperCpp => self.engine.whisper_cpp.model_size = v,
                BackendChoice::Moonshine => self.engine.moonshine.model_size = v,
                BackendChoice::Parakeet => self.engine.parakeet.model_size = v,
                BackendChoice::RemoteOpenAi => {}
            }
        }
        if let Some(v) = var("VOXCTRL_SERVER_DEVICE") {
            match self.engine.backend {
                BackendChoice::WhisperCpp => self.engine.whisper_cpp.device = v,
                BackendChoice::Moonshine => self.engine.moonshine.device = v,
                BackendChoice::Parakeet => self.engine.parakeet.device = v,
                BackendChoice::RemoteOpenAi => {}
            }
        }
        if let Some(v) = var("VOXCTRL_SERVER_LANGUAGE") {
            match self.engine.backend {
                BackendChoice::WhisperCpp => self.engine.whisper_cpp.language = v,
                BackendChoice::Moonshine => self.engine.moonshine.language = v,
                BackendChoice::Parakeet => self.engine.parakeet.language = v,
                BackendChoice::RemoteOpenAi => {}
            }
        }
        Ok(())
    }

    /// Reject settings the server cannot run with, before any model loads.
    pub fn validate(&self) -> Result<()> {
        if self.engine.backend == BackendChoice::RemoteOpenAi {
            bail!(
                "engine.backend is 'remote-openai', which forwards audio to another server. \
                 A VoxCtrl server runs the model itself: use 'whisper-cpp', 'moonshine' or 'parakeet'."
            );
        }
        if self.max_upload_mb == 0 {
            bail!("max_upload_mb must be at least 1");
        }
        self.bind
            .parse::<std::net::SocketAddr>()
            .with_context(|| format!("bind must be an address and port such as {DEFAULT_BIND}, got '{}'", self.bind))?;
        Ok(())
    }

    /// The desktop app's config shape, which is what the inference crate's
    /// backend builder reads. Everything outside `engine` stays at defaults.
    pub fn to_app_config(&self) -> AppConfig {
        let mut app = AppConfig::default();
        app.engine.backend = self.engine.backend.clone();
        app.engine.whisper_cpp = self.engine.whisper_cpp.clone();
        app.engine.moonshine = self.engine.moonshine.clone();
        app.engine.parakeet = self.engine.parakeet.clone();
        app
    }
}

fn parse_bool(name: &str, value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => bail!("{name} must be true or false, got '{other}'"),
    }
}

fn parse_backend(value: &str) -> Result<BackendChoice> {
    // Through serde, so the names and aliases are exactly the config file's.
    serde_json::from_value(serde_json::Value::String(value.to_ascii_lowercase())).map_err(|_| {
        anyhow::anyhow!(
            "VOXCTRL_SERVER_BACKEND must be 'whisper-cpp', 'moonshine' or 'parakeet', got '{value}'"
        )
    })
}

/// A new API key: 32 random bytes as hex, behind a prefix that makes it
/// recognizable in a config file or a log line.
pub fn generate_api_key() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("No random source for the API key: {e}"))?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("vx_{hex}"))
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // `mode` only applies when the file is created; tighten one that existed.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(text.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
    }

    #[test]
    fn empty_file_is_the_defaults() {
        let cfg = ServerConfig::from_toml_str("").unwrap();
        assert_eq!(cfg, ServerConfig::default());
        assert_eq!(cfg.bind, DEFAULT_BIND);
        assert!(cfg.require_api_key);
    }

    #[test]
    fn a_section_may_name_only_some_keys() {
        let cfg = ServerConfig::from_toml_str(
            "[engine]\nbackend = \"parakeet\"\n\n[engine.whisper_cpp]\nmodel_size = \"large-v3-turbo\"\n",
        )
        .unwrap();
        assert_eq!(cfg.engine.backend, BackendChoice::Parakeet);
        assert_eq!(cfg.engine.whisper_cpp.model_size, "large-v3-turbo");
        // Untouched keys keep their defaults.
        assert_eq!(cfg.engine.whisper_cpp.device, WhisperCppConfig::default().device);
        assert_eq!(cfg.engine.parakeet, ParakeetConfig::default());
    }

    #[test]
    fn saved_config_reads_back_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("server.toml");
        let mut cfg = ServerConfig { api_key: "vx_test".into(), ..Default::default() };
        cfg.engine.backend = BackendChoice::Moonshine;
        cfg.save(&path).unwrap();
        assert_eq!(ServerConfig::load(&path).unwrap(), cfg);
    }

    #[cfg(unix)]
    #[test]
    fn saved_config_is_private_to_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.toml");
        std::fs::write(&path, "").unwrap();
        ServerConfig::default().save(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn missing_file_is_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(ServerConfig::load(&dir.path().join("none.toml")).unwrap(), ServerConfig::default());
    }

    #[test]
    fn env_overrides_apply_to_the_selected_backend() {
        let mut cfg = ServerConfig::default();
        cfg.apply_env(env(&[
            ("VOXCTRL_SERVER_BIND", "127.0.0.1:9000"),
            ("VOXCTRL_SERVER_API_KEY", " secret "),
            ("VOXCTRL_SERVER_BACKEND", "Parakeet"),
            ("VOXCTRL_SERVER_MODEL", "tdt-0.6b-v3"),
            ("VOXCTRL_SERVER_DEVICE", "auto"),
            ("VOXCTRL_SERVER_LANGUAGE", "en"),
            ("VOXCTRL_SERVER_MAX_UPLOAD_MB", "64"),
        ]))
        .unwrap();
        assert_eq!(cfg.bind, "127.0.0.1:9000");
        assert_eq!(cfg.api_key, "secret");
        assert_eq!(cfg.engine.backend, BackendChoice::Parakeet);
        assert_eq!(cfg.engine.parakeet.model_size, "tdt-0.6b-v3");
        assert_eq!(cfg.engine.parakeet.device, "auto");
        assert_eq!(cfg.max_upload_mb, 64);
        // The whisper section was not the selected backend, so it is untouched.
        assert_eq!(cfg.engine.whisper_cpp, WhisperCppConfig::default());
    }

    #[test]
    fn blank_env_values_are_ignored() {
        let mut cfg = ServerConfig { api_key: "from-file".into(), ..Default::default() };
        cfg.apply_env(env(&[("VOXCTRL_SERVER_API_KEY", "  "), ("VOXCTRL_SERVER_BIND", "")])).unwrap();
        assert_eq!(cfg.api_key, "from-file");
        assert_eq!(cfg.bind, DEFAULT_BIND);
    }

    #[test]
    fn bad_env_values_are_reported_by_name() {
        let mut cfg = ServerConfig::default();
        let err = cfg.apply_env(env(&[("VOXCTRL_SERVER_BACKEND", "wisper")])).unwrap_err();
        assert!(err.to_string().contains("VOXCTRL_SERVER_BACKEND"), "{err}");
        let err = cfg.apply_env(env(&[("VOXCTRL_SERVER_REQUIRE_API_KEY", "maybe")])).unwrap_err();
        assert!(err.to_string().contains("VOXCTRL_SERVER_REQUIRE_API_KEY"), "{err}");
    }

    #[test]
    fn validate_rejects_a_remote_backend_and_a_bad_bind() {
        let mut cfg = ServerConfig::default();
        assert!(cfg.validate().is_ok());
        cfg.engine.backend = BackendChoice::RemoteOpenAi;
        assert!(cfg.validate().unwrap_err().to_string().contains("remote-openai"));
        cfg.engine.backend = BackendChoice::WhisperCpp;
        cfg.bind = "not-an-address".into();
        assert!(cfg.validate().unwrap_err().to_string().contains("bind"));
    }

    #[test]
    fn generated_keys_are_long_and_differ() {
        let a = generate_api_key().unwrap();
        let b = generate_api_key().unwrap();
        assert!(a.starts_with("vx_"));
        assert_eq!(a.len(), 3 + 64);
        assert_ne!(a, b);
    }

    #[test]
    fn app_config_carries_the_engine_section() {
        let mut cfg = ServerConfig::default();
        cfg.engine.backend = BackendChoice::Moonshine;
        cfg.engine.moonshine.model_size = "tiny".into();
        let app = cfg.to_app_config();
        assert_eq!(app.engine.backend, BackendChoice::Moonshine);
        assert_eq!(app.engine.moonshine.model_size, "tiny");
    }
}
