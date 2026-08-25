//! Word level transcription with whisper.cpp.
//!
//! The trick for word timings is `max_len(1)` together with `split_on_word`:
//! whisper.cpp then emits one segment per word instead of one per sentence, and
//! each segment carries its own start and end time. `token_timestamps` has to be
//! on for those times to be meaningful.
//!
//! Models are never bundled and never fetched from here. The caller passes a
//! path to a GGML/GGUF whisper model it already has on disk.

use std::path::Path;

use wortlaut_core::{Error, Result, Transcriber, Word};
use whisper_rs::{
    DtwMode, DtwParameters, FullParams, SamplingStrategy, WhisperContext,
    WhisperContextParameters,
};

/// A loaded whisper model.
pub struct WhisperTranscriber {
    ctx: WhisperContext,
    /// BCP 47 style code, or `auto` to let whisper decide per clip.
    language: String,
}

impl WhisperTranscriber {
    /// Load a GGML/GGUF whisper model from disk.
    pub fn load(model_path: &Path, language: &str) -> Result<Self> {
        if !model_path.is_file() {
            return Err(Error::ModelMissing);
        }
        let path = model_path
            .to_str()
            .ok_or_else(|| Error::Transcription("model path is not valid UTF-8".into()))?;

        let mut cparams = WhisperContextParameters::default();
        // DTW alignment instead of the decoder's own token timestamps. Those
        // are guesses spread evenly across a segment: on a clip with wind or
        // traffic they report no pause at all, so captions run through silence.
        // DTW aligns tokens against the encoder's attention and reports where a
        // word really lands. Flash attention has to go: whisper.cpp cannot
        // produce DTW timestamps with it on. Costs encoder speed, buys timing.
        cparams.flash_attn(false);
        // The shipped presets stop at large-v3, and large-v3-turbo has 4 decoder
        // layers instead of 32, so its alignment heads are not those. TopMost is
        // the model agnostic fallback and works for any layer count.
        cparams.dtw_parameters(DtwParameters {
            mode: DtwMode::TopMost { n_top: 2 },
            dtw_mem_size: 1024 * 1024 * 128,
        });

        let ctx = WhisperContext::new_with_params(path, cparams)
            .map_err(|e| Error::Transcription(format!("failed to load model: {e}")))?;

        // An empty choice means "let whisper decide". Passing "auto" through is
        // what makes a clip that switches languages mid-way transcribe in the
        // language actually spoken, instead of forcing every segment into one.
        let language = match language.trim() {
            "" => "auto".to_string(),
            other => other.to_string(),
        };
        Ok(Self { ctx, language })
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe_words(&self, wav_path: &Path) -> Result<Vec<Word>> {
        let pcm = read_wav_16k_mono(wav_path)?;
        if pcm.is_empty() {
            return Ok(Vec::new());
        }

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| Error::Transcription(format!("create_state: {e}")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        // whisper.h: "for auto-detection, set to nullptr, \"\" or \"auto\"".
        // The separate detect_language flag is NOT this: it makes whisper detect
        // the language and then return without transcribing, which produced runs
        // with zero captions.
        params.set_language(Some(&self.language));
        params.set_translate(false);
        // Clips often mix languages, typically German with English terms. A
        // prompt in that shape nudges whisper to keep borrowed words in their
        // own spelling instead of germanising them. It is a hint, not a
        // guarantee: whisper decodes one language per window, so a single
        // English word inside a German sentence can still come out German.
        params.set_initial_prompt(
            "Gesprochener Text, teils Deutsch, teils English. Keep English words in English.",
        );
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_non_speech_tokens(true);
        // One word per segment, with per token timing behind it. This pair is
        // what turns a sentence transcript into caption grade word timings.
        params.set_token_timestamps(true);
        params.set_max_len(1);
        params.set_split_on_word(true);
        params.set_n_threads(recommended_threads());

        state
            .full(params, &pcm)
            .map_err(|e| Error::Transcription(format!("whisper full: {e}")))?;

        let n = state
            .full_n_segments()
            .map_err(|e| Error::Transcription(format!("n_segments: {e}")))?;

        let mut words = Vec::with_capacity(n.max(0) as usize);
        for i in 0..n {
            let text = state
                .full_get_segment_text_lossy(i)
                .unwrap_or_default()
                .trim()
                .to_string();
            if text.is_empty() {
                continue;
            }
            // Segment times are coarse: with max_len(1) whisper still reports
            // the span it decoded in, so a word next to a pause inherits the
            // whole pause. Token times inside the segment are tighter, so use
            // them when they look sane and fall back to the segment otherwise.
            let seg_t0 = state.full_get_segment_t0(i).unwrap_or(0).max(0) as u64 * 10;
            let seg_t1 = state.full_get_segment_t1(i).unwrap_or(0).max(0) as u64 * 10;
            let (mut t0, mut t1) = (seg_t0, seg_t1);
            if let Ok(n_tok) = state.full_n_tokens(i) {
                // Never mix the two clocks. A DTW point and a decoder span
                // measure different things, and taking min/max across both
                // stretches a word to the union of the two, which is how a
                // spoken "okay." ended up three seconds long.
                let mut dtw_lo = u64::MAX;
                let mut dtw_hi = 0u64;
                let mut tok_lo = u64::MAX;
                let mut tok_hi = 0u64;
                for j in 0..n_tok {
                    if let Ok(td) = state.full_get_token_data(i, j) {
                        if td.t_dtw > 0 {
                            let c = td.t_dtw as u64 * 10;
                            dtw_lo = dtw_lo.min(c);
                            dtw_hi = dtw_hi.max(c);
                        }
                        let a = td.t0.max(0) as u64 * 10;
                        let b = td.t1.max(0) as u64 * 10;
                        if b > a {
                            tok_lo = tok_lo.min(a);
                            tok_hi = tok_hi.max(b);
                        }
                    }
                }
                if dtw_lo != u64::MAX {
                    // DTW marks where a token starts, so the span between the
                    // first and last token of a word is a lower bound on its
                    // length. A one token word gets a zero length span here;
                    // settle_durations gives it a readable one later.
                    t0 = dtw_lo;
                    t1 = dtw_hi.max(dtw_lo);
                } else if tok_lo != u64::MAX && tok_hi > tok_lo {
                    t0 = tok_lo;
                    t1 = tok_hi;
                }
            }
            words.push(Word::new(text, t0, t1));
        }

        Ok(clamp_overlaps(words))
    }
}

/// Whisper occasionally emits a segment that ends after the next one starts, or
/// a zero length one. Both make the karaoke timing jitter, so tidy them here
/// rather than in the ASS writer.
fn clamp_overlaps(mut words: Vec<Word>) -> Vec<Word> {
    for i in 0..words.len() {
        if let Some(next_start) = words.get(i + 1).map(|w| w.start_ms) {
            if words[i].end_ms > next_start {
                words[i].end_ms = next_start;
            }
        }
        if words[i].end_ms <= words[i].start_ms {
            words[i].end_ms = words[i].start_ms + 80;
        }
    }
    words
}

fn recommended_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
        .clamp(1, 8)
}

/// Read the WAV that `wortlaut_core::ffmpeg::extract_audio` produced.
///
/// ffmpeg is asked for 16 kHz mono PCM, so this only has to handle that shape
/// plus float WAVs, and it fails loudly rather than resampling silently wrong.
fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let reader = hound::WavReader::open(path)
        .map_err(|e| Error::Transcription(format!("cannot read {}: {e}", path.display())))?;
    let spec = reader.spec();

    if spec.channels != 1 || spec.sample_rate != 16_000 {
        return Err(Error::Transcription(format!(
            "expected 16 kHz mono audio, got {} Hz with {} channel(s)",
            spec.sample_rate, spec.channels
        )));
    }

    let samples: std::result::Result<Vec<f32>, hound::Error> = match spec.sample_format {
        hound::SampleFormat::Float => reader.into_samples::<f32>().collect(),
        hound::SampleFormat::Int => reader
            .into_samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32768.0))
            .collect(),
    };

    samples.map_err(|e| Error::Transcription(format!("cannot decode audio: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_segments_are_trimmed_to_the_next_start() {
        let words = clamp_overlaps(vec![
            Word::new("eins", 0, 600),
            Word::new("zwei", 400, 400),
            Word::new("drei", 900, 1200),
        ]);
        assert_eq!(words[0].end_ms, 400, "first word must not run into the second");
        assert_eq!(words[1].end_ms, 480, "zero length words get a minimum duration");
        assert_eq!(words[2].end_ms, 1200);
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::Transcriber;

    /// Real transcription against the installed model. Ignored by default
    /// because it needs the model on disk and takes a few seconds.
    ///
    /// ```text
    /// cargo test -p wortlaut-asr --release -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs the whisper model and a wav at /tmp/wl-test.wav"]
    fn auto_language_still_returns_words() {
        let model = dirs_model_path();
        let wav = std::path::Path::new("/tmp/wl-test.wav");
        if !model.is_file() || !wav.is_file() {
            eprintln!("skipping: model or wav missing");
            return;
        }
        let t = WhisperTranscriber::load(&model, "auto").expect("load");
        let words = t.transcribe_words(wav).expect("transcribe");
        eprintln!("got {} words", words.len());
        for w in words.iter().take(8) {
            eprintln!("  {:>6}..{:<6} {}", w.start_ms, w.end_ms, w.text);
        }
        assert!(!words.is_empty(), "auto language produced no words at all");
        assert!(
            words.iter().any(|w| w.end_ms > w.start_ms),
            "words carry no duration"
        );
    }

    fn dirs_model_path() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Library/Application Support/Wortlaut/models/ggml-large-v3-turbo.bin")
    }
}

#[cfg(test)]
mod dump_tests {
    use super::*;
    use crate::Transcriber;

    /// Dump real word timings so caption timing can be inspected against audio.
    #[test]
    #[ignore = "needs model + /tmp/wl-test.wav"]
    fn dump_word_timings() {
        let model = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Library/Application Support/Wortlaut/models/ggml-large-v3-turbo.bin");
        let wav = std::path::Path::new("/tmp/wl-test.wav");
        if !model.is_file() || !wav.is_file() {
            return;
        }
        let t = WhisperTranscriber::load(&model, "auto").unwrap();
        let words = t.transcribe_words(wav).unwrap();
        let json: Vec<String> = words
            .iter()
            .map(|w| format!("{{\"t\":\"{}\",\"s\":{},\"e\":{}}}", w.text.replace('"', ""), w.start_ms, w.end_ms))
            .collect();
        std::fs::write("/tmp/wl-words.json", format!("[{}]", json.join(","))).unwrap();
        eprintln!("wrote {} words", words.len());
    }
}
