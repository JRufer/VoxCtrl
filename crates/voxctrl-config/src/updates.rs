//! Automatic update checks.
//!
//! Off by default. VoxCtrl only contacts GitHub on its own once the user has
//! turned this on; until then the only check is the one they press in
//! Settings → General. See `docs/privacy.md`.
//!
//! The setting is `check_automatically`, not the `auto_check` key older
//! configs may still carry. That key defaulted to *on* before automatic checks
//! were removed after 0.6.3, so nearly every config that has it says `true` without
//! the user ever having chosen it. Reading it would opt those users in
//! silently, which is exactly what an opt-in must not do.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// Ask GitHub about a minute after launch, and then once a day, whether a
    /// newer release has been published. Never runs on an installation that
    /// a package manager owns, or on a development build.
    #[serde(default)]
    pub check_automatically: bool,
    /// A version the user chose to skip, so automatic checks stay quiet about
    /// it. A newer release is a new decision and is still raised; a manual
    /// check always shows what it finds.
    #[serde(default)]
    pub skipped_version: Option<String>,
}
