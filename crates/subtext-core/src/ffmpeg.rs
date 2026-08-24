//! Thin wrappers around the system ffmpeg binary.
//!
//! ffmpeg is called as a subprocess rather than linked, so subtext stays MIT and
//! the user keeps control over which build (and which codecs) they run.
//!
//! macOS detail that matters: an app launched from Finder inherits a minimal
//! PATH of `/usr/bin:/bin:/usr/sbin:/sbin`, so a Homebrew ffmpeg is invisible to
//! it. [`resolve_ffmpeg`] therefore probes the known install locations directly
//! instead of trusting PATH.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// Where a Homebrew, MacPorts or hand installed ffmpeg usually lives.
const SEARCH_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/opt/local/bin",
    "/usr/bin",
];

/// Override for both binaries, useful for tests and for bundling later.
const ENV_OVERRIDE: &str = "SUBTEXT_FFMPEG_DIR";

/// What we need to know about the source video before generating subtitles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
}

pub fn resolve_ffmpeg() -> Result<PathBuf> {
    resolve_binary("ffmpeg")
}

pub fn resolve_ffprobe() -> Result<PathBuf> {
    resolve_binary("ffprobe")
}

/// True when ffmpeg is installed. The UI calls this on startup so it can point
/// the user at the install instructions before they drop a file.
pub fn ffmpeg_available() -> bool {
    resolve_ffmpeg().is_ok()
}

fn resolve_binary(name: &str) -> Result<PathBuf> {
    let mut searched: Vec<String> = Vec::new();

    if let Ok(dir) = std::env::var(ENV_OVERRIDE) {
        let cand = PathBuf::from(&dir).join(name);
        if cand.is_file() {
            return Ok(cand);
        }
        searched.push(cand.display().to_string());
    }

    for dir in SEARCH_DIRS {
        let cand = PathBuf::from(dir).join(name);
        if cand.is_file() {
            return Ok(cand);
        }
        searched.push(cand.display().to_string());
    }

    // Last resort: whatever PATH says, which works when started from a shell.
    if let Ok(out) = Command::new("/usr/bin/which").arg(name).output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() && Path::new(&p).is_file() {
                return Ok(PathBuf::from(p));
            }
        }
    }
    searched.push("PATH".to_string());

    Err(Error::FfmpegMissing {
        searched: searched.join(", "),
    })
}

/// Read frame size and duration. Falls back to a reduced query for older
/// ffprobe builds that reject the side data selector.
pub fn probe_video(video: &Path) -> Result<VideoInfo> {
    if !video.is_file() {
        return Err(Error::FileNotFound(video.to_path_buf()));
    }
    let ffprobe = resolve_ffprobe()?;

    let full = &[
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height:stream_tags=rotate:stream_side_data=rotation:format=duration",
        "-of",
        "default=noprint_wrappers=1",
    ][..];
    let reduced = &[
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height:format=duration",
        "-of",
        "default=noprint_wrappers=1",
    ][..];

    let mut text = run_capture(&ffprobe, full, video).unwrap_or_default();
    if text.trim().is_empty() {
        text = run_capture(&ffprobe, reduced, video)?;
    }

    let mut width = 0u32;
    let mut height = 0u32;
    let mut duration_ms = 0u64;
    let mut rotation = 0i64;

    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "width" => width = value.parse().unwrap_or(0),
            "height" => height = value.parse().unwrap_or(0),
            "duration" if duration_ms == 0 => {
                duration_ms = (value.parse::<f64>().unwrap_or(0.0) * 1000.0).round() as u64;
            }
            // Both spellings appear depending on ffprobe version.
            "rotation" | "TAG:rotate" | "rotate" => {
                rotation = value.parse::<f64>().unwrap_or(0.0).round() as i64;
            }
            _ => {}
        }
    }

    if width == 0 || height == 0 {
        return Err(Error::UnknownDuration(video.to_path_buf()));
    }
    // ffmpeg auto rotates on decode, so a quarter turn swaps the frame we render into.
    if rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut width, &mut height);
    }

    Ok(VideoInfo {
        width,
        height,
        duration_ms,
    })
}

fn run_capture(bin: &Path, args: &[&str], file: &Path) -> Result<String> {
    let out = Command::new(bin).args(args).arg(file).output()?;
    if !out.status.success() {
        return Err(Error::FfmpegFailed {
            context: "probe".to_string(),
            stderr: tail(&String::from_utf8_lossy(&out.stderr)),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Decode the audio track to the 16 kHz mono PCM that whisper.cpp expects.
pub fn extract_audio(video: &Path, out_wav: &Path) -> Result<()> {
    if !video.is_file() {
        return Err(Error::FileNotFound(video.to_path_buf()));
    }
    let ffmpeg = resolve_ffmpeg()?;

    let out = Command::new(&ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(video)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
        .arg(out_wav)
        .output()?;

    if !out.status.success() {
        return Err(Error::FfmpegFailed {
            context: "extract_audio".to_string(),
            stderr: tail(&String::from_utf8_lossy(&out.stderr)),
        });
    }
    Ok(())
}

/// Render the ASS script into the video with libass and write a new MP4.
///
/// `on_progress` receives a value between 0.0 and 1.0 whenever ffmpeg reports a
/// new position. It is never called with a value above 1.0.
pub fn burn_in(
    video: &Path,
    ass_file: &Path,
    out_mp4: &Path,
    total_duration_ms: u64,
    on_progress: &dyn Fn(f32),
) -> Result<()> {
    if !video.is_file() {
        return Err(Error::FileNotFound(video.to_path_buf()));
    }
    if !ass_file.is_file() {
        return Err(Error::FileNotFound(ass_file.to_path_buf()));
    }
    let ffmpeg = resolve_ffmpeg()?;

    // The ass filter argument is parsed by ffmpeg's filter grammar, where a
    // colon separates options and a backslash escapes. Rather than escape an
    // arbitrary user path, run with the working directory set to the scratch
    // folder and reference the file by its (known, simple) name.
    let work_dir = ass_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let ass_name = ass_file
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "subs.ass".to_string());

    let mut child = Command::new(&ffmpeg)
        .current_dir(&work_dir)
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-progress",
            "pipe:1",
            "-nostats",
            "-i",
        ])
        .arg(video)
        .arg("-vf")
        .arg(format!("ass={ass_name}"))
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "20",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "copy",
            "-movflags",
            "+faststart",
        ])
        .arg(out_mp4)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Drain stderr on a side thread; a full pipe would otherwise deadlock the
    // encode as soon as ffmpeg writes more than the buffer holds.
    let mut stderr = child.stderr.take();
    let err_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(std::result::Result::ok) {
            // `out_time` is the only unambiguous field: `out_time_ms` is
            // microseconds in most builds despite the name.
            if let Some(v) = line.strip_prefix("out_time=") {
                if total_duration_ms > 0 {
                    if let Some(pos) = parse_timecode_ms(v.trim()) {
                        let frac = pos as f32 / total_duration_ms as f32;
                        on_progress(frac.clamp(0.0, 1.0));
                    }
                }
            }
        }
    }

    let status = child.wait()?;
    let stderr_text = err_handle.join().unwrap_or_default();
    if !status.success() {
        return Err(Error::FfmpegFailed {
            context: "burn_in".to_string(),
            stderr: tail(&stderr_text),
        });
    }
    on_progress(1.0);
    Ok(())
}

/// `HH:MM:SS.micros` as printed by `-progress`, into milliseconds.
fn parse_timecode_ms(tc: &str) -> Option<u64> {
    let mut parts = tc.split(':');
    let h: u64 = parts.next()?.trim().parse().ok()?;
    let m: u64 = parts.next()?.trim().parse().ok()?;
    let s: f64 = parts.next()?.trim().parse().ok()?;
    Some(((h * 3600 + m * 60) as f64 * 1000.0 + s * 1000.0).round() as u64)
}

/// Keep error messages short enough to show in a dialog.
fn tail(s: &str) -> String {
    let trimmed = s.trim();
    let lines: Vec<&str> = trimmed.lines().rev().take(6).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecodes_from_ffmpeg_progress_parse_to_milliseconds() {
        assert_eq!(parse_timecode_ms("00:00:00.000000"), Some(0));
        assert_eq!(parse_timecode_ms("00:00:12.500000"), Some(12_500));
        assert_eq!(parse_timecode_ms("01:02:03.250000"), Some(3_723_250));
        assert_eq!(parse_timecode_ms("N/A"), None);
    }
}
