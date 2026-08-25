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

/// DTW reports where a word *starts*, not how long it lasts, so a word can
/// arrive with a zero length span. This gives every word a duration that is
/// long enough to read but never runs into the next word, which is what keeps
/// a pause on screen as a pause.
///
/// Two things are fixed here. A word that claims far more time than it could
/// possibly take is cut back, so the silence it swallowed becomes a real gap
/// again. A word with no measurable length is stretched to a readable one, but
/// only into time that is actually free before the next word begins.
pub fn settle_durations(words: &[Word]) -> Vec<Word> {
    words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let plausible = plausible_duration_ms(&w.text);
            // Room available before the next word starts. The last word has no
            // neighbour, so it may take the full plausible span.
            let room = words
                .get(i + 1)
                .map(|nx| nx.start_ms.saturating_sub(w.start_ms))
                .unwrap_or(u64::MAX);
            let reported = w.end_ms.saturating_sub(w.start_ms);
            // Trust a reported length that is in proportion; otherwise fall
            // back to what the text itself suggests.
            let wanted = if reported > 0 && reported <= plausible * 2 {
                reported.max(MIN_WORD_MS)
            } else {
                plausible
            };
            Word {
                text: w.text.clone(),
                start_ms: w.start_ms,
                end_ms: w.start_ms + wanted.min(room).max(1),
            }
        })
        .collect()
}

/// Shortest a single word may stay on screen. Below this it reads as a flicker.
const MIN_WORD_MS: u64 = 180;

#[cfg(test)]
mod silence_tests {
    use super::*;

    fn w(text: &str, s: u64, e: u64) -> Word {
        Word { text: text.to_string(), start_ms: s, end_ms: e }
    }

    #[test]
    fn a_word_that_swallowed_a_pause_gets_trimmed() {
        // "Kleiner," reported as 2.14s: real speech plus 1.5s of silence.
        let out = settle_durations(&[w("Kleiner,", 960, 3100)]);
        let dur = out[0].end_ms - out[0].start_ms;
        assert!(dur < 800, "expected a plausible duration, got {dur}ms");
        assert_eq!(out[0].start_ms, 960, "the start must not move");
    }

    #[test]
    fn normal_timings_are_left_alone() {
        let ws = [w("Aber", 6000, 6380), w("warte", 6380, 6850)];
        let out = settle_durations(&ws);
        assert_eq!(out[0].end_ms, 6380);
        assert_eq!(out[1].end_ms, 6850);
    }

    #[test]
    fn trimming_reveals_the_gap_that_was_hidden_in_the_word() {
        let ws = [w("Nein.", 8000, 10000), w("Ich", 10000, 10570)];
        let out = settle_durations(&ws);
        let gap = out[1].start_ms - out[0].end_ms;
        assert!(gap > 800, "the pause must become visible again, got {gap}ms");
    }

    #[test]
    fn a_long_word_is_allowed_to_take_longer() {
        // Long words legitimately take longer than short ones.
        assert!(plausible_duration_ms("Geschwindigkeit") > plausible_duration_ms("ich"));
    }

    #[test]
    fn a_zero_length_dtw_word_becomes_readable() {
        // DTW marks a start; a single token word arrives with no span at all.
        let out = settle_durations(&[w("Komm", 5040, 5040)]);
        let dur = out[0].end_ms - out[0].start_ms;
        assert!(dur >= MIN_WORD_MS, "expected a readable duration, got {dur}ms");
    }

    #[test]
    fn a_word_never_eats_into_the_next_one() {
        // Two words 200ms apart: the first must stop before the second starts,
        // even though its text alone suggests a longer span.
        let out = settle_durations(&[w("Wahnsinn", 1000, 1000), w("ja", 1200, 1400)]);
        assert!(
            out[0].end_ms <= out[1].start_ms,
            "{} ran into {}",
            out[0].end_ms,
            out[1].start_ms
        );
    }

    #[test]
    fn a_pause_after_a_word_survives_as_a_gap() {
        // "noch." at 7.88s, next word at 10.18s: a 2.3s pause must stay a pause.
        let out = settle_durations(&[w("noch.", 7880, 10000), w("Nein.", 10180, 10380)]);
        let gap = out[1].start_ms - out[0].end_ms;
        assert!(gap > 1500, "pause collapsed to {gap}ms");
    }
}
