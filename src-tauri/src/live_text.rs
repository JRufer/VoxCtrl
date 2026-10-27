//! Live transcript for the overlay: when to run a mid-recording pass, which
//! audio it covers, and how its results become committed and tentative words.
//!
//! The recording is split into a *committed* head and an uncommitted *tail*.
//! Live passes transcribe only the tail, so each pass stays short however long
//! the user talks. Once the tail is long enough, it is cut at its quietest
//! moment (a pause between words); the audio before the cut is transcribed
//! once more as a *commit* pass, whose text is appended to the committed text
//! and never changes again. Within the tail, words that two live passes in a
//! row agree on are shown as committed too (the LocalAgreement idea used by
//! streaming Whisper systems); the rest are tentative and may still be
//! corrected by the next pass.
//!
//! Everything here is pure (no audio devices, threads or models) so it can be
//! unit-tested directly. `pipeline.rs` drives it.

/// Audio arrives at 16 kHz mono.
pub const SAMPLE_RATE: usize = 16_000;
/// A pass needs at least this much audio (0.5 s)...
pub const MIN_PASS_SAMPLES: usize = SAMPLE_RATE / 2;
/// ...and this much new audio since the last pass (0.3 s)...
pub const STEP_SAMPLES: usize = 4_800;
/// ...and at least this long since the last pass started.
pub const MIN_PASS_INTERVAL_MS: u128 = 400;

/// How long to wait between pass starts, given how long the last pass took.
///
/// Each pass runs the speech model on every core, so passes started back to
/// back keep the whole CPU busy for as long as the user talks, which starves
/// everything else on the machine, the overlay's own rendering included. With
/// at least twice the last pass's duration between starts, the model is busy
/// at most half the time; a fast (GPU) engine still updates every 0.4 s.
pub fn pass_interval_ms(last_pass_ms: u32) -> u128 {
    MIN_PASS_INTERVAL_MS.max(2 * u128::from(last_pass_ms))
}

/// A tail at least this long is cut at its next good pause.
const COMMIT_AFTER: usize = 6 * SAMPLE_RATE;
/// A tail this long is cut at its quietest point even without a clear pause,
/// so a pass never has to transcribe much more than this.
const FORCE_COMMIT_AFTER: usize = 12 * SAMPLE_RATE;
/// The cut leaves at least this much audio in the committed piece...
const MIN_COMMIT_SAMPLES: usize = 2 * SAMPLE_RATE;
/// ...and at least this much after it, so the words being spoken right now
/// stay in the tail.
const KEEP_TAIL_SAMPLES: usize = SAMPLE_RATE;
/// Loudness is compared over 100 ms frames.
const FRAME: usize = SAMPLE_RATE / 10;
/// A frame counts as a pause when its loudness is under this fraction of the
/// tail's median frame loudness.
const PAUSE_RATIO: f32 = 0.35;

/// Whether the coordinator should start a new mid-recording pass now.
///
/// `last_pass_total` is the recording length when the previous pass started.
/// The step is measured on the whole recording, never on the window a pass
/// covers: the window can stop growing (it is capped), and measuring on it is
/// what used to stop live text after the first ten seconds.
///
/// `interval_ms` is the minimum time between pass starts, from
/// [`pass_interval_ms`].
pub fn pass_due(
    total: usize,
    last_pass_total: usize,
    ms_since_last_pass: u128,
    interval_ms: u128,
    in_flight: bool,
) -> bool {
    !in_flight
        && total >= MIN_PASS_SAMPLES
        && total.saturating_sub(last_pass_total) >= STEP_SAMPLES
        && ms_since_last_pass >= interval_ms.max(MIN_PASS_INTERVAL_MS)
}

fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
}

/// Where to cut the tail `audio[tail_start..]` into a committed piece, if it
/// is time to: the middle of its quietest 100 ms frame, when that frame is a
/// real pause, or regardless once the tail has run long.
pub fn plan_commit(audio: &[f32], tail_start: usize) -> Option<usize> {
    let total = audio.len();
    let tail = total.saturating_sub(tail_start);
    if tail < COMMIT_AFTER {
        return None;
    }
    let lo = tail_start + MIN_COMMIT_SAMPLES;
    let hi = total.saturating_sub(KEEP_TAIL_SAMPLES);
    if hi <= lo + FRAME {
        return None;
    }

    let mut frames: Vec<(usize, f32)> = Vec::new();
    let mut at = lo;
    while at + FRAME <= hi {
        frames.push((at, rms(&audio[at..at + FRAME])));
        at += FRAME;
    }
    let (quiet_at, quiet_rms) = frames
        .iter()
        .copied()
        .min_by(|a, b| a.1.total_cmp(&b.1))?;

    let mut levels: Vec<f32> = frames.iter().map(|f| f.1).collect();
    levels.sort_by(f32::total_cmp);
    let median = levels[levels.len() / 2];
    let is_pause = quiet_rms <= median * PAUSE_RATIO;

    (is_pause || tail >= FORCE_COMMIT_AFTER).then_some(quiet_at + FRAME / 2)
}

/// What a mid-recording pass in flight is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassKind {
    /// Transcribes the tail; its words are shown, mostly as tentative.
    Live,
    /// Transcribes `tail_start..cut` once; its words become committed and the
    /// tail then starts at `cut`.
    Commit { cut: usize },
}

/// What the overlay shows: words that will not change, then words that may.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveView {
    pub committed: String,
    pub tentative: String,
}

impl LiveView {
    /// The whole transcript as one string, for overlays that show it whole.
    pub fn full_text(&self) -> String {
        join(&self.committed, &self.tentative)
    }
}

fn join(a: &str, b: &str) -> String {
    match (a.trim(), b.trim()) {
        ("", b) => b.to_string(),
        (a, "") => a.to_string(),
        (a, b) => format!("{a} {b}"),
    }
}

/// How two words compare for agreement: case and surrounding punctuation are
/// ignored, since engines flip those between passes ("release" / "release.").
fn word_key(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// The live transcript of one recording.
#[derive(Debug, Default)]
pub struct LiveSession {
    pub session_id: u64,
    /// Text of the committed pieces so far.
    committed: String,
    /// Where the uncommitted tail begins, in samples.
    tail_start: usize,
    /// The previous live pass's words, for agreement.
    prev_words: Vec<String>,
    /// The pass currently being transcribed, if any.
    in_flight: Option<PassKind>,
    /// The last view sent to the overlay.
    view: LiveView,
}

impl LiveSession {
    pub fn new(session_id: u64) -> Self {
        Self { session_id, ..Self::default() }
    }

    pub fn tail_start(&self) -> usize {
        self.tail_start
    }

    pub fn view(&self) -> &LiveView {
        &self.view
    }

    /// Choose and record the next pass for a recording of `audio.len()`
    /// samples: the audio range it covers and what it is for.
    pub fn start_pass(&mut self, audio: &[f32]) -> (std::ops::Range<usize>, PassKind) {
        let kind = match plan_commit(audio, self.tail_start) {
            Some(cut) => PassKind::Commit { cut },
            None => PassKind::Live,
        };
        let range = match kind {
            PassKind::Commit { cut } => self.tail_start..cut,
            PassKind::Live => self.tail_start..audio.len(),
        };
        self.in_flight = Some(kind);
        (range, kind)
    }

    /// The pass in flight was dropped without a result (for example, the
    /// final transcription replaced it in the queue).
    pub fn abandon_pass(&mut self) {
        self.in_flight = None;
    }

    /// Take the result of the pass in flight and return the new view. An
    /// empty `text` is a valid result (the noise gate found only silence);
    /// a commit pass still moves the tail on.
    pub fn finish_pass(&mut self, text: &str) -> &LiveView {
        let words: Vec<String> = text.split_whitespace().map(str::to_string).collect();
        match self.in_flight.take() {
            Some(PassKind::Commit { cut }) => {
                self.committed = join(&self.committed, text);
                self.tail_start = cut;
                // Until the next live pass covers the new tail, keep showing
                // what the previous one heard after the committed words.
                let rest: Vec<String> = self.prev_words.iter().skip(words.len()).cloned().collect();
                self.prev_words.clear();
                self.view = LiveView { committed: self.committed.clone(), tentative: rest.join(" ") };
            }
            Some(PassKind::Live) | None => {
                let agreed = self
                    .prev_words
                    .iter()
                    .zip(&words)
                    .take_while(|(a, b)| word_key(a) == word_key(b))
                    .count();
                self.view = LiveView {
                    committed: join(&self.committed, &words[..agreed].join(" ")),
                    tentative: words[agreed..].join(" "),
                };
                self.prev_words = words;
            }
        }
        &self.view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: usize = SAMPLE_RATE;

    /// `secs` of "speech" (a loud tone) with silent gaps at the given seconds.
    fn speech(secs: f32, pauses_at: &[f32]) -> Vec<f32> {
        let n = (secs * S as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / S as f32;
                if pauses_at.iter().any(|p| (t - p).abs() < 0.15) {
                    0.0005
                } else {
                    0.3 * (t * 440.0 * std::f32::consts::TAU).sin()
                }
            })
            .collect()
    }

    #[test]
    fn passes_keep_coming_for_a_long_dictation() {
        // Simulate 30 s of audio in 100 ms chunks, with each pass taking
        // 150 ms. Live text used to stop updating at the 10 s mark.
        let chunk = S / 10;
        let (mut total, mut last_total, mut last_ms, mut busy_until) = (0usize, 0usize, 0u128, 0u128);
        let mut pass_times = Vec::new();
        for i in 1..=300u128 {
            total += chunk;
            let now = i * 100;
            let in_flight = now < busy_until;
            if pass_due(total, last_total, now - last_ms, pass_interval_ms(150), in_flight) {
                last_total = total;
                last_ms = now;
                busy_until = now + 150;
                pass_times.push(now);
            }
        }
        assert!(*pass_times.last().unwrap() >= 29_000, "last pass at {:?}", pass_times.last());
        assert!(pass_times.len() >= 50);
    }

    #[test]
    fn no_pass_while_one_is_running_or_too_soon() {
        let gap = MIN_PASS_INTERVAL_MS;
        assert!(!pass_due(5 * S, 0, 1000, gap, true));
        assert!(!pass_due(5 * S, 0, 100, gap, false));
        assert!(!pass_due(5 * S, 5 * S - 100, 1000, gap, false));
        assert!(!pass_due(MIN_PASS_SAMPLES - 1, 0, 1000, gap, false));
        assert!(pass_due(5 * S, 0, 1000, gap, false));
    }

    #[test]
    fn slow_passes_leave_the_cpu_idle_at_least_half_the_time() {
        // A CPU engine taking 700 ms a pass: simulate 30 s of speech and
        // measure how much of that time a pass was running.
        let pass_ms = 700u128;
        let chunk = S / 10;
        let (mut total, mut last_total, mut last_ms, mut busy_until, mut busy) = (0usize, 0usize, 0u128, 0u128, 0u128);
        for i in 1..=300u128 {
            total += chunk;
            let now = i * 100;
            if pass_due(total, last_total, now - last_ms, pass_interval_ms(pass_ms as u32), now < busy_until) {
                last_total = total;
                last_ms = now;
                busy_until = now + pass_ms;
                busy += pass_ms;
            }
        }
        let duty = busy as f64 / 30_000.0;
        assert!(duty <= 0.55, "busy {:.0}% of the time", duty * 100.0);
        assert!(busy >= 10 * pass_ms, "still updates regularly: {busy} ms busy");
    }

    #[test]
    fn fast_passes_keep_the_quick_cadence() {
        assert_eq!(pass_interval_ms(0), MIN_PASS_INTERVAL_MS);
        assert_eq!(pass_interval_ms(120), MIN_PASS_INTERVAL_MS);
        assert_eq!(pass_interval_ms(700), 1400);
    }

    #[test]
    fn short_tail_is_not_cut() {
        assert_eq!(plan_commit(&speech(4.0, &[2.0]), 0), None);
    }

    #[test]
    fn long_tail_is_cut_at_the_pause() {
        let audio = speech(7.0, &[4.2]);
        let cut = plan_commit(&audio, 0).expect("cut");
        assert!((cut as f32 / S as f32 - 4.2).abs() < 0.2, "cut at {}", cut as f32 / S as f32);
    }

    #[test]
    fn cut_is_relative_to_the_tail() {
        let audio = speech(15.0, &[5.0, 12.0]);
        let cut = plan_commit(&audio, 6 * S).expect("cut");
        assert!((cut as f32 / S as f32 - 12.0).abs() < 0.2);
    }

    #[test]
    fn no_pause_waits_until_the_tail_runs_long() {
        assert_eq!(plan_commit(&speech(8.0, &[]), 0), None);
        let cut = plan_commit(&speech(12.5, &[]), 0).expect("forced cut");
        assert!(cut >= MIN_COMMIT_SAMPLES && cut <= 12 * S);
    }

    #[test]
    fn words_two_passes_agree_on_become_committed() {
        let mut s = LiveSession::new(1);
        let audio = vec![0.1; 3 * S];
        s.start_pass(&audio);
        let v = s.finish_pass("let me draft a").clone();
        assert_eq!(v.committed, "");
        assert_eq!(v.tentative, "let me draft a");

        s.start_pass(&audio);
        let v = s.finish_pass("Let me draft a quick note").clone();
        assert_eq!(v.committed, "Let me draft a");
        assert_eq!(v.tentative, "quick note");
        assert_eq!(v.full_text(), "Let me draft a quick note");
    }

    #[test]
    fn a_correction_stays_tentative() {
        let mut s = LiveSession::new(1);
        let audio = vec![0.1; 3 * S];
        s.start_pass(&audio);
        s.finish_pass("push it to thirsty");
        s.start_pass(&audio);
        let v = s.finish_pass("push it to Thursday,").clone();
        assert_eq!(v.committed, "push it to");
        assert_eq!(v.tentative, "Thursday,");
    }

    #[test]
    fn commit_pass_moves_the_tail_and_keeps_text() {
        let mut s = LiveSession::new(1);
        let audio = speech(7.0, &[4.2]);
        let (range, kind) = s.start_pass(&audio[..3 * S]);
        assert_eq!((range, kind), (0..3 * S, PassKind::Live));
        s.finish_pass("one two three four five six");

        let (range, kind) = s.start_pass(&audio);
        let PassKind::Commit { cut } = kind else { panic!("expected a commit pass") };
        assert_eq!(range, 0..cut);
        let v = s.finish_pass("one two three four").clone();
        assert_eq!(v.committed, "one two three four");
        assert_eq!(v.tentative, "five six");
        assert_eq!(s.tail_start(), cut);

        let (range, kind) = s.start_pass(&audio);
        assert_eq!((range.start, kind), (cut, PassKind::Live));
        let v = s.finish_pass("five six seven").clone();
        assert_eq!(v.committed, "one two three four");
        assert_eq!(v.tentative, "five six seven");
    }

    #[test]
    fn silent_commit_still_advances() {
        let mut s = LiveSession::new(1);
        let audio = speech(7.0, &[4.2]);
        let (_, kind) = s.start_pass(&audio);
        let PassKind::Commit { cut } = kind else { panic!() };
        s.finish_pass("");
        assert_eq!(s.tail_start(), cut);
        assert_eq!(s.view().committed, "");
    }
}
