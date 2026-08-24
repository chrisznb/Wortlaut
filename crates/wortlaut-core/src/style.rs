//! Caption look and feel.
//!
//! Sizes are stored as a percentage of the video height, not as pixels, so a
//! preset renders the same on a 1080p reel and on a 4K clip. The absolute ASS
//! values are computed in [`ass::build_ass`] from `play_res_y`.

use serde::{Deserialize, Serialize};

/// Plain sRGB colour. ASS stores colours as `&HAABBGGRR`, so the byte order is
/// reversed on the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Style line form, with an alpha byte. `alpha` is ASS transparency:
    /// 0 is fully opaque, 255 fully transparent.
    pub fn to_ass_style(self, alpha: u8) -> String {
        format!("&H{:02X}{:02X}{:02X}{:02X}", alpha, self.b, self.g, self.r)
    }

    /// Inline override form, as used by `\1c` and friends.
    pub fn to_ass_inline(self) -> String {
        format!("&H{:02X}{:02X}{:02X}&", self.b, self.g, self.r)
    }
}

/// How the currently spoken word is picked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightMode {
    /// Classic ASS karaoke. One event per caption line; every word carries a
    /// `\k` tag. Words start in SecondaryColour and flip to PrimaryColour as
    /// they are spoken, so the line fills up from left to right.
    KaraokeFill,
    /// One event per word. The whole line is redrawn for each word and only the
    /// word being spoken carries the highlight colour and the scale pop, which
    /// is the look people know from Submagic and CapCut. The spoken duration is
    /// still written as a `\k` tag on the active word.
    ActiveWord,
}

/// Everything `build_ass` needs to render a caption track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubtitleStyle {
    /// Stable preset id. Used by the UI and by output file naming.
    pub id: String,
    /// Font family name as macOS knows it. Not bundled, must exist on the system.
    pub font: String,
    /// Font size as a percentage of the video height.
    pub font_size_pct: f32,
    /// Outline thickness as a percentage of the video height.
    pub outline_pct: f32,
    /// Drop shadow offset as a percentage of the video height.
    pub shadow_pct: f32,
    /// Distance from the frame edge as a percentage of the video height.
    pub margin_v_pct: f32,
    /// Left and right margin as a percentage of the video width.
    pub margin_h_pct: f32,
    pub base_color: Rgb,
    pub highlight_color: Rgb,
    pub outline_color: Rgb,
    pub bold: bool,
    /// Shout case. Common for reels, and it hides whisper's inconsistent casing.
    pub uppercase: bool,
    /// ASS numpad alignment: 1 to 3 bottom, 4 to 6 middle, 7 to 9 top.
    pub alignment: u8,
    /// Scale of the active word in percent. 100 disables the pop.
    pub highlight_scale: u32,
    pub max_words_per_line: usize,
    /// A silence longer than this starts a new caption line.
    pub max_gap_ms: u64,
    /// Hard cap on how long one caption line may stay on screen.
    pub max_line_ms: u64,
    /// Floor for how long a caption line stays readable. Fast speech produces
    /// word groups of a few hundred milliseconds, which flash by unread, so a
    /// short group is merged with the next one and its dialogue event is held
    /// open into the following pause when there is room.
    pub min_line_ms: u64,
    pub highlight: HighlightMode,
    /// Script resolution. The pipeline overwrites this with the real video size
    /// so libass does not have to guess a scale factor.
    pub play_res_x: u32,
    pub play_res_y: u32,
}

/// The presets the UI offers. Adding a variant is the only thing needed to add a
/// preset, the UI reads the list from the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StylePreset {
    BoldCenter,
    MinimalBottom,
    KaraokeLine,
}

impl StylePreset {
    pub const ALL: [StylePreset; 3] = [
        StylePreset::BoldCenter,
        StylePreset::MinimalBottom,
        StylePreset::KaraokeLine,
    ];

    pub fn id(self) -> &'static str {
        match self {
            StylePreset::BoldCenter => "bold-center",
            StylePreset::MinimalBottom => "minimal-bottom",
            StylePreset::KaraokeLine => "karaoke-line",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn style(self) -> SubtitleStyle {
        match self {
            // Big, shouty, dead centre. Three words at a time, active word in
            // amber with a slight pop. The default reel look.
            StylePreset::BoldCenter => SubtitleStyle {
                id: self.id().to_string(),
                font: "Arial Black".to_string(),
                font_size_pct: 6.4,
                outline_pct: 0.55,
                shadow_pct: 0.0,
                margin_v_pct: 0.0,
                margin_h_pct: 8.0,
                base_color: Rgb::new(0xFF, 0xFF, 0xFF),
                highlight_color: Rgb::new(0xFF, 0xD3, 0x4D),
                outline_color: Rgb::new(0x0A, 0x0A, 0x0A),
                bold: true,
                uppercase: true,
                alignment: 5,
                highlight_scale: 112,
                max_words_per_line: 3,
                max_gap_ms: 700,
                max_line_ms: 4000,
                min_line_ms: 1000,
                highlight: HighlightMode::ActiveWord,
                play_res_x: 1080,
                play_res_y: 1920,
            },
            // Quiet lower third. Longer lines, no scale pop, teal accent.
            StylePreset::MinimalBottom => SubtitleStyle {
                id: self.id().to_string(),
                font: "Helvetica Neue".to_string(),
                font_size_pct: 4.2,
                outline_pct: 0.35,
                shadow_pct: 0.15,
                margin_v_pct: 8.0,
                margin_h_pct: 10.0,
                base_color: Rgb::new(0xF7, 0xF6, 0xF2),
                highlight_color: Rgb::new(0x7F, 0xD8, 0xC4),
                outline_color: Rgb::new(0x14, 0x1A, 0x19),
                bold: false,
                uppercase: false,
                alignment: 2,
                highlight_scale: 100,
                max_words_per_line: 6,
                max_gap_ms: 900,
                max_line_ms: 5000,
                min_line_ms: 1000,
                highlight: HighlightMode::ActiveWord,
                play_res_x: 1080,
                play_res_y: 1920,
            },
            // True karaoke: the line fills up left to right and stays filled.
            StylePreset::KaraokeLine => SubtitleStyle {
                id: self.id().to_string(),
                // Named weight instead of "Avenir Next" plus the bold flag:
                // fontconfig resolves that combination to the Bold Italic face
                // out of the .ttc and the captions come out slanted.
                font: "Avenir Next Demi Bold".to_string(),
                font_size_pct: 5.0,
                outline_pct: 0.45,
                shadow_pct: 0.0,
                margin_v_pct: 10.0,
                margin_h_pct: 9.0,
                base_color: Rgb::new(0xF7, 0xF6, 0xF2),
                highlight_color: Rgb::new(0x2A, 0xC3, 0xA6),
                outline_color: Rgb::new(0x10, 0x14, 0x13),
                bold: false,
                uppercase: false,
                alignment: 2,
                highlight_scale: 100,
                max_words_per_line: 5,
                max_gap_ms: 900,
                max_line_ms: 5000,
                min_line_ms: 1000,
                highlight: HighlightMode::KaraokeFill,
                play_res_x: 1080,
                play_res_y: 1920,
            },
        }
    }
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        StylePreset::BoldCenter.style()
    }
}
