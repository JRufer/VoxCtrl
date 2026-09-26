//! One-time rewrites of settings saved by older versions.

use super::*;

/// Lift a HuggingFace token stored per engine onto the single `tts.hf_token`,
/// clearing the old copies. Returns whether anything moved, so the caller
/// knows to rewrite the file.
///
/// Pocket-TTS and Breeze-TTS-2 each used to hold their own copy, synchronized
/// on load; they now share one key, and the same token downloads both.
pub(crate) fn migrate_hf_token(data: &mut AppConfig) -> bool {
    let legacy = data
        .tts
        .pocket_tts
        .legacy_hf_token
        .take()
        .or_else(|| data.tts.breeze_tts_2.legacy_hf_token.take());
    data.tts.breeze_tts_2.legacy_hf_token = None;

    let Some(token) = legacy else { return false };
    if data.tts.hf_token.is_none() {
        data.tts.hf_token = Some(token);
    }
    true
}

/// Rename `<base>/voxctrl/pocket-tts-voices` to `<base>/voxctrl/cloned-tts-voices`,
/// the shared clip folder's new name now that it is used by every
/// voice-cloning TTS engine, not just Pocket-TTS. Returns whether a rename
/// happened; a no-op once it has, or if the user never had the old folder.
pub(crate) fn migrate_cloned_voices_dir_at(base: &std::path::Path) -> bool {
    let old_dir = base.join("voxctrl").join("pocket-tts-voices");
    let new_dir = base.join("voxctrl").join("cloned-tts-voices");
    if old_dir.exists() && !new_dir.exists() {
        match std::fs::rename(&old_dir, &new_dir) {
            Ok(()) => return true,
            Err(e) => tracing::error!(
                "Failed to migrate {} to {}: {e}",
                old_dir.display(),
                new_dir.display()
            ),
        }
    }
    false
}

pub(crate) fn migrate_cloned_voices_dir() {
    if let Some(base) = dirs::data_local_dir() {
        migrate_cloned_voices_dir_at(&base);
    }
}

/// The canonical evdev name for a key name an older VoxCtrl may have saved, or
/// `None` when `name` is already canonical (or not one we know to rewrite).
///
/// - `KEY_ESCAPE` → `KEY_ESC`: the evdev crate's name is `KEY_ESC`.
/// - Punctuation: the recorders used to build a name from the typed character,
///   so `.` was saved as `KEY_.` — a name no backend reports, so the shortcut
///   could never fire. The unshifted and shifted characters of a US layout are
///   both mapped, since Shift changes the character typed on the same key.
///
/// Numpad digits were saved as the top-row digit (`KEY_1`), which is a real
/// key name, so those cannot be told apart and are left alone.
pub fn canonical_key_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "KEY_ESCAPE" => "KEY_ESC",
        "KEY_." | "KEY_>" => "KEY_DOT",
        "KEY_," | "KEY_<" => "KEY_COMMA",
        "KEY_/" | "KEY_?" => "KEY_SLASH",
        "KEY_;" | "KEY_:" => "KEY_SEMICOLON",
        "KEY_'" | "KEY_\"" => "KEY_APOSTROPHE",
        "KEY_`" | "KEY_~" => "KEY_GRAVE",
        "KEY_-" | "KEY__" => "KEY_MINUS",
        "KEY_=" | "KEY_+" => "KEY_EQUAL",
        "KEY_[" | "KEY_{" => "KEY_LEFTBRACE",
        "KEY_]" | "KEY_}" => "KEY_RIGHTBRACE",
        "KEY_\\" | "KEY_|" => "KEY_BACKSLASH",
        _ => return None,
    })
}

/// Rewrite every legacy key name in `keys` to its canonical spelling,
/// returning whether anything changed.
pub fn canonicalize_key_names(keys: &mut [String]) -> bool {
    let mut changed = false;
    for key in keys.iter_mut() {
        if let Some(canonical) = canonical_key_name(key) {
            *key = canonical.to_string();
            changed = true;
        }
    }
    changed
}
