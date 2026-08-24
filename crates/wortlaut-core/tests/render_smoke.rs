//! End to end check that libass actually renders what `build_ass` writes.
//!
//! Ignored by default because it needs a working ffmpeg and takes a few
//! seconds. Run it with:
//!
//! ```text
//! cargo test -p wortlaut-core --test render_smoke -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::process::Command;

use wortlaut_core::{build_ass, ffmpeg, StylePreset, Word};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("Wortlaut-render-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
#[ignore = "needs ffmpeg on the system"]
fn karaoke_subtitles_burn_into_a_real_video() {
    let ffmpeg_bin = ffmpeg::resolve_ffmpeg().expect("ffmpeg must be installed for this test");

    // A three second 720x1280 clip with a tone, so the pipeline has real pixels
    // and a real audio track to chew on.
    let source = scratch("source.mp4");
    let status = Command::new(&ffmpeg_bin)
        .args([
            "-y", "-v", "error",
            "-f", "lavfi", "-i", "testsrc=size=720x1280:rate=30:duration=3",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=3",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest",
        ])
        .arg(&source)
        .status()
        .unwrap();
    assert!(status.success(), "could not build the test clip");

    let info = ffmpeg::probe_video(&source).expect("probe");
    assert_eq!((info.width, info.height), (720, 1280));
    assert!(info.duration_ms >= 2_900, "duration was {}", info.duration_ms);

    for preset in StylePreset::ALL {
        let mut style = preset.style();
        style.play_res_x = info.width;
        style.play_res_y = info.height;

        let words = vec![
            Word::new("das", 0, 400),
            Word::new("läuft", 400, 900),
            Word::new("komplett", 900, 1500),
            Word::new("lokal", 1500, 2200),
            Word::new("hier", 2400, 2900),
        ];

        let ass_path = scratch(&format!("{}.ass", preset.id()));
        std::fs::write(&ass_path, build_ass(&words, &style)).unwrap();

        let out = scratch(&format!("out-{}.mp4", preset.id()));
        let _ = std::fs::remove_file(&out);
        // The callback is `Fn`, so interior mutability is the way to collect ticks.
        let seen = std::cell::RefCell::new(Vec::<f32>::new());
        ffmpeg::burn_in(&source, &ass_path, &out, info.duration_ms, &|f| {
            seen.borrow_mut().push(f)
        })
        .unwrap_or_else(|e| panic!("burn_in failed for {}: {e}", preset.id()));
        let seen = seen.into_inner();

        let size = std::fs::metadata(&out).unwrap().len();
        assert!(size > 10_000, "{} produced only {size} bytes", preset.id());
        assert!(seen.iter().any(|f| *f >= 1.0), "progress never reached 100 percent");
        println!("{}: {size} bytes, {} progress ticks", preset.id(), seen.len());
    }
}
