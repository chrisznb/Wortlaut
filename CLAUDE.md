# Wortlaut — Repo-Kontext

macOS-App, die fertige Kurzvideos lokal mit animierten, wortgenauen Untertiteln
versieht (Submagic-/CapCut-Pro-Alternative, dort 19-39 USD/Monat). Rust/Tauri v2,
React/TS-Frontend, whisper.cpp fuer Wort-Zeitstempel, ffmpeg + libass fuers
Rendern. MIT. Name: "Wortlaut".

## Kernidee
Der Karaoke-Effekt wird NICHT frameweise gerendert, sondern als ASS-Untertitel
mit `\k`-Tags beschrieben und in EINEM ffmpeg-Durchlauf von libass gezeichnet.
Deshalb ist die Pipeline kurz und schnell. Wer hier Frames malen will, hat die
Architektur missverstanden.

## Struktur
- `crates/wortlaut-core` — die ganze Domaenenlogik, ohne Modell und ohne GUI:
  - `ass.rs` — Herzstueck. `build_ass(words, style) -> String`. Zwei Modi:
    `KaraokeFill` (ein Event pro Zeile, `\k` pro Wort, Zeile fuellt sich) und
    `ActiveWord` (ein Event pro Wort, ganze Zeile neu gezeichnet, nur das
    aktive Wort in Highlight-Farbe plus Scale-Pop). Tests liegen daneben.
  - `ffmpeg.rs` — Subprozess-Wrapper. WICHTIG: aus dem Finder gestartete Apps
    haben eine minimale PATH, deshalb werden `/opt/homebrew/bin` usw. direkt
    abgesucht statt auf PATH zu vertrauen. Der `ass=`-Filter bekommt nur den
    Dateinamen, das cwd zeigt aufs Scratch-Verzeichnis — so muss kein
    Benutzerpfad durch die ffmpeg-Filter-Grammatik escaped werden.
  - `pipeline.rs` — Orchestrierung plus Fortschritts-Gewichte, `ScratchDir`
    raeumt sich per Drop selbst weg.
  - `style.rs` — Presets. Groessen sind PROZENT der Videohoehe, nicht Pixel,
    damit 1080p und 4K gleich aussehen. `pipeline` setzt `play_res_*` auf die
    echte Framegroesse.
- `crates/wortlaut-asr` — whisper-rs. Wort-Zeitstempel entstehen durch
  `token_timestamps(true)` + `max_len(1)` + `split_on_word(true)`: whisper gibt
  dann ein Segment PRO WORT aus. Implementiert `Transcriber` aus core.
- `src-tauri` — Tauri-Shell. Alle langsamen Commands ueber
  `tauri::async_runtime::spawn_blocking`, sonst friert das Webview ein.
  Fortschritt via `app.emit("wortlaut://progress", Progress)`.
- `ui` — React + Vite SPA, zweisprachig.

## Build
```bash
cd ui && pnpm install && pnpm build && cd ..   # ZUERST, cargo bettet ui/dist ein
cargo build --release
cargo test -p wortlaut-core
ui/node_modules/.bin/tauri build --bundles app dmg
```
whisper.cpp ist der langsamste Teil. Notausgang, wenn es klemmt:
`cargo build --release --no-default-features -p wortlaut-tauri` — dann meldet
die ASR `asr_disabled`, alles andere laeuft weiter.

## Konventionen
- UI ist ZWEISPRACHIG (de/en) via `ui/src/i18n.ts` + `ui/src/locales/*.json`.
  Jeden neuen UI-String als Key in BEIDEN Dateien anlegen, nie hardcoden. Auch
  Rust darf keine user-facing Texte enthalten: Commands geben stabile Codes
  zurueck (`Error::code()`, `Stage`), die UI uebersetzt sie.
- Keine Emojis, Icons kommen aus `lucide-react`. Keine Gedankenstriche in
  UI-Strings. Code-Kommentare englisch, deutsche UI-Texte in `de.json` mit
  echten Umlauten.
- Kein Next.js, kein Server-Runtime. Vite-SPA, Tauri serviert das Bundle.
- Netzwerk nur fuer den optionalen Modell-Download (`src-tauri/src/models.rs`,
  per System-`curl`). Sonst laeuft alles offline — das ist das Produkt.
- Modelle NIE ins Repo, NIE ins Bundle. Sie liegen in
  `~/Library/Application Support/Wortlaut/models`.

## Fallen, die schon zugeschnappt sind
- **Fontconfig + Bold-Flag**: `Fontname: Avenir Next` zusammen mit `Bold: -1`
  matcht die Bold-**Italic**-Schnitt aus der .ttc, die Untertitel kommen kursiv
  raus. Loesung: expliziter Schnittname (`Avenir Next Demi Bold`) und
  `Bold: 0`. Neue Presets immer einmal rendern und anschauen, nicht nur
  kompilieren.
- **Tauri-Icons muessen RGBA sein.** Mit `-pix_fmt rgba` erzeugen, sonst
  bricht `tauri-build` mit "icon ... is not RGBA" ab.
- **ui/dist muss VOR `cargo build` existieren**, `generate_context!` bettet es
  zur Compile-Zeit ein.
- ffmpegs `testsrc` malt einen Frame-Zaehler ins Bild. Der sieht im Testframe
  aus wie ein Tofu-Kaestchen und ist KEIN Render-Fehler.

## Test
```bash
cargo test -p wortlaut-core                                          # schnell, ohne ffmpeg
cargo test -p wortlaut-core --test render_smoke -- --ignored --nocapture   # echter Burn-in
```
Der Smoke-Test baut sich sein Testvideo selbst und rendert alle Presets durch.

## Bewusst noch offen
- Bounce-/Pop-Animation ueber `\t`-Transforms, Emoji-Rendering, per-Wort-Fonts.
  Das ist laut Recherche der eigentliche Aufwand Richtung Submagic-Niveau.
- Keine Vorschau vor dem Rendern (bisher nur statische Preset-Kacheln).
- Nur macOS getestet. ffmpeg-Suchpfade und die AppleScript-Dateiauswahl sind
  macOS-spezifisch.
- Rotation wird ueber das ffprobe-Rotationsfeld erkannt; exotische
  Display-Matrizen koennen daneben liegen.
