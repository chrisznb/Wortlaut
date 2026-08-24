//! Orchestration: one video in, one captioned video out.
//!
//! The stage weights below are guesses at wall clock share, not measurements.
//! They exist so the progress bar moves smoothly instead of sitting at zero for
//! the whole transcription and then jumping.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ass::build_ass;
use crate::error::{Error, Result};
use crate::ffmpeg;
use crate::style::SubtitleStyle;
use crate::word::Transcriber;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Probing,
    ExtractingAudio,
    Transcribing,
    BuildingSubtitles,
    Rendering,
    Done,
}

/// Progress ticket handed to the caller. The UI maps `stage` onto a translated
/// label, so no user facing English travels through here.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub stage: Stage,
    /// 0.0 to 1.0 across the whole job.
    pub percent: f32,
}

// Cumulative start of each stage on the overall 0..1 scale.
const P_PROBE: f32 = 0.00;
const P_EXTRACT: f32 = 0.03;
const P_TRANSCRIBE: f32 = 0.12;
const P_BUILD: f32 = 0.55;
const P_RENDER: f32 = 0.58;

/// Run the full pipeline and return the path of the captioned video.
///
/// Nothing here touches the network. The scratch directory is deleted on the way
/// out, including when a stage fails.
pub fn process_video(
    video: &Path,
    style: &SubtitleStyle,
    transcriber: &dyn Transcriber,
    on_progress: &dyn Fn(Progress),
) -> Result<PathBuf> {
    if !video.is_file() {
        return Err(Error::FileNotFound(video.to_path_buf()));
    }

    let work = ScratchDir::new()?;

    on_progress(Progress { stage: Stage::Probing, percent: P_PROBE });
    let info = ffmpeg::probe_video(video)?;

    on_progress(Progress { stage: Stage::ExtractingAudio, percent: P_EXTRACT });
    let wav = work.path().join("audio.wav");
    ffmpeg::extract_audio(video, &wav)?;

    on_progress(Progress { stage: Stage::Transcribing, percent: P_TRANSCRIBE });
    let words = transcriber.transcribe_words(&wav)?;

    on_progress(Progress { stage: Stage::BuildingSubtitles, percent: P_BUILD });
    // Match the script resolution to the real frame so libass does not have to
    // rescale, which is what keeps the font size honest across 1080p and 4K.
    let mut style = style.clone();
    style.play_res_x = info.width;
    style.play_res_y = info.height;
    let ass = build_ass(&words, &style);
    let ass_path = work.path().join("subs.ass");
    std::fs::write(&ass_path, ass)?;

    on_progress(Progress { stage: Stage::Rendering, percent: P_RENDER });
    let out_path = output_path_for(video, &style.id);
    ffmpeg::burn_in(video, &ass_path, &out_path, info.duration_ms, &|frac| {
        on_progress(Progress {
            stage: Stage::Rendering,
            percent: P_RENDER + (1.0 - P_RENDER) * frac,
        });
    })?;

    on_progress(Progress { stage: Stage::Done, percent: 1.0 });
    Ok(out_path)
}

/// `clip.mov` becomes `clip-subtext.mp4`, next to the original. A numeric suffix
/// is appended rather than overwriting an earlier run.
pub fn output_path_for(video: &Path, _style_id: &str) -> PathBuf {
    let dir = video.parent().unwrap_or_else(|| Path::new("."));
    let stem = video
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "video".to_string());

    let first = dir.join(format!("{stem}-subtext.mp4"));
    if !first.exists() {
        return first;
    }
    for n in 2..1000 {
        let cand = dir.join(format!("{stem}-subtext-{n}.mp4"));
        if !cand.exists() {
            return cand;
        }
    }
    first
}

/// Temporary working directory that removes itself, so a failed run does not
/// leave a multi hundred megabyte WAV behind.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new() -> Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("subtext-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_lands_next_to_the_source_with_an_mp4_extension() {
        let out = output_path_for(Path::new("/tmp/does-not-exist-xyz/clip.mov"), "bold-center");
        assert_eq!(out, PathBuf::from("/tmp/does-not-exist-xyz/clip-subtext.mp4"));
    }
}
