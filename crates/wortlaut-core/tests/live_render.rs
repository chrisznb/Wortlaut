//! Render a real clip end to end so caption timing can be judged against the
//! actual audio. Ignored by default: needs ffmpeg, the whisper model, and a
//! clip at $WORTLAUT_TEST_VIDEO.
//!
//! ```text
//! WORTLAUT_TEST_VIDEO=/path/clip.mov \
//!   cargo test -p wortlaut-core --release --test live_render -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use wortlaut_core::{process_video, Progress, StylePreset};

#[test]
#[ignore = "needs ffmpeg, a model and WORTLAUT_TEST_VIDEO"]
fn render_a_real_clip() {
    let Ok(video) = std::env::var("WORTLAUT_TEST_VIDEO") else {
        eprintln!("set WORTLAUT_TEST_VIDEO");
        return;
    };
    let video = PathBuf::from(video);
    let model = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Application Support/Wortlaut/models/ggml-large-v3-turbo.bin");
    if !model.is_file() {
        eprintln!("model missing at {}", model.display());
        return;
    }

    let transcriber = wortlaut_asr::WhisperTranscriber::load(&model, "auto").expect("load model");
    let style = StylePreset::BoldCenter.style();
    let out = process_video(&video, &style, &transcriber, &|p: Progress| {
        eprintln!("  {:?} {}%", p.stage, p.percent);
    })
    .expect("render");
    eprintln!("wrote {}", out.display());
}
