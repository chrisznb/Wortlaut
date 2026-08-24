//! One error type for the whole pipeline. The variants exist so the UI can tell
//! "you need to install ffmpeg" apart from "this file is not a video", instead of
//! showing a raw process error to someone who is not a developer.

use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No ffmpeg binary found. Carries the places that were searched so the UI
    /// can show them.
    #[error("ffmpeg was not found (looked in: {searched})")]
    FfmpegMissing { searched: String },

    /// ffmpeg started but exited non zero. `stderr` is the tail of its output.
    #[error("ffmpeg failed ({context}): {stderr}")]
    FfmpegFailed { context: String, stderr: String },

    #[error("file not found: {0}")]
    FileNotFound(PathBuf),

    /// No speech model has been placed in the model directory yet.
    #[error("no speech model installed")]
    ModelMissing,

    #[error("transcription failed: {0}")]
    Transcription(String),

    #[error("could not read the media duration of {0}")]
    UnknownDuration(PathBuf),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Stable machine readable code. The UI maps this onto a translated string
    /// instead of showing the English `Display` text.
    pub fn code(&self) -> &'static str {
        match self {
            Error::FfmpegMissing { .. } => "ffmpeg_missing",
            Error::FfmpegFailed { .. } => "ffmpeg_failed",
            Error::FileNotFound(_) => "file_not_found",
            Error::ModelMissing => "model_missing",
            Error::Transcription(_) => "transcription_failed",
            Error::UnknownDuration(_) => "unknown_duration",
            Error::Io(_) => "io_error",
        }
    }
}
