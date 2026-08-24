//! wortlaut-core - the offline captioning pipeline.
//!
//! Stages, in order:
//!   1. [`ffmpeg::extract_audio`]  video -> 16 kHz mono WAV
//!   2. [`Transcriber::transcribe_words`]  WAV -> word level timings
//!   3. [`ass::build_ass`]  words -> an ASS subtitle script with karaoke tags
//!   4. [`ffmpeg::burn_in`]  video + ASS -> a new MP4 with the captions rendered in
//!
//! Everything runs locally. The only process this crate ever starts is ffmpeg,
//! and the only file it writes outside the output path is a scratch WAV plus the
//! generated .ass script inside a temporary working directory.

pub mod ass;
pub mod error;
pub mod ffmpeg;
pub mod pipeline;
pub mod style;
pub mod word;

pub use ass::build_ass;
pub use error::{Error, Result};
pub use ffmpeg::{burn_in, extract_audio};
pub use pipeline::{process_video, Progress, Stage};
pub use style::{HighlightMode, Rgb, StylePreset, SubtitleStyle};
pub use word::{Transcriber, Word};
