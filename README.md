# subtext

Animated, word by word captions for short form video. Drop a clip in, get the
same clip back with karaoke style subtitles burned in. Everything runs on your
machine: no upload, no account, no subscription.

Submagic, CapCut Pro and Captions charge 19 to 39 USD per month for this.
subtext is MIT licensed and costs nothing.

Status: early. The pipeline works end to end; the visual polish (bounce
animation, emoji rendering, per word font control) is not there yet.

## What it does

1. Pulls the audio out of your video with ffmpeg.
2. Transcribes it locally with whisper.cpp, asking for word level timestamps.
3. Writes an ASS subtitle script with karaoke (`\k`) timing, so each word
   lights up exactly when it is spoken.
4. Burns the subtitles into a new MP4 with ffmpeg and libass.

The original file is never modified. The result is written next to it as
`yourclip-subtext.mp4`.

Three presets ship today:

| Preset | Look |
| --- | --- |
| Bold Center | Three words, dead centre, active word pops in amber |
| Minimal Bottom | Quiet lower third, longer lines, teal accent |
| Karaoke Line | The line fills up left to right as it is spoken |

## Requirements

- macOS 13 or newer, Apple Silicon or Intel
- **ffmpeg**, installed separately: `brew install ffmpeg`
  subtext calls the ffmpeg binary rather than linking it, so you stay in
  control of which build and which codecs you run. It looks in
  `/opt/homebrew/bin`, `/usr/local/bin`, `/opt/local/bin` and `/usr/bin`, and
  you can override the location with the `SUBTEXT_FFMPEG_DIR` environment
  variable.
- **A whisper model.** Models are not bundled: they are large and carry their
  own licence. On first launch subtext offers to download one from the
  whisper.cpp model repository into
  `~/Library/Application Support/subtext/models`. That download is the only
  network request subtext ever makes. Nothing else leaves your machine.

## Install

Grab the latest `.dmg` from the releases page, drag subtext to Applications,
install ffmpeg, launch, and let it fetch a model.

If macOS reports the app as damaged, it is unsigned rather than broken:

```bash
xattr -cr /Applications/subtext.app
```

## Build from source

You need Rust (1.82 or newer), Node 20 or newer, pnpm, and ffmpeg.

```bash
git clone <this repo> subtext
cd subtext

# Frontend first: the Rust build embeds ui/dist at compile time.
cd ui && pnpm install && pnpm build && cd ..

cargo build --release          # all crates
cargo test -p subtext-core     # subtitle generation tests
```

Bundle the macOS app:

```bash
ui/node_modules/.bin/tauri build --bundles app dmg
```

whisper.cpp is compiled as part of `subtext-asr` and is by far the slowest part
of a cold build. If it will not build on your machine, everything else still
compiles:

```bash
cargo build --release --no-default-features -p subtext-tauri
```

Speech recognition then reports `asr_disabled` and the rest of the pipeline
(audio extraction, subtitle generation, rendering) stays intact.

## Tests

```bash
cargo test -p subtext-core          # subtitle generation, fast, no ffmpeg needed
cargo test -p subtext-core --test render_smoke -- --ignored --nocapture
```

The second one builds its own test clip and renders every preset through
ffmpeg, so it needs ffmpeg installed and takes a few seconds.

## Layout

```
crates/subtext-core   Word timings, ASS generation, ffmpeg wrappers, pipeline
crates/subtext-asr    whisper.cpp binding, implements the Transcriber trait
src-tauri             Tauri v2 shell, commands, progress events
ui                    React + Vite frontend, English and German
```

`subtext-core` has no model and no GUI dependency, which is why its tests run in
under a second.

## Privacy

There is no telemetry, no analytics and no account. The only outbound request
is the optional model download, and you can skip it entirely by placing a
`ggml-*.bin` file in the model directory yourself.

## License

MIT, see [LICENSE](LICENSE).

subtext calls ffmpeg (LGPL/GPL depending on your build) as an external process
and does not link or redistribute it. whisper.cpp is MIT. Whisper models are
released by OpenAI under MIT and are downloaded from the whisper.cpp model
repository at runtime, not shipped with this software.
