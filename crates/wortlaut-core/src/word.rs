//! Word level transcription results and the trait that produces them.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// A single spoken word with its position in the audio.
///
/// Timestamps are milliseconds from the start of the media. `end_ms` is
/// exclusive and is always clamped to be at least `start_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl Word {
    pub fn new(text: impl Into<String>, start_ms: u64, end_ms: u64) -> Self {
        Self {
            text: text.into(),
            start_ms,
            end_ms: end_ms.max(start_ms),
        }
    }

    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// Anything that can turn a 16 kHz mono WAV file into word level timings.
///
/// Declared here rather than in the ASR crate so that `wortlaut-core` (and its
/// tests) never has to link whisper.cpp. The real implementation lives in
/// `wortlaut-asr`; tests use a stub.
pub trait Transcriber: Send + Sync {
    fn transcribe_words(&self, wav_path: &Path) -> crate::Result<Vec<Word>>;
}

/// Roughly how long a word of this text takes to say, in milliseconds.
/// Speech runs at about 12 to 15 characters per second, so ~75ms per
/// character, with a floor for very short words and a ceiling because no
/// single ordinary word takes longer than about a second and a half.
pub fn plausible_duration_ms(text: &str) -> u64 {
    let chars = text.chars().filter(|c| c.is_alphanumeric()).count().max(1) as u64;
    (chars * 75).clamp(180, 1500)
}

/// Whisper attributes the silence after a word to the word itself: a word
/// followed by a pause is reported as lasting several seconds. That hides the
/// pause from line grouping, so captions get built across silence and appear
/// long before they are spoken.
///
/// This trims each word back to a plausible speaking duration whenever the
/// reported one is far longer, turning the swallowed silence back into a real
/// gap. Timings that already look sane are left untouched.
pub fn trim_trailing_silence(words: &[Word]) -> Vec<Word> {
    words
        .iter()
        .map(|w| {
            let reported = w.end_ms.saturating_sub(w.start_ms);
            let plausible = plausible_duration_ms(&w.text);
            // Only step in when the claim is clearly out of proportion, so
            // slow, drawn out speech survives.
            if reported > plausible * 2 {
                Word {
                    text: w.text.clone(),
                    start_ms: w.start_ms,
                    end_ms: w.start_ms + plausible,
                }
            } else {
                w.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod silence_tests {
    use super::*;

    fn w(text: &str, s: u64, e: u64) -> Word {
        Word { text: text.to_string(), start_ms: s, end_ms: e }
    }

    #[test]
    fn a_word_that_swallowed_a_pause_gets_trimmed() {
        // "Kleiner," reported as 2.14s: real speech plus 1.5s of silence.
        let out = trim_trailing_silence(&[w("Kleiner,", 960, 3100)]);
        let dur = out[0].end_ms - out[0].start_ms;
        assert!(dur < 800, "expected a plausible duration, got {dur}ms");
        assert_eq!(out[0].start_ms, 960, "the start must not move");
    }

    #[test]
    fn normal_timings_are_left_alone() {
        let ws = [w("Aber", 6000, 6380), w("warte", 6380, 6850)];
        let out = trim_trailing_silence(&ws);
        assert_eq!(out[0].end_ms, 6380);
        assert_eq!(out[1].end_ms, 6850);
    }

    #[test]
    fn trimming_reveals_the_gap_that_was_hidden_in_the_word() {
        let ws = [w("Nein.", 8000, 10000), w("Ich", 10000, 10570)];
        let out = trim_trailing_silence(&ws);
        let gap = out[1].start_ms - out[0].end_ms;
        assert!(gap > 800, "the pause must become visible again, got {gap}ms");
    }

    #[test]
    fn a_long_word_is_allowed_to_take_longer() {
        // Long words legitimately take longer than short ones.
        assert!(plausible_duration_ms("Geschwindigkeit") > plausible_duration_ms("ich"));
    }
}
