//! Whisper model bookkeeping.
//!
//! Models are not shipped with the app: they are large and carry their own
//! licence. They live in the app support directory and are fetched on demand,
//! which is the one and only time subtext touches the network.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The models offered in the UI, smallest first.
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "base",
        file: "ggml-base.bin",
        approx_bytes: 147_951_465,
    },
    ModelSpec {
        id: "small",
        file: "ggml-small.bin",
        approx_bytes: 487_601_967,
    },
    ModelSpec {
        id: "large-v3-turbo",
        file: "ggml-large-v3-turbo.bin",
        approx_bytes: 1_624_555_275,
    },
];

const BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModelSpec {
    pub id: &'static str,
    pub file: &'static str,
    /// Used only to drive the download progress bar.
    pub approx_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub installed: bool,
    pub approx_bytes: u64,
}

pub fn spec(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|m| m.id == id)
}

/// `~/Library/Application Support/subtext/models`
pub fn model_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join("Library/Application Support/subtext/models")
}

pub fn statuses() -> Vec<ModelStatus> {
    CATALOG
        .iter()
        .map(|m| ModelStatus {
            id: m.id.to_string(),
            installed: model_dir().join(m.file).is_file(),
            approx_bytes: m.approx_bytes,
        })
        .collect()
}

/// First installed model, smallest first. `None` means nothing is set up yet.
pub fn first_installed() -> Option<(String, PathBuf)> {
    CATALOG.iter().find_map(|m| {
        let p = model_dir().join(m.file);
        p.is_file().then(|| (m.id.to_string(), p))
    })
}

/// Download one model with the system curl and report progress.
///
/// curl rather than an HTTP crate: no TLS stack to vendor, no extra dependency,
/// and macOS always has it. Progress comes from watching the partial file grow,
/// which avoids parsing curl's progress output across versions.
pub fn download(id: &str, on_progress: &dyn Fn(f32)) -> Result<PathBuf, String> {
    let spec = spec(id).ok_or_else(|| format!("unknown model: {id}"))?;
    let dir = model_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let final_path = dir.join(spec.file);
    if final_path.is_file() {
        on_progress(1.0);
        return Ok(final_path);
    }
    let part_path = dir.join(format!("{}.part", spec.file));
    let url = format!("{BASE_URL}/{}", spec.file);

    let mut child = std::process::Command::new("/usr/bin/curl")
        .args(["-L", "--fail", "--silent", "--show-error", "-o"])
        .arg(&part_path)
        .arg(&url)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                if !status.success() {
                    let _ = std::fs::remove_file(&part_path);
                    return Err("download failed".to_string());
                }
                break;
            }
            None => {
                if let Ok(meta) = std::fs::metadata(&part_path) {
                    let frac = meta.len() as f32 / spec.approx_bytes as f32;
                    on_progress(frac.clamp(0.0, 0.99));
                }
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        }
    }

    std::fs::rename(&part_path, &final_path).map_err(|e| e.to_string())?;
    on_progress(1.0);
    Ok(final_path)
}
