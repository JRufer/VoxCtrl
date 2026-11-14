//! "Clean up models": find the downloaded models this machine no longer uses
//! and delete the ones the user confirms.
//!
//! Written for someone who ran VoxCtrl with everything on their own computer
//! and has since pointed it at a server: gigabytes of speech and voice models
//! are still on disk, and none of them will be loaded again.
//!
//! This module deletes files, so it is deliberately narrow about which:
//!
//! - **Only what VoxCtrl downloaded.** Inside VoxCtrl's own data directory a
//!   model's whole folder is its own. In a folder the user chose (a custom
//!   Whisper or Piper directory, which may hold other things) only files with
//!   the exact names VoxCtrl downloads are considered.
//! - **Never custom voices.** The cloned-voice folder is not scanned at all,
//!   and nothing inside it can be reached from an item's paths.
//! - **Never anything in use.** The selected speech engine's model, the
//!   fallback engine's model, the cleanup model while cleanup is on and runs
//!   on this machine, and the selected text-to-speech engine's model are
//!   listed as kept, and [`execute`] refuses them even if asked.
//! - **The backend decides the paths.** The frontend sends back item ids; the
//!   plan is rebuilt here before anything is removed.
//!
//! Engine runtimes (the Piper and audio.cpp programs) are not models and are
//! left alone.

use std::path::{Path, PathBuf};

use voxctrl_config::{AppConfig, BackendChoice, FallbackChoice, TtsEngine};
use voxctrl_routing::HotkeyBinding;

/// Where VoxCtrl keeps what it downloads. A value rather than a set of global
/// lookups so tests can point it at a temporary directory.
#[derive(Debug, Clone)]
pub struct Layout {
    /// `<data-local>/voxctrl`.
    pub data_dir: PathBuf,
}

impl Layout {
    pub fn system() -> Self {
        Self {
            data_dir: dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("voxctrl"),
        }
    }

    fn models(&self) -> PathBuf {
        self.data_dir.join("models")
    }

    /// The folder of user-supplied voice clips. Never scanned, never deleted.
    pub fn custom_voices(&self) -> PathBuf {
        self.data_dir.join("cloned-tts-voices")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// A speech-to-text model.
    Speech,
    /// The dictation cleanup model.
    Cleanup,
    /// A text-to-speech model or voice.
    Voice,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CleanupItem {
    /// Stable id the frontend sends back, e.g. `whisper:base`.
    pub id: String,
    pub kind: ItemKind,
    /// What the user knows it as, e.g. "Whisper.cpp base".
    pub label: String,
    pub bytes: u64,
    /// `Some(why)` when the model is in use and will be kept.
    pub kept_because: Option<String>,
    /// The files and folders that make it up. Not sent to the frontend.
    #[serde(skip)]
    pub paths: Vec<PathBuf>,
}

impl CleanupItem {
    pub fn removable(&self) -> bool {
        self.kept_because.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CleanupPlan {
    pub items: Vec<CleanupItem>,
    /// Total size of everything that is not in use.
    pub removable_bytes: u64,
    /// Every folder that holds custom voices: the default one, and any the
    /// user pointed a voice-cloning engine at.
    #[serde(skip)]
    pub protected: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct CleanupResult {
    /// Labels of the models that were removed.
    pub removed: Vec<String>,
    pub freed_bytes: u64,
    /// One line per model that could not be removed, or was refused.
    pub errors: Vec<String>,
}

// ── Planning ──────────────────────────────────────────────────────────────────

/// List every downloaded model and say which are in use.
///
/// `fallback` is the fallback engine to plan for. With a remote speech engine
/// that is the one model kept for speech; it comes from the dialog, where the
/// user picks it before anything is removed, and may differ from the saved
/// setting. With a local engine it is ignored: the selected model is kept.
pub fn plan(cfg: &AppConfig, bindings: &[HotkeyBinding], fallback: FallbackChoice, layout: &Layout) -> CleanupPlan {
    let speech = speech_in_use(cfg, fallback);
    let mut items = Vec::new();
    items.extend(whisper_items(cfg, &speech, layout));
    items.extend(folder_per_model_items(
        &layout.models().join("moonshine"),
        "moonshine",
        "Moonshine",
        speech.as_ref().filter(|s| s.engine == BackendChoice::Moonshine),
    ));
    items.extend(folder_per_model_items(
        &layout.models().join("parakeet"),
        "parakeet",
        "Parakeet",
        speech.as_ref().filter(|s| s.engine == BackendChoice::Parakeet),
    ));
    items.extend(cleanup_model_item(cfg, bindings, layout));
    items.extend(piper_items(cfg, layout));
    items.extend(tts_folder_items(cfg, layout));

    // A custom voices folder can be anywhere, including (if someone chose so)
    // inside a model's folder. Such a model is kept whole.
    let protected = custom_voice_dirs(cfg, layout);
    for item in &mut items {
        if item.paths.iter().any(|p| touches_any(p, &protected)) {
            item.kept_because = Some("Its folder holds your custom voices".to_string());
        }
    }

    let removable_bytes = items.iter().filter(|i| i.removable()).map(|i| i.bytes).sum();
    CleanupPlan { items, removable_bytes, protected }
}

fn custom_voice_dirs(cfg: &AppConfig, layout: &Layout) -> Vec<PathBuf> {
    let mut dirs = vec![layout.custom_voices()];
    for configured in [
        &cfg.tts.pocket_tts.voice_dir,
        &cfg.tts.breeze_tts_2.voice_dir,
        &cfg.tts.vox_cpm_2.voice_dir,
        &cfg.tts.kitten_tts_2.voice_dir,
    ] {
        let configured = configured.trim();
        if !configured.is_empty() {
            dirs.push(expand_tilde(configured));
        }
    }
    dirs
}

/// Whether removing `path` could remove anything in one of `protected`.
fn touches_any(path: &Path, protected: &[PathBuf]) -> bool {
    protected.iter().any(|dir| path.starts_with(dir) || dir.starts_with(path))
}

/// The speech model this machine still needs, and why.
struct SpeechInUse {
    engine: BackendChoice,
    model: String,
    reason: &'static str,
}

fn speech_in_use(cfg: &AppConfig, fallback: FallbackChoice) -> Option<SpeechInUse> {
    let (engine, reason) = match cfg.engine.backend {
        BackendChoice::RemoteOpenAi => (fallback.backend()?, "Fallback engine"),
        ref local => (local.clone(), "Selected speech engine"),
    };
    let model = match engine {
        BackendChoice::WhisperCpp => cfg.engine.whisper_cpp.model_size.clone(),
        BackendChoice::Moonshine => cfg.engine.moonshine.model_size.clone(),
        BackendChoice::Parakeet => cfg.engine.parakeet.model_size.clone(),
        BackendChoice::RemoteOpenAi => return None,
    };
    Some(SpeechInUse { engine, model, reason })
}

fn whisper_items(cfg: &AppConfig, speech: &Option<SpeechInUse>, layout: &Layout) -> Vec<CleanupItem> {
    let configured = cfg.engine.whisper_cpp.model_dir.trim();
    let dir = if configured.is_empty() { layout.models() } else { expand_tilde(configured) };
    let in_use = speech.as_ref().filter(|s| s.engine == BackendChoice::WhisperCpp);

    let mut items = Vec::new();
    for (size, files) in voxctrl_inference::whisper_cpp::known_model_files() {
        // Only these exact file names: a custom model folder may be shared
        // with other programs, or hold models the user converted themselves.
        let paths: Vec<PathBuf> = files.iter().map(|f| dir.join(f)).filter(|p| is_plain_file(p)).collect();
        if paths.is_empty() {
            continue;
        }
        let kept = in_use.and_then(|s| {
            // The setting is a size name, or a path straight to a model file.
            let by_name = s.model == *size;
            let by_path = paths.iter().any(|p| same_file(p, &expand_tilde(&s.model)));
            (by_name || by_path).then(|| s.reason.to_string())
        });
        items.push(CleanupItem {
            id: format!("whisper:{size}"),
            kind: ItemKind::Speech,
            label: format!("Whisper.cpp {size}"),
            bytes: paths.iter().map(|p| size_of(p)).sum(),
            kept_because: kept,
            paths,
        });
    }
    items
}

/// Engines that keep each model in its own folder under VoxCtrl's data
/// directory (`models/<engine>/<model>/`). Every such folder is a model.
fn folder_per_model_items(
    engine_dir: &Path,
    id_prefix: &str,
    name: &str,
    in_use: Option<&SpeechInUse>,
) -> Vec<CleanupItem> {
    let Ok(entries) = std::fs::read_dir(engine_dir) else {
        return Vec::new();
    };
    let mut items: Vec<CleanupItem> = entries
        .flatten()
        .filter(|e| is_plain_dir(&e.path()))
        .filter_map(|e| {
            let model = e.file_name().to_str()?.to_string();
            let path = e.path();
            Some(CleanupItem {
                id: format!("{id_prefix}:{model}"),
                kind: ItemKind::Speech,
                label: format!("{name} {model}"),
                bytes: size_of(&path),
                kept_because: in_use.filter(|s| s.model == model).map(|s| s.reason.to_string()),
                paths: vec![path],
            })
        })
        .collect();
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items
}

fn cleanup_model_item(cfg: &AppConfig, bindings: &[HotkeyBinding], layout: &Layout) -> Option<CleanupItem> {
    let dir = layout.models().join("s1-mini");
    if !is_plain_dir(&dir) {
        return None;
    }
    // Cleanup can be switched on for a single shortcut while off overall.
    // Handed to a server, it never loads this machine's copy of the model.
    let kept = if cfg.engine.server_cleanup().is_some() {
        None
    } else if cfg.engine.s1_mini.enabled {
        Some("Dictation cleanup is on".to_string())
    } else if bindings.iter().any(|b| b.s1_mini_enabled == Some(true)) {
        Some("Dictation cleanup is on for a shortcut".to_string())
    } else {
        None
    };
    Some(CleanupItem {
        id: "s1-mini".into(),
        kind: ItemKind::Cleanup,
        label: "S1-mini dictation cleanup".into(),
        bytes: size_of(&dir),
        kept_because: kept,
        paths: vec![dir],
    })
}

fn piper_items(cfg: &AppConfig, layout: &Layout) -> Vec<CleanupItem> {
    let configured = cfg.tts.voice_dir.trim();
    let dir = if configured.is_empty() { layout.data_dir.join("piper-voices") } else { expand_tilde(configured) };
    let selected = (cfg.tts.enabled && cfg.tts.engine == TtsEngine::Piper).then(|| cfg.tts.voice.as_str());

    let mut items = Vec::new();
    // Only voices from VoxCtrl's own catalogue: a Piper voice the user put in
    // the folder themselves is theirs, not a download.
    for voice in voxctrl_tts::PIPER_VOICES {
        let model = dir.join(voice.filename);
        if !is_plain_file(&model) {
            continue;
        }
        let mut paths = vec![model];
        let config = dir.join(format!("{}.json", voice.filename));
        if is_plain_file(&config) {
            paths.push(config);
        }
        items.push(CleanupItem {
            id: format!("piper:{}", voice.name),
            kind: ItemKind::Voice,
            label: format!("Piper voice {}", voice.name),
            bytes: paths.iter().map(|p| size_of(p)).sum(),
            kept_because: (selected == Some(voice.name)).then(|| "Selected voice".to_string()),
            paths,
        });
    }
    items
}

/// The text-to-speech engines that keep one model folder under VoxCtrl's data
/// directory. One the user has moved to a folder of their own (`model_dir`)
/// is left alone: that folder is theirs.
fn tts_folder_items(cfg: &AppConfig, layout: &Layout) -> Vec<CleanupItem> {
    let engines: [(&str, &str, &str, TtsEngine, bool); 5] = [
        ("pocket-tts", "pocket-tts", "Pocket-TTS", TtsEngine::PocketTts, true),
        ("breeze-tts-2", "breeze-tts-2", "Breeze-TTS-2", TtsEngine::BreezeTts2, cfg.tts.breeze_tts_2.model_dir.trim().is_empty()),
        ("voxcpm2", "voxcpm2", "VoxCPM2", TtsEngine::VoxCpm2, cfg.tts.vox_cpm_2.model_dir.trim().is_empty()),
        (
            "kitten-tts-2",
            "kitten-tts-2",
            "Kitten TTS 2",
            TtsEngine::KittenTts2,
            cfg.tts.kitten_tts_2.model_dir.trim().is_empty(),
        ),
        ("inflect-micro", "inflect-micro", "Inflect-Micro", TtsEngine::InflectMicro, cfg.tts.inflect_micro.model_dir.trim().is_empty()),
    ];
    engines
        .into_iter()
        .filter(|(_, _, _, _, in_default_location)| *in_default_location)
        .filter_map(|(id, folder, name, engine, _)| {
            let dir = layout.models().join(folder);
            is_plain_dir(&dir).then(|| CleanupItem {
                id: id.to_string(),
                kind: ItemKind::Voice,
                label: format!("{name} voice model"),
                bytes: size_of(&dir),
                kept_because: (cfg.tts.enabled && cfg.tts.engine == engine)
                    .then(|| "Selected text-to-speech engine".to_string()),
                paths: vec![dir],
            })
        })
        .collect()
}

// ── Removing ──────────────────────────────────────────────────────────────────

/// Remove the items of `plan` whose ids are in `ids`.
///
/// An id that is not in the plan, or that names a model in use, is refused and
/// reported; the rest still go ahead. A model whose files cannot all be
/// removed is reported too, with what was freed counted as far as it got.
pub fn execute(plan: &CleanupPlan, ids: &[String]) -> CleanupResult {
    let mut result = CleanupResult::default();

    for id in ids {
        let Some(item) = plan.items.iter().find(|i| &i.id == id) else {
            result.errors.push(format!("{id}: not a downloaded model"));
            continue;
        };
        if let Some(reason) = &item.kept_because {
            result.errors.push(format!("{}: kept, because it is in use ({reason})", item.label));
            continue;
        }

        let mut failed = None;
        for path in &item.paths {
            // Belt and braces: planning already keeps any such model.
            if touches_any(path, &plan.protected) {
                failed = Some("refusing to touch the custom voices folder".to_string());
                break;
            }
            let bytes = size_of(path);
            match remove(path) {
                Ok(()) => result.freed_bytes += bytes,
                Err(e) => {
                    failed = Some(e.to_string());
                    break;
                }
            }
        }
        match failed {
            None => result.removed.push(item.label.clone()),
            Some(why) => result.errors.push(format!("{}: {why}", item.label)),
        }
    }
    result
}

fn remove(path: &Path) -> std::io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        // Already gone: nothing left to do.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.is_dir() {
        // Does not follow symlinks inside the folder: a link is unlinked, and
        // whatever it points at is left where it is.
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

// ── Filesystem helpers ────────────────────────────────────────────────────────

/// A real file, not a symlink to one: a link in a model folder points at
/// something that lives elsewhere and is not VoxCtrl's to count or delete.
fn is_plain_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

fn is_plain_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// Size on disk of a file, or of everything in a folder. Symlinks count as
/// nothing and are not followed.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        meta.len()
    } else if meta.is_dir() {
        std::fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| size_of(&e.path())).sum())
            .unwrap_or(0)
    } else {
        0
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A data directory with one of everything, each file `size` bytes.
    struct Fixture {
        _tmp: tempfile::TempDir,
        layout: Layout,
    }

    fn write(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0u8; bytes]).unwrap();
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("voxctrl");
        let models = data.join("models");
        write(&models.join("ggml-tiny-q5_1.bin"), 100);
        write(&models.join("ggml-base-q5_1.bin"), 200);
        write(&models.join("ggml-large-v3-turbo-q5_0.bin"), 900);
        write(&models.join("moonshine/base/encoder_model.onnx"), 300);
        write(&models.join("moonshine/base/decoder_model_merged.onnx"), 50);
        write(&models.join("parakeet/tdt-0.6b-v3/encoder-model.int8.onnx"), 600);
        write(&models.join("parakeet/tdt-0.6b-v3/vocab.txt"), 5);
        write(&models.join("parakeet/phonon-2/encoder.onnx"), 400);
        write(&models.join("s1-mini/s1-mini-q4_k_m.gguf"), 480);
        write(&models.join("pocket-tts/pocket-tts-english-q8_0.gguf"), 250);
        write(&models.join("pocket-tts/embeddings/alba.safetensors"), 10);
        write(&models.join("voxcpm2/voxcpm2-q8_0.gguf"), 700);
        write(&data.join("piper-voices/en_US-lessac-low.onnx"), 60);
        write(&data.join("piper-voices/en_US-lessac-low.onnx.json"), 2);
        write(&data.join("piper-voices/en_GB-alan-low.onnx"), 61);
        write(&data.join("cloned-tts-voices/my-own-voice.wav"), 999);
        Fixture { layout: Layout { data_dir: data }, _tmp: tmp }
    }

    fn binding(s1_mini_enabled: Option<bool>) -> HotkeyBinding {
        let mut b: HotkeyBinding =
            serde_json::from_str(r#"{"id":"b","keys":["F9"],"gesture":"hold","target_id":"default"}"#).unwrap();
        b.s1_mini_enabled = s1_mini_enabled;
        b
    }

    fn remote(cfg: &mut AppConfig) {
        cfg.engine.backend = BackendChoice::RemoteOpenAi;
    }

    fn item<'a>(plan: &'a CleanupPlan, id: &str) -> &'a CleanupItem {
        plan.items.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("no item {id}: {:?}", ids(plan)))
    }

    fn ids(plan: &CleanupPlan) -> Vec<&str> {
        plan.items.iter().map(|i| i.id.as_str()).collect()
    }

    fn removable(plan: &CleanupPlan) -> Vec<String> {
        plan.items.iter().filter(|i| i.removable()).map(|i| i.id.clone()).collect()
    }

    #[test]
    fn lists_every_downloaded_model_with_its_size() {
        let fx = fixture();
        let plan = plan(&AppConfig::default(), &[], FallbackChoice::None, &fx.layout);
        assert_eq!(
            ids(&plan),
            vec![
                "whisper:tiny",
                "whisper:base",
                "whisper:large-v3-turbo",
                "moonshine:base",
                "parakeet:phonon-2",
                "parakeet:tdt-0.6b-v3",
                "s1-mini",
                "piper:en-us-lessac-low",
                "piper:en-gb-alan-low",
                "pocket-tts",
                "voxcpm2",
            ]
        );
        assert_eq!(item(&plan, "whisper:base").bytes, 200);
        assert_eq!(item(&plan, "moonshine:base").bytes, 350);
        assert_eq!(item(&plan, "parakeet:tdt-0.6b-v3").bytes, 605);
        // A voice is its model and its config file together.
        assert_eq!(item(&plan, "piper:en-us-lessac-low").bytes, 62);
        assert_eq!(item(&plan, "pocket-tts").bytes, 260);
        assert_eq!(item(&plan, "whisper:base").label, "Whisper.cpp base");
    }

    #[test]
    fn custom_voices_are_never_listed() {
        let fx = fixture();
        let plan = plan(&AppConfig::default(), &[], FallbackChoice::None, &fx.layout);
        let voices = fx.layout.custom_voices();
        for item in &plan.items {
            for path in &item.paths {
                assert!(!path.starts_with(&voices), "{} reaches into the custom voices", item.id);
                assert!(!voices.starts_with(path), "{} contains the custom voices", item.id);
            }
        }
    }

    #[test]
    fn a_local_engine_keeps_its_selected_model_and_nothing_else_for_speech() {
        let fx = fixture();
        let mut cfg = AppConfig::default();
        cfg.engine.backend = BackendChoice::Parakeet;
        cfg.engine.parakeet.model_size = "tdt-0.6b-v3".into();
        // The fallback choice means nothing while a local engine is selected.
        let plan = plan(&cfg, &[], FallbackChoice::WhisperCpp, &fx.layout);
        assert_eq!(item(&plan, "parakeet:tdt-0.6b-v3").kept_because.as_deref(), Some("Selected speech engine"));
        assert!(item(&plan, "parakeet:phonon-2").removable());
        assert!(item(&plan, "whisper:tiny").removable());
        assert!(item(&plan, "moonshine:base").removable());
    }

    #[test]
    fn a_remote_engine_keeps_only_the_fallback_picked_in_the_dialog() {
        let fx = fixture();
        let mut cfg = AppConfig::default();
        remote(&mut cfg);
        cfg.engine.whisper_cpp.model_size = "base".into();
        // What is saved is not what counts: the dialog's pick is.
        cfg.engine.remote_openai.fallback = FallbackChoice::Parakeet;

        let picked = plan(&cfg, &[], FallbackChoice::WhisperCpp, &fx.layout);
        assert_eq!(item(&picked, "whisper:base").kept_because.as_deref(), Some("Fallback engine"));
        assert!(item(&picked, "whisper:tiny").removable());
        assert!(item(&picked, "parakeet:tdt-0.6b-v3").removable());

        // No fallback: no speech model is needed on this machine at all.
        let none = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        assert!(none.items.iter().filter(|i| i.kind == ItemKind::Speech).all(CleanupItem::removable));
    }

    #[test]
    fn the_cleanup_model_is_kept_while_cleanup_is_on_anywhere() {
        let fx = fixture();
        let mut cfg = AppConfig::default();
        assert!(item(&plan(&cfg, &[], FallbackChoice::None, &fx.layout), "s1-mini").removable());

        cfg.engine.s1_mini.enabled = true;
        assert_eq!(
            item(&plan(&cfg, &[], FallbackChoice::None, &fx.layout), "s1-mini").kept_because.as_deref(),
            Some("Dictation cleanup is on")
        );

        // Off overall, but one shortcut turns it on for itself.
        cfg.engine.s1_mini.enabled = false;
        assert!(!item(&plan(&cfg, &[binding(Some(true))], FallbackChoice::None, &fx.layout), "s1-mini").removable());
        assert!(item(&plan(&cfg, &[binding(Some(false))], FallbackChoice::None, &fx.layout), "s1-mini").removable());

        // Cleanup handed to the server: on, but this machine's model is idle.
        cfg.engine.s1_mini.enabled = true;
        remote(&mut cfg);
        assert!(!item(&plan(&cfg, &[binding(Some(true))], FallbackChoice::None, &fx.layout), "s1-mini").removable());
        cfg.engine.remote_openai.cleanup_on_server = true;
        assert!(item(&plan(&cfg, &[binding(Some(true))], FallbackChoice::None, &fx.layout), "s1-mini").removable());
    }

    #[test]
    fn only_the_selected_voice_of_an_enabled_tts_engine_is_kept() {
        let fx = fixture();
        let mut cfg = AppConfig::default();
        cfg.tts.enabled = true;
        cfg.tts.engine = TtsEngine::Piper;
        cfg.tts.voice = "en-us-lessac-low".into();
        let piper = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        assert_eq!(item(&piper, "piper:en-us-lessac-low").kept_because.as_deref(), Some("Selected voice"));
        assert!(item(&piper, "piper:en-gb-alan-low").removable());
        assert!(item(&piper, "pocket-tts").removable());

        cfg.tts.engine = TtsEngine::PocketTts;
        let pocket = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        assert!(!item(&pocket, "pocket-tts").removable());
        assert!(item(&pocket, "piper:en-us-lessac-low").removable());
        assert!(item(&pocket, "voxcpm2").removable());

        // Text-to-speech switched off: none of its models is in use.
        cfg.tts.enabled = false;
        let off = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        assert!(off.items.iter().filter(|i| i.kind == ItemKind::Voice).all(CleanupItem::removable));
    }

    #[test]
    fn kitten_tts_2_is_listed_and_kept_only_while_selected() {
        let fx = fixture();
        write(&fx.layout.models().join("kitten-tts-2/kitten-tts2-20261021-q8_0.gguf"), 330);
        let mut cfg = AppConfig::default();
        cfg.tts.enabled = true;
        cfg.tts.engine = TtsEngine::KittenTts2;
        let selected = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        let kitten = item(&selected, "kitten-tts-2");
        assert_eq!(kitten.bytes, 330);
        assert_eq!(kitten.kept_because.as_deref(), Some("Selected text-to-speech engine"));

        cfg.tts.engine = TtsEngine::Piper;
        let other = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        assert!(item(&other, "kitten-tts-2").removable());

        // A model folder of the user's own is theirs, not VoxCtrl's to offer.
        cfg.tts.kitten_tts_2.model_dir = "/somewhere/else".into();
        let moved = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        assert!(moved.items.iter().all(|i| i.id != "kitten-tts-2"));
    }

    #[test]
    fn removable_bytes_counts_only_what_is_not_in_use() {
        let fx = fixture();
        let mut cfg = AppConfig::default();
        cfg.engine.backend = BackendChoice::WhisperCpp;
        cfg.engine.whisper_cpp.model_size = "large-v3-turbo".into();
        let plan = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        let all: u64 = plan.items.iter().map(|i| i.bytes).sum();
        assert_eq!(plan.removable_bytes, all - 900);
    }

    #[test]
    fn a_custom_whisper_folder_gives_up_only_voxctrls_own_downloads() {
        let fx = fixture();
        let shared = fx.layout.data_dir.parent().unwrap().join("my-models");
        write(&shared.join("ggml-small-q5_1.bin"), 400);
        write(&shared.join("ggml-my-finetune.bin"), 500);
        write(&shared.join("notes.txt"), 3);
        let mut cfg = AppConfig::default();
        remote(&mut cfg);
        cfg.engine.whisper_cpp.model_dir = shared.to_string_lossy().into_owned();

        let plan = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        let whisper: Vec<&str> = ids(&plan).into_iter().filter(|i| i.starts_with("whisper:")).collect();
        assert_eq!(whisper, vec!["whisper:small"], "the default folder is not scanned, nor unknown files");

        let result = execute(&plan, &["whisper:small".to_string()]);
        assert_eq!(result.errors, Vec::<String>::new());
        assert!(!shared.join("ggml-small-q5_1.bin").exists());
        assert!(shared.join("ggml-my-finetune.bin").exists());
        assert!(shared.join("notes.txt").exists());
    }

    #[test]
    fn a_piper_voice_that_is_not_from_the_catalogue_is_left_alone() {
        let fx = fixture();
        write(&fx.layout.data_dir.join("piper-voices/my-own-voice.onnx"), 70);
        let plan = plan(&AppConfig::default(), &[], FallbackChoice::None, &fx.layout);
        assert!(plan.items.iter().all(|i| !i.id.contains("my-own-voice")));
    }

    #[test]
    fn a_tts_model_kept_in_a_folder_of_the_users_own_is_not_offered() {
        let fx = fixture();
        let mut cfg = AppConfig::default();
        cfg.tts.vox_cpm_2.model_dir = "/somewhere/else".into();
        let plan = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        assert!(plan.items.iter().all(|i| i.id != "voxcpm2"));
        assert!(plan.items.iter().any(|i| i.id == "pocket-tts"));
    }

    #[test]
    fn a_model_folder_that_holds_custom_voices_is_kept_whole() {
        let fx = fixture();
        let voices = fx.layout.models().join("pocket-tts/my-voices");
        write(&voices.join("me.wav"), 40);
        let mut cfg = AppConfig::default();
        cfg.tts.pocket_tts.voice_dir = voices.to_string_lossy().into_owned();

        let plan = plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        assert_eq!(item(&plan, "pocket-tts").kept_because.as_deref(), Some("Its folder holds your custom voices"));
        let result = execute(&plan, &["pocket-tts".to_string()]);
        assert!(result.removed.is_empty());
        assert!(voices.join("me.wav").exists());
        assert!(fx.layout.models().join("pocket-tts/pocket-tts-english-q8_0.gguf").exists());
    }

    #[test]
    fn removes_what_was_asked_and_nothing_else() {
        let fx = fixture();
        let mut cfg = AppConfig::default();
        remote(&mut cfg);
        cfg.engine.parakeet.model_size = "tdt-0.6b-v3".into();
        let plan = plan(&cfg, &[], FallbackChoice::Parakeet, &fx.layout);
        let models = fx.layout.models();

        let asked = vec!["whisper:base".to_string(), "moonshine:base".to_string(), "piper:en-us-lessac-low".to_string()];
        let result = execute(&plan, &asked);

        assert_eq!(result.errors, Vec::<String>::new());
        assert_eq!(result.removed, vec!["Whisper.cpp base", "Moonshine base", "Piper voice en-us-lessac-low"]);
        assert_eq!(result.freed_bytes, 200 + 350 + 62);
        assert!(!models.join("ggml-base-q5_1.bin").exists());
        assert!(!models.join("moonshine/base").exists());
        assert!(!fx.layout.data_dir.join("piper-voices/en_US-lessac-low.onnx.json").exists());
        // Not asked for, so still there, removable or not.
        assert!(models.join("ggml-tiny-q5_1.bin").exists());
        assert!(models.join("parakeet/phonon-2/encoder.onnx").exists());
        assert!(models.join("parakeet/tdt-0.6b-v3/vocab.txt").exists());
        assert!(fx.layout.data_dir.join("piper-voices/en_GB-alan-low.onnx").exists());
        assert!(fx.layout.custom_voices().join("my-own-voice.wav").exists());
    }

    #[test]
    fn a_model_in_use_is_refused_even_when_asked_for() {
        let fx = fixture();
        let mut cfg = AppConfig::default();
        remote(&mut cfg);
        cfg.engine.parakeet.model_size = "tdt-0.6b-v3".into();
        let plan = plan(&cfg, &[], FallbackChoice::Parakeet, &fx.layout);

        let asked = vec!["parakeet:tdt-0.6b-v3".to_string(), "no-such-model".to_string(), "whisper:tiny".to_string()];
        let result = execute(&plan, &asked);

        assert_eq!(result.removed, vec!["Whisper.cpp tiny"]);
        assert_eq!(result.errors.len(), 2);
        assert!(result.errors[0].contains("in use") && result.errors[0].contains("Fallback engine"), "{:?}", result.errors);
        assert!(result.errors[1].contains("no-such-model"));
        assert!(fx.layout.models().join("parakeet/tdt-0.6b-v3/encoder-model.int8.onnx").exists());
    }

    #[test]
    fn removing_everything_removable_leaves_the_kept_models_and_custom_voices() {
        let fx = fixture();
        let mut cfg = AppConfig::default();
        remote(&mut cfg);
        cfg.engine.whisper_cpp.model_size = "base".into();
        cfg.engine.s1_mini.enabled = true;
        let before = plan(&cfg, &[], FallbackChoice::WhisperCpp, &fx.layout);
        let result = execute(&before, &removable(&before));
        assert_eq!(result.errors, Vec::<String>::new());
        assert_eq!(result.freed_bytes, before.removable_bytes);

        let after = plan(&cfg, &[], FallbackChoice::WhisperCpp, &fx.layout);
        assert_eq!(ids(&after), vec!["whisper:base", "s1-mini"]);
        assert_eq!(after.removable_bytes, 0);
        assert!(fx.layout.custom_voices().join("my-own-voice.wav").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_model_is_not_counted_and_its_target_is_never_deleted() {
        let fx = fixture();
        let elsewhere = fx.layout.data_dir.parent().unwrap().join("elsewhere");
        write(&elsewhere.join("precious.bin"), 777);
        // A known Whisper file name that is really a link to a file elsewhere.
        std::os::unix::fs::symlink(elsewhere.join("precious.bin"), fx.layout.models().join("ggml-small-q5_1.bin")).unwrap();
        // And a link inside a model folder that is about to be removed.
        std::os::unix::fs::symlink(&elsewhere, fx.layout.models().join("moonshine/base/link")).unwrap();

        let plan = plan(&AppConfig::default(), &[], FallbackChoice::None, &fx.layout);
        assert!(plan.items.iter().all(|i| i.id != "whisper:small"), "a link is not a download");
        assert_eq!(item(&plan, "moonshine:base").bytes, 350, "the link's target is not counted");

        let mut cfg = AppConfig::default();
        remote(&mut cfg);
        let plan = super::plan(&cfg, &[], FallbackChoice::None, &fx.layout);
        let result = execute(&plan, &["moonshine:base".to_string()]);
        assert_eq!(result.errors, Vec::<String>::new());
        assert!(!fx.layout.models().join("moonshine/base").exists());
        assert!(elsewhere.join("precious.bin").exists(), "followed a link out of the model folder");
    }

    #[test]
    fn an_empty_or_missing_data_directory_has_nothing_to_clean() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout { data_dir: tmp.path().join("nothing-here") };
        let plan = plan(&AppConfig::default(), &[], FallbackChoice::None, &layout);
        assert!(plan.items.is_empty());
        assert_eq!(plan.removable_bytes, 0);
        assert_eq!(execute(&plan, &[]), CleanupResult::default());
    }
}
