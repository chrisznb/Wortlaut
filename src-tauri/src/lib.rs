//! Tauri glue. Every command that can take more than a few milliseconds runs on
//! `spawn_blocking`, otherwise the whole webview freezes for the length of an
//! ffmpeg encode.
//!
//! Nothing user facing is written in English here: commands return error codes
//! and stage names, and the frontend turns them into translated strings.

mod models;

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use wortlaut_core::{
    ffmpeg, pipeline, style::StylePreset, Progress, SubtitleStyle, Transcriber, Word,
};
use tauri::{AppHandle, Emitter, Manager};

const EVENT_PROGRESS: &str = "wortlaut://progress";
const EVENT_DOWNLOAD: &str = "wortlaut://download";

/// Error shape handed to the frontend. `code` is a stable key the UI translates,
/// `detail` is diagnostic text that is only shown in the expandable details.
#[derive(Debug, Serialize)]
pub struct CommandError {
    code: String,
    detail: String,
}

impl From<wortlaut_core::Error> for CommandError {
    fn from(e: wortlaut_core::Error) -> Self {
        Self {
            code: e.code().to_string(),
            detail: e.to_string(),
        }
    }
}

impl CommandError {
    fn new(code: &str, detail: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            detail: detail.into(),
        }
    }
}

/// Guard so two encodes cannot fight over the same output file.
#[derive(Default)]
struct Busy(Mutex<bool>);

// --- status ---------------------------------------------------------------

#[derive(Debug, Serialize)]
struct AppStatus {
    ffmpeg_available: bool,
    ffmpeg_path: Option<String>,
    model_dir: String,
    installed_model: Option<String>,
    models: Vec<models::ModelStatus>,
}

#[tauri::command]
fn app_status() -> AppStatus {
    let ffmpeg = ffmpeg::resolve_ffmpeg().ok();
    AppStatus {
        ffmpeg_available: ffmpeg.is_some(),
        ffmpeg_path: ffmpeg.map(|p| p.display().to_string()),
        model_dir: models::model_dir().display().to_string(),
        installed_model: models::first_installed().map(|(id, _)| id),
        models: models::statuses(),
    }
}

/// Preset metadata for the picker. Labels stay in the frontend locale files, so
/// only the visual properties travel across the bridge.
#[derive(Debug, Serialize)]
struct StyleInfo {
    id: String,
    base_color: String,
    highlight_color: String,
    uppercase: bool,
    /// ASS numpad alignment, used by the UI to place its preview text.
    alignment: u8,
    max_words_per_line: usize,
}

#[tauri::command]
fn list_styles() -> Vec<StyleInfo> {
    StylePreset::ALL
        .iter()
        .map(|p| {
            let s = p.style();
            StyleInfo {
                id: s.id.clone(),
                base_color: hex(&s.base_color),
                highlight_color: hex(&s.highlight_color),
                uppercase: s.uppercase,
                alignment: s.alignment,
                max_words_per_line: s.max_words_per_line,
            }
        })
        .collect()
}

fn hex(c: &wortlaut_core::Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

// --- file picking ---------------------------------------------------------

#[tauri::command]
async fn pick_video() -> Option<String> {
    tauri::async_runtime::spawn_blocking(pick_video_blocking)
        .await
        .ok()
        .flatten()
}

/// AppleScript instead of a dialog crate: it is the native panel, it needs no
/// extra dependency, and it already speaks the user's language.
fn pick_video_blocking() -> Option<String> {
    let script = r#"POSIX path of (choose file with prompt "Choose a video" of type {"public.movie"})"#;
    let out = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!p.is_empty()).then_some(p)
}

#[tauri::command]
fn reveal_in_finder(path: String) {
    let _ = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .spawn();
}

// --- models ---------------------------------------------------------------

#[tauri::command]
async fn download_model(app: AppHandle, id: String) -> Result<String, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        models::download(&id, &|frac| {
            let _ = app.emit(EVENT_DOWNLOAD, DownloadProgress { percent: frac });
        })
        .map(|p| p.display().to_string())
        .map_err(|e| CommandError::new("download_failed", e))
    })
    .await
    .map_err(|e| CommandError::new("join_failed", e.to_string()))?
}

#[derive(Debug, Clone, Copy, Serialize)]
struct DownloadProgress {
    percent: f32,
}

// --- the actual work ------------------------------------------------------

/// How far the captions may be shifted against the audio, in milliseconds.
/// Negative moves them earlier. Wider than anyone should need, but a clip with
/// a badly muxed audio track can be off by a lot.
const OFFSET_MIN_MS: i64 = -2000;
const OFFSET_MAX_MS: i64 = 2000;

#[tauri::command]
async fn process_video(
    app: AppHandle,
    path: String,
    style_id: String,
    language: String,
    offset_ms: i64,
) -> Result<String, CommandError> {
    {
        let busy = app.state::<Busy>();
        let mut flag = busy.0.lock().map_err(|_| CommandError::new("busy", "lock poisoned"))?;
        if *flag {
            return Err(CommandError::new("busy", "another video is being processed"));
        }
        *flag = true;
    }

    let app_for_job = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        run_job(&app_for_job, &path, &style_id, &language, offset_ms)
    })
    .await;

    if let Ok(mut flag) = app.state::<Busy>().0.lock() {
        *flag = false;
    }

    match result {
        Ok(inner) => inner,
        Err(e) => Err(CommandError::new("join_failed", e.to_string())),
    }
}

fn run_job(
    app: &AppHandle,
    path: &str,
    style_id: &str,
    language: &str,
    offset_ms: i64,
) -> Result<String, CommandError> {
    let mut style: SubtitleStyle = StylePreset::from_id(style_id)
        .ok_or_else(|| CommandError::new("unknown_style", style_id))?
        .style();
    // A caption that lands a moment before the word is read as in sync; one
    // that lands after it is read as late. The preset carries a small lead for
    // that, and this lets the user nudge it when a clip needs more or less.
    style.offset_ms = offset_ms.clamp(OFFSET_MIN_MS, OFFSET_MAX_MS);

    let (_model_id, model_path) =
        models::first_installed().ok_or_else(|| CommandError::new("model_missing", "no model"))?;

    let transcriber = build_transcriber(&model_path, language)?;

    let out = pipeline::process_video(
        &PathBuf::from(path),
        &style,
        transcriber.as_ref(),
        &|p: Progress| {
            let _ = app.emit(EVENT_PROGRESS, p);
        },
    )?;

    Ok(out.display().to_string())
}

#[cfg(feature = "whisper")]
fn build_transcriber(
    model_path: &std::path::Path,
    language: &str,
) -> Result<Box<dyn Transcriber>, CommandError> {
    let t = wortlaut_asr::WhisperTranscriber::load(model_path, language)?;
    Ok(Box::new(t))
}

/// Build without the `whisper` feature: everything except the actual speech
/// recognition still runs, which keeps the pipeline testable on machines where
/// whisper.cpp cannot be compiled. The UI surfaces this as a plain error.
#[cfg(not(feature = "whisper"))]
fn build_transcriber(
    _model_path: &std::path::Path,
    _language: &str,
) -> Result<Box<dyn Transcriber>, CommandError> {
    Err(CommandError::new(
        "asr_disabled",
        "built without the `whisper` feature",
    ))
}

/// Kept so the `Word` import stays meaningful for downstream tooling and so a
/// stub engine is one line away during UI work.
#[allow(dead_code)]
struct NullTranscriber;

impl Transcriber for NullTranscriber {
    fn transcribe_words(&self, _wav: &std::path::Path) -> wortlaut_core::Result<Vec<Word>> {
        Ok(Vec::new())
    }
}

// --- setup ----------------------------------------------------------------

pub fn run() {
    tauri::Builder::default()
        .manage(Busy::default())
        .invoke_handler(tauri::generate_handler![
            app_status,
            list_styles,
            pick_video,
            reveal_in_finder,
            download_model,
            process_video,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Wortlaut");
}
