//! Limits that stop a speech decoder which has fallen into a loop.
//!
//! Every backend here decodes greedily, one token at a time, and each can
//! reach a state where the likeliest next token is the one it just produced.
//! Nothing in the model notices; left alone it emits the same word until some
//! length limit ends the pass. These are those limits, kept apart from the
//! backends (and from ONNX Runtime) so they can be tested without a model.
//!
//! They are the first line of defence. The second is
//! [`voxctrl_text::collapse_repetition_loops`], which every transcript passes
//! through whatever produced it.

/// Identical tokens emitted in a row on one Parakeet encoder frame before the
/// decoder is taken to be stuck there.
///
/// A TDT decoder may emit several tokens on one 80 ms frame (the duration it
/// predicts for each is zero until it wants to move on), and those may
/// legitimately repeat: the digits of "1000" are the same piece three times.
/// Five of the same piece without the model once asking to advance is past
/// anything speech produces; a stuck decoder runs to the per-frame limit of
/// `parakeet::MAX_TOKENS_PER_STEP` every time.
pub const STUCK_FRAME_REPEATS: usize = 5;

/// Tracks the run of identical tokens at the end of what one frame has
/// emitted so far.
#[derive(Debug, Default)]
pub struct FrameRun {
    token: Option<usize>,
    len: usize,
}

impl FrameRun {
    /// Start a new frame: nothing emitted on it yet.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Record `token` as emitted on this frame. Returns `true` when it starts
    /// a new run, which is when the caller should save the decoder state to
    /// come back to if the run turns out to be a loop.
    pub fn push(&mut self, token: usize) -> bool {
        if self.token == Some(token) {
            self.len += 1;
            false
        } else {
            self.token = Some(token);
            self.len = 1;
            true
        }
    }

    /// How many copies of the current token to take back, once the run is
    /// long enough to be a loop: all but the first. `None` while it is not.
    pub fn stuck_excess(&self) -> Option<usize> {
        (self.len >= STUCK_FRAME_REPEATS).then(|| self.len - 1)
    }
}

/// Tokens Moonshine may emit per second of audio.
///
/// Useful Sensors' own runtime uses 6; Hugging Face's uses 6.5. Fast English
/// speech is about four words, or five to six tokens, a second.
const MOONSHINE_TOKENS_PER_SEC: f32 = 6.5;
/// Added to the per-second allowance so a very short clip still has room for
/// a word, its punctuation and the end-of-text token.
const MOONSHINE_TOKEN_SLACK: usize = 3;

/// The most tokens a Moonshine pass over `n_samples` of 16 kHz audio may
/// emit, never more than `hard_cap`.
///
/// Moonshine has no other brake: without a limit tied to the length of the
/// audio, a half-second clip (every live-text pass starts as one) could run
/// to the full `hard_cap` of one repeated word.
pub fn moonshine_token_limit(n_samples: usize, hard_cap: usize) -> usize {
    let secs = n_samples as f32 / 16_000.0;
    let allowed = (secs * MOONSHINE_TOKENS_PER_SEC).ceil() as usize + MOONSHINE_TOKEN_SLACK;
    allowed.min(hard_cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_that_repeats_a_token_as_speech_does_is_not_stuck() {
        // "1000": one piece, then the same piece three times.
        let mut run = FrameRun::default();
        assert!(run.push(11));
        assert!(run.push(7));
        assert!(!run.push(7));
        assert!(!run.push(7));
        assert_eq!(run.stuck_excess(), None);
    }

    #[test]
    fn a_frame_stuck_on_one_token_gives_back_all_but_the_first() {
        let mut run = FrameRun::default();
        run.push(3);
        for _ in 0..STUCK_FRAME_REPEATS {
            run.push(42);
        }
        assert_eq!(run.stuck_excess(), Some(STUCK_FRAME_REPEATS - 1));
    }

    #[test]
    fn a_different_token_ends_the_run_and_a_new_frame_forgets_it() {
        let mut run = FrameRun::default();
        for _ in 0..STUCK_FRAME_REPEATS - 1 {
            run.push(42);
        }
        assert!(run.push(43), "a new token starts a new run");
        assert_eq!(run.stuck_excess(), None);

        for _ in 0..STUCK_FRAME_REPEATS - 1 {
            run.push(43);
        }
        run.reset();
        assert!(run.push(43), "a new frame starts a new run");
        assert_eq!(run.stuck_excess(), None);
    }

    #[test]
    fn moonshine_may_say_no_more_than_the_audio_has_room_for() {
        // Half a second: a word or two, not 192 tokens of one word.
        assert_eq!(moonshine_token_limit(8_000, 192), 7);
        // Ten seconds of fast speech fits.
        assert_eq!(moonshine_token_limit(160_000, 192), 68);
        // Long audio is still held to the model's own limit.
        assert_eq!(moonshine_token_limit(60 * 16_000, 192), 192);
        assert_eq!(moonshine_token_limit(0, 192), 3);
    }
}
