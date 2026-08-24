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
