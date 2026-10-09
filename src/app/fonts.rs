//! Lazy font loading.
//!
//! The big Noto fallback fonts (JP/KR/SC/Cuneiform) are embedded gzip-compressed and are only
//! decompressed and registered with egui when text containing their scripts is about to be
//! rendered. Roboto is always loaded as the primary UI font.
//!
//! Fonts are registered as `FontPriority::Lowest` fallbacks of the `Proportional` family, so
//! Latin text keeps rendering with Roboto and only missing glyphs come from the Noto fonts.

use dashmap::DashSet;
use eframe::egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use eframe::egui::{Context, FontData, FontFamily};
use flate2::read::GzDecoder;
use std::io::Read;
use std::sync::{Arc, OnceLock};
use tokio::runtime::Runtime;

use crate::api::{Playlist, PlaylistDetail, Song, SongDTO};
use crate::debug_log;

/// Text contains kana (and most common kanji) → `NotoSansJP`.
pub const SCRIPT_JP: u8 = 1 << 0;
/// Text contains Hangul → `NotoSansKR`.
pub const SCRIPT_KR: u8 = 1 << 1;
/// Text contains Han glyphs outside NotoSansJP coverage (Ext-A, compat ideographs, Ext-B+) → `NotoSansSC`.
pub const SCRIPT_SC: u8 = 1 << 2;
/// Text contains cuneiform codepoints (the playlist-listing prank range) → `NotoSansCuneiform`.
pub const SCRIPT_CUNEIFORM: u8 = 1 << 3;

/// Detect which lazy fallback fonts a string needs.
pub fn detect_scripts(s: &str) -> u8 {
    let mut mask = 0u8;
    for c in s.chars() {
        let cp = c as u32;
        match cp {
            // Hiragana, Katakana, CJK punctuation, halfwidth katakana
            0x3040..=0x309F | 0x30A0..=0x30FF | 0x3000..=0x303F | 0xFF66..=0xFF9F => {
                mask |= SCRIPT_JP;
            }
            // Hangul jamo, compatibility jamo, Hangul syllables
            0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => {
                mask |= SCRIPT_KR;
            }
            // Common CJK unified ideographs: NotoSansJP covers the vast majority
            0x4E00..=0x9FFF => {
                mask |= SCRIPT_JP;
            }
            // Ranges NotoSansJP lacks → need NotoSansSC
            0x3400..=0x4DBF | 0xF900..=0xFAFF | 0x20000..=0x2FA1DF => {
                mask |= SCRIPT_SC;
            }
            // Cuneiform block
            0x12000..=0x123FF => {
                mask |= SCRIPT_CUNEIFORM;
            }
            _ => {}
        }
    }
    mask
}

struct LazyFontSpec {
    bit: u8,
    name: &'static str,
    gz: &'static [u8],
}

const LAZY_FONTS: [LazyFontSpec; 4] = [
    LazyFontSpec {
        bit: SCRIPT_JP,
        name: "noto-sans-jp",
        gz: include_bytes!("../../assets/fonts/NotoSansJP-Regular.ttf.gz"),
    },
    LazyFontSpec {
        bit: SCRIPT_KR,
        name: "noto-sans-kr",
        gz: include_bytes!("../../assets/fonts/NotoSansKR-Regular.ttf.gz"),
    },
    LazyFontSpec {
        bit: SCRIPT_SC,
        name: "noto-sans-sc",
        gz: include_bytes!("../../assets/fonts/NotoSansSC-Regular.ttf.gz"),
    },
    LazyFontSpec {
        bit: SCRIPT_CUNEIFORM,
        name: "noto-sans-cuneiform",
        gz: include_bytes!("../../assets/fonts/NotoSansCuneiform-Regular.ttf.gz"),
    },
];

fn decompress_gz(gz: &'static [u8]) -> anyhow::Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(gz);
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Registry of lazily loaded fonts. Decompression happens on the tokio runtime,
/// the font is then inserted into egui and a repaint is requested.
pub struct LazyFonts {
    ctx: Context,
    rt: Arc<Runtime>,
    /// Font names already queued/loaded — makes `ensure` idempotent so egui's
    /// font atlas is only rebuilt once per font.
    loaded: Arc<DashSet<String>>,
}

static REGISTRY: OnceLock<Arc<LazyFonts>> = OnceLock::new();

impl LazyFonts {
    pub fn new(ctx: Context, rt: Arc<Runtime>) -> Self {
        Self {
            ctx,
            rt,
            loaded: Arc::new(DashSet::new()),
        }
    }

    /// Register every lazy font required by `mask` that isn't registered yet.
    pub fn ensure(&self, mask: u8) {
        if mask == 0 {
            return;
        }
        for spec in LAZY_FONTS.iter() {
            if mask & spec.bit == 0 {
                continue;
            }
            // `insert` returns false when the font is already queued/loaded.
            if !self.loaded.insert(spec.name.to_string()) {
                continue;
            }
            let ctx = self.ctx.clone();
            let name = spec.name;
            let gz = spec.gz;
            self.rt.spawn(async move {
                match decompress_gz(gz) {
                    Ok(bytes) => {
                        let len = bytes.len();
                        ctx.add_font(FontInsert::new(
                            name,
                            FontData::from_owned(bytes),
                            vec![InsertFontFamily {
                                family: FontFamily::Proportional,
                                priority: FontPriority::Lowest,
                            }],
                        ));
                        ctx.request_repaint();
                        debug_log!(
                            "🔤 [LazyFonts] Registered {} ({} bytes decompressed)",
                            name,
                            len
                        );
                    }
                    Err(e) => {
                        debug_log!("❌ [LazyFonts] Failed to decompress {}: {}", name, e);
                    }
                }
            });
        }
    }
}

/// Initialize the global registry (called once from `create_app`).
pub fn init_lazy_fonts(ctx: Context, rt: Arc<Runtime>) -> Arc<LazyFonts> {
    let registry = Arc::new(LazyFonts::new(ctx, rt));
    // Ignore the error if init is called twice (e.g. tests); keep the first registry.
    let _ = REGISTRY.set(registry.clone());
    registry
}

fn registry() -> Option<&'static Arc<LazyFonts>> {
    REGISTRY.get()
}

/// Global entry points: no-ops before `init_lazy_fonts` (e.g. in unit tests).
pub fn ensure_scripts(mask: u8) {
    if let Some(reg) = registry() {
        reg.ensure(mask);
    }
}

pub fn ensure_str(s: &str) {
    ensure_scripts(detect_scripts(s));
}

pub fn scan_song(song: &Song) {
    let mut mask = detect_scripts(&song.title);
    for a in song.original_artists.iter() {
        mask |= detect_scripts(a);
    }
    for a in song.cover_artists.iter() {
        mask |= detect_scripts(a);
    }
    ensure_scripts(mask);
}

pub fn scan_song_dto(dto: &SongDTO) {
    scan_song_dtos(std::slice::from_ref(dto));
}

pub fn scan_song_dtos(songs: &[SongDTO]) {
    let mut mask = 0;
    for dto in songs {
        mask |= detect_scripts(&dto.title);
        for a in dto.original_artists.iter() {
            mask |= detect_scripts(a);
        }
        for a in dto.cover_artists.iter() {
            mask |= detect_scripts(a);
        }
    }
    ensure_scripts(mask);
}

pub fn scan_playlists(playlists: &[Playlist]) {
    let mut mask = 0;
    for p in playlists {
        mask |= detect_scripts(&p.name);
        mask |= detect_scripts(&p.creator);
        mask |= detect_scripts(&p.description);
    }
    ensure_scripts(mask);
}

pub fn scan_playlist_detail(detail: &PlaylistDetail) {
    let mut mask = detect_scripts(&detail.name);
    for dto in &detail.songs {
        mask |= detect_scripts(&dto.title);
        for a in dto.original_artists.iter() {
            mask |= detect_scripts(a);
        }
        for a in dto.cover_artists.iter() {
            mask |= detect_scripts(a);
        }
    }
    ensure_scripts(mask);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_needs_no_lazy_fonts() {
        assert_eq!(detect_scripts("Neuro Karaoke - 12345"), 0);
        assert_eq!(detect_scripts(""), 0);
    }

    #[test]
    fn japanese_kana_needs_jp() {
        assert_eq!(detect_scripts("カラオケ"), SCRIPT_JP);
        assert_eq!(detect_scripts("ひらがな"), SCRIPT_JP);
    }

    #[test]
    fn kanji_needs_jp() {
        assert_eq!(detect_scripts("歌"), SCRIPT_JP);
    }

    #[test]
    fn korean_needs_kr() {
        assert_eq!(detect_scripts("노래"), SCRIPT_KR);
    }

    #[test]
    fn cjk_ext_a_needs_sc() {
        assert_eq!(detect_scripts("\u{3400}"), SCRIPT_SC);
        assert_eq!(detect_scripts("\u{20000}"), SCRIPT_SC);
    }

    #[test]
    fn cuneiform_needs_cuneiform() {
        assert_eq!(detect_scripts("\u{12000}"), SCRIPT_CUNEIFORM);
        assert_eq!(detect_scripts("\u{123FF}"), SCRIPT_CUNEIFORM);
    }

    #[test]
    fn mixed_scripts_combine_masks() {
        assert_eq!(
            detect_scripts("カラオケ 노래 \u{12000}"),
            SCRIPT_JP | SCRIPT_KR | SCRIPT_CUNEIFORM
        );
    }

    #[test]
    fn ensure_is_a_noop_without_registry() {
        // Global registry is not initialized in tests → must not panic.
        ensure_scripts(SCRIPT_JP | SCRIPT_CUNEIFORM);
        ensure_str("カラオケ");
    }

    #[test]
    fn ensure_is_idempotent() {
        let ctx = Context::default();
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
        );
        let fonts = LazyFonts::new(ctx, rt);
        // First call queues the fonts, second call must not re-queue them.
        fonts.ensure(SCRIPT_JP);
        assert!(fonts.loaded.contains("noto-sans-jp"));
        fonts.ensure(SCRIPT_JP);
        assert_eq!(fonts.loaded.len(), 1);
        fonts.ensure(0);
        assert_eq!(fonts.loaded.len(), 1);
    }
}


