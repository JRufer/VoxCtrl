//! Synthesised keyboard input on Windows, and the marker that identifies it.
//!
//! Everything that puts text into the focused window on Windows goes through
//! here, so there is one implementation to get right and one marker for the
//! hotkey hook to recognise.
//!
//! This replaced a `powershell.exe -Command SendKeys::SendWait` call, which was
//! wrong in a way that only showed up on real dictation. `SendKeys` reads
//! `+ ^ % ~ ( ) { } [ ]` as its own syntax, so "50% (a+b)" arrived as "50"
//! followed by a couple of stray chords, and "array[0]" as "array0". The old
//! code base64-encoded the payload — which does protect it from *PowerShell*
//! parsing, and was a real defence — but the decoded string still went through
//! SendKeys' own escaping, so ordinary prose came out mangled. It also spawned
//! a process per dictation, on the path where latency is most visible.
//!
//! `SendInput` with `KEYEVENTF_UNICODE` carries the character itself rather
//! than a keystroke to be interpreted: no escaping exists to get wrong, no
//! keyboard layout is consulted, and nothing is spawned.
//!
//! The chunking logic below is deliberately outside the platform gate so its
//! tests run on the Linux lane, where the suite actually runs.

/// Marker stamped into the `dwExtraInfo` of every keystroke VoxCtrl
/// synthesises, so its own keyboard hook can tell the app's output from the
/// user typing.
///
/// Without it, dictating text that completes a binding would re-trigger that
/// binding from VoxCtrl's own output.
///
/// Arbitrary, but deliberately not a small integer: other software stamps
/// `dwExtraInfo` too, and 0 or 1 would collide.
pub const INJECTED_TAG: usize = 0x9605_C731;

/// Above this many characters, typing is abandoned for a clipboard paste.
///
/// `SendInput` is a single call whatever the length, but the receiving
/// application still processes one message per character, and a long
/// transcription visibly crawls into editors that do syntax work per keystroke.
/// A paste is one operation regardless of size.
pub const PASTE_THRESHOLD_CHARS: usize = 2000;

/// How many UTF-16 code units to submit per `SendInput` call.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const CHUNK_UNITS: usize = 512;

/// Split a UTF-16 buffer into chunks of at most `max` units without ever
/// separating a surrogate pair.
///
/// A high surrogate delivered in one `SendInput` call and its low surrogate in
/// the next is not a character: the receiving application sees two lone
/// surrogates and renders replacement glyphs. Emoji and the rarer CJK blocks
/// are exactly the text this protects.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn chunk_boundaries(units: &[u16], max: usize) -> Vec<std::ops::Range<usize>> {
    let is_high_surrogate = |u: u16| (0xD800..0xDC00).contains(&u);

    let mut ranges = Vec::new();
    let mut start = 0;
    while start < units.len() {
        let mut end = (start + max).min(units.len());
        // A high surrogate is never the last unit of a chunk.
        if end < units.len() && is_high_surrogate(units[end - 1]) {
            if end - start > 1 {
                // Push the pair into the next chunk.
                end -= 1;
            } else {
                // The chunk is the high surrogate alone, so there is nothing to
                // push it into — dropping it would leave an empty range and
                // make no progress. Take the low surrogate too and run one unit
                // over `max`, which the API does not mind.
                end += 1;
            }
        }
        ranges.push(start..end);
        start = end;
    }
    ranges
}

/// The keyboard shortcut that pastes, which differs by application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    /// Ctrl+V — nearly everything.
    CtrlV,
    /// Ctrl+Shift+V — Linux terminal emulators, where Ctrl+V is a control
    /// character.
    CtrlShiftV,
    /// Shift+Insert — Windows consoles, mintty and PuTTY, which predate
    /// Ctrl+V (and still honour it where Ctrl+V is off).
    ShiftInsert,
}

impl Shortcut {
    /// Parse a configured name. `None` for "auto" and anything unrecognised,
    /// which means "pick one for the focused application".
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().replace(' ', "").as_str() {
            "ctrl+v" => Some(Self::CtrlV),
            "ctrl+shift+v" => Some(Self::CtrlShiftV),
            "shift+insert" => Some(Self::ShiftInsert),
            _ => None,
        }
    }

    /// The keys this shortcut presses, under the evdev names bindings are
    /// stored with.
    ///
    /// Used to tell whether a hotkey binding shares a key with the paste
    /// VoxCtrl is in the middle of sending — see [`begin_self_injection`].
    /// The names have to match `HotkeyBinding::keys` exactly, which is why
    /// they live beside the shortcut rather than being derived at the call
    /// site from the platform keycodes.
    pub fn key_names(self) -> &'static [&'static str] {
        match self {
            Self::CtrlV => &["KEY_LEFTCTRL", "KEY_V"],
            Self::CtrlShiftV => &["KEY_LEFTCTRL", "KEY_LEFTSHIFT", "KEY_V"],
            Self::ShiftInsert => &["KEY_LEFTSHIFT", "KEY_INSERT"],
        }
    }
}

/// Window classes (Windows) whose paste shortcut is Shift+Insert.
///
/// `ConsoleWindowClass` is the classic console host, where Ctrl+V works only
/// if the user has not turned the "Ctrl key shortcuts" option off; mintty
/// (Git Bash, Cygwin) and PuTTY never bind plain Ctrl+V to paste.
pub fn windows_class_wants_shift_insert(class: &str) -> bool {
    matches!(
        class,
        "ConsoleWindowClass" | "mintty" | "PuTTY" | "VirtualConsoleClass" | "Console_2_Main"
    )
}

/// Whether `text` is long enough to be worth pasting rather than typing.
pub fn prefers_paste(text: &str) -> bool {
    text.chars().count() > PASTE_THRESHOLD_CHARS
}

// ── Self-injection window ─────────────────────────────────────────────────────

/// How long after an injection finishes its keys are still treated as VoxCtrl's
/// own.
///
/// The compositor's signal does not have to arrive while the keys are still
/// being sent. In the report this was written for it landed 9ms *before* the
/// send call returned, but nothing guarantees that ordering — the portal round
/// trip is asynchronous and the signal is delivered on a different connection
/// from the one sending the keys.
///
/// It cannot swallow a real release: a genuine one inside this window would
/// mean a hold that started and ended entirely within a paste that VoxCtrl
/// itself triggered.
const SELF_INJECTION_GRACE: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Default)]
struct SelfInjectionState {
    /// How many injections are in flight. A counter rather than a flag because
    /// the window must not close while a second one is still sending.
    depth: usize,
    /// Keys claimed by the injections in this window, as evdev names.
    keys: Vec<&'static str>,
    /// When the window closes, once nothing is in flight.
    expires: Option<std::time::Instant>,
}

static SELF_INJECTION: std::sync::Mutex<SelfInjectionState> =
    std::sync::Mutex::new(SelfInjectionState {
        depth: 0,
        keys: Vec::new(),
        expires: None,
    });

/// Open on drop: the window stays open for [`SELF_INJECTION_GRACE`] afterwards.
#[must_use = "the window closes as soon as the guard is dropped"]
pub struct SelfInjectionGuard {
    _private: (),
}

impl Drop for SelfInjectionGuard {
    fn drop(&mut self) {
        let Ok(mut state) = SELF_INJECTION.lock() else {
            return;
        };
        state.depth = state.depth.saturating_sub(1);
        if state.depth == 0 {
            state.expires = Some(std::time::Instant::now() + SELF_INJECTION_GRACE);
        }
    }
}

/// How long a binding's keys stay suspect after VoxCtrl stops pressing them.
///
/// Callers that defer a decision rather than making one immediately wait this
/// long before concluding the signal was genuine.
pub fn self_injection_grace() -> std::time::Duration {
    SELF_INJECTION_GRACE
}

/// Declare that VoxCtrl is about to synthesise `keys` itself.
///
/// This is the Linux counterpart of [`INJECTED_TAG`]. On Windows the hook reads
/// the tag off each keystroke, and the evdev and X11 backends recognise their
/// own output by device — but the XDG `GlobalShortcuts` portal hands over whole
/// shortcuts with no keystrokes to inspect, so the only thing that can tell a
/// phantom signal from a real one is knowing what VoxCtrl was pressing at the
/// time.
///
/// Why it is needed at all: pasting with Ctrl+V while the user holds a Ctrl+V
/// binding — Ctrl+Space, say — makes the compositor re-evaluate that grab and
/// report the shortcut released. The portal backend takes that at face value
/// and ends the recording mid-sentence.
pub fn begin_self_injection(keys: &[&'static str]) -> SelfInjectionGuard {
    if let Ok(mut state) = SELF_INJECTION.lock() {
        if state.depth == 0 {
            state.keys.clear();
        }
        state.depth += 1;
        state.expires = None;
        for k in keys {
            if !state.keys.contains(k) {
                state.keys.push(k);
            }
        }
    }
    SelfInjectionGuard { _private: () }
}

/// The keys VoxCtrl is currently synthesising, if any.
///
/// `None` once the window has closed. Callers compare these against a
/// binding's own keys and ignore the signal only when the two overlap, so an
/// unrelated binding is never affected by a paste.
pub fn self_injecting_keys() -> Option<Vec<&'static str>> {
    let mut state = SELF_INJECTION.lock().ok()?;
    let open = state.depth > 0
        || state
            .expires
            .is_some_and(|e| std::time::Instant::now() < e);
    if !open {
        // Closed: drop the keys so a later caller is not comparing against a
        // paste that finished minutes ago.
        state.keys.clear();
        state.expires = None;
        return None;
    }
    Some(state.keys.clone())
}

/// True when `binding_keys` shares a key with an injection in flight.
///
/// The overlap test is the whole safety margin: a Super+Space binding is
/// untouched by a Ctrl+V paste, so suppression can only ever hide a signal
/// that the injection could plausibly have caused.
pub fn injection_overlaps<S: AsRef<str>>(binding_keys: &[S]) -> bool {
    let Some(injected) = self_injecting_keys() else {
        return false;
    };
    binding_keys
        .iter()
        .any(|k| injected.contains(&k.as_ref()))
}

#[cfg(target_os = "windows")]
mod imp {
    use anyhow::{bail, Result};
    use tracing::debug;

    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_CONTROL, VK_INSERT, VK_LWIN,
        VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow};

    use super::{chunk_boundaries, windows_class_wants_shift_insert, Shortcut, CHUNK_UNITS, INJECTED_TAG};

    /// Type `text` into whatever currently has focus, character by character.
    pub fn type_text(text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let units: Vec<u16> = text.encode_utf16().collect();
        for range in chunk_boundaries(&units, CHUNK_UNITS) {
            send_unicode(&units[range])?;
        }
        debug!(chars = text.chars().count(), "typed via SendInput");
        Ok(())
    }

    fn send_unicode(units: &[u16]) -> Result<()> {
        let mut inputs: Vec<INPUT> = Vec::with_capacity(units.len() * 2);
        for unit in units {
            inputs.push(key_event(0, *unit, KEYEVENTF_UNICODE));
            inputs.push(key_event(0, *unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        submit(&inputs)
    }

    fn key_event(vk: u16, scan: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: INJECTED_TAG,
                },
            },
        }
    }

    fn submit(inputs: &[INPUT]) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize != inputs.len() {
            // The usual cause is UIPI: a more-privileged window has focus, and
            // an unelevated process may not synthesise input into it. Say so,
            // because the alternative symptom is dictation that silently does
            // nothing.
            bail!(
                "SendInput delivered {sent} of {} events ({}). If the focused window \
                 belongs to an elevated application, Windows blocks input from \
                 unelevated processes into it.",
                inputs.len(),
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }

    /// The program behind the window that has keyboard focus, as its file
    /// name without the extension (`slack`, `thunderbird`, `code`), if any.
    ///
    /// Window classes are often shared (every Chromium/Electron app reports
    /// `Chrome_WidgetWin_1`), so this is what per-app settings match against.
    pub fn foreground_app() -> Option<String> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
            CloseHandle(process);
            if ok == 0 || len == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            super::program_stem(&path)
        }
    }

    /// The class name of the window that has keyboard focus, if any.
    pub fn foreground_class() -> Option<String> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut buf = [0u16; 256];
            let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if n <= 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..n as usize]))
        }
    }

    /// Pick the paste shortcut for the focused window.
    pub fn auto_shortcut() -> Shortcut {
        match foreground_class() {
            Some(c) if windows_class_wants_shift_insert(&c) => Shortcut::ShiftInsert,
            _ => Shortcut::CtrlV,
        }
    }

    /// Modifier keys the user is physically still holding (typically the
    /// dictation hotkey) would turn the paste into a different shortcut, so
    /// they are released first — the equivalent of `xdotool --clearmodifiers`.
    fn release_held_modifiers() -> Result<()> {
        let held: Vec<INPUT> = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
            .into_iter()
            .filter(|vk| unsafe { GetAsyncKeyState(*vk as i32) } < 0)
            .map(|vk| key_event(vk, 0, KEYEVENTF_KEYUP))
            .collect();
        submit(&held)
    }

    /// Press the paste shortcut in the focused window.
    ///
    /// The clipboard handling around it (backup, restore) lives in
    /// `voxctrl-inject`, which is shared with Linux.
    pub fn press_paste(shortcut: Shortcut) -> Result<()> {
        release_held_modifiers()?;
        let inputs: Vec<INPUT> = match shortcut {
            Shortcut::CtrlV => vec![
                key_event(VK_CONTROL, 0, 0),
                key_event(VK_V, 0, 0),
                key_event(VK_V, 0, KEYEVENTF_KEYUP),
                key_event(VK_CONTROL, 0, KEYEVENTF_KEYUP),
            ],
            Shortcut::CtrlShiftV => vec![
                key_event(VK_CONTROL, 0, 0),
                key_event(VK_SHIFT, 0, 0),
                key_event(VK_V, 0, 0),
                key_event(VK_V, 0, KEYEVENTF_KEYUP),
                key_event(VK_SHIFT, 0, KEYEVENTF_KEYUP),
                key_event(VK_CONTROL, 0, KEYEVENTF_KEYUP),
            ],
            // VK_INSERT is an extended key; without the flag the numpad's
            // Insert (NumLock-dependent) is sent instead.
            Shortcut::ShiftInsert => vec![
                key_event(VK_SHIFT, 0, 0),
                key_event(VK_INSERT, 0, KEYEVENTF_EXTENDEDKEY),
                key_event(VK_INSERT, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP),
                key_event(VK_SHIFT, 0, KEYEVENTF_KEYUP),
            ],
        };
        submit(&inputs)
    }
}

#[cfg(target_os = "windows")]
pub use imp::{auto_shortcut, foreground_app, foreground_class, press_paste, type_text};

/// A program's name from its full path: the file name without its extension,
/// lowercased (`C:\\...\\Slack.exe` → `slack`).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn program_stem(path: &str) -> Option<String> {
    let file = path.rsplit(['\\', '/']).next()?;
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    (!stem.is_empty()).then(|| stem.to_lowercase())
}

#[cfg(test)]
mod self_injection_tests {
    use super::*;

    /// The window is a process global, and `cargo test` runs these in
    /// parallel. Without this they would see each other's injections.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn locked() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn nothing_is_claimed_when_no_injection_is_running() {
        let _lock = locked();
        // A test that ran before this one may have left a grace period
        // running. Nothing can open a new window while the lock is held, so
        // waiting it out here is enough to reach a genuinely idle state.
        std::thread::sleep(SELF_INJECTION_GRACE + std::time::Duration::from_millis(50));

        assert_eq!(self_injecting_keys(), None);
        assert!(!injection_overlaps(&["KEY_LEFTCTRL", "KEY_SPACE"]));
    }

    #[test]
    fn a_paste_claims_its_own_keys_while_it_runs() {
        let _lock = locked();
        let guard = begin_self_injection(Shortcut::CtrlV.key_names());

        assert!(
            injection_overlaps(&["KEY_LEFTCTRL", "KEY_SPACE"]),
            "a Ctrl+Space binding shares Ctrl with a Ctrl+V paste"
        );
        drop(guard);
    }

    #[test]
    fn a_binding_that_shares_no_key_is_untouched() {
        // The reason the overlap test exists: suppressing every signal during
        // a paste would hide real releases on unrelated bindings.
        let _lock = locked();
        let guard = begin_self_injection(Shortcut::CtrlV.key_names());

        assert!(
            !injection_overlaps(&["KEY_LEFTMETA", "KEY_SPACE"]),
            "Super+Space has nothing in common with Ctrl+V"
        );
        drop(guard);
    }

    #[test]
    fn the_window_closes_after_the_grace_period() {
        let _lock = locked();
        drop(begin_self_injection(Shortcut::CtrlV.key_names()));

        assert!(
            injection_overlaps(&["KEY_LEFTCTRL"]),
            "the grace period keeps the window open past the send"
        );
        std::thread::sleep(SELF_INJECTION_GRACE + std::time::Duration::from_millis(50));
        assert_eq!(self_injecting_keys(), None);
    }

    #[test]
    fn an_overlapping_second_paste_holds_the_window_open() {
        // Dropping the first guard must not close a window the second one is
        // still inside, or its keys stop being recognised mid-send.
        let _lock = locked();
        let first = begin_self_injection(Shortcut::CtrlV.key_names());
        let second = begin_self_injection(Shortcut::ShiftInsert.key_names());

        drop(first);
        let claimed = self_injecting_keys().expect("still injecting");
        assert!(claimed.contains(&"KEY_INSERT"));
        assert!(
            claimed.contains(&"KEY_LEFTCTRL"),
            "the first paste's keys stay claimed until the window closes"
        );

        drop(second);
    }

    #[test]
    fn stale_keys_are_not_carried_into_the_next_paste() {
        let _lock = locked();
        drop(begin_self_injection(Shortcut::CtrlV.key_names()));
        std::thread::sleep(SELF_INJECTION_GRACE + std::time::Duration::from_millis(50));

        let guard = begin_self_injection(Shortcut::ShiftInsert.key_names());
        let claimed = self_injecting_keys().expect("injecting");
        assert!(
            !claimed.contains(&"KEY_LEFTCTRL"),
            "a finished paste's keys must not leak into the next window"
        );
        drop(guard);
    }
}

#[cfg(test)]
mod program_stem_tests {
    use super::program_stem;

    #[test]
    fn takes_the_lowercased_file_stem() {
        assert_eq!(program_stem(r"C:\Users\me\AppData\Local\slack\app-4.41\Slack.exe").as_deref(), Some("slack"));
        assert_eq!(program_stem(r"C:\Program Files\Mozilla Thunderbird\thunderbird.exe").as_deref(), Some("thunderbird"));
        assert_eq!(program_stem("Code").as_deref(), Some("code"));
        assert_eq!(program_stem(r"C:\dir\").as_deref(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_surrogate_pair_is_never_split_across_chunks() {
        // "😀" is one code point and two UTF-16 units. Delivering them in
        // separate SendInput calls makes the target render two replacement
        // glyphs instead of the emoji.
        let units: Vec<u16> = "aa😀bb".encode_utf16().collect();
        for max in 1..=units.len() + 2 {
            for range in chunk_boundaries(&units, max) {
                if let Some(last) = units[range.clone()].last() {
                    assert!(
                        !(0xD800..0xDC00).contains(last),
                        "max={max}: chunk {range:?} ends on a high surrogate"
                    );
                }
            }
        }
    }

    #[test]
    fn chunking_covers_the_whole_string_exactly_once() {
        let units: Vec<u16> = "the quick brown 🦊 jumps".encode_utf16().collect();
        for max in 1..=units.len() + 2 {
            let rejoined: Vec<u16> = chunk_boundaries(&units, max)
                .iter()
                .flat_map(|r| units[r.clone()].to_vec())
                .collect();
            assert_eq!(rejoined, units, "max={max} lost or duplicated text");
        }
    }

    #[test]
    fn chunking_always_makes_progress() {
        // Regression guard. Shrinking a chunk to keep a surrogate pair together
        // used to be able to shrink it to nothing when the chunk was one unit
        // long, so the loop pushed empty ranges forever and the process was
        // killed for exhausting memory.
        let units: Vec<u16> = "😀😀😀".encode_utf16().collect();
        for max in 1..=4 {
            let ranges = chunk_boundaries(&units, max);
            assert!(
                ranges.iter().all(|r| !r.is_empty()),
                "max={max} produced an empty chunk"
            );
            assert!(ranges.len() <= units.len(), "max={max} did not terminate cleanly");
        }
    }

    #[test]
    fn an_empty_string_produces_no_chunks() {
        assert!(chunk_boundaries(&[], CHUNK_UNITS).is_empty());
    }

    #[test]
    fn the_metacharacters_sendkeys_ate_are_just_text_here() {
        // The regression this crate exists for. These are all SendKeys syntax —
        // `%` alt, `^` ctrl, `+` shift, `~` enter, and the four bracket pairs
        // are grouping — so the old PowerShell path delivered chords and
        // dropped characters. As UTF-16 code units they are unremarkable, and
        // every one survives chunking.
        for sample in ["50% of users", "f(x) = a + b", "array[0]", "{braces}", "a~b^c"] {
            let units: Vec<u16> = sample.encode_utf16().collect();
            let rejoined: Vec<u16> = chunk_boundaries(&units, 4)
                .iter()
                .flat_map(|r| units[r.clone()].to_vec())
                .collect();
            assert_eq!(
                String::from_utf16(&rejoined).unwrap(),
                sample,
                "{sample:?} did not survive"
            );
        }
    }

    #[test]
    fn long_text_prefers_a_paste_and_short_text_does_not() {
        assert!(prefers_paste(&"a".repeat(PASTE_THRESHOLD_CHARS + 1)));
        assert!(!prefers_paste(&"a".repeat(PASTE_THRESHOLD_CHARS)));
        assert!(!prefers_paste("hello"));
    }

    #[test]
    fn shortcut_names_parse_and_auto_means_pick_for_me() {
        assert_eq!(Shortcut::parse("Ctrl+V"), Some(Shortcut::CtrlV));
        assert_eq!(Shortcut::parse("ctrl + shift + v"), Some(Shortcut::CtrlShiftV));
        assert_eq!(Shortcut::parse("Shift+Insert"), Some(Shortcut::ShiftInsert));
        assert_eq!(Shortcut::parse("auto"), None);
        assert_eq!(Shortcut::parse(""), None);
    }

    #[test]
    fn consoles_get_shift_insert_and_ordinary_windows_do_not() {
        assert!(windows_class_wants_shift_insert("ConsoleWindowClass"));
        assert!(windows_class_wants_shift_insert("mintty"));
        assert!(!windows_class_wants_shift_insert("Chrome_WidgetWin_1"));
        assert!(!windows_class_wants_shift_insert("CASCADIA_HOSTING_WINDOW_CLASS"));
    }
}
